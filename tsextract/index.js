"use strict";

// Entrypoint: `node index.js <target-dir>` — argv -> neutral facts JSON on stdout.
//
// The TypeScript twin of `pyextract/__main__.py` / `goextract/main.go`: a single target-dir
// argument, the facts JSON written to STDOUT ONLY, and every tool-diagnostic-about-itself written to
// STDERR with a non-zero exit. The Rust subprocess driver (`analyze::helper::run_tsextract`) maps a
// non-zero exit to `HelperExit` and unparsable stdout to `FactsParse` — so the contract is strict:
// stdout is the facts JSON and nothing else.
//
// THIS WAVE (04-01) ships a STUB `run()` that emits an empty-but-valid facts envelope so the host
// seam (detect -> dispatch -> run_tsextract -> node) is exercisable end-to-end before the real
// Compiler-API extractor lands in 04-02/04-03. The stub does NOT read the target's `*.ts`; the real
// extractor (rule 1: facts ONLY from the source's own TS types via the Compiler API; static-only,
// never executed) replaces the stub body in a later wave.

const path = require("path");
const fs = require("fs");

// Build the neutral facts document for `targetDir`.
//
// Returns a deterministic, sorted, byte-stable JSON facts string. Top-level keys EXACTLY:
// `module, routes, schemas, diagnostics` (the `facts.rs` deny_unknown_fields contract). The stub
// emits empty `routes/schemas/diagnostics`; `module` = basename(targetDir) (the snapshot rule).
function run(targetDir) {
  const doc = {
    module: path.basename(targetDir),
    routes: [],
    schemas: [],
    diagnostics: [],
  };
  // Sorted keys + compact separators for internal determinism (the host re-sorts too, but the
  // sidecar owns byte-stable output — RESEARCH facts.js marshal pattern).
  return JSON.stringify(doc, Object.keys(doc).sort());
}

// CLI guard + orchestration. Returns the process exit code.
function main(argv) {
  if (argv.length < 3) {
    process.stderr.write("usage: node index.js <target-dir>\n");
    return 1;
  }
  let targetDir;
  try {
    targetDir = fs.realpathSync(argv[2]);
  } catch (_err) {
    // A non-existent/unreadable target is the sidecar's own diagnostic, not facts. The host maps the
    // non-zero exit to HelperExit; stdout stays reserved for the facts JSON.
    process.stderr.write("tsextract: cannot resolve target dir: " + argv[2] + "\n");
    return 1;
  }
  try {
    process.stdout.write(run(targetDir));
    process.stdout.write("\n");
  } catch (exc) {
    // Surface ANY failure (an unhandled shape -> TypeError, etc.) to stderr so a genuine internal
    // bug is diagnosable, not masked as a clean tool diagnostic. stdout stays facts-only; the
    // non-zero exit still maps to HelperExit.
    process.stderr.write("tsextract: " + (exc && exc.stack ? exc.stack : exc) + "\n");
    return 1;
  }
  return 0;
}

process.exitCode = main(process.argv);
