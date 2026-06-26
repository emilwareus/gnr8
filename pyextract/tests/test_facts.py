"""Tests for ``pyextract.facts`` (deterministic marshal) + the end-to-end harness.

Asserts: the marshal sorts arrays + keys and is byte-identical on re-marshal; each
fact's key set matches the contract (so the host's ``deny_unknown_fields`` would
accept it); and ``python3 -m pyextract fixtures/fastapi-bookstore`` emits a
deterministic facts document whose ``schemas`` section matches the FastAPI graph
snapshot's schema shapes.

ROUTES are intentionally empty until Plan 03 (FastAPI recognizer); this is asserted,
not a defect.
"""

import json
import os
import subprocess
import sys
import unittest

from pyextract import facts

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(REPO_ROOT, "fixtures", "fastapi-bookstore")

# Contract key sets (mirror crates/gnr8-core/src/analyze/facts.rs).
DOC_KEYS = {"module", "routes", "schemas", "diagnostics"}
SCHEMA_KEYS = {"id", "name", "body", "span"}
FIELD_KEYS = {
    "json_name",
    "required",
    "optional",
    "nullable",
    "schema",
    "description",
    "example",
}
SPAN_KEYS = {"file", "start_line", "end_line"}
DIAG_KEYS = {"severity", "message", "file", "line"}


def _sample_doc():
    return facts.build_doc(
        module="m",
        routes=[],
        schemas=[
            {
                "id": "m.B",
                "name": "B",
                "body": {"type": "enum", "of": ["z", "a", "m"]},
                "span": {"file": "m.py", "start_line": 2, "end_line": 2},
            },
            {
                "id": "m.A",
                "name": "A",
                "body": {
                    "type": "object",
                    "of": [
                        {
                            "json_name": "y",
                            "required": True,
                            "optional": False,
                            "nullable": False,
                            "schema": {"type": "primitive", "of": {"prim": "string"}},
                            "description": None,
                            "example": None,
                        },
                        {
                            "json_name": "x",
                            "required": True,
                            "optional": False,
                            "nullable": False,
                            "schema": {"type": "primitive", "of": {"prim": "bool"}},
                            "description": None,
                            "example": None,
                        },
                    ],
                },
                "span": {"file": "m.py", "start_line": 1, "end_line": 1},
            },
        ],
        diagnostics=[
            {"severity": "WARN", "message": "b", "file": "m.py", "line": 9},
            {"severity": "WARN", "message": "a", "file": "m.py", "line": 9},
        ],
    )


class MarshalTests(unittest.TestCase):
    def test_parses_as_json(self):
        out = facts.marshal(_sample_doc())
        json.loads(out)  # must not raise

    def test_re_marshal_is_byte_identical(self):
        first = facts.marshal(_sample_doc())
        second = facts.marshal(_sample_doc())
        self.assertEqual(first, second)

    def test_arrays_are_sorted(self):
        doc = json.loads(facts.marshal(_sample_doc()))
        # schemas by id
        self.assertEqual([s["id"] for s in doc["schemas"]], ["m.A", "m.B"])
        # object fields by json_name
        a = next(s for s in doc["schemas"] if s["id"] == "m.A")
        self.assertEqual([f["json_name"] for f in a["body"]["of"]], ["x", "y"])
        # enum members lexically
        b = next(s for s in doc["schemas"] if s["id"] == "m.B")
        self.assertEqual(b["body"]["of"], ["a", "m", "z"])
        # diagnostics by (file, line, message)
        self.assertEqual([d["message"] for d in doc["diagnostics"]], ["a", "b"])

    def test_union_members_keep_source_order(self):
        doc = facts.build_doc(
            "m",
            [],
            [
                {
                    "id": "m.U",
                    "name": "U",
                    "body": {
                        "type": "union",
                        "of": [
                            {"type": "named", "of": "m.Z"},
                            {"type": "named", "of": "m.A"},
                        ],
                    },
                    "span": {"file": "m.py", "start_line": 1, "end_line": 1},
                }
            ],
            [],
        )
        out = json.loads(facts.marshal(doc))
        members = [m["of"] for m in out["schemas"][0]["body"]["of"]]
        # NOT sorted — source order preserved (Z before A).
        self.assertEqual(members, ["m.Z", "m.A"])


class FixtureEndToEndTests(unittest.TestCase):
    """Run the sidecar as a subprocess over the real FastAPI fixture."""

    def _run(self):
        proc = subprocess.run(
            [sys.executable, "-m", "pyextract", FIXTURE],
            cwd=REPO_ROOT,
            capture_output=True,
        )
        self.assertEqual(
            proc.returncode,
            0,
            "pyextract exited nonzero: {}".format(proc.stderr.decode("utf-8")),
        )
        return proc.stdout

    def test_module_and_schema_ids_match_snapshot(self):
        doc = json.loads(self._run())
        self.assertEqual(doc["module"], "fastapi-bookstore")
        ids = [s["id"] for s in doc["schemas"]]
        # The exact schema-id set from snapshot_fastapi_graph (no SortOrder: a
        # Literal alias is inlined, never a standalone schema).
        self.assertEqual(
            ids,
            [
                "app.models.Author",
                "app.models.Book",
                "app.models.BookFilters",
                "app.models.BookFormat",
                "app.models.BookOrError",
                "app.models.CreatedMessage",
                "app.models.ListBooksResponse",
                "app.models.OutOfStock",
            ],
        )

    def test_routes_recognized_for_fastapi(self):
        # Plan 03 lands FastAPI route recognition: the four bookstore routes appear
        # (detailed shapes are asserted in test_routes.py / test_fastapi_golden.py).
        doc = json.loads(self._run())
        ids = sorted(r["operation_id"] for r in doc["routes"])
        self.assertEqual(
            ids, ["create_book", "get_book", "list_books", "update_book"]
        )

    def test_type_and_axis_shapes_match_snapshot(self):
        doc = json.loads(self._run())
        by = {s["id"]: s for s in doc["schemas"]}

        # BookFormat: enum, members SORTED.
        self.assertEqual(
            by["app.models.BookFormat"]["body"],
            {"type": "enum", "of": ["hardcover", "paperback"]},
        )
        # BookOrError: union of two named refs in SOURCE order.
        self.assertEqual(
            by["app.models.BookOrError"]["body"],
            {
                "type": "union",
                "of": [
                    {"type": "named", "of": "app.models.Book"},
                    {"type": "named", "of": "app.models.OutOfStock"},
                ],
            },
        )
        # BookFilters: the four-axis matrix + inline Literal enum on sort.
        fields = {
            f["json_name"]: f for f in by["app.models.BookFilters"]["body"]["of"]
        }
        self.assertEqual(
            (fields["genre"]["optional"], fields["genre"]["nullable"]), (False, False)
        )
        self.assertEqual(
            (fields["in_stock"]["optional"], fields["in_stock"]["nullable"]),
            (True, False),
        )
        self.assertEqual(
            (fields["published"]["optional"], fields["published"]["nullable"]),
            (False, True),
        )
        self.assertEqual(
            (fields["sort"]["optional"], fields["sort"]["nullable"]), (True, True)
        )
        self.assertEqual(
            fields["sort"]["schema"], {"type": "enum", "of": ["asc", "desc"]}
        )
        # int -> int64 signed; float -> float64 (Book.id + Book.rating union).
        book = {f["json_name"]: f for f in by["app.models.Book"]["body"]["of"]}
        self.assertEqual(
            book["id"]["schema"],
            {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}},
        )
        self.assertEqual(
            book["rating"]["schema"]["of"],
            [
                {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}},
                {"type": "primitive", "of": {"prim": "float", "bits": 64}},
            ],
        )

    def test_output_is_byte_identical_across_two_runs(self):
        self.assertEqual(self._run(), self._run())

    def test_every_fact_key_set_matches_contract(self):
        doc = json.loads(self._run())
        self.assertEqual(set(doc.keys()), DOC_KEYS)
        for schema in doc["schemas"]:
            self.assertEqual(set(schema.keys()), SCHEMA_KEYS)
            self.assertEqual(set(schema["span"].keys()), SPAN_KEYS)
            self._check_body_keys(schema["body"])
        for diag in doc["diagnostics"]:
            self.assertEqual(set(diag.keys()), DIAG_KEYS)

    def _check_body_keys(self, body):
        self.assertEqual(set(body.keys()), {"type", "of"})
        if body["type"] == "object":
            for field in body["of"]:
                self.assertEqual(set(field.keys()), FIELD_KEYS)
                self._check_body_keys(field["schema"])
        elif body["type"] in ("union",):
            for member in body["of"]:
                self._check_body_keys(member)
        elif body["type"] == "array":
            self._check_body_keys(body["of"])
        elif body["type"] == "map":
            self._check_body_keys(body["of"]["key"])
            self._check_body_keys(body["of"]["value"])


if __name__ == "__main__":
    unittest.main()
