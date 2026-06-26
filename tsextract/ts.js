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
// If `typescript` resolves from NEITHER, `resolveTypescript` throws a clear, one-line error.
//
// WR-04: resolution is LAZY. The throw fires the FIRST time the compiler namespace is touched (inside
// `load()`, which runs inside `index.js`'s `main()` try/catch) or when `probe()` is called by the
// doctor — NEVER at module-evaluation time. So `index.js`'s carefully formatted one-line stderr
// diagnostic wraps the failure instead of Node printing an uncaught-exception V8 stack trace. The Rust
// host maps the sidecar's non-zero exit to `CoreError::TypeScriptToolchainMissing`, and `gnr8 doctor`
// exercises this SAME resolution up front via `probe.js` — never a silent fallback to a guessed shape.

// Resolve the user's `typescript` toolchain for `targetDir`. The deterministic search order is the
// target project first, then this sidecar's own `node_modules`. `targetDir` is the absolute target the
// caller already has (IN-01: no second `process.argv[2]` realpath here); `node`'s own resolver follows
// symlinks, so the host's already-canonical absolute path needs no extra `realpathSync`.
function resolveTypescript(targetDir) {
  const searchDirs = [];
  if (targetDir) {
    searchDirs.push(targetDir);
  }
  searchDirs.push(__dirname);

  try {
    return require(require.resolve("typescript", { paths: searchDirs }));
  } catch (_e) {
    const err = new Error(
      "tsextract: could not resolve the `typescript` toolchain from the target project " +
        "(" + (targetDir || "<no target>") + ") or the sidecar. Install it in your project: " +
        "`npm install --save-dev typescript`.",
    );
    // Marker so `index.js`'s catch renders this as the clean one-line diagnostic (the message IS the
    // actionable text), not a V8 stack trace — it is a user toolchain gap, not an internal bug (WR-04).
    err.toolchainMissing = true;
    throw err;
  }
}

// The compiler namespace, resolved ONCE and memoized. The target dir is read from `process.argv[2]`
// (the host always passes it) at first-access time, so a stray `require("./ts")` does not resolve
// `typescript` at import time — only actual USE of the namespace does (WR-04 lazy boundary).
let _cached = null;
function compiler() {
  if (_cached === null) {
    _cached = resolveTypescript(process.argv[2]);
  }
  return _cached;
}

// A lazy, memoizing view of the compiler namespace. Every existing `const ts = require("./ts")` consumer
// (`load.js`/`schemas.js`/`routes.js`/`types.js`) keeps using `ts.<member>` unchanged; the FIRST member
// access triggers `compiler()` (and thus resolution), so the resolve throw lands inside the caller's
// guarded scope rather than at module load (WR-04). Read-only: a static-analysis sidecar never mutates
// the compiler namespace.
//
// `resolveTypescript` is exposed as an OWN member that the trap returns WITHOUT touching `compiler()`,
// so `require("./ts").resolveTypescript` (used by `probe.js`) does NOT eagerly resolve — preserving the
// WR-04 lazy boundary for the probe's deliberately-guarded call.
const _own = { resolveTypescript };
const lazyTs = new Proxy(
  {},
  {
    get(_target, prop) {
      if (Object.prototype.hasOwnProperty.call(_own, prop)) {
        return _own[prop];
      }
      return compiler()[prop];
    },
    has(_target, prop) {
      if (Object.prototype.hasOwnProperty.call(_own, prop)) {
        return true;
      }
      return prop in compiler();
    },
  },
);

module.exports = lazyTs;
