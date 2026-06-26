---
phase: 06-cross-language-hardening-examples-docs
reviewed: 2026-06-26T00:00:00Z
depth: standard
files_reviewed: 7
files_reviewed_list:
  - crates/gnr8-core/src/analyze/mod.rs
  - crates/gnr8/src/doctor.rs
  - crates/gnr8/src/main.rs
  - crates/gnr8/src/watch.rs
  - examples/fastapi-bookstore/.gnr8/src/main.rs
  - examples/nestjs-bookstore/.gnr8/src/main.rs
  - tsextract/ts.js
findings:
  critical: 0
  warning: 5
  info: 3
  total: 8
status: issues_found
---

# Phase 06: Code Review Report

**Reviewed:** 2026-06-26
**Depth:** standard
**Files Reviewed:** 7
**Status:** issues_found

## Summary

Reviewed the cross-language toolchain generalization: the new `pub source_toolchain`/`SourceToolchain` API in `analyze/mod.rs`, the doctor/watch dispatch that consumes it, the two example `.gnr8/` Pipeline crates, and the new `tsextract/ts.js` resolver.

The CLAUDE.md rule-3 invariant (one deterministic decision, no fallback chain) holds in the code paths that matter: `detect_language` is a single count over three marker booleans, `source_toolchain` is a pure mapping over it, and both `doctor` (`probe_source_lang_toolchain`) and `watch` (`run`) consume exactly one `source_toolchain` decision with no try-A-then-B. Rule 2 holds for `ts.js` (Node built-ins `fs`/`require` only; `typescript` is borrowed from the target, not bundled — `node_modules` is gitignored). No production `unwrap`/`expect`/`panic`; every `unwrap`/`expect` is scoped to `#[cfg(test)]`. Subprocess calls use discrete args, never `sh -c`. The example crates compose the correct pipelines (FastApi→OpenApi31+PySdk, NestJs→OpenApi31+TsSdk).

The defects are correctness gaps and doc-vs-behavior mismatches rather than rule-3/rule-2 violations. The most material are: (1) `watch::is_trigger_path` does NOT actually exclude `.gnr8/` despite its own docs and the core detector both claiming it does — a source-language file under `.gnr8/` (outside `target`/`cache`) will spuriously trigger regeneration; (2) `doctor` cannot detect a missing `typescript` toolchain (it probes only `node`), contradicting the explicit claim in `ts.js` that "gnr8 doctor detects the missing toolchain up front" — a NestJS project without `typescript` passes doctor but fails `generate`; (3) `detect_language`/`scan_markers` only special-cases `.gnr8/`, so a Rust project's own `target/` (or `node_modules`/`.git`) holding a vendored other-language file can spoof a false ambiguity error on an otherwise single-language tree.

## Warnings

### WR-01: `watch::is_trigger_path` does not exclude `.gnr8/` source-language files (doc/behavior mismatch, spurious-trigger vector)

**File:** `crates/gnr8/src/watch.rs:99-115` (also docs at `:20`, `:91`, `:113`)
**Issue:** The module docs (line 20: "anywhere under the project root that is NOT under `.gnr8/`"), the function doc (line 91: "NOT under `.gnr8/`"), and the inline comment (line 113: "Only edits in the DETECTED source language outside `.gnr8/`") all promise that source-language files under `.gnr8/` are excluded. The code does NOT implement that. `is_trigger_path` only: (a) triggers on `*.rs` under `gnr8_src`, (b) drops paths under `output_set` (which is `.gnr8/target`, `.gnr8/cache`, and manifest-recorded outputs), then (c) triggers ANY path whose extension == `source_ext`. A source-language file living under `.gnr8/` but NOT under `target`/`cache` — e.g. `.gnr8/src/helper.ts` in a TypeScript project, or a `.py`/`.go`/`.ts` file the user drops anywhere else in the crate — falls through to (c) and triggers a regeneration. This both contradicts the documented contract and diverges from `analyze::scan_markers`, which DOES exclude `.gnr8/` from detection. The two modules disagree about what `.gnr8/` means.
**Fix:** Make the runtime filter match the documented invariant and the core detector: exclude anything under `.gnr8/` from the source-language trigger (the `*.rs`-under-`gnr8_src` trigger stays). Thread the canonicalized `.gnr8` root in alongside `gnr8_src`:
```rust
fn is_trigger_path(
    path: &Path,
    output_set: &HashSet<PathBuf>,
    gnr8_root: &Path,   // canonicalized project_root/.gnr8
    gnr8_src: &Path,
    source_ext: &str,
) -> bool {
    if path.starts_with(gnr8_src) && path.extension().is_some_and(|e| e == "rs") {
        return true;
    }
    if is_under_any_output(path, output_set) {
        return false;
    }
    // A source-language file anywhere under .gnr8/ is the crate's, not the user's API source.
    if path.starts_with(gnr8_root) {
        return false;
    }
    path.extension().is_some_and(|ext| ext == source_ext)
}
```
Either fix the code or, if the current behavior is actually intended, correct all three doc sites — but the divergence from `scan_markers` argues for the code fix.

### WR-02: `doctor` cannot detect a missing `typescript` toolchain — contradicts `ts.js`'s stated contract

**File:** `crates/gnr8/src/main.rs:216-231` (probe); `crates/gnr8-core/src/analyze/mod.rs:115-121` (`probe_binary`); claim in `tsextract/ts.js:14-16`
**Issue:** For a TypeScript source, `SourceToolchain::TypeScript::probe_binary()` returns `"node"`, so `probe_source_lang_toolchain` spawns `node --version` and reports the toolchain healthy whenever `node` is on PATH. But the sidecar's REAL required toolchain is `typescript`, resolved by `ts.js` from the target project / sidecar. `ts.js:14-16` explicitly promises: "the Rust host maps the sidecar's non-zero exit to `CoreError::TypeScriptToolchainMissing` (and `gnr8 doctor` detects the missing toolchain up front)". `doctor` does NOT do this — it never invokes `tsextract` for a presence probe, only `node`. Net effect: a NestJS project with `node` installed but no `typescript` (dev)dependency passes `gnr8 doctor` as healthy, then fails `gnr8 generate` with a `HelperExit` (from `run_tsextract_with`, since `ts.js` throws at require-time, not `TypeScriptToolchainMissing`). The doctor "up front" guarantee in the comment is false for the language that needs it most. (Go/Python are unaffected: `go`/`python3` ARE the whole toolchain.)
**Fix:** Either (a) make the TS probe actually exercise `typescript` resolution (e.g. spawn `node -e "require.resolve('typescript', {paths:[target,sidecar]})"` or a tiny probe entry in the sidecar) so doctor reflects real readiness, or (b) at minimum correct the `ts.js:14-16` comment to stop claiming doctor detects it, and document that a missing `typescript` surfaces only at `generate`. Option (a) is the honest fix given the comment's promise.

### WR-03: `scan_markers` only excludes `.gnr8/` — a Rust `target/`, `node_modules/`, or `.git/` holding a vendored other-language file spoofs a false ambiguity

**File:** `crates/gnr8-core/src/analyze/mod.rs:172-212`
**Issue:** `scan_markers` walks the whole tree and special-cases ONLY a dir literally named `.gnr8` (line 188). Every other directory is descended, including a project-root Rust `target/` (NOT under `.gnr8/`), `node_modules/`, vendor dirs, or `.git/`. The function comment at line 184-187 acknowledges exactly this spoofing risk for `.gnr8/target` and excludes it — but the same vendored-other-language-file hazard exists for any build/vendor tree elsewhere in the source root. Concretely: a single-language TypeScript service whose repo root also has a Rust workspace `target/` containing a vendored `*.go` (or a `node_modules` package shipping `*.ts` + a `*.go` codegen helper) would set two marker booleans and `detect_language` would reject the whole project as "ambiguous" — refusing to run on a tree that is unambiguously TypeScript from the user's perspective. The chosen design (whole-tree scan, exclude only `.gnr8`) makes the ambiguity guard over-trigger on real-world repos.
**Fix:** Exclude the well-known non-source dirs from the walk alongside `.gnr8/`, e.g. skip `node_modules`, `.git`, and a top-level `target/` (or any dir starting with `.`), mirroring `load.js:49` which already skips `node_modules`. Keep it a single deterministic scan (no fallback) — just a wider skip set:
```rust
if let Some(n) = path.file_name().and_then(|n| n.to_str()) {
    if matches!(n, ".gnr8" | "node_modules" | ".git" | "target") {
        continue;
    }
}
```
At minimum exclude `node_modules` and `.git`; `target` is judgment-dependent but is the most likely real spoof source for a Rust-adjacent repo.

### WR-04: `ts.js` resolver runs at `require` time, so its clear error message is bypassed by `index.js`'s `main()` try/catch

**File:** `tsextract/ts.js:43` (`module.exports = resolveTypescript()`); consumed via `tsextract/load.js:21` ← `tsextract/index.js:21`
**Issue:** `ts.js` calls `resolveTypescript()` at module-evaluation time and exports the result. `index.js` requires `./load` (line 21) at the top of the file, and `load.js` requires `./ts` (line 21) — so if `typescript` cannot be resolved, the `throw new Error(...)` fires during the top-level `require` chain, BEFORE `main(process.argv)` runs (line 82). The carefully constructed try/catch in `main()` (index.js:69-78) that formats a clean stderr line never wraps this throw. Node instead prints an uncaught-exception stack trace and exits non-zero. The host still maps it to `HelperExit` and the message text is present, but it is buried in a V8 stack trace rather than delivered as the clean one-line diagnostic the code clearly intends. This makes the "clear error path" weaker than designed for the single most likely TS failure (missing `typescript`).
**Fix:** Make resolution lazy so the throw happens inside `main()`'s guarded scope. Export a function and call it where errors are already caught:
```js
function resolveTypescript() { /* ... unchanged ... */ }
module.exports = { resolveTypescript };
```
and in `load.js` resolve inside `load()` (or wrap the require in `index.js`'s try/catch). Alternatively, add a top-level `process.on('uncaughtException', ...)` in `index.js` that prints the clean line. Lazy resolution is cleaner and keeps stdout reserved for facts.

### WR-05: `probe_source_lang_toolchain` treats spawn-success as toolchain-present, masking a broken binary

**File:** `crates/gnr8/src/main.rs:226-230`
**Issue:** `present` is `Command::new(binary).arg(version_arg).output().is_ok()`. `.output()` returns `Ok` whenever the process SPAWNED — regardless of exit status or output. A `go`/`python3`/`node` shim that exists on PATH but is broken (exits non-zero, prints an error, or is a wrong/stub binary) is reported as a healthy toolchain. The doc comment (main.rs:208-209) states this is intentional ("a non-zero exit still means it exists"), so this is a deliberate trade-off, but it weakens the doctor signal: doctor reports "OK" for a `node` that cannot actually run the version probe, then `generate` fails later. Combined with WR-02 (TS doesn't probe `typescript` at all), the doctor "source toolchain present" line is more optimistic than the actual readiness it implies.
**Fix:** If the intent is "binary is runnable," check the exit status too, while still mapping a not-found spawn error to absent:
```rust
let present = std::process::Command::new(toolchain.probe_binary())
    .arg(version_arg)
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false);
```
If spawn-success-only is genuinely intended, leave as-is but tighten the doc to say doctor verifies the binary is *spawnable*, not *functional* — and reconcile with the `ts.js` "detects the missing toolchain" claim (WR-02).

## Info

### IN-01: `ts.js` realpath of the target is computed twice (sidecar + index.js), one of which is redundant

**File:** `tsextract/ts.js:25` and `tsextract/index.js:62`
**Issue:** Both `ts.js` (`fs.realpathSync(targetArg)`) and `index.js` (`fs.realpathSync(argv[2])`) independently realpath `process.argv[2]`. The host always passes an absolute, already-canonical target (`analyze::build_graph` → `helper::resolve_target` canonicalizes before spawn), so both calls operate on the same absolute path and agree. It works, but the two modules each re-read `process.argv[2]` as an ambient global rather than `index.js` passing the resolved dir to the resolver — slight coupling/duplication. Low impact; flagged only because it pairs with WR-04 (the resolver reaching into `process.argv` directly is what forces eager evaluation).
**Fix:** Pass the resolved `targetDir` from `index.js` into a `resolveTypescript(targetDir)` (ties in with the WR-04 lazy refactor) so there is one realpath and one argv read.

### IN-02: `count_trigger_paths` sums across drained debounce batches, which can over-count duplicates as multi-file

**File:** `crates/gnr8/src/watch.rs:308-312`
**Issue:** The drain loop does `triggers += more` across separately-debounced batches. `count_trigger_paths` de-dups WITHIN a single batch (HashSet), but the same file edited across two coalesced batches counts twice, so a repeated single-file edit can be labeled `multi-file-edit`. This is an acknowledged WR-03 (planning) approximation affecting only the `scenario` label in the latency report, not correctness or loop-safety. Noted for completeness.
**Fix:** If precise labeling matters, accumulate the distinct trigger PATHS across drained batches (send `Vec<PathBuf>` or a path set) and count distinct at the end, rather than summing per-batch counts. Otherwise document the label as approximate.

### IN-03: Doctor source-toolchain render claims a generic "install the source language's toolchain" even when `node` is present but `typescript` is missing

**File:** `crates/gnr8/src/doctor.rs:279-287`
**Issue:** The LIFECYCLE line renders `Source toolchain (typescript): OK` based solely on `node` presence (see WR-02). When `typescript` is the actual missing piece, the report shows OK and gives the user no signal. This is a downstream symptom of WR-02 rather than an independent defect; once WR-02 is addressed the render is correct.
**Fix:** Resolve via WR-02 (probe the real toolchain). No standalone change needed in `doctor.rs`.

---

_Reviewed: 2026-06-26_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
