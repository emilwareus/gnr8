"""Unit-level regression tests for ``pyextract.routes`` edge cases.

These drive the route helpers directly with crafted ASTs (not via the fixture
subprocess) so rule-3 edge cases the hand-tuned fixtures do not exercise are
locked. Currently: CR-03 — a typed query ``AnnAssign`` whose target is not a bare
``Name`` must diagnose + skip, never emit an invalid empty-named param.
"""

import ast
import unittest

from pyextract import routes
from pyextract.diagnostics import Diagnostics
from pyextract.symtab import SymbolTable


class _FakeModule:
    def __init__(self, dotted, source):
        self.dotted = dotted
        self.tree = ast.parse(source)
        self.abs_path = "/virtual/{}.py".format(dotted.replace(".", "/"))


def _func(source):
    """Parse a module source and return its single top-level function def node."""
    mod = ast.parse(source)
    for stmt in mod.body:
        if isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
            return stmt
    raise AssertionError("no function def in source")


class FlaskQueryParamTargetTests(unittest.TestCase):
    """CR-03: a non-Name AnnAssign target for a typed query param must be skipped
    with a diagnostic, never appended as ``"name": ""`` (invalid OpenAPI)."""

    def _run(self, source):
        func = _func(source)
        module = _FakeModule("app.routes", source)
        table = SymbolTable([module])
        diags = Diagnostics()
        params, body = routes._flask_body_and_params(
            func,
            "GET",
            "/",
            "app.routes",
            module.abs_path,
            table,
            diags,
        )
        return params, body, diags

    def test_bare_name_target_emits_named_param(self):
        # Baseline: a bare-Name target still produces a normal query param.
        src = (
            "def handler():\n"
            "    status: str = request.args.get('status')\n"
        )
        params, _body, diags = self._run(src)
        self.assertEqual(len(params), 1)
        self.assertEqual(params[0]["name"], "status")
        self.assertEqual(diags.items(), [])

    def test_attribute_target_skipped_with_diagnostic(self):
        # obj.attr: str = request.args.get(...) — a non-Name target. rule 3: skip + diagnose.
        src = (
            "def handler():\n"
            "    obj.attr: str = request.args.get('status')\n"
        )
        params, _body, diags = self._run(src)
        # No param fabricated.
        self.assertEqual(params, [])
        # ...and absolutely never an empty-named param.
        self.assertFalse(any(p["name"] == "" for p in params))
        items = diags.items()
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0]["severity"], "WARN")
        self.assertIn("non-name target", items[0]["message"])

    def test_subscript_target_skipped_with_diagnostic(self):
        # d['k']: str = request.args.get(...) — a Subscript target.
        src = (
            "def handler():\n"
            "    d['k']: str = request.args.get('status')\n"
        )
        params, _body, diags = self._run(src)
        self.assertEqual(params, [])
        self.assertEqual(len(diags.items()), 1)


class FastAPIKwOnlyParamTests(unittest.TestCase):
    """WR-06: keyword-only params (after ``*``) are common in FastAPI handlers and
    must NOT be silently dropped; required-ness comes from ``kw_defaults``. Also
    positional-only params (before ``/``) must count in default alignment."""

    def _params(self, source, path="/", method="POST"):
        func = _func(source)
        module = _FakeModule("app.main", source)
        table = SymbolTable([module])
        diags = Diagnostics()
        params, _body = routes._build_params(
            func, path, method, "app.main", module.abs_path, table, diags
        )
        return {p["name"]: p for p in params}, diags

    def test_keyword_only_param_is_emitted(self):
        src = (
            "def handler(*, genre: str, sort: str = 'asc'):\n"
            "    pass\n"
        )
        params, _diags = self._params(src)
        # Both kwonly params must appear (not dropped).
        self.assertIn("genre", params)
        self.assertIn("sort", params)
        # genre has no kw_default -> required; sort has one -> not required.
        self.assertTrue(params["genre"]["required"])
        self.assertFalse(params["sort"]["required"])
        self.assertEqual(params["genre"]["location"], "query")

    def test_positional_only_default_alignment(self):
        # def f(a, b, /, c='x') — posonlyargs a,b; args c with one END-aligned default.
        src = (
            "def handler(a: str, b: int, /, c: str = 'x'):\n"
            "    pass\n"
        )
        params, _diags = self._params(src)
        self.assertIn("a", params)
        self.assertIn("b", params)
        self.assertIn("c", params)
        self.assertTrue(params["a"]["required"])
        self.assertTrue(params["b"]["required"])
        self.assertFalse(params["c"]["required"])


class FlaskBodylessMethodTests(unittest.TestCase):
    """WR-04: a GET/HEAD/DELETE handler must never derive a request body fact even
    if it reads request.json (semantically a body-less method)."""

    SRC = (
        "from app.dto import OrderInput\n"
        "def handler() -> int:\n"
        "    order: OrderInput = OrderInput(**request.json)\n"
        "    return 1\n"
    )

    DTO = "class OrderInput:\n    x: int\n"

    def _run_multi(self, method):
        modules = [
            _FakeModule("app.routes", self.SRC),
            _FakeModule("app.dto", self.DTO),
        ]
        func = _func(self.SRC)
        table = SymbolTable(modules)
        diags = Diagnostics()
        return routes._flask_body_and_params(
            func, method, "/", "app.routes", modules[0].abs_path, table, diags
        )

    def test_post_derives_body(self):
        _params, body = self._run_multi("POST")
        self.assertEqual(body, {"ref_id": "app.dto.OrderInput"})

    def test_get_omits_body(self):
        _params, body = self._run_multi("GET")
        self.assertIsNone(body)

    def test_delete_omits_body(self):
        _params, body = self._run_multi("DELETE")
        self.assertIsNone(body)


class FastAPIBodylessMethodTests(unittest.TestCase):
    """A FastAPI model-typed handler param is a request body only on a body-bearing
    method; on GET/HEAD/DELETE it is omitted (no guess) — matching the Flask guard."""

    SRC = (
        "from app.dto import CreateInput\n"
        "def handler(payload: CreateInput):\n"
        "    pass\n"
    )
    DTO = "class CreateInput:\n    x: int\n"

    def _body(self, method):
        modules = [
            _FakeModule("app.main", self.SRC),
            _FakeModule("app.dto", self.DTO),
        ]
        func = _func(self.SRC)
        table = SymbolTable(modules)
        diags = Diagnostics()
        _params, body = routes._build_params(
            func, "/", method, "app.main", modules[0].abs_path, table, diags
        )
        return body

    def test_post_derives_body(self):
        self.assertEqual(self._body("POST"), {"ref_id": "app.dto.CreateInput"})

    def test_get_omits_body(self):
        self.assertIsNone(self._body("GET"))

    def test_delete_omits_body(self):
        self.assertIsNone(self._body("DELETE"))


class StaticPrefixCompositionTests(unittest.TestCase):
    def _recognize_fastapi(self, source):
        module = _FakeModule("app.main", source)
        diags = Diagnostics()
        found = routes.recognize_fastapi([module], SymbolTable([module]), diags)
        return found, diags

    def _recognize_flask(self, source):
        module = _FakeModule("app.routes", source)
        diags = Diagnostics()
        found = routes.recognize_flask([module], SymbolTable([module]), diags)
        return found, diags

    def test_fastapi_constructor_and_include_prefixes_compose(self):
        found, diags = self._recognize_fastapi(
            "app = FastAPI()\n"
            "router = APIRouter(prefix='/books')\n"
            "app.include_router(router, prefix='/v1')\n"
            "@router.get('/')\n"
            "def list_books():\n"
            "    pass\n"
        )
        self.assertEqual(found[0]["path"], "/v1/books/")
        self.assertEqual(diags.items(), [])

    def test_multiple_fastapi_routers_keep_distinct_prefixes(self):
        found, diags = self._recognize_fastapi(
            "books = APIRouter(prefix='/books')\n"
            "users = APIRouter(prefix='/users')\n"
            "@books.get('/{item_id}')\n"
            "def get_book(item_id: str):\n"
            "    pass\n"
            "@users.get('/{item_id}')\n"
            "def get_user(item_id: str):\n"
            "    pass\n"
        )
        self.assertEqual(
            {route["operation_id"]: route["path"] for route in found},
            {"get_book": "/books/{item_id}", "get_user": "/users/{item_id}"},
        )
        self.assertEqual(diags.items(), [])

    def test_flask_constructor_and_registration_prefixes_compose(self):
        found, diags = self._recognize_flask(
            "app = Flask(__name__)\n"
            "bp = Blueprint('books', __name__, url_prefix='/books')\n"
            "app.register_blueprint(bp, url_prefix='/v1')\n"
            "@bp.route('/')\n"
            "def list_books():\n"
            "    pass\n"
        )
        self.assertEqual(found[0]["path"], "/v1/books/")
        self.assertEqual(len(diags.items()), 1)  # untyped response only

    def test_dynamic_fastapi_prefix_is_diagnosed_and_omitted(self):
        found, diags = self._recognize_fastapi(
            "PREFIX = '/books'\n"
            "router = APIRouter(prefix=PREFIX)\n"
            "@router.get('/')\n"
            "def list_books():\n"
            "    pass\n"
        )
        self.assertEqual(found, [])
        self.assertTrue(any("dynamic prefix=" in d["message"] for d in diags.items()))

    def test_cross_module_fastapi_include_prefix_composes(self):
        modules = [
            _FakeModule(
                "app.main",
                "from app.books import router\n"
                "app = FastAPI()\n"
                "app.include_router(router, prefix='/v1')\n",
            ),
            _FakeModule(
                "app.books",
                "router = APIRouter(prefix='/books')\n"
                "@router.get('/')\n"
                "def list_books():\n"
                "    pass\n",
            ),
        ]
        diags = Diagnostics()
        found = routes.recognize_fastapi(modules, SymbolTable(modules), diags)
        self.assertEqual(found[0]["path"], "/v1/books/")

    def test_cross_module_flask_registration_prefix_composes(self):
        modules = [
            _FakeModule(
                "app.main",
                "from app.books import bp\n"
                "app = Flask(__name__)\n"
                "app.register_blueprint(bp, url_prefix='/v1')\n",
            ),
            _FakeModule(
                "app.books",
                "bp = Blueprint('books', __name__, url_prefix='/books')\n"
                "@bp.route('/')\n"
                "def list_books():\n"
                "    pass\n",
            ),
        ]
        diags = Diagnostics()
        found = routes.recognize_flask(modules, SymbolTable(modules), diags)
        self.assertEqual(found[0]["path"], "/v1/books/")


class FastAPIResponseAndDependencyTests(unittest.TestCase):
    MODEL_SOURCE = "class Book(BaseModel):\n    title: str\n"

    def _recognize(self, handler_source):
        modules = [
            _FakeModule("app.main", "app = FastAPI()\n" + handler_source),
            _FakeModule("app.models", self.MODEL_SOURCE),
        ]
        diags = Diagnostics()
        synthetic = []
        found = routes.recognize_fastapi(
            modules, SymbolTable(modules), diags, synthetic
        )
        return found, synthetic, diags

    def test_async_return_annotation_supplies_response_model(self):
        found, synthetic, _diags = self._recognize(
            "from app.models import Book\n"
            "@app.get('/book')\n"
            "async def get_book() -> Book:\n"
            "    pass\n"
        )
        self.assertEqual(
            found[0]["responses"][0]["body"], {"ref_id": "app.models.Book"}
        )
        self.assertEqual(synthetic, [])

    def test_list_return_annotation_synthesizes_array_response_schema(self):
        found, synthetic, _diags = self._recognize(
            "from app.models import Book\n"
            "@app.get('/books')\n"
            "async def list_books() -> list[Book]:\n"
            "    pass\n"
        )
        response_ref = found[0]["responses"][0]["body"]["ref_id"]
        self.assertEqual(len(synthetic), 1)
        self.assertEqual(synthetic[0]["id"], response_ref)
        self.assertEqual(
            synthetic[0]["body"],
            {"type": "array", "of": {"type": "named", "of": "app.models.Book"}},
        )

    def test_list_response_model_synthesizes_array_response_schema(self):
        found, synthetic, _diags = self._recognize(
            "from app.models import Book\n"
            "@app.get('/books', response_model=list[Book])\n"
            "async def list_books():\n"
            "    pass\n"
        )
        self.assertEqual(len(synthetic), 1)
        self.assertEqual(
            found[0]["responses"][0]["body"], {"ref_id": synthetic[0]["id"]}
        )
        self.assertEqual(synthetic[0]["body"]["type"], "array")

    def test_depends_default_is_not_a_request_body_or_query_param(self):
        found, _synthetic, _diags = self._recognize(
            "from app.models import Book\n"
            "@app.post('/books')\n"
            "async def create_book(service: Book = Depends()):\n"
            "    pass\n"
        )
        self.assertIsNone(found[0]["request_body"])
        self.assertEqual(found[0]["params"], [])

    def test_annotated_depends_is_not_a_request_body_or_query_param(self):
        found, _synthetic, _diags = self._recognize(
            "from app.models import Book\n"
            "@app.post('/books')\n"
            "async def create_book(service: Annotated[Book, Depends()]):\n"
            "    pass\n"
        )
        self.assertIsNone(found[0]["request_body"])
        self.assertEqual(found[0]["params"], [])


if __name__ == "__main__":
    unittest.main()
