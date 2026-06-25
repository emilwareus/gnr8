"""Golden line-provenance guard for the Flask bookstore fixture (RESEARCH OQ2 / A5).

Runs the sidecar over `fixtures/flask-bookstore` and asserts that the produced
provenance `start_line` for every ROUTE, every route PARAM, and every SCHEMA — AND
each of the three untyped-surface DIAGNOSTIC lines — equals the value the committed
Rust snapshot (`snapshot_flask_graph__flask_graph.snap`) asserts. This guards line
equality independently of the Rust snapshot harness: a fixture edit that drifts any
anchor fails here in pure Python before the Rust snapshot ever runs.

The expected lines below ARE the snapshot's values — the snapshot is the byte-exact
spec; this test mirrors its line numbers, never the reverse. The three diagnostic
lines (42 / 69 / 78) resolve RESEARCH OQ2: each diagnostic anchors to its precise AST
node (the `request.args.get("q")` read, the untyped `def`, the `request.json` read),
reconciled into the fixture as pure source positions (rule 1).
"""

import json
import os
import subprocess
import sys
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(REPO_ROOT, "fixtures", "flask-bookstore")

# Route def-line anchors (snapshot routes.py spans).
EXPECTED_ROUTE_LINES = {
    "list_orders": 31,
    "create_order": 48,
    "create_order_raw": 69,
    "get_order": 59,
}
# Per-param anchors: the typed query AnnAssign line + the converter path-param line.
EXPECTED_PARAM_LINES = {
    ("list_orders", "status"): 41,
    ("get_order", "order_id"): 59,
}
# Schema ClassDef-line anchors (snapshot dto.py spans).
EXPECTED_SCHEMA_LINES = {
    "app.dto.Availability": 31,
    "app.dto.OrderConfirmation": 96,
    "app.dto.OrderInput": 60,
    "app.dto.Price": 49,
}
# The three untyped-surface diagnostic lines (OQ2 resolution).
EXPECTED_DIAGNOSTIC_LINES = [42, 69, 78]


def _extract(target):
    out = subprocess.run(
        [sys.executable, "-m", "pyextract", target],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return json.loads(out.stdout.decode("utf-8"))


class FlaskGoldenLineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = _extract(FIXTURE)
        cls.routes = {r["operation_id"]: r for r in cls.doc["routes"]}
        cls.schemas = {s["id"]: s for s in cls.doc["schemas"]}

    def test_route_span_lines_match_snapshot(self):
        for oid, line in EXPECTED_ROUTE_LINES.items():
            self.assertIn(oid, self.routes)
            self.assertEqual(self.routes[oid]["span"]["start_line"], line, oid)

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

    def test_diagnostic_lines_are_42_69_78(self):
        lines = sorted(d["line"] for d in self.doc["diagnostics"])
        self.assertEqual(lines, EXPECTED_DIAGNOSTIC_LINES)

    def test_fixture_keeps_all_four_axes_and_the_union(self):
        # The reconciliation must not have damaged the acceptance vocabulary. In the
        # Flask fixture the four optional x nullable combinations are split across the
        # request DTO (OrderInput) and the response DTO (OrderConfirmation) — the
        # "nullable only" (F, T) axis lives on OrderConfirmation.message (required,
        # no default, Optional[str]); the other three live on OrderInput.
        order_input = {
            f["json_name"]: f
            for f in self.schemas["app.dto.OrderInput"]["body"]["of"]
        }
        confirmation = {
            f["json_name"]: f
            for f in self.schemas["app.dto.OrderConfirmation"]["body"]["of"]
        }
        axes = {
            (f["optional"], f["nullable"])
            for f in list(order_input.values()) + list(confirmation.values())
        }
        self.assertEqual(
            axes, {(False, False), (True, False), (False, True), (True, True)}
        )
        # The "nullable only" axis is specifically OrderConfirmation.message.
        self.assertEqual(
            (confirmation["message"]["optional"], confirmation["message"]["nullable"]),
            (False, True),
        )
        self.assertEqual(
            order_input["discount"]["schema"],
            {
                "type": "union",
                "of": [
                    {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}},
                    {"type": "primitive", "of": {"prim": "float", "bits": 64}},
                ],
            },
        )


if __name__ == "__main__":
    unittest.main()
