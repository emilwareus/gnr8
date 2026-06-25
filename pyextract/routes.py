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

#: The constructor names that bind a Flask route-registering instance (rule 1, by NAME).
_FLASK_CTORS = frozenset({"Flask", "Blueprint"})

#: HTTP methods that carry no request body — a request.json read here is never a body fact.
_BODYLESS_METHODS = frozenset({"GET", "HEAD", "DELETE"})


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


# ---------------------------------------------------------------------------
# Flask route recognition (the HONEST typed-envelope, Plan 04).
#
# A Flask route is a ``@<bp>.route("/path", methods=[...])`` decorator on a def
# whose ``<bp>`` is a module-level ``Flask()`` / ``Blueprint()`` binding. Flask is
# genuinely less typed than FastAPI: the request/response bodies are ordinary
# ``request.json`` reads unless the author OPTS IN to a typed DTO. So the Flask
# recognizer derives every fact from the SOURCE's own typed constructs (rule 1) and
# emits a DIAGNOSTIC + OMITS the fact for every untyped surface (rule 3, no fallback):
#   * a typed return annotation -> the response body $ref; status is METHOD-DERIVED
#     (POST -> 201, else 200) — a code fact, NEVER read from the docstring.
#   * a local annotated ``order: OrderInput = ...`` -> the request body $ref.
#   * a local annotated ``status: str = request.args.get(...)`` -> a typed query param.
#   * an UNTYPED ``request.json`` read / unannotated ``request.args.get(...)`` /
#     missing return annotation -> a diagnostic, no fact.
# The Flask ``url_prefix`` is recorded SEPARATELY and NEVER folded into the path (rule 1).
# ---------------------------------------------------------------------------


def _flask_bindings(module):
    """Map each module-level Flask/Blueprint variable name -> its ``url_prefix`` string.

    ``bp = Blueprint("orders", __name__, url_prefix="/orders")`` -> ``{"bp": "/orders"}``;
    ``app = Flask(__name__)`` -> ``{"app": ""}``. The prefix is recorded for provenance
    ONLY — it is never folded into a route path (rule 1; the snapshot base_path is ``/``).
    """
    bindings = {}
    for stmt in module.tree.body:
        if not isinstance(stmt, ast.Assign):
            continue
        if len(stmt.targets) != 1 or not isinstance(stmt.targets[0], ast.Name):
            continue
        ctor = _ctor_name(stmt.value)
        if ctor in _FLASK_CTORS:
            prefix = _const_kwarg(stmt.value, "url_prefix") or ""
            bindings[stmt.targets[0].id] = prefix
    return bindings


def _flask_route_decorator(func, bindings):
    """Return the ``@<bp>.route(...)`` decorator Call registering ``func``, or None.

    Recognizes ``@<name>.route(...)`` where ``<name>`` is a known Flask/Blueprint
    binding. The HTTP method(s) live in the ``methods=[...]`` keyword (handled by the
    caller), not in the attribute name (which is always ``route``).
    """
    for dec in func.decorator_list:
        if not isinstance(dec, ast.Call):
            continue
        attr = dec.func
        if not isinstance(attr, ast.Attribute):
            continue
        if not isinstance(attr.value, ast.Name):
            continue
        if attr.value.id in bindings and attr.attr == "route":
            return dec
    return None


def _flask_methods(call):
    """Return the upper-cased HTTP methods from the ``methods=[...]`` keyword.

    ``methods=["GET", "POST"]`` -> ``["GET", "POST"]`` (source order). Absent ->
    Flask's documented default is GET; we read the SOURCE's own list and default to
    ``["GET"]`` only when the keyword is wholly absent (a code fact, not a guess).
    """
    for kw in call.keywords:
        if kw.arg == "methods" and isinstance(kw.value, (ast.List, ast.Tuple)):
            out = []
            for elt in kw.value.elts:
                if isinstance(elt, ast.Constant) and isinstance(elt.value, str):
                    out.append(elt.value.upper())
            return out
    return ["GET"]


def _flask_path(raw):
    """Convert a Flask route path to the neutral group-relative path + path-param types.

    ``"/<int:order_id>"`` -> ``("/{order_id}", {"order_id": <int64 schema>})`` (strip the
    converter, brace the name, the ``int`` converter drives the param schema). ``"/<name>"``
    (no converter) braces the name with no recorded type. A bare ``/`` stays ``/``.
    """
    out = []
    converters = {}
    i = 0
    while i < len(raw):
        ch = raw[i]
        if ch == "<":
            end = raw.find(">", i)
            if end == -1:
                out.append(ch)
                i += 1
                continue
            inner = raw[i + 1 : end]
            if ":" in inner:
                conv, name = inner.split(":", 1)
            else:
                conv, name = "", inner
            out.append("{" + name + "}")
            if conv == "int":
                converters[name] = {
                    "type": "primitive",
                    "of": {"prim": "int", "bits": 64, "signed": True},
                }
            i = end + 1
        else:
            out.append(ch)
            i += 1
    return "".join(out), converters


def _is_request_attr(node, attr):
    """Whether ``node`` is the AST for ``request.<attr>`` (e.g. ``request.json``)."""
    return (
        isinstance(node, ast.Attribute)
        and node.attr == attr
        and isinstance(node.value, ast.Name)
        and node.value.id == "request"
    )


def _is_request_args_get(node):
    """Whether ``node`` is a ``request.args.get(...)`` Call."""
    if not isinstance(node, ast.Call):
        return False
    func = node.func
    return (
        isinstance(func, ast.Attribute)
        and func.attr == "get"
        and isinstance(func.value, ast.Attribute)
        and func.value.attr == "args"
        and isinstance(func.value.value, ast.Name)
        and func.value.value.id == "request"
    )


def _flask_body_and_params(
    func, method, path, in_module, abs_path, table, diags
):
    """Walk a Flask handler body for the typed/untyped request body + query params.

    Returns ``(params, request_body)``. Each statement is inspected ONCE, top to
    bottom (deterministic, single pass — no fallback):
      * an annotated assign ``x: T = request.json``/``T(**request.json)`` whose ``T``
        resolves to a class -> the request body $ref.
      * an annotated assign ``x: str = request.args.get(...)`` -> a typed query param.
      * a PLAIN assign reading ``request.json`` -> untyped-body diagnostic, no body.
      * a PLAIN assign reading ``request.args.get(...)`` -> untyped-query diagnostic, no param.
    ``method``/``path`` name the operation in the untyped diagnostics (a code fact:
    the HTTP method and the code-derived path, never a docstring).
    """
    params = []
    request_body = None
    # A bodyless HTTP method (GET/HEAD/DELETE) has no request body by definition; a
    # request.json read in such a handler is a code smell, never an emitted body
    # fact (WR-04). The method is a code fact (the decorator's methods=[...]).
    allows_body = method not in _BODYLESS_METHODS

    for stmt in func.body:
        if isinstance(stmt, ast.AnnAssign):
            annotation = stmt.annotation
            value = stmt.value
            # Typed request body: an annotated local whose type is a class and whose
            # value reads request.json (directly or via T(**request.json)).
            body_ref = _resolves_to_class(annotation, in_module, table)
            if body_ref is not None and _reads_request_json(value):
                if allows_body and request_body is None:
                    request_body = {"ref_id": body_ref}
                continue
            # Typed query param: an annotated local reading request.args.get(...).
            if _is_request_args_get(value):
                schema, _nullable = types.map_field_annotation(
                    annotation, in_module, table, diags
                )
                if schema is not None:
                    params.append(
                        {
                            "name": stmt.target.id
                            if isinstance(stmt.target, ast.Name)
                            else "",
                            "location": "query",
                            "required": False,
                            "schema": schema,
                            "span": _span(abs_path, stmt),
                        }
                    )
                continue
        elif isinstance(stmt, ast.Assign):
            value = stmt.value
            target_name = (
                stmt.targets[0].id
                if len(stmt.targets) == 1 and isinstance(stmt.targets[0], ast.Name)
                else ""
            )
            # Untyped request body: a plain assign reading request.json with no DTO.
            if _reads_request_json(value):
                diags.warn(
                    "untyped request body on {} {}: read via request.json with no "
                    "typed DTO; body shape under-specified, no schema "
                    "inferred".format(method, path),
                    abs_path,
                    getattr(stmt, "lineno", 0),
                )
                continue
            # Untyped query param: a plain assign reading request.args.get(...).
            if _is_request_args_get(value):
                diags.warn(
                    "untyped query param '{}' on {} {}: read via request.args.get "
                    "with no annotation; param type/required-ness under-specified, "
                    "type inferred as string only".format(target_name, method, path),
                    abs_path,
                    getattr(stmt, "lineno", 0),
                )
                continue

    return params, request_body


def _reads_request_json(value):
    """Whether an assign value reads ``request.json`` (directly or ``T(**request.json)``)."""
    if value is None:
        return False
    if _is_request_attr(value, "json"):
        return True
    # T(**request.json) — a Call with a ** keyword whose value is request.json.
    if isinstance(value, ast.Call):
        for kw in value.keywords:
            if kw.arg is None and _is_request_attr(kw.value, "json"):
                return True
    return False


def recognize_flask(modules, table, diags):
    """Recognize Flask routes across ``modules`` -> a list of RouteFact dicts.

    For each module: index its Flask/Blueprint bindings (with their separately-recorded
    ``url_prefix``), then walk every ``def``/``async def`` carrying an ``@<bp>.route(...)``
    decorator and assemble ONE RouteFact PER method in its ``methods=[...]`` list. The
    status is METHOD-DERIVED for a typed handler (POST -> 201, else 200); an untyped
    handler (no resolvable return annotation) emits ``responses: []`` + a diagnostic.
    """
    routes = []
    for module in sorted(modules, key=lambda m: m.dotted):
        bindings = _flask_bindings(module)
        if not bindings:
            continue
        abs_path = module.abs_path
        for stmt in module.tree.body:
            if not isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            decorator = _flask_route_decorator(stmt, bindings)
            if decorator is None:
                continue
            raw_path = _route_path(decorator)
            if raw_path is None:
                diags.warn(
                    "Flask route decorator has no constant path; route omitted "
                    "(no fallback)",
                    abs_path,
                    getattr(stmt, "lineno", 0),
                )
                continue
            path, path_converters = _flask_path(raw_path)
            methods = _flask_methods(decorator)

            # Path params (from the converters), independent of the method split.
            path_params = []
            for name in sorted(path_converters):
                path_params.append(
                    {
                        "name": name,
                        "location": "path",
                        "required": True,
                        "schema": path_converters[name],
                        "span": _span(abs_path, stmt),
                    }
                )

            # Response body: the return annotation (a typed class) drives the $ref;
            # NO annotation -> untyped-response diagnostic + responses: [] (rule 3).
            body_ref = None
            if stmt.returns is not None:
                body_ref = _resolves_to_class(stmt.returns, module.dotted, table)

            for method in methods:
                # Body/query facts are derived once per (method, handler): the body
                # walk is identical across methods, but a diagnostic must anchor to the
                # untyped node ONCE — so derive on the FIRST method only and reuse.
                query_params, request_body = _flask_body_and_params(
                    stmt,
                    method,
                    path,
                    module.dotted,
                    abs_path,
                    table,
                    diags if method == methods[0] else _NullDiags(),
                )
                if body_ref is not None:
                    response = {
                        "status": 201 if method == "POST" else 200,
                        "body": {"ref_id": body_ref},
                    }
                    responses = [response]
                else:
                    if method == methods[0]:
                        diags.warn(
                            "untyped response on {} {}: handler has no return "
                            "annotation; response shape under-specified, no schema "
                            "inferred".format(method, path),
                            abs_path,
                            getattr(stmt, "lineno", 0),
                        )
                    responses = []
                routes.append(
                    {
                        "method": method,
                        "path": path,
                        "handler": stmt.name,
                        "operation_id": stmt.name,
                        "params": path_params + query_params,
                        "request_body": request_body,
                        "responses": responses,
                        "span": _span(abs_path, stmt),
                    }
                )
    return routes


class _NullDiags:
    """A no-op diagnostics sink so a per-method re-walk never double-records a warning."""

    def warn(self, *_args, **_kwargs):
        return None
