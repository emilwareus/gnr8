"use strict";

// Golden line-assertion test (node:assert) — locks the fixture line reconciliation
// against the committed graph snapshot's asserted spans (04-03 Task 2). Every
// operation / param / schema anchor produced by tsextract must land on the EXACT
// line the snapshot asserts; if a future fixture edit shifts a declaration, this
// fails fast with a precise (name, produced, expected) message — independent of
// (and complementary to) the Rust insta snapshot.
//
// The expected lines are the snapshot's authoritative `start_line` values
// (crates/gnr8-core/tests/snapshots/snapshot_nestjs_graph__nestjs_graph.snap).
// ZERO snapshot edits: this test reconciles the fixture TO the snapshot, never the
// reverse. Anchors: operation = method-name line; param = param-name line; schema =
// class/type-alias declaration line.

const assert = require("node:assert");
const path = require("node:path");

const load = require("../load");
const { Diagnostics } = require("../diagnostics");
const { Registry } = require("../schemas");
const { recognizeNestController } = require("../routes");
const { buildSchemas } = require("../schemas");

const FIXTURE = path.join(__dirname, "..", "..", "fixtures", "nestjs-bookstore");

// Snapshot-asserted operation span lines (method-name line).
const OP_LINES = {
  listBooks: 41,
  createBook: 51,
  getBook: 57,
  updateBook: 65,
};

// Snapshot-asserted param span lines (param-name line), keyed "op.param".
const PARAM_LINES = {
  "listBooks.genre": 42,
  "listBooks.sort": 43,
  "listBooks.cursor": 44,
  "getBook.bookId": 58,
  "getBook.fmt": 59,
  "updateBook.bookId": 66,
};

// Snapshot-asserted schema span lines (declaration line), keyed by short name.
const SCHEMA_LINES = {
  AuthorDto: 41,
  BookDto: 47,
  BookFilters: 56,
  BookFormat: 36,
  BookOrError: 73,
  CreatedMessage: 75,
  ListBooksResponse: 80,
  OutOfStockDto: 70,
};

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const registry = new Registry();
const routes = recognizeNestController(loaded, diags, registry);
const schemas = buildSchemas(loaded, diags, registry);

for (const r of routes) {
  assert.strictEqual(
    r.span.start_line,
    OP_LINES[r.operation_id],
    "operation '" +
      r.operation_id +
      "' span line " +
      r.span.start_line +
      " != snapshot line " +
      OP_LINES[r.operation_id]
  );
  for (const p of r.params) {
    const key = r.operation_id + "." + p.name;
    assert.strictEqual(
      p.span.start_line,
      PARAM_LINES[key],
      "param '" +
        key +
        "' span line " +
        p.span.start_line +
        " != snapshot line " +
        PARAM_LINES[key]
    );
  }
}

for (const s of schemas) {
  const short = s.id.replace("src/books.dto.", "");
  assert.strictEqual(
    s.span.start_line,
    SCHEMA_LINES[short],
    "schema '" +
      short +
      "' span line " +
      s.span.start_line +
      " != snapshot line " +
      SCHEMA_LINES[short]
  );
}

console.log(
  "lines.test.js: OK (all " +
    (Object.keys(OP_LINES).length +
      Object.keys(PARAM_LINES).length +
      Object.keys(SCHEMA_LINES).length) +
    " span anchors match the committed snapshot lines)"
);
