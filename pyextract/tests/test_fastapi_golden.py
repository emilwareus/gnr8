"""Golden line-provenance guard for the FastAPI bookstore fixture (RESEARCH OQ2 / A5).

Runs the sidecar over `fixtures/fastapi-bookstore` and asserts that the produced
provenance `start_line` for every ROUTE, every route PARAM, and every SCHEMA equals
the value the committed Rust snapshot
(`snapshot_fastapi_graph__fastapi_graph.snap`) asserts. This guards line equality
independently of the Rust snapshot harness: if a fixture edit drifts any anchor line,
this test fails in pure Python before the Rust snapshot ever runs.

The expected lines below ARE the snapshot's values — the snapshot is the byte-exact
spec; this test mirrors its line numbers, never the reverse.
"""

import json
import os
import subprocess
import sys
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(REPO_ROOT, "fixtures", "fastapi-bookstore")

# Route def-line + per-param signature-line anchors (snapshot main.py spans).
EXPECTED_ROUTE_LINES = {
    "list_books": 38,
    "create_book": 57,
    "get_book": 65,
    "update_book": 75,
}
EXPECTED_PARAM_LINES = {
    ("list_books", "genre"): 39,
    ("list_books", "sort"): 40,
    ("list_books", "cursor"): 41,
    ("get_book", "book_id"): 66,
    ("get_book", "fmt"): 66,
    ("update_book", "book_id"): 76,
}
# Schema ClassDef / Assign-line anchors (snapshot models.py spans).
EXPECTED_SCHEMA_LINES = {
    "app.models.Author": 53,
    "app.models.Book": 64,
    "app.models.BookFilters": 95,
    "app.models.BookFormat": 33,
    "app.models.BookOrError": 130,
    "app.models.CreatedMessage": 113,
    "app.models.ListBooksResponse": 135,
    "app.models.OutOfStock": 124,
}


def _extract(target):
    out = subprocess.run(
        [sys.executable, "-m", "pyextract", target],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return json.loads(out.stdout.decode("utf-8"))


class FastapiGoldenLineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = _extract(FIXTURE)
        cls.routes = {r["operation_id"]: r for r in cls.doc["routes"]}
        cls.schemas = {s["id"]: s for s in cls.doc["schemas"]}

    def test_route_span_lines_match_snapshot(self):
        for oid, line in EXPECTED_ROUTE_LINES.items():
            self.assertIn(oid, self.routes)
            self.assertEqual(
                self.routes[oid]["span"]["start_line"], line, oid
            )

    def test_param_span_lines_match_snapshot(self):
        for (oid, pname), line in EXPECTED_PARAM_LINES.items():
            params = {p["name"]: p for p in self.routes[oid]["params"]}
            self.assertIn(pname, params, "{}.{}".format(oid, pname))
            self.assertEqual(
                params[pname]["span"]["start_line"],
                line,
                "{}.{}".format(oid, pname),
            )

    def test_schema_span_lines_match_snapshot(self):
        self.assertEqual(set(self.schemas), set(EXPECTED_SCHEMA_LINES))
        for sid, line in EXPECTED_SCHEMA_LINES.items():
            self.assertEqual(self.schemas[sid]["span"]["start_line"], line, sid)

    def test_fixture_keeps_all_four_axes_and_the_union(self):
        # The reconciliation must not have damaged the acceptance vocabulary.
        filters = {
            f["json_name"]: f
            for f in self.schemas["app.models.BookFilters"]["body"]["of"]
        }
        axes = {(f["optional"], f["nullable"]) for f in filters.values()}
        self.assertEqual(
            axes, {(False, False), (True, False), (False, True), (True, True)}
        )
        self.assertEqual(
            self.schemas["app.models.BookOrError"]["body"],
            {
                "type": "union",
                "of": [
                    {"type": "named", "of": "app.models.Book"},
                    {"type": "named", "of": "app.models.OutOfStock"},
                ],
            },
        )


if __name__ == "__main__":
    unittest.main()
