"""The OWNED cross-module symbol table — the one design surface with no Go-file analog.

Go gets cross-module name/type resolution for free from ``go/types``. Python has no
stdlib type-checker, so the sidecar must OWN the resolver (rule 2 — hand-rolled, no
library). It is STATIC: it indexes the parsed ASTs and follows ``from x import Y``
statements by dictionary lookup. It NEVER imports/executes the target to resolve a
name (threat T-static-import / PYSRC-03).

For each module it indexes:
  * ``classes``: ``{ClassName: ast.ClassDef}`` (with their base-name list).
  * ``aliases``: ``{Name: annotation_ast}`` for top-level assignments whose value is
    an annotation expression, e.g. ``SortOrder = Literal["asc", "desc"]`` or
    ``BookOrError = Union[Book, OutOfStock]``.
  * ``imports``: ``{local_name: (source_dotted_module, original_name)}`` read out of
    each ``ImportFrom`` statement (the ``import Y`` and ``import Y as Z`` forms).

``resolve(name, in_module)`` returns a :class:`Resolution` describing what ``name``
denotes in ``in_module``: a local/imported class, a local/imported alias, or the
distinct :data:`UNRESOLVABLE` sentinel (NOT a guessed string) so the caller emits a
diagnostic and OMITS the fact (rule 3).
"""

import ast


class Resolution:
    """The result of resolving a name to a declaration in some module.

    Attributes:
        kind: one of ``"class"`` or ``"alias"``.
        qualified_id: ``"<dotted-module>.<Name>"`` — the stable schema id.
        module: the dotted module the declaration lives in.
        name: the original declared name.
        node: the ``ast.ClassDef`` (for a class) or the alias value ``ast`` node.
    """

    __slots__ = ("kind", "qualified_id", "module", "name", "node")

    def __init__(self, kind, qualified_id, module, name, node):
        self.kind = kind
        self.qualified_id = qualified_id
        self.module = module
        self.name = name
        self.node = node


class _Unresolvable:
    """A distinct sentinel signalling a name could not be resolved statically.

    Returned (not raised) so the caller branches on identity (``is UNRESOLVABLE``)
    and emits a diagnostic + omits the fact — never a guessed default (rule 3).
    """

    __slots__ = ()

    def __repr__(self):
        return "UNRESOLVABLE"

    def __bool__(self):
        return False


#: The single shared unresolvable sentinel.
UNRESOLVABLE = _Unresolvable()


class _ModuleIndex:
    """The per-module index of classes, aliases, and imports."""

    __slots__ = ("dotted", "classes", "aliases", "imports")

    def __init__(self, dotted):
        self.dotted = dotted
        self.classes = {}
        self.aliases = {}
        self.imports = {}


def _is_annotation_expr(node):
    """Whether a top-level assignment value looks like a type annotation alias.

    Recognized: a ``Subscript`` (``Literal[...]``, ``Union[...]``, ``Optional[...]``,
    ``list[...]``), a ``BinOp`` with ``BitOr`` (``A | B``), or a bare ``Name``
    (``Foo = Bar``). A literal value like ``= "asc"`` or ``= 3`` is NOT an alias.
    """
    return isinstance(node, (ast.Subscript, ast.BinOp, ast.Name))


class SymbolTable:
    """An owned, static, cross-module symbol table built from parsed ASTs."""

    def __init__(self, modules):
        # modules: list of load.Module. Iterate in sorted dotted order for
        # determinism (the host re-sorts the final slices, but the table stays
        # internally deterministic — Pattern B).
        self._index = {}
        for module in sorted(modules, key=lambda m: m.dotted):
            self._index[module.dotted] = self._build_index(module)

    @staticmethod
    def _build_index(module):
        idx = _ModuleIndex(module.dotted)
        for stmt in module.tree.body:
            if isinstance(stmt, ast.ClassDef):
                idx.classes[stmt.name] = stmt
            elif isinstance(stmt, ast.Assign):
                # Only simple single-target top-level assignments are aliases.
                if (
                    len(stmt.targets) == 1
                    and isinstance(stmt.targets[0], ast.Name)
                    and _is_annotation_expr(stmt.value)
                ):
                    idx.aliases[stmt.targets[0].id] = stmt.value
            elif isinstance(stmt, ast.ImportFrom):
                # `from x import Y` / `from x import Y as Z`. Relative imports
                # (level > 0) and `import *` are not statically followed here.
                if stmt.module is None or stmt.level:
                    continue
                for alias in stmt.names:
                    if alias.name == "*":
                        continue
                    local = alias.asname or alias.name
                    idx.imports[local] = (stmt.module, alias.name)
        return idx

    def modules(self):
        """Return the indexed modules in sorted dotted-id order."""
        return [self._index[k] for k in sorted(self._index)]

    def class_def(self, module, name):
        """Return the local ``ast.ClassDef`` for ``name`` in ``module`` or None."""
        idx = self._index.get(module)
        if idx is None:
            return None
        return idx.classes.get(name)

    def resolve(self, name, in_module, _seen=None):
        """Resolve ``name`` as seen from ``in_module``.

        Returns a :class:`Resolution` for a local/imported class or alias, or the
        :data:`UNRESOLVABLE` sentinel when the name is absent from this module's
        classes/aliases/imports (and any module it imports from). Import chains are
        followed statically (dict lookup only — never an exec).
        """
        if _seen is None:
            _seen = set()
        key = (name, in_module)
        if key in _seen:
            return UNRESOLVABLE  # cyclic import alias — bail deterministically
        _seen.add(key)

        idx = self._index.get(in_module)
        if idx is None:
            return UNRESOLVABLE

        if name in idx.classes:
            return Resolution(
                "class",
                "{}.{}".format(in_module, name),
                in_module,
                name,
                idx.classes[name],
            )
        if name in idx.aliases:
            return Resolution(
                "alias",
                "{}.{}".format(in_module, name),
                in_module,
                name,
                idx.aliases[name],
            )
        if name in idx.imports:
            source_module, original = idx.imports[name]
            return self.resolve(original, source_module, _seen)

        return UNRESOLVABLE
