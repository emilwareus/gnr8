"use strict";

// Schema builder: DTO class / type-alias -> neutral SchemaFact — the TypeScript
// twin of `pyextract/schemas.py`. The real implementation (transitive collection
// + the named-vs-inline predicate) lands in Task 2 (TDD); this Task-1 placeholder
// returns an empty list so the `index.js` pipeline (load -> schemas -> marshal)
// is exercisable end-to-end and byte-deterministic before the type mapper exists.

// Build the full sorted-by-id list of SchemaFact objects for a loaded program.
function buildSchemas(_loaded, _diags) {
  return [];
}

module.exports = { buildSchemas };
