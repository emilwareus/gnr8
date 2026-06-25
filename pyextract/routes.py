"""FastAPI route recognition -> neutral RouteFact dicts (the Python twin of routes.go).

Recognition is STATIC and derived from the SOURCE's own constructs (rule 1): an
``@<router>.<method>(...)`` decorator on a ``def``/``async def`` whose ``<router>`` is a
module-level binding to a ``FastAPI()`` / ``APIRouter(...)`` instance. Nothing here reads
FastAPI's runtime ``/openapi.json`` or any third-party schema dialect — the method, path,
params, body and response are all read off the decorator Call + the typed handler signature.

The ``APIRouter(prefix="/books")`` prefix is recorded SEPARATELY and NEVER folded into the
code-derived path (rule 1): the snapshot's operation paths stay group-relative (``/`` /
``/{book_id}``) and ``/books`` is a lowering-time base path supplied by the host.

A RouteFact has EXACTLY the keys the host ``RouteFact`` DTO (``deny_unknown_fields``)
requires: ``method, path, handler, operation_id, params, request_body, responses, span``.
A ParamFact has EXACTLY ``name, location, required, schema, span``. A ResponseFact has
EXACTLY ``status, body``.
"""

import ast

from pyextract import symtab, types

#: The HTTP verbs an ``@<router>.<verb>(...)`` decorator may name (FastAPI route methods).
_HTTP_METHODS = frozenset(
    {"get", "post", "put", "delete", "patch", "head", "options", "trace"}
)

#: The constructor names that bind a route-registering instance (recognized by NAME, rule 1).
_ROUTER_CTORS = frozenset({"FastAPI", "APIRouter"})


def _span(abs_path, node):
    line = getattr(node, "lineno", 0)
    return {"file": abs_path, "start_line": line, "end_line": line}


def _ctor_name(call):
    """Return the simple constructor name of a Call (``FastAPI`` / ``APIRouter``) or None."""
    if not isinstance(call, ast.Call):
        return None
    name = types._name_of(call.func)
    return name.split(".")[-1] if name else None


def _const_kwarg(call, key):
    """Return the Constant value of keyword ``key`` on a Call, else None."""
    for kw in call.keywords:
        if kw.arg == key and isinstance(kw.value, ast.Constant):
            return kw.value.value
    return None


def _router_bindings(module):
    """Map each module-level router/app variable name -> its recorded prefix string.

    ``app = FastAPI(title=..)`` -> ``{"app": ""}``;
    ``router = APIRouter(prefix="/books")`` -> ``{"router": "/books"}``.
    The prefix is captured for provenance ONLY — it is never folded into a path (rule 1).
    """
    bindings = {}
    for stmt in module.tree.body:
        if not isinstance(stmt, ast.Assign):
            continue
        if len(stmt.targets) != 1 or not isinstance(stmt.targets[0], ast.Name):
            continue
        ctor = _ctor_name(stmt.value)
        if ctor in _ROUTER_CTORS:
            prefix = _const_kwarg(stmt.value, "prefix") or ""
            bindings[stmt.targets[0].id] = prefix
    return bindings


def _route_decorator(func, bindings):
    """Return the decorator Call that registers ``func`` as a route, or None.

    Recognizes ``@<name>.<verb>(...)`` where ``<name>`` is a known router binding and
    ``<verb>`` is an HTTP method. Returns the ``ast.Call`` decorator node.
    """
    for dec in func.decorator_list:
        if not isinstance(dec, ast.Call):
            continue
        attr = dec.func
        if not isinstance(attr, ast.Attribute):
            continue
        if not isinstance(attr.value, ast.Name):
            continue
        if attr.value.id in bindings and attr.attr in _HTTP_METHODS:
            return dec
    return None


def _route_path(call):
    """Return the route path: the first positional Constant arg, VERBATIM, else None."""
    for arg in call.args:
        if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
            return arg.value
    return None


def _path_param_names(path):
    """Return the set of ``{name}`` template names embedded in a route path."""
    names = set()
    i = 0
    while i < len(path):
        if path[i] == "{":
            end = path.find("}", i)
            if end == -1:
                break
            names.add(path[i + 1 : end])
            i = end + 1
        else:
            i += 1
    return names


def _has_default(args, index):
    """Whether the positional arg at ``index`` carries a default value.

    Python aligns defaults to the END of the positional args: a function with N
    positional args and D defaults gives the last D args their defaults.
    """
    total = len(args.args)
    num_defaults = len(args.defaults)
    return index >= total - num_defaults


def _is_union_alias(node):
    """Whether an alias value node is a ``Union[...]`` / ``A | B`` (a standalone schema)."""
    if isinstance(node, ast.Subscript):
        base = types._subscript_value(node)
        return bool(base) and base.split(".")[-1] == "Union"
    return isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr)


def _resolves_to_class(annotation, in_module, table):
    """Whether ``annotation`` is a bare Name/Attribute resolving to a class (a body candidate)."""
    if not isinstance(annotation, (ast.Name, ast.Attribute)):
        return None
    name = types._name_of(annotation)
    if not name:
        return None
    simple = name.split(".")[-1]
    res = table.resolve(simple, in_module)
    if res is symtab.UNRESOLVABLE:
        return None
    if res.kind == "class":
        return res.qualified_id
    return None


def _resolves_to_schema_ref(annotation, in_module, table):
    """Resolve a Name/Attribute to a standalone-schema $ref id (a class OR a Union alias).

    A model/``@dataclass`` class is a schema; a top-level ``Union`` alias (``BookOrError``)
    is ALSO emitted as a standalone schema (schemas.py) and is therefore a valid response
    body ref. A ``Literal`` alias is inline-only and never a ref (rule 3 — one source).
    """
    if not isinstance(annotation, (ast.Name, ast.Attribute)):
        return None
    name = types._name_of(annotation)
    if not name:
        return None
    simple = name.split(".")[-1]
    res = table.resolve(simple, in_module)
    if res is symtab.UNRESOLVABLE:
        return None
    if res.kind == "class":
        return res.qualified_id
    if res.kind == "alias" and _is_union_alias(res.node):
        return res.qualified_id
    return None


def _build_params(func, path, in_module, abs_path, table, diags):
    """Build the param + request_body facts from the typed handler signature.

    A param whose name appears in the path template -> ``location: path`` (required);
    otherwise -> ``location: query`` (required = the param has NO default). The first
    param whose annotation resolves to a class becomes the request body ($ref).
    """
    params = []
    request_body = None
    path_names = _path_param_names(path)
    pos = func.args.args
    for index, arg in enumerate(pos):
        annotation = arg.annotation
        if annotation is None:
            continue
        # A parameter typed by a model/@dataclass class is the request body, not a param.
        body_ref = _resolves_to_class(annotation, in_module, table)
        if body_ref is not None and arg.arg not in path_names:
            if request_body is None:
                request_body = {"ref_id": body_ref}
            continue

        in_path = arg.arg in path_names
        schema, _nullable = types.map_field_annotation(
            annotation, in_module, table, diags
        )
        if schema is None:
            continue
        if in_path:
            required = True
        else:
            required = not _has_default(func.args, index)
        params.append(
            {
                "name": arg.arg,
                "location": "path" if in_path else "query",
                "required": required,
                "schema": schema,
                "span": _span(abs_path, arg),
            }
        )
    return params, request_body


def _build_response(call):
    """Build the single response fact from ``response_model=`` / ``status_code=``.

    ``status_code=`` Constant -> status (default 200 when absent); ``response_model=``
    a class Name -> the response body $ref. The id is resolved by the caller's symtab
    pass via the annotation name; here we capture the raw name node.
    """
    status = _const_kwarg(call, "status_code")
    if not isinstance(status, int):
        status = 200
    return status


def _response_model_ref(call, in_module, table):
    """Resolve a ``response_model=ClassName`` keyword to a schema $ref id, or None."""
    for kw in call.keywords:
        if kw.arg == "response_model":
            return _resolves_to_schema_ref(kw.value, in_module, table)
    return None


def recognize_fastapi(modules, table, diags):
    """Recognize FastAPI routes across ``modules`` -> a list of RouteFact dicts.

    For each module: index its router/app bindings (with their separately-recorded
    prefix), then walk every ``def``/``async def`` carrying an ``@<router>.<verb>(...)``
    decorator and assemble its method/path/params/body/response facts. The host
    re-sorts the routes; the sidecar stays internally deterministic.
    """
    routes = []
    for module in sorted(modules, key=lambda m: m.dotted):
        bindings = _router_bindings(module)
        if not bindings:
            continue
        abs_path = module.abs_path
        for stmt in module.tree.body:
            if not isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            decorator = _route_decorator(stmt, bindings)
            if decorator is None:
                continue
            path = _route_path(decorator)
            if path is None:
                diags.warn(
                    "FastAPI route decorator has no constant path; route omitted "
                    "(no fallback)",
                    abs_path,
                    getattr(stmt, "lineno", 0),
                )
                continue
            method = decorator.func.attr.upper()
            params, request_body = _build_params(
                stmt, path, module.dotted, abs_path, table, diags
            )
            status = _build_response(decorator)
            body_ref = _response_model_ref(decorator, module.dotted, table)
            response = {
                "status": status,
                "body": {"ref_id": body_ref} if body_ref is not None else None,
            }
            routes.append(
                {
                    "method": method,
                    "path": path,
                    "handler": stmt.name,
                    "operation_id": stmt.name,
                    "params": params,
                    "request_body": request_body,
                    "responses": [response],
                    "span": _span(abs_path, stmt),
                }
            )
    return routes
