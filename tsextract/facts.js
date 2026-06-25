"use strict";

// Deterministic facts marshal — the contract boundary (twin of
// `pyextract/facts.py` / `goextract/internal/facts/facts.go`).
//
// The emitted JSON must deserialize into the Rust `GoFacts` DTO under
// `deny_unknown_fields` — any extra/missing key fails the host. Determinism
// (identical input -> byte-identical output) is achieved two ways, both
// required:
//
//   * `_sortDoc` orders every ARRAY by the exact keys `facts.go::sortDoc` uses —
//     schemas by id; object fields by `json_name`; enum members lexically;
//     diagnostics by `(file, line, message)`; routes by `(path, method)`; params
//     by `(name, location)`; responses by status. Union members are NOT sorted
//     (source order — RESEARCH Pitfall 5).
//   * `stringify` serializes with a RECURSIVE key-sorted replacer, so every
//     object's KEYS are emitted in a stable lexical order (the JS analog of
//     Python's `json.dumps(..., sort_keys=True)`).

// Assemble the top-level neutral facts document.
//
// `routes` is empty in Plan 04-02 (the NestJS route recognizer lands in 04-03);
// the key is still present and emitted as `[]`.
function buildDoc(module, routes, schemas, diagnostics) {
  return {
    module: module,
    routes: routes.slice(),
    schemas: schemas.slice(),
    diagnostics: diagnostics.slice(),
  };
}

// Return the sorted, byte-stable JSON string for a facts `doc`. Sorts every
// array in place (`_sortDoc`) then serializes with sorted object keys and
// compact separators (no whitespace), mirroring goextract's
// `SetEscapeHTML(false)` by leaving non-ASCII characters unescaped (JSON.stringify
// only escapes the JSON-mandatory characters, never HTML metacharacters).
function marshal(doc) {
  _sortDoc(doc);
  return _stableStringify(doc);
}

function _byString(key) {
  return (a, b) => {
    const av = a[key];
    const bv = b[key];
    return av < bv ? -1 : av > bv ? 1 : 0;
  };
}

function _sortDoc(doc) {
  doc.schemas.sort(_byString("id"));
  for (const schema of doc.schemas) {
    _sortType(schema.body);
  }

  doc.diagnostics.sort((a, b) => {
    if (a.file !== b.file) return a.file < b.file ? -1 : 1;
    if (a.line !== b.line) return a.line - b.line;
    return a.message < b.message ? -1 : a.message > b.message ? 1 : 0;
  });

  doc.routes.sort((a, b) => {
    if (a.path !== b.path) return a.path < b.path ? -1 : 1;
    return a.method < b.method ? -1 : a.method > b.method ? 1 : 0;
  });
  for (const route of doc.routes) {
    _sortRoute(route);
  }
}

// Recursively order the deterministic parts of a Type body. Object fields by
// `json_name`; enum members lexically; recurse through array / map / union /
// object payloads. Union member ORDER is preserved (source order), but each
// member's inner type is still recursively sorted.
function _sortType(t) {
  if (t === null || typeof t !== "object") {
    return;
  }
  const kind = t.type;
  const payload = t.of;

  if (kind === "object" && Array.isArray(payload)) {
    payload.sort(_byString("json_name"));
    for (const field of payload) {
      _sortType(field.schema);
    }
  } else if (kind === "enum" && Array.isArray(payload)) {
    payload.sort();
  } else if (kind === "union" && Array.isArray(payload)) {
    // Source order preserved — only recurse into members.
    for (const member of payload) {
      _sortType(member);
    }
  } else if (kind === "array") {
    _sortType(payload);
  } else if (kind === "map" && payload && typeof payload === "object") {
    _sortType(payload.key);
    _sortType(payload.value);
  }
}

function _sortRoute(route) {
  route.params.sort((a, b) => {
    if (a.name !== b.name) return a.name < b.name ? -1 : 1;
    return a.location < b.location ? -1 : a.location > b.location ? 1 : 0;
  });
  for (const param of route.params) {
    _sortType(param.schema);
  }
  route.responses.sort((a, b) => a.status - b.status);
}

// Serialize `value` with recursively-sorted object keys and compact separators.
// `JSON.stringify` with a replacer cannot reorder keys, so we rebuild every
// plain object with its keys inserted in lexical order before serializing
// (V8 preserves string-key insertion order). Arrays and primitives pass through
// unchanged so the pre-sorted array order is honored.
function _keySorted(value) {
  if (Array.isArray(value)) {
    return value.map(_keySorted);
  }
  if (value !== null && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) {
      out[key] = _keySorted(value[key]);
    }
    return out;
  }
  return value;
}

function _stableStringify(value) {
  return JSON.stringify(_keySorted(value));
}

module.exports = { buildDoc, marshal };
