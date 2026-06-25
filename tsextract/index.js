"use strict";

// Entrypoint: `node index.js <target-dir>` — argv -> neutral facts JSON on stdout.
//
// The TypeScript twin of `pyextract/__main__.py` / `goextract/main.go`: a single target-dir
// argument, the facts JSON written to STDOUT ONLY, and every tool-diagnostic-about-itself written to
// STDERR with a non-zero exit. The Rust subprocess driver (`analyze::helper::run_tsextract`) maps a
// non-zero exit to `HelperExit` and unparsable stdout to `FactsParse` — so the contract is strict:
// stdout is the facts JSON and nothing else.
//
// 04-02 plugs the real extractor CORE into this entrypoint: the loader builds a
// `ts.Program` + `TypeChecker` over the target's `*.ts` (static-only — rule 1/3, never
// executed), and the schema builder collects the DTO classes/aliases into neutral
// SchemaFacts. Route recognition (the @Controller/@Get walk) lands in 04-03, so `routes`
// stays `[]` this wave; the 2 NestJS snapshots remain red until then.

const path = require("path");
const fs = require("fs");

const load = require("./load");
const facts = require("./facts");
const { Diagnostics } = require("./diagnostics");
const { buildSchemas } = require("./schemas");

// Build the neutral facts document for `targetDir`.
//
// Pipeline order (the pyextract twin): load -> diagnostics -> schemas -> module basename
// -> routes (empty this wave) -> assemble -> marshal. Returns a deterministic, sorted,
// byte-stable JSON facts string. Top-level keys EXACTLY: `module, routes, schemas,
// diagnostics` (the `facts.rs` deny_unknown_fields contract). `module` =
// basename(targetDir) (the snapshot rule).
function run(targetDir) {
  const diags = new Diagnostics();
  const loaded = load.load(targetDir, diags);

  const schemas = buildSchemas(loaded, diags);

  const module = path.basename(targetDir);
  const routes = []; // NestJS route recognition lands in 04-03.

  const doc = facts.buildDoc(module, routes, schemas, diags.items());
  return facts.marshal(doc);
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
