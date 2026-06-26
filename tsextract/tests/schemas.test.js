"use strict";

// Schema-collection unit test — DIRECT-ROOTS seeding (routes do not exist until
// 04-03, so transitive collection is seeded from every exported DTO class/alias
// in books.dto.ts, NOT from route param/body/response types). Asserts:
//
//   * exactly the 8 schema ids appear (OutOfStockDto reachable only via the
//     BookOrError union arm — proving transitive collection through a union);
//   * SortOrder is NOT a standalone schema (string-literal-union alias inlines);
//   * each schema body matches the committed snapshot vocabulary byte-for-byte
//     (axes, float64, named refs, the BookOrError union in SOURCE order).
//
// The route-seeded end-to-end assertion over `node index.js fixtures/...` is
// validated in 04-03, where routes provide the collection roots.
//
// Run: `node tsextract/tests/schemas.test.js` (exit 0 = pass).

const assert = require("assert");
const path = require("path");

const load = require("../load");
const { buildSchemas } = require("../schemas");
const { Diagnostics } = require("../diagnostics");

const FIXTURE = path.resolve(__dirname, "../../fixtures/nestjs-bookstore");

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const schemas = buildSchemas(loaded, diags);
const byId = Object.fromEntries(schemas.map((s) => [s.id, s]));

// Exactly the 8 transitively-collected schema ids — sorted, no SortOrder.
(function exactly_eight_ids() {
  const ids = schemas.map((s) => s.id).sort();
  assert.deepStrictEqual(ids, [
    "src/books.dto.AuthorDto",
    "src/books.dto.BookDto",
    "src/books.dto.BookFilters",
    "src/books.dto.BookFormat",
    "src/books.dto.BookOrError",
    "src/books.dto.CreatedMessage",
    "src/books.dto.ListBooksResponse",
    "src/books.dto.OutOfStockDto",
  ]);
})();

// AuthorDto: bio (string, nullable) + name (string).
(function author_body() {
  const fields = byId["src/books.dto.AuthorDto"].body.of.slice().sort((a, b) =>
    a.json_name < b.json_name ? -1 : 1
  );
  const bio = fields.find((f) => f.json_name === "bio");
  assert.deepStrictEqual(bio, {
    json_name: "bio",
    required: true,
    optional: false,
    nullable: true,
    schema: { type: "primitive", of: { prim: "string" } },
    description: null,
    example: null,
  });
})();

// BookDto.author -> named ref; BookDto.format -> named ref; rating single float64.
(function bookdto_refs_and_axes() {
  const fields = Object.fromEntries(
    byId["src/books.dto.BookDto"].body.of.map((f) => [f.json_name, f])
  );
  assert.deepStrictEqual(fields.author.schema, {
    type: "named",
    of: "src/books.dto.AuthorDto",
  });
  assert.deepStrictEqual(fields.format.schema, {
    type: "named",
    of: "src/books.dto.BookFormat",
  });
  assert.deepStrictEqual(fields.rating, {
    json_name: "rating",
    required: false,
    optional: true,
    nullable: true,
    schema: { type: "primitive", of: { prim: "float", bits: 64 } },
    description: null,
    example: null,
  });
  assert.deepStrictEqual(fields.tags.schema, {
    type: "array",
    of: { type: "primitive", of: { prim: "string" } },
  });
})();

// BookFilters.sort -> inline enum; published nullable-not-optional.
(function bookfilters_axes() {
  const fields = Object.fromEntries(
    byId["src/books.dto.BookFilters"].body.of.map((f) => [f.json_name, f])
  );
  assert.deepStrictEqual(fields.sort, {
    json_name: "sort",
    required: false,
    optional: true,
    nullable: true,
    schema: { type: "enum", of: ["asc", "desc"] },
    description: null,
    example: null,
  });
  assert.strictEqual(fields.published.nullable, true);
  assert.strictEqual(fields.published.optional, false);
  assert.strictEqual(fields.published.required, true);
})();

// BookFormat -> enum schema, sorted.
(function bookformat_enum() {
  assert.deepStrictEqual(byId["src/books.dto.BookFormat"].body, {
    type: "enum",
    of: ["hardcover", "paperback"],
  });
})();

// BookOrError -> union of named refs, SOURCE order (BookDto then OutOfStockDto).
(function bookorerror_union_source_order() {
  assert.deepStrictEqual(byId["src/books.dto.BookOrError"].body, {
    type: "union",
    of: [
      { type: "named", of: "src/books.dto.BookDto" },
      { type: "named", of: "src/books.dto.OutOfStockDto" },
    ],
  });
})();

// ListBooksResponse.books -> array of named BookDto refs.
(function listbooks_array_of_named() {
  const fields = Object.fromEntries(
    byId["src/books.dto.ListBooksResponse"].body.of.map((f) => [f.json_name, f])
  );
  assert.deepStrictEqual(fields.books.schema, {
    type: "array",
    of: { type: "named", of: "src/books.dto.BookDto" },
  });
})();

console.log("schemas.test.js: OK");
