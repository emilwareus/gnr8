"use strict";

// Resolve the user's OWN `typescript` toolchain — exactly as `goextract` uses the user's `go`
// and `pyextract` uses the user's `python3`. gnr8 ships NO `typescript`; any TypeScript project
// already carries it as a (dev)dependency, so the sidecar borrows the project's own compiler
// (and thereby its exact tsconfig/language version). This keeps gnr8-core + all sidecars +
// generated SDKs dependency-free: `typescript` is a *required toolchain*, not a bundled dep.
//
// Resolution is a SINGLE deterministic search (rule 3 — not a guessed fallback, the same class as
// a PATH lookup for `go`/`python3`):
//   1. the TARGET project being analyzed (`process.argv[2]`) — the user's own `typescript`.
//   2. this sidecar's own `node_modules` (`__dirname`) — present only for gnr8's OWN test suite
//      (a gitignored dev/CI `npm ci`); never shipped to users.
// If `typescript` resolves from NEITHER, the require throws a clear error; the Rust host maps the
// sidecar's non-zero exit to `CoreError::TypeScriptToolchainMissing` (and `gnr8 doctor` detects the
// missing toolchain up front) — never a silent fallback to a guessed shape.

const fs = require("fs");

function resolveTypescript() {
  const searchDirs = [];
  const targetArg = process.argv[2];
  if (targetArg) {
    try {
      searchDirs.push(fs.realpathSync(targetArg));
    } catch (_e) {
      // A bad target dir is the host's problem (reported as a typed error there), not ours here.
    }
  }
  searchDirs.push(__dirname);

  try {
    return require(require.resolve("typescript", { paths: searchDirs }));
  } catch (_e) {
    throw new Error(
      "tsextract: could not resolve the `typescript` toolchain from the target project " +
        "(" + (targetArg || "<no target>") + ") or the sidecar. Install it in your project: " +
        "`npm install --save-dev typescript`.",
    );
  }
}

module.exports = resolveTypescript();
