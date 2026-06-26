"use strict";

// Golden test pinning Open Question 1 (RESEARCH Pitfall 4): the named-vs-inline
// enum predicate, regression-locked against the committed snapshot.
//
//   * `format: BookFormat` (an alias used as the SOLE type) -> a NAMED ref
//     `src/books.dto.BookFormat` AND a standalone BookFormat enum schema
//     `[hardcover, paperback]` (sorted).
//   * `sort?: SortOrder | null` (the alias appears with a stripped null/undefined
//     arm, so the residual literal union has lost the aliasSymbol) -> an INLINE
//     `enum [asc, desc]` (sorted) with optional+nullable, and NO standalone
//     SortOrder schema.
//
// Run: `node tsextract/tests/golden.test.js` (exit 0 = pass).

const assert = require("assert");
const path = require("path");

const load = require("../load");
const types = require("../types");
const { buildSchemas } = require("../schemas");
const { Diagnostics } = require("../diagnostics");

const FIXTURE = path.resolve(__dirname, "../../fixtures/nestjs-bookstore");

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);

// Index the two properties under test.
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

// format -> NAMED ref.
(function format_is_named_ref() {
  const r = types.mapType(loaded, props["BookDto.format"], diags);
  assert.deepStrictEqual(
    r.schema,
    { type: "named", of: "src/books.dto.BookFormat" },
    "format: BookFormat (sole type) must be a NAMED ref"
  );
})();

// sort -> INLINE sorted enum with both axes.
(function sort_is_inline_enum() {
  const r = types.mapType(loaded, props["BookFilters.sort"], diags);
  assert.deepStrictEqual(
    r.schema,
    { type: "enum", of: ["asc", "desc"] },
    "sort?: SortOrder | null must INLINE as a sorted enum (aliasSymbol lost on the residual)"
  );
  assert.strictEqual(r.optional, true);
  assert.strictEqual(r.nullable, true);
})();

// The BookFormat schema must exist (enum, sorted) and SortOrder must NOT.
(function schemas_pin_named_vs_inline() {
  // Seed transitive collection from ALL exported DTO classes/aliases (direct
  // roots — routes do not exist until 04-03).
  const schemas = buildSchemas(loaded, diags);
  const byId = Object.fromEntries(schemas.map((s) => [s.id, s]));

  assert.ok(
    byId["src/books.dto.BookFormat"],
    "BookFormat must be emitted as a standalone schema"
  );
  assert.deepStrictEqual(byId["src/books.dto.BookFormat"].body, {
    type: "enum",
    of: ["hardcover", "paperback"],
  });

  assert.ok(
    !byId["src/books.dto.SortOrder"],
    "SortOrder must NOT be a standalone schema (it only ever inlines)"
  );
})();

console.log("golden.test.js: OK");
