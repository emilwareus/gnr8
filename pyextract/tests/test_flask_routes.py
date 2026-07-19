"""Flask route recognition tests — the HONEST typed envelope (Plan 04).

Runs the sidecar over `fixtures/flask-bookstore` and asserts the four routes, the
`<int:order_id>` converter path, the per-method split, the METHOD-DERIVED statuses
(POST -> 201, GET -> 200), the empty `responses` on the untyped raw route, and the
three untyped-surface diagnostics' EXACT message strings (rule 3: untyped -> diagnostic
+ omit, never a guess). The diagnostic strings here are the committed snapshot's
strings byte-for-byte — the snapshot is the spec; this test mirrors it.
"""

import json
import os
import subprocess
import sys
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(REPO_ROOT, "fixtures", "flask-bookstore")

EXPECTED_DIAGS = [
    "untyped query param 'q' on GET /orders/: read via request.args.get with no "
    "annotation; param type/required-ness under-specified, type inferred as "
    "string only",
    "untyped response on POST /orders/raw: handler has no return annotation; response "
    "shape under-specified, no schema inferred",
    "untyped request body on POST /orders/raw: read via request.json with no typed DTO; "
    "body shape under-specified, no schema inferred",
]


def _extract(target):
    out = subprocess.run(
        [sys.executable, "-m", "pyextract", target],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return json.loads(out.stdout.decode("utf-8"))


class FlaskRouteTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = _extract(FIXTURE)
        cls.routes = {r["operation_id"]: r for r in cls.doc["routes"]}

    def test_the_four_routes_are_recognized(self):
        self.assertEqual(
            set(self.routes),
            {"list_orders", "create_order", "create_order_raw", "get_order"},
        )

    def test_blueprint_prefix_is_folded_into_path(self):
        self.assertEqual(self.routes["list_orders"]["path"], "/orders/")
        self.assertEqual(self.routes["create_order"]["path"], "/orders/")
        self.assertEqual(self.routes["create_order_raw"]["path"], "/orders/raw")
        self.assertEqual(self.routes["get_order"]["path"], "/orders/{order_id}")

    def test_methods_and_per_method_split(self):
        self.assertEqual(self.routes["list_orders"]["method"], "GET")
        self.assertEqual(self.routes["create_order"]["method"], "POST")
        self.assertEqual(self.routes["create_order_raw"]["method"], "POST")
        self.assertEqual(self.routes["get_order"]["method"], "GET")
        # `/` has a GET and a POST handler -> two distinct operations, one per method.
        slash_ops = sorted(
            r["operation_id"]
            for r in self.routes.values()
            if r["path"] == "/orders/"
        )
        self.assertEqual(slash_ops, ["create_order", "list_orders"])

    def test_int_converter_path_param(self):
        params = self.routes["get_order"]["params"]
        self.assertEqual(len(params), 1)
        p = params[0]
        self.assertEqual(p["name"], "order_id")
        self.assertEqual(p["location"], "path")
        self.assertTrue(p["required"])
        self.assertEqual(
            p["schema"],
            {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}},
        )

    def test_method_derived_statuses(self):
        # POST typed -> 201; GET typed -> 200 (a CODE fact, never a docstring).
        self.assertEqual(self.routes["create_order"]["responses"][0]["status"], 201)
        self.assertEqual(self.routes["list_orders"]["responses"][0]["status"], 200)
        self.assertEqual(self.routes["get_order"]["responses"][0]["status"], 200)

    def test_typed_response_and_body_refs(self):
        self.assertEqual(
            self.routes["create_order"]["request_body"],
            {"ref_id": "app.dto.OrderInput"},
        )
        for oid in ("list_orders", "create_order", "get_order"):
            self.assertEqual(
                self.routes[oid]["responses"][0]["body"],
                {"ref_id": "app.dto.OrderConfirmation"},
            )

    def test_typed_query_param_on_list_orders(self):
        params = {p["name"]: p for p in self.routes["list_orders"]["params"]}
        # `status: str = request.args.get(...)` -> a typed query param fact.
        self.assertIn("status", params)
        self.assertEqual(params["status"]["location"], "query")
        self.assertFalse(params["status"]["required"])
        self.assertEqual(
            params["status"]["schema"],
            {"type": "primitive", "of": {"prim": "string"}},
        )
        # `q = request.args.get(...)` (unannotated) -> NO param fact (rule 3).
        self.assertNotIn("q", params)

    def test_untyped_raw_route_omits_facts(self):
        raw = self.routes["create_order_raw"]
        self.assertEqual(raw["responses"], [])  # no return annotation -> no response
        self.assertIsNone(raw["request_body"])  # raw request.json -> no body
        self.assertEqual(raw["params"], [])

    def test_diagnostic_messages_match_snapshot_byte_for_byte(self):
        messages = [d["message"] for d in self.doc["diagnostics"]]
        for expected in EXPECTED_DIAGS:
            self.assertIn(expected, messages)
        self.assertEqual(len(self.doc["diagnostics"]), 3)


if __name__ == "__main__":
    unittest.main()
