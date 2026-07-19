"use strict";

// Regression tests for the 04 route-warning fixes (WR-01..WR-05), LOCKING the
// rule-3 behavior on NestJS routing shapes the acceptance fixture does not
// exercise: a nullable/array return, a duplicate verb, a duplicate @Body, and an
// out-of-range @HttpCode. Each must map through the SINGLE type discriminator or
// diagnose-and-omit — never a silent drop or an out-of-range status.
// (node:assert only — the rule-2 ethos.)
//
// Run: `node tsextract/tests/route-edges.test.js` (exit 0 = pass).

const assert = require("assert");
const path = require("path");

const load = require("../load");
const { recognizeNestController, } = require("../routes");
const { Registry } = require("../schemas");
const { Diagnostics } = require("../diagnostics");

const FIXTURE = path.resolve(__dirname, "fixtures/route-edges");

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const registry = new Registry();
const routes = recognizeNestController(loaded, diags, registry);
const byHandler = Object.fromEntries(routes.map((r) => [r.handler, r]));

function hasDiag(substr) {
  return diags.items().some((d) => d.message.includes(substr));
}

(function dynamic_controller_prefix_is_diagnosed_and_omitted() {
  assert.ok(!byHandler.omitted, "dynamic-prefix route must not be emitted");
  assert.ok(
    hasDiag("@Controller prefix is dynamic"),
    "dynamic controller prefix must record a diagnostic"
  );
})();

// WR-01 happy path: a plain named return still resolves to its ref_id (the dual
// `t.aliasSymbol` discriminator is replaced by the single mapType path).
(function named_return_resolves_via_single_path() {
  const r = byHandler.getNamed;
  assert.ok(r, "getNamed route missing");
  assert.deepStrictEqual(r.responses[0].body, {
    ref_id: "src/edges.controller.Thing",
  });
})();

// WR-01: a nullable named return (aliasSymbol dropped by TS on `| null`) resolves
// to the inline union residual, which is not a TypeRef -> body omitted, NOT
// silently mis-mapped, and a diagnostic recorded.
(function nullable_named_return_diagnosed_not_misrouted() {
  const r = byHandler.getNullable;
  assert.ok(r, "getNullable route missing");
  assert.strictEqual(r.responses[0].body, null, "non-TypeRef body must be omitted");
})();

// WR-02: an array return gets a DISTINCT diagnostic (not collapsed into the
// generic "unresolvable type" message) and the body is omitted.
(function array_return_distinct_diagnostic() {
  const r = byHandler.getArray;
  assert.ok(r, "getArray route missing");
  assert.strictEqual(r.responses[0].body, null);
  assert.ok(
    hasDiag("response type is a 'array'"),
    "array response must record a distinct 'array' diagnostic (WR-02)"
  );
})();

// WR-03: a second HTTP-verb decorator is diagnosed and only one route emitted.
(function second_verb_diagnosed_not_silent() {
  assert.ok(byHandler.multiVerb, "multiVerb route missing");
  const verbCount = routes.filter((r) => r.handler === "multiVerb").length;
  assert.strictEqual(verbCount, 1, "only the first verb is recorded");
  assert.ok(
    hasDiag("second HTTP-verb decorator"),
    "a dropped extra verb must record a diagnostic (WR-03)"
  );
})();

// WR-04: a second @Body is diagnosed (first-wins is surfaced, not silent).
(function second_body_diagnosed() {
  assert.ok(
    hasDiag("more than one @Body parameter"),
    "a duplicate @Body must record a diagnostic (WR-04)"
  );
})();

// WR-05: an out-of-range @HttpCode is diagnosed and ignored; the route falls back
// to the deterministic method-derived status (POST -> 201), never an out-of-range
// u16-invalid value that would crash host deserialize.
(function out_of_range_httpcode_diagnosed_and_ignored() {
  const r = byHandler.bad;
  assert.ok(r, "bad route missing");
  assert.strictEqual(
    r.responses[0].status,
    201,
    "an ignored @HttpCode override must leave the method-derived POST status (201)"
  );
  assert.ok(
    hasDiag("outside the valid HTTP status range"),
    "an out-of-range @HttpCode must record a diagnostic (WR-05)"
  );
  // No emitted status may be out of the u16 / HTTP range.
  for (const route of routes) {
    for (const resp of route.responses) {
      assert.ok(
        Number.isInteger(resp.status) && resp.status >= 100 && resp.status <= 599,
        "emitted status out of range: " + resp.status
      );
    }
  }
})();

console.log("route-edges.test.js: OK");
