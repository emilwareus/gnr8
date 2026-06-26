"use strict";

// The diagnostics accumulator — the TypeScript twin of `pyextract/diagnostics.py`
// / `goextract/internal/diag`.
//
// A diagnostic is emitted (CLAUDE.md rule 3) whenever a fact cannot be derived
// from a single deterministic code source: an unresolvable/foreign type name, an
// untyped surface, etc. The fact is then OMITTED — never guessed.
//
// A DiagnosticFact carries EXACTLY `severity, message, file, line` — `line` is a
// single 1-based integer, NOT a span (facts.rs DiagnosticFact). Severity is
// `WARN` for every diagnostic this sidecar emits.

// Accumulates WARN diagnostics as plain objects in the neutral facts shape.
class Diagnostics {
  constructor() {
    this._items = [];
  }

  // Record a WARN diagnostic.
  //   message: the human-readable rule + identity.
  //   file:    the source file (canonical absolute path; the host relativizes).
  //   line:    the 1-based line number (a single integer, never a span).
  warn(message, file, line) {
    this._items.push({
      severity: "WARN",
      message: String(message),
      file: String(file),
      line: Math.trunc(Number(line)) || 0,
    });
  }

  // Return the accumulated diagnostic objects (host re-sorts the final slice).
  items() {
    return this._items.slice();
  }
}

module.exports = { Diagnostics };
