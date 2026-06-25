"""Annotation AST -> neutral Type dict — the Python twin of goextract's type extract.

The neutral vocabulary is byte-fixed by ``crates/gnr8-core/src/analyze/facts.rs``
(``Type`` / ``Prim``), and the mapping table by the committed FastAPI snapshot:

  * ``str`` -> ``{"type":"primitive","of":{"prim":"string"}}``
  * ``bool`` -> ``{"prim":"bool"}``
  * ``int`` -> ``{"prim":"int","bits":64,"signed":true}`` (Python int -> int64 signed)
  * ``float`` -> ``{"prim":"float","bits":64}``
  * ``Optional[T]`` / ``T | None`` -> unwrap to T; the None arm is the FIELD's
    ``nullable`` axis, NOT a union member (see :func:`map_field_annotation`).
  * ``Union[A,B]`` (no None) -> ``{"type":"union","of":[map(A),map(B)]}``, SOURCE order.
  * ``list[T]`` / ``List[T]`` -> ``{"type":"array","of":map(T)}``.
  * ``dict[K,V]`` / ``Dict[K,V]`` -> ``{"type":"map","of":{"key":..,"value":..}}``.
  * ``Literal["a","b"]`` -> ``{"type":"enum","of":sorted([...])}`` (inline).
  * a name resolving to an ``enum.Enum`` subclass or a model/``@dataclass`` class ->
    ``{"type":"named","of":"<id>"}`` (a ref; the class is emitted as its own schema).
  * an unresolvable / foreign name -> ``(None, diagnostic)`` — NEVER ``{"type":"any"}``
    as a silent default (rule 3).

Every fact derives from the SOURCE's own constructs (rule 1): base-class / decorator
NAMES are recognized statically in the AST; the target's pydantic/dataclasses/enum
are never imported to introspect.
"""

import ast

from pyextract import symtab


def _prim(prim, **extra):
    payload = {"prim": prim}
    payload.update(extra)
    return {"type": "primitive", "of": payload}


# str/bool/int/float -> their fixed neutral primitive dicts.
_PRIMITIVES = {
    "str": lambda: _prim("string"),
    "bool": lambda: _prim("bool"),
    "int": lambda: _prim("int", bits=64, signed=True),
    "float": lambda: _prim("float", bits=64),
    "bytes": lambda: _prim("bytes"),
}


def _name_of(node):
    """Return the dotted/simple name of a Name or Attribute node, else None."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        base = _name_of(node.value)
        return "{}.{}".format(base, node.attr) if base else node.attr
    return None


def _subscript_value(node):
    """Return the base name of a Subscript (``list``/``List``/``Optional``/...)."""
    return _name_of(node.value)


def _subscript_args(node):
    """Return the element nodes inside a Subscript, normalizing single vs tuple."""
    sliced = node.slice
    # Python 3.9+: node.slice is the expression directly (no ast.Index wrapper for
    # the cases we parse here); a tuple subscript is an ast.Tuple.
    if isinstance(sliced, ast.Index):  # pragma: no cover - defensive for <3.9 shapes
        sliced = sliced.value
    if isinstance(sliced, ast.Tuple):
        return list(sliced.elts)
    return [sliced]


def _is_none(node):
    """Whether an annotation node denotes ``None`` (the NoneType arm)."""
    if isinstance(node, ast.Constant) and node.value is None:
        return True
    return isinstance(node, ast.Name) and node.id == "None"


def _flatten_bitor(node):
    """Flatten a chain of ``A | B | C`` BinOp(BitOr) into a list of operand nodes."""
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return _flatten_bitor(node.left) + _flatten_bitor(node.right)
    return [node]


def _literal_members(node):
    """Return sorted string members of a ``Literal[...]`` subscript."""
    members = []
    for arg in _subscript_args(node):
        if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
            members.append(arg.value)
    return sorted(members)


def map_annotation(node, in_module, table, diags):
    """Map a (non-optional-context) annotation node to a neutral Type dict.

    Returns the Type dict, or ``None`` (and records a diagnostic) when the
    annotation cannot be resolved to a single deterministic fact (rule 3).
    """
    return _map(node, in_module, table, diags)


def map_field_annotation(node, in_module, table, diags):
    """Map a field/param annotation, splitting out the nullable axis.

    Returns ``(type_dict_or_None, nullable_bool)``. ``Optional[T]`` and ``T | None``
    unwrap to ``T`` with ``nullable=True``; the None arm is the FIELD's nullable axis,
    never a union member (RESEARCH Pitfall 3/5).
    """
    inner, nullable = _strip_optional(node)
    return _map(inner, in_module, table, diags), nullable


def _strip_optional(node):
    """Strip an ``Optional[T]`` / ``T | None`` wrapper, returning (inner, nullable)."""
    # Optional[T]
    if isinstance(node, ast.Subscript) and _subscript_value(node) in (
        "Optional",
        "typing.Optional",
    ):
        return node.slice, True
    # Union[..., None] -> drop the None arm, set nullable; keep the rest.
    if isinstance(node, ast.Subscript) and _subscript_value(node) in (
        "Union",
        "typing.Union",
    ):
        args = _subscript_args(node)
        non_none = [a for a in args if not _is_none(a)]
        if len(non_none) != len(args):
            if len(non_none) == 1:
                return non_none[0], True
            # Re-wrap the remaining arms as a Union subscript for downstream mapping.
            return _rebuild_union(node, non_none), True
        return node, False
    # PEP 604 `A | None`
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        operands = _flatten_bitor(node)
        non_none = [a for a in operands if not _is_none(a)]
        if len(non_none) != len(operands):
            if len(non_none) == 1:
                return non_none[0], True
            return _rebuild_bitor(non_none), True
        return node, False
    return node, False


def _rebuild_union(original, arms):
    """Rebuild a ``Union[...]`` subscript node from a reduced arm list."""
    return ast.Subscript(
        value=original.value,
        slice=ast.Tuple(elts=arms, ctx=ast.Load()),
        ctx=ast.Load(),
    )


def _rebuild_bitor(arms):
    """Rebuild a left-folded ``A | B | C`` BinOp chain from an operand list."""
    node = arms[0]
    for arm in arms[1:]:
        node = ast.BinOp(left=node, op=ast.BitOr(), right=arm)
    return node


def _map(node, in_module, table, diags):
    # Bare name: primitive, or a resolvable named class/alias.
    name = _name_of(node)
    if isinstance(node, ast.Name) and name in _PRIMITIVES:
        return _PRIMITIVES[name]()

    if isinstance(node, ast.Subscript):
        return _map_subscript(node, in_module, table, diags)

    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        # A | B (no None here — None handled in _strip_optional for fields). As a
        # plain type position it is a union in source order.
        operands = _flatten_bitor(node)
        return _map_union(operands, in_module, table, diags)

    if isinstance(node, (ast.Name, ast.Attribute)):
        return _map_named(node, name, in_module, table, diags)

    diags.warn(
        "unsupported type annotation: {}".format(_describe(node)),
        _file_of(table, in_module),
        getattr(node, "lineno", 0),
    )
    return None


def _map_subscript(node, in_module, table, diags):
    base = _subscript_value(node)
    simple = base.split(".")[-1] if base else base

    if simple in ("list", "List", "Sequence", "Iterable", "Set", "FrozenSet", "set"):
        args = _subscript_args(node)
        elem = _map(args[0], in_module, table, diags) if args else None
        if elem is None:
            return None
        return {"type": "array", "of": elem}

    if simple in ("dict", "Dict", "Mapping", "MutableMapping"):
        args = _subscript_args(node)
        if len(args) != 2:
            # rule 3: a bare/malformed mapping cannot yield a deterministic
            # key/value fact — diagnose + OMIT, never default to string -> any.
            diags.warn(
                "unsupported mapping annotation: {} needs exactly two type args; "
                "fact omitted (no fallback)".format(base),
                _file_of(table, in_module),
                getattr(node, "lineno", 0),
            )
            return None
        key = _map(args[0], in_module, table, diags)
        value = _map(args[1], in_module, table, diags)
        if key is None or value is None:
            return None
        return {"type": "map", "of": {"key": key, "value": value}}

    if simple == "Literal":
        return {"type": "enum", "of": _literal_members(node)}

    if simple == "Union":
        return _map_union(_subscript_args(node), in_module, table, diags)

    if simple == "Optional":
        # An Optional in a non-field position: map the inner type (nullability is a
        # field-level axis and is lost here, which is the correct behavior for a
        # nested/standalone Optional element type).
        inner = _subscript_args(node)
        return _map(inner[0], in_module, table, diags) if inner else None

    # An unrecognized subscript (e.g. a foreign generic) — diagnostic + omit.
    diags.warn(
        "unsupported generic type: {}".format(base),
        _file_of(table, in_module),
        getattr(node, "lineno", 0),
    )
    return None


def _map_union(arg_nodes, in_module, table, diags):
    members = []
    for arg in arg_nodes:
        if _is_none(arg):
            # A None arm inside a plain union position is dropped (nullability is a
            # field axis); it never becomes a union member (Pitfall 5).
            continue
        mapped = _map(arg, in_module, table, diags)
        if mapped is None:
            return None
        members.append(mapped)
    return {"type": "union", "of": members}


def _map_named(node, name, in_module, table, diags):
    simple = name.split(".")[-1] if name else name
    res = table.resolve(simple, in_module)
    if res is symtab.UNRESOLVABLE:
        diags.warn(
            "unresolvable type name '{}': not a local/imported class or alias; "
            "fact omitted (no fallback)".format(simple),
            _file_of(table, in_module),
            getattr(node, "lineno", 0),
        )
        return None
    if res.kind == "alias":
        # An alias used in a type position maps to whatever it aliases (e.g.
        # ``SortOrder = Literal[...]`` -> inline enum).
        return _map(res.node, res.module, table, diags)
    # A class -> a named ref (the class is emitted as its own schema separately).
    return {"type": "named", "of": res.qualified_id}


def _file_of(table, module):
    """Best-effort canonical file for a module (for diagnostics); '' if unknown."""
    idx = getattr(table, "_index", {}).get(module)
    return getattr(idx, "abs_path", "") if idx is not None else ""


def _describe(node):
    try:
        return ast.dump(node)
    except Exception:  # noqa: BLE001 - defensive; never let describe crash mapping
        return type(node).__name__
