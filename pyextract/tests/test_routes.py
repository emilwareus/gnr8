"""Tests for ``pyextract.routes`` — FastAPI route/param/body/response recognition.

Asserts the four FastAPI bookstore routes' methods, paths, params (path/query split,
required-ness, the named-ref enum param), request bodies and responses (the 201 status,
the union response) against golden dicts derived from the committed FastAPI graph
snapshot. Also re-asserts the load-bearing invariants: no router prefix is folded into
any path (rule 1) and the sidecar is static-only (no exec/import of the target).

Route SHAPES are asserted here; exact span LINES are guarded in test_fastapi_golden.py.
"""

import json
import os
import subprocess
import sys
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(REPO_ROOT, "fixtures", "fastapi-bookstore")

ROUTE_KEYS = {
    "method",
    "path",
    "handler",
    "operation_id",
    "params",
    "request_body",
    "responses",
    "span",
}
PARAM_KEYS = {"name", "location", "required", "schema", "span"}

_STRING = {"type": "primitive", "of": {"prim": "string"}}
_INT64 = {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}}


def _extract(target):
    out = subprocess.run(
        [sys.executable, "-m", "pyextract", target],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return json.loads(out.stdout.decode("utf-8"))


class RouteRecognitionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = _extract(FIXTURE)
        cls.by = {r["operation_id"]: r for r in cls.doc["routes"]}

    def _param(self, route, name):
        for p in route["params"]:
            if p["name"] == name:
                return p
        self.fail("param {!r} not found on {!r}".format(name, route["operation_id"]))

    def test_exactly_the_four_routes(self):
        self.assertEqual(
            sorted(self.by),
            ["create_book", "get_book", "list_books", "update_book"],
        )

    def test_route_key_set_matches_contract(self):
        for route in self.doc["routes"]:
            self.assertEqual(set(route), ROUTE_KEYS, route["operation_id"])
            for param in route["params"]:
                self.assertEqual(set(param), PARAM_KEYS, param["name"])

    def test_list_books_methods_path_and_query_params(self):
        r = self.by["list_books"]
        self.assertEqual(r["method"], "GET")
        self.assertEqual(r["path"], "/")
        self.assertIsNone(r["request_body"])
        # genre: required (no default); sort/cursor: not required (have defaults).
        self.assertTrue(self._param(r, "genre")["required"])
        self.assertEqual(self._param(r, "genre")["location"], "query")
        self.assertEqual(self._param(r, "genre")["schema"], _STRING)
        self.assertFalse(self._param(r, "sort")["required"])
        self.assertFalse(self._param(r, "cursor")["required"])
        self.assertEqual(
            r["responses"],
            [{"status": 200, "body": {"ref_id": "app.models.ListBooksResponse"}}],
        )

    def test_create_book_body_and_201_status(self):
        r = self.by["create_book"]
        self.assertEqual(r["method"], "POST")
        self.assertEqual(r["path"], "/")
        self.assertEqual(r["request_body"], {"ref_id": "app.models.Book"})
        self.assertEqual(
            r["responses"],
            [{"status": 201, "body": {"ref_id": "app.models.CreatedMessage"}}],
        )

    def test_get_book_path_param_named_ref_query_and_union_response(self):
        r = self.by["get_book"]
        self.assertEqual(r["method"], "GET")
        self.assertEqual(r["path"], "/{book_id}")
        book_id = self._param(r, "book_id")
        self.assertEqual(book_id["location"], "path")
        self.assertTrue(book_id["required"])
        self.assertEqual(book_id["schema"], _INT64)
        fmt = self._param(r, "fmt")
        self.assertEqual(fmt["location"], "query")
        self.assertFalse(fmt["required"])
        self.assertEqual(
            fmt["schema"], {"type": "named", "of": "app.models.BookFormat"}
        )
        self.assertEqual(
            r["responses"],
            [{"status": 200, "body": {"ref_id": "app.models.BookOrError"}}],
        )

    def test_update_book_path_param_and_body(self):
        r = self.by["update_book"]
        self.assertEqual(r["method"], "PUT")
        self.assertEqual(r["path"], "/{book_id}")
        self.assertEqual(self._param(r, "book_id")["location"], "path")
        self.assertEqual(r["request_body"], {"ref_id": "app.models.BookFilters"})
        self.assertEqual(
            r["responses"],
            [{"status": 200, "body": {"ref_id": "app.models.CreatedMessage"}}],
        )

    def test_router_prefix_never_folded_into_path(self):
        # APIRouter(prefix="/books") must NOT be folded into any code-derived path
        # (rule 1): every emitted path is "/" or "/{book_id}", never "/books...".
        for route in self.doc["routes"]:
            self.assertIn(route["path"], ("/", "/{book_id}"), route["operation_id"])

    def test_static_only_gate(self):
        # The recognizer must never exec/eval/compile/import the target (PYSRC-03).
        pyextract_dir = os.path.join(REPO_ROOT, "pyextract")
        forbidden = ("exec(", "eval(", "compile(", "importlib", "__import__", "runpy")
        for name in os.listdir(pyextract_dir):
            if not name.endswith(".py"):
                continue
            with open(os.path.join(pyextract_dir, name), encoding="utf-8") as fh:
                text = fh.read()
            for token in forbidden:
                self.assertNotIn(token, text, "{} in {}".format(token, name))


if __name__ == "__main__":
    unittest.main()
