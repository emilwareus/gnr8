"use strict";

// Type-mapper unit tests (node:assert, no test framework — the rule-2 ethos:
// the runtime's own assert is the only dependency). Each case pins a VERIFIED
// row of the TS-type -> neutral-Type mapping against the committed nestjs
// snapshot vocabulary: number -> float64; `?`/`| null` strip to the
// optional/nullable axes (NOT union members); `T[]` -> array; a class/alias used
// as the sole type -> a named ref.
//
// Run: `node tsextract/tests/types.test.js` (exit 0 = pass).

const assert = require("assert");
const path = require("path");

const load = require("../load");
const types = require("../types");
const { Diagnostics } = require("../diagnostics");

const FIXTURE = path.resolve(__dirname, "../../fixtures/nestjs-bookstore");

// Build the program once and index every class property by `Class.prop` so each
// case can resolve a real fixture node through the TypeChecker.
const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const props = {};
for (const sf of loaded.program.getSourceFiles()) {
  if (!sf.fileName.includes("books.dto")) continue;
  sf.forEachChild((node) => {
    if (load.ts.isClassDeclaration(node) && node.name) {
      for (const member of node.members) {
        if (load.ts.isPropertyDeclaration(member) && member.name) {
          props[node.name.text + "." + member.name.getText(sf)] = member;
        }
      }
    }
  });
}

function mapProp(key) {
  const node = props[key];
  assert.ok(node, "fixture property not found: " + key);
  return types.mapType(loaded, node, diags);
}

// number -> {prim:float,bits:64} (NEVER int — Pitfall 2).
(function number_is_float64() {
  const r = mapProp("BookDto.id");
  assert.deepStrictEqual(r.schema, {
    type: "primitive",
    of: { prim: "float", bits: 64 },
  });
  assert.strictEqual(r.optional, false);
  assert.strictEqual(r.nullable, false);
})();

// string -> {prim:string}; nullable only.
(function string_nullable_strips_null_arm() {
  const r = mapProp("AuthorDto.bio");
  assert.deepStrictEqual(r.schema, { type: "primitive", of: { prim: "string" } });
  assert.strictEqual(r.optional, false);
  assert.strictEqual(r.nullable, true, "bio: string | null -> nullable, not a union");
})();

// rating?: number | null -> single float64 with BOTH axes (NOT a union).
(function optional_and_nullable_number_collapses_to_single() {
  const r = mapProp("BookDto.rating");
  assert.deepStrictEqual(
    r.schema,
    { type: "primitive", of: { prim: "float", bits: 64 } },
    "rating?: number | null must strip both arms to a single float64, never a union"
  );
  assert.strictEqual(r.optional, true);
  assert.strictEqual(r.nullable, true);
})();

// tags?: string[] -> array of string, optional only.
(function optional_array_strips_undefined() {
  const r = mapProp("BookDto.tags");
  assert.deepStrictEqual(r.schema, {
    type: "array",
    of: { type: "primitive", of: { prim: "string" } },
  });
  assert.strictEqual(r.optional, true);
  assert.strictEqual(r.nullable, false);
})();

// boolean -> {prim:bool}; optional only.
(function boolean_is_bool() {
  const r = mapProp("BookFilters.inStock");
  assert.deepStrictEqual(r.schema, { type: "primitive", of: { prim: "bool" } });
  assert.strictEqual(r.optional, true);
  assert.strictEqual(r.nullable, false);
})();

// author: AuthorDto -> a named ref (the class is emitted as its own schema).
(function class_is_named_ref() {
  const r = mapProp("BookDto.author");
  assert.deepStrictEqual(r.schema, {
    type: "named",
    of: "src/books.dto.AuthorDto",
  });
  assert.strictEqual(r.optional, false);
  assert.strictEqual(r.nullable, false);
})();

console.log("types.test.js: OK");
