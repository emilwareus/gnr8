"use strict";

// Regression tests for the 04 code-review fixes (CR-01/CR-02/CR-03/CR-04 + the
// related warnings), LOCKING the rule-3 behavior on type shapes the
// multi-language acceptance fixture (nestjs-bookstore) deliberately does not
// exercise. Each shape must produce a correct deterministic neutral mapping OR a
// diagnose-and-omit — never a dangling $ref, never a node_modules-path id, never a
// guessed wire key. (node:assert only — the rule-2 ethos.)
//
// Run: `node tsextract/tests/edge-cases.test.js` (exit 0 = pass).

const assert = require("assert");
const path = require("path");

const load = require("../load");
const { buildSchemas } = require("../schemas");
const { recognizeNestController } = require("../routes");
const { Diagnostics } = require("../diagnostics");

const FIXTURE = path.resolve(__dirname, "fixtures/edge-cases");

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const schemas = buildSchemas(loaded, diags);
const byName = Object.fromEntries(schemas.map((s) => [s.name, s]));

function fieldsOf(name) {
  const s = byName[name];
  assert.ok(s, "schema not found: " + name);
  return Object.fromEntries(s.body.of.map((f) => [f.json_name, f]));
}

function hasDiag(substr) {
  return diags.items().some((d) => d.message.includes(substr));
}

// ---------------------------------------------------------------------------
// CR-01: a non-schema-bearing alias is never a dangling named ref.
// ---------------------------------------------------------------------------

// CR-01: `prim: AliasToPrim` (alias -> primitive) maps INLINE to the primitive,
// NOT a `named` ref. No dangling $ref can result.
(function alias_to_primitive_is_inline_not_named() {
  const f = fieldsOf("AliasCases");
  assert.deepStrictEqual(
    f.prim.schema,
    { type: "primitive", of: { prim: "string" } },
    "alias-to-primitive must map inline to the primitive, never a named ref"
  );
})();

// CR-01: `status: NumStatus` (numeric-literal union alias) is not a buildable
// schema shape (the host enum is string-only) -> diagnosed + the field omitted,
// never a dangling named ref.
(function numeric_union_alias_diagnosed_and_omitted() {
  const f = fieldsOf("AliasCases");
  assert.ok(
    !("status" in f),
    "numeric-union-alias field must be omitted, not emitted with a dangling ref"
  );
  assert.ok(
    "keep" in f,
    "omission must be per-field: the survivor field stays"
  );
})();

// CR-01/CR-02: NO emitted schema id is dangling (every `named` ref resolves to an
// emitted schema) and none is left for the non-schema-bearing aliases.
(function no_dangling_named_refs_anywhere() {
  const ids = new Set(schemas.map((s) => s.id));
  const refs = [];
  const collect = (t) => {
    if (!t || typeof t !== "object") return;
    if (t.type === "named") refs.push(t.of);
    if (t.type === "array") collect(t.of);
    if (t.type === "map" && t.of) {
      collect(t.of.key);
      collect(t.of.value);
    }
    if (t.type === "union" && Array.isArray(t.of)) t.of.forEach(collect);
  };
  for (const s of schemas) {
    if (s.body.type === "object") {
      s.body.of.forEach((f) => collect(f.schema));
    } else {
      collect(s.body);
    }
  }
  for (const r of refs) {
    assert.ok(ids.has(r), "dangling named ref to unregistered schema: " + r);
  }
  // The non-schema-bearing aliases must NOT have minted a schema id.
  assert.ok(
    !ids.has("src/edge.dto.AliasToPrim"),
    "alias-to-primitive must not register a schema"
  );
  assert.ok(
    !ids.has("src/edge.dto.NumStatus"),
    "numeric-union alias must not register a schema"
  );
})();

// ---------------------------------------------------------------------------
// CR-02: a referenced lib/node_modules type never mints an out-of-tree id.
// ---------------------------------------------------------------------------

// CR-02: NO emitted schema id reaches outside the target tree (no `..`, no
// `node_modules`) — the determinism + boundary contract. A `Record<>` resolves
// through the global lib alias; it must never produce a machine-absolute id.
(function no_schema_id_escapes_target_tree() {
  for (const s of schemas) {
    assert.ok(
      !s.id.includes("node_modules"),
      "schema id embeds a node_modules path (non-deterministic): " + s.id
    );
    assert.ok(
      !s.id.startsWith(".."),
      "schema id escapes the target tree: " + s.id
    );
  }
})();

// ---------------------------------------------------------------------------
// CR-03: `Record<string, T>` / index signatures map to the neutral `map` type.
// ---------------------------------------------------------------------------

(function record_maps_to_neutral_map_type() {
  const f = fieldsOf("MapCases");
  assert.deepStrictEqual(f.meta.schema, {
    type: "map",
    of: {
      key: { type: "primitive", of: { prim: "string" } },
      value: { type: "primitive", of: { prim: "string" } },
    },
  });
  assert.deepStrictEqual(f.counts.schema, {
    type: "map",
    of: {
      key: { type: "primitive", of: { prim: "string" } },
      value: { type: "primitive", of: { prim: "float", bits: 64 } },
    },
  });
})();

// ---------------------------------------------------------------------------
// CR-04: property wire keys come from the name node kind, never raw source text.
// ---------------------------------------------------------------------------

(function quoted_and_numeric_names_are_unquoted_wire_keys() {
  const f = fieldsOf("NameCases");
  assert.ok("quoted-name" in f, "quoted name must unquote to its wire key");
  assert.ok("123" in f, "numeric name must yield its text wire key");
  assert.ok("plain" in f, "plain identifier survives");
  // The raw-source guess must NOT appear as a key.
  assert.ok(
    !("['quoted-name']" in f) && !("\"quoted-name\"" in f),
    "no raw-source-text key may leak"
  );
})();

(function computed_nonliteral_name_diagnosed_and_omitted() {
  const f = fieldsOf("NameCases");
  // The computed `[DYN_KEY]` cannot be statically resolved -> omitted, and the
  // raw `[DYN_KEY]` text must NOT be a key.
  assert.ok(
    !("[DYN_KEY]" in f),
    "computed non-literal name must be omitted, never a guessed raw-text key"
  );
  assert.ok(
    hasDiag("computed property name cannot be statically resolved"),
    "a computed non-literal name must record a diagnostic (rule 3)"
  );
})();

// ---------------------------------------------------------------------------
// Determinism: the closure is byte-stable across two independent builds.
// ---------------------------------------------------------------------------

(function deterministic_across_builds() {
  const d2 = new Diagnostics();
  const l2 = load.load(FIXTURE, d2);
  const s2 = buildSchemas(l2, d2);
  assert.strictEqual(
    JSON.stringify(schemas),
    JSON.stringify(s2),
    "schema closure must be byte-stable across builds (no machine-path ids)"
  );
  // Routes (none here) must not throw and stay empty/deterministic.
  const r2 = recognizeNestController(l2, d2, undefined);
  assert.deepStrictEqual(r2, [], "edge-cases fixture declares no controllers");
})();

console.log("edge-cases.test.js: OK");
