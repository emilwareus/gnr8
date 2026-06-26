"""Schema builder: ClassDef / Enum / alias -> neutral SchemaFact dicts.

The Python twin of ``goextract/internal/types/extract.go``. It walks every parsed
module and emits one SchemaFact per:

  * a model class (a ``BaseModel`` subclass) or a ``@dataclass`` -> an ``object`` body
    of FieldFacts carrying the four optional/nullable axes.
  * an ``enum.Enum`` subclass -> an ``enum`` body of SORTED member VALUES.
  * a top-level alias whose value is a ``Literal[...]`` (-> inline ``enum``) or a
    ``Union[...]`` / ``A | B`` (-> a ``union`` body of mapped members).

Recognition is STATIC and by NAME (rule 1): a base class named ``BaseModel`` / a base
ending in ``Enum`` / a ``@dataclass`` decorator name. The target's pydantic /
dataclasses / enum are never imported to introspect.

A SchemaFact has keys ``id, name, body, span``. ``id`` = ``<dotted-module>.<Name>``.
A FieldFact has EXACTLY ``json_name, required, optional, nullable, schema,
description, example`` — ``description`` / ``example`` are always ``None`` here. The
four axes (RESEARCH Pitfall 3):

  * ``optional`` = the field has a default (``= x`` / ``field(default=..)`` /
    ``field(default_factory=..)``).
  * ``nullable`` = the type admits ``None`` (``Optional[T]`` / ``T | None``).
  * ``required`` = ``not optional`` (nullable does NOT affect required).
"""

import ast

from pyextract import types


def _span(abs_path, node):
    line = getattr(node, "lineno", 0)
    return {"file": abs_path, "start_line": line, "end_line": line}


def _base_names(class_def):
    """Return the simple names of a ClassDef's bases (e.g. ``BaseModel``, ``Enum``)."""
    out = []
    for base in class_def.bases:
        name = types._name_of(base)
        if name:
            out.append(name.split(".")[-1])
    return out


def _decorator_names(node):
    """Return the simple names of a node's decorators (e.g. ``dataclass``)."""
    out = []
    for dec in getattr(node, "decorator_list", []):
        # @dataclass or @dataclass(...) or @dataclasses.dataclass
        target = dec.func if isinstance(dec, ast.Call) else dec
        name = types._name_of(target)
        if name:
            out.append(name.split(".")[-1])
    return out


def _is_enum_class(class_def):
    return any(b == "Enum" or b.endswith("Enum") for b in _base_names(class_def))


def _is_model_class(class_def):
    bases = _base_names(class_def)
    return "BaseModel" in bases


def _is_dataclass(class_def):
    return "dataclass" in _decorator_names(class_def)


def _enum_members(class_def):
    """Return the SORTED string VALUES of an ``enum.Enum`` subclass.

    Reads each ``NAME = "value"`` assignment in the class body and emits the VALUE
    string (``"paperback"``, not ``PAPERBACK``), sorted lexically (Pitfall 4).
    """
    members = []
    for stmt in class_def.body:
        if isinstance(stmt, ast.Assign) and len(stmt.targets) == 1:
            if (
                isinstance(stmt.value, ast.Constant)
                and isinstance(stmt.value.value, str)
            ):
                members.append(stmt.value.value)
    return sorted(members)


def _has_default(stmt):
    """Whether an ``AnnAssign`` field statement carries a default value."""
    # AnnAssign.value is the RHS of `name: T = value`; None means no default.
    return getattr(stmt, "value", None) is not None


def _build_field(stmt, in_module, table, diags):
    """Build a single FieldFact dict from a class-body ``AnnAssign`` statement.

    Returns ``None`` when the annotation is unresolvable (the field is omitted and a
    diagnostic is recorded by the type mapper — rule 3).
    """
    json_name = stmt.target.id
    schema, nullable = types.map_field_annotation(
        stmt.annotation, in_module, table, diags
    )
    if schema is None:
        return None
    optional = _has_default(stmt)
    return {
        "json_name": json_name,
        "required": not optional,
        "optional": optional,
        "nullable": nullable,
        "schema": schema,
        "description": None,
        "example": None,
    }


def _build_object_fields(class_def, in_module, table, diags):
    fields = []
    for stmt in class_def.body:
        if isinstance(stmt, ast.AnnAssign) and isinstance(stmt.target, ast.Name):
            field = _build_field(stmt, in_module, table, diags)
            if field is not None:
                fields.append(field)
    return fields


def build_schemas(modules, table, diags):
    """Build the full sorted-by-id list of SchemaFact dicts for every module.

    Walks every ClassDef (model/dataclass -> object; enum -> enum) and every
    top-level alias (``Literal`` -> inline enum; ``Union`` / ``A | B`` -> union) into
    a SchemaFact. The host re-sorts, but the sidecar stays internally deterministic.
    """
    schemas = []
    for module in sorted(modules, key=lambda m: m.dotted):
        abs_path = module.abs_path
        for stmt in module.tree.body:
            if isinstance(stmt, ast.ClassDef):
                schema = _build_class_schema(
                    stmt, module.dotted, abs_path, table, diags
                )
                if schema is not None:
                    schemas.append(schema)
            elif isinstance(stmt, ast.Assign):
                schema = _build_alias_schema(
                    stmt, module.dotted, abs_path, table, diags
                )
                if schema is not None:
                    schemas.append(schema)
    schemas.sort(key=lambda s: s["id"])
    return schemas


def _build_class_schema(class_def, dotted, abs_path, table, diags):
    name = class_def.name
    qualified = "{}.{}".format(dotted, name)
    if _is_enum_class(class_def):
        members = _enum_members(class_def)
        if not members:
            # Only string-valued members are representable as a neutral string enum.
            # An int/auto()/tuple-valued enum yields no members — omit the fact with a
            # diagnostic rather than emitting an invalid empty `enum: []` (rule 3: no
            # guess; mirrors the empty-Literal guard in types.py).
            diags.warn(
                "enum '{}' has no string-valued members (int/auto()/tuple enums are not "
                "representable as a neutral string enum); schema omitted".format(name),
                abs_path,
                getattr(class_def, "lineno", 0),
            )
            return None
        body = {"type": "enum", "of": members}
    elif _is_model_class(class_def) or _is_dataclass(class_def):
        body = {
            "type": "object",
            "of": _build_object_fields(class_def, dotted, table, diags),
        }
    else:
        # Not a recognized schema-bearing class (rule 1: only model/dataclass/enum
        # NAMES are recognized). Skip silently — it is not a fact, not an error.
        return None
    return {
        "id": qualified,
        "name": name,
        "body": body,
        "span": _span(abs_path, class_def),
    }


def _build_alias_schema(stmt, dotted, abs_path, table, diags):
    """Build a SchemaFact for a top-level ``Name = Union[...]`` / ``A | B`` alias.

    Only a UNION alias becomes a standalone schema (``BookOrError`` is referenced by
    ``ref_id`` from a route, so it must exist as a named schema). A ``Literal[...]``
    alias (``SortOrder``) is NEVER a standalone schema — it is only ever inlined where
    used (``BookFilters.sort`` -> an inline ``enum`` body); the snapshot is the
    authority and contains no ``SortOrder`` schema. A bare ``Foo = Bar`` re-binding is
    likewise not a schema.
    """
    if len(stmt.targets) != 1 or not isinstance(stmt.targets[0], ast.Name):
        return None
    name = stmt.targets[0].id
    value = stmt.value

    is_union = (
        isinstance(value, ast.Subscript)
        and (types._subscript_value(value) or "").split(".")[-1] == "Union"
    ) or (isinstance(value, ast.BinOp) and isinstance(value.op, ast.BitOr))

    if not is_union:
        return None

    body = types.map_annotation(value, dotted, table, diags)
    if body is None:
        return None
    return {
        "id": "{}.{}".format(dotted, name),
        "name": name,
        "body": body,
        "span": _span(abs_path, stmt),
    }
