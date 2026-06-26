"use strict";

// Route-recognition unit test (node:assert, zero test framework — the rule-2
// ethos). Drives the real NestJS bookstore fixture through `recognizeNestController`
// and asserts the 4 routes' verbs/paths/params/body-refs/response-refs and the
// method-derived status (createBook 201, others 200). The @Controller('books')
// prefix must NOT be folded into any operation path (rule 1).

const assert = require("node:assert");
const path = require("node:path");

const load = require("../load");
const { Diagnostics } = require("../diagnostics");
const { Registry } = require("../schemas");
const { recognizeNestController } = require("../routes");

const FIXTURE = path.join(__dirname, "..", "..", "fixtures", "nestjs-bookstore");

const diags = new Diagnostics();
const loaded = load.load(FIXTURE, diags);
const registry = new Registry();
const routes = recognizeNestController(loaded, diags, registry);

// Index routes by operation_id for clarity.
const byId = {};
for (const r of routes) {
  byId[r.operation_id] = r;
}

assert.strictEqual(routes.length, 4, "expected exactly 4 routes");
assert.deepStrictEqual(
  Object.keys(byId).sort(),
  ["createBook", "getBook", "listBooks", "updateBook"],
  "expected the 4 named operations"
);

// --- listBooks: GET / with cursor/genre/sort query params, no body, 200 ----
const list = byId.listBooks;
assert.strictEqual(list.method, "GET");
assert.strictEqual(list.path, "/", "listBooks path is group-relative '/'");
assert.strictEqual(list.handler, "listBooks");
assert.strictEqual(list.request_body, null, "listBooks has no request body");
assert.strictEqual(list.responses.length, 1);
assert.strictEqual(list.responses[0].status, 200, "GET -> 200");
assert.strictEqual(
  list.responses[0].body.ref_id,
  "src/books.dto.ListBooksResponse"
);
const listQ = list.params.map((p) => p.name).sort();
assert.deepStrictEqual(
  listQ,
  ["cursor", "genre", "sort"],
  "listBooks query params"
);
for (const p of list.params) {
  assert.strictEqual(p.location, "query", "listBooks params are query");
}
const genre = list.params.find((p) => p.name === "genre");
assert.strictEqual(genre.required, true, "genre (no ?/default) is required");
const sort = list.params.find((p) => p.name === "sort");
assert.strictEqual(sort.required, false, "sort (has default) is optional");
const cursor = list.params.find((p) => p.name === "cursor");
assert.strictEqual(cursor.required, false, "cursor (?) is optional");

// --- createBook: POST / with a request body, no params, 201 ---------------
const create = byId.createBook;
assert.strictEqual(create.method, "POST");
assert.strictEqual(create.path, "/", "createBook path is group-relative '/'");
assert.deepStrictEqual(create.params, [], "createBook has no params (body only)");
assert.notStrictEqual(create.request_body, null, "createBook has a request body");
assert.strictEqual(create.request_body.ref_id, "src/books.dto.BookDto");
assert.strictEqual(create.responses[0].status, 201, "typed POST -> 201");
assert.strictEqual(
  create.responses[0].body.ref_id,
  "src/books.dto.CreatedMessage"
);

// --- getBook: GET /{bookId} with a path param + an enum query, union resp --
const get = byId.getBook;
assert.strictEqual(get.method, "GET");
assert.strictEqual(get.path, "/{bookId}", "getBook ':bookId' -> '{bookId}'");
const getBookId = get.params.find((p) => p.name === "bookId");
assert.strictEqual(getBookId.location, "path", "bookId is a path param");
assert.strictEqual(getBookId.required, true, "path param is required");
const fmt = get.params.find((p) => p.name === "fmt");
assert.strictEqual(fmt.location, "query");
assert.strictEqual(fmt.required, false, "fmt (?) is optional");
assert.strictEqual(get.responses[0].status, 200, "GET -> 200");
assert.strictEqual(get.responses[0].body.ref_id, "src/books.dto.BookOrError");
assert.strictEqual(get.request_body, null);

// --- updateBook: PUT /{bookId} with a path param + a body, 200 ------------
const update = byId.updateBook;
assert.strictEqual(update.method, "PUT");
assert.strictEqual(update.path, "/{bookId}");
const updBookId = update.params.find((p) => p.name === "bookId");
assert.strictEqual(updBookId.location, "path");
assert.strictEqual(updBookId.required, true);
assert.strictEqual(update.request_body.ref_id, "src/books.dto.BookFilters");
assert.strictEqual(update.responses[0].status, 200, "PUT -> 200");
assert.strictEqual(
  update.responses[0].body.ref_id,
  "src/books.dto.CreatedMessage"
);

// --- the @Controller('books') prefix is NEVER folded (rule 1) -------------
for (const r of routes) {
  assert.ok(
    !r.path.startsWith("/books"),
    "operation path '" + r.path + "' must not carry the @Controller prefix"
  );
}

console.log("routes.test.js: OK (4 routes, group-relative paths, method-derived status)");
