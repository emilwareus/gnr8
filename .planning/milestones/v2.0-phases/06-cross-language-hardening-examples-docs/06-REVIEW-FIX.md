---
phase: 06-cross-language-hardening-examples-docs
fixed_at: 2026-06-26T00:00:00Z
review_path: .planning/phases/06-cross-language-hardening-examples-docs/06-REVIEW.md
iteration: 1
findings_in_scope: 5
fixed: 5
skipped: 0
status: all_fixed
---

# Phase 06: Code Review Fix Report

**Fixed at:** 2026-06-26
**Source review:** .planning/phases/06-cross-language-hardening-examples-docs/06-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 5 (all 5 WARNINGs; the 3 INFO findings were out of scope for `critical_warning`)
- Fixed: 5
- Skipped: 0

All fixes were applied inside an isolated git worktree, each committed atomically. Acceptance verified:
`make check` exits 0 (GREEN — fmt, clippy `-D warnings`, the full test suite, the cross-language
`examples-check` regen-diff for Go/Python/TS, and the doctor/watch tests), and `cargo test -p gnr8` is
green. The 6 multi-language snapshots and all three examples' committed `generated/` bytes stay
byte-identical (a `gnr8 generate --force` in each example reproduced the committed bytes with zero git
diff; the only first-run `examples-check` noise was an empty, gitignored `.gnr8/cache/manifest.json` in
the fresh worktree, not a content change). `gnr8-core/Cargo.toml` is unchanged — no new dependency; the
TS sidecar stays Node-built-ins only, and `typescript` stays the user's resolved toolchain (never
bundled). Every fix is a single deterministic decision (CLAUDE.md rule 3) and uses typed errors / no
production `unwrap`/`expect`/`panic`.

## Fixed Issues

### WR-01: `watch::is_trigger_path` does not exclude `.gnr8/` source-language files

**Files modified:** `crates/gnr8/src/watch.rs`
**Commit:** da8c0a5
**Applied fix:** Threaded the canonicalized `.gnr8` crate root (`gnr8_root`) through `is_trigger_path`
/ `batch_should_regenerate` / `count_trigger_paths` and the `run` closure, and added a
`path.starts_with(gnr8_root) => false` rule after the output-set filter. A source-language file
anywhere under `.gnr8/` (e.g. `.gnr8/helper.ts`, `.gnr8/scratch/notes.py`) no longer spuriously
triggers regeneration — the runtime filter now matches the module/function docs AND `analyze::scan_markers`
(one consistent `.gnr8/` meaning). The `.gnr8/src/**.rs` pipeline-source trigger is preserved. Added a
regression test (`dot_gnr8_source_language_file_does_not_trigger`) and updated all existing
`is_trigger_path`/batch/count call sites for the new `gnr8_root` parameter.

### WR-02: `doctor` cannot detect a missing `typescript` toolchain

**Files modified:** `crates/gnr8-core/src/analyze/helper.rs`, `crates/gnr8-core/src/analyze/mod.rs`,
`crates/gnr8/src/main.rs`, `tsextract/probe.js` (new)
**Commit:** 634606d
**Applied fix:** Added `tsextract/probe.js` — a doctor health probe that exits 0 iff `typescript` is
resolvable, reusing the EXACT same `ts.resolveTypescript` resolution order (target project, then
sidecar) the extractor uses at generate time, so there is one source of truth (no second detector, no
fallback). Added `helper::typescript_toolchain_present` (spawns `node probe.js <target>` from
`tsextract_dir` with discrete args) and exposed it as `pub fn analyze::typescript_toolchain_present`.
`probe_source_lang_toolchain` now routes the single `source_toolchain` decision: TypeScript → the
node+typescript probe; Go/Python → their discrete binary probe. A NestJS project with `node` but no
`typescript` now reports unhealthy in `doctor` instead of passing-then-failing at `generate`. Added
regression tests in the helper test module (present-from-sidecar; never-panics-for-a-bare-target).
**Note (requires human verification):** the TS probe's *negative* path (node-present-but-typescript-
absent ⇒ unhealthy) is exercised end-to-end by `probe.js` + the `examples-check` wiring, but the Rust
unit test for it can only assert the call returns a `bool` without panic, because the sidecar's own dev
`node_modules` (restored by `make tsextract-deps`) is the always-present second search root in the unit
environment and cannot be removed mid-test cleanly. The behavior was verified manually (probe.js exits
1 with a clean one-line stderr when `tsextract/node_modules` is hidden).

### WR-03: `scan_markers` only excludes `.gnr8/` — build/vendor dirs spoof a false ambiguity

**Files modified:** `crates/gnr8-core/src/analyze/mod.rs`
**Commit:** 19e47c9
**Applied fix:** Widened the single deterministic skip set in `scan_markers` from just `.gnr8` to
`{ .gnr8, .git, node_modules, target }`, mirroring `tsextract/load.js`'s `node_modules` skip. A
single-language tree whose root also holds a build/vendor/VCS dir with a vendored other-language file
no longer trips the ambiguity guard. Still one deterministic walk (no fallback). Added a regression
test (`detect_language_skips_build_and_vendor_dirs_so_vendored_files_do_not_spoof_ambiguity`).

### WR-04: `ts.js` resolver runs at `require` time, bypassing `index.js`'s clean error formatting

**Files modified:** `tsextract/ts.js`, `tsextract/index.js`
**Commit:** 8984097
**Applied fix:** Made `typescript` resolution lazy and memoized via a `Proxy` whose first member access
triggers `resolveTypescript(process.argv[2])` — so the throw fires inside `load()` (which runs inside
`main()`'s try/catch), not at module-evaluation time. Every existing `const ts = require("./ts")`
consumer keeps working unchanged. The resolve error is tagged `toolchainMissing`, and `index.js`'s
catch renders tagged errors as the clean one-line stderr diagnostic (message only) while other failures
keep the full stack. The deterministic resolution order (target project, then sidecar) is unchanged.
Verified: a missing `typescript` now prints exactly one clean stderr line and exits 1.
**Co-fixed IN-01:** `resolveTypescript` now takes the caller's target dir, eliminating the duplicate
`process.argv[2]` `realpathSync` in `ts.js` (one realpath, in `index.js`).

### WR-05: `probe_source_lang_toolchain` treats spawn-success as toolchain-present

**Files modified:** `crates/gnr8/src/main.rs`
**Commit:** 634606d
**Applied fix:** Changed the Go/Python probe from `.output().is_ok()` (spawn-only) to
`.output().is_ok_and(|o| o.status.success())`, so a binary that exists on PATH but exits non-zero
(broken/stub) is now reported absent. A spawn `io::Error` (not found) still maps to absent. The
TypeScript arm already requires a successful probe exit via the WR-02 `node probe.js` check.
**Note (requires human verification):** this tightens the health semantics; the doctor's "source
toolchain present" line now reflects a *functional* probe rather than a *spawnable* binary. Worth a
manual confirm that no CI/doctor expectation depended on the looser spawn-only behavior (the full
`make check` doctor `--json` shape tests pass).

## Skipped Issues

No in-scope findings were skipped.

### Out-of-scope INFO findings (fix_scope = critical_warning)

- **IN-01** (`ts.js` double realpath): co-fixed as part of WR-04 — `resolveTypescript` now takes the
  target dir from the caller, so there is one realpath read.
- **IN-02** (`count_trigger_paths` sums across drained debounce batches; can over-count duplicates as
  multi-file): NOT fixed. It affects only the `scenario` label in the watch latency report, not
  correctness or loop-safety, and the reviewer flagged it as an acknowledged approximation. Fixing it
  cleanly requires sending distinct `Vec<PathBuf>` across batches (a larger watch-channel refactor) and
  was not trivially co-fixable with the warning fixes.
- **IN-03** (doctor renders "install the source toolchain" generically when `node` is present but
  `typescript` is missing): resolved transitively by WR-02 — the doctor source-toolchain probe now
  reflects real TS readiness, so the render is correct without a standalone `doctor.rs` change.

---

_Fixed: 2026-06-26_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
