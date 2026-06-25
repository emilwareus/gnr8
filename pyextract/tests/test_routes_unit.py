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

    def _params(self, source, path="/"):
        func = _func(source)
        module = _FakeModule("app.main", source)
        table = SymbolTable([module])
        diags = Diagnostics()
        params, _body = routes._build_params(
            func, path, "app.main", module.abs_path, table, diags
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


if __name__ == "__main__":
    unittest.main()
