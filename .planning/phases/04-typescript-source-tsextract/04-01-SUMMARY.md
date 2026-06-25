---
phase: 04-typescript-source-tsextract
plan: 01
subsystem: api
tags: [typescript, nestjs, tsextract, node, sidecar, language-detection, source-builtin]

# Dependency graph
requires:
  - phase: 01-language-neutral-ir
    provides: neutral facts contract (facts.rs deny_unknown_fields), Lang/detect_language/build_graph seam, the 2 red-by-design nestjs snapshots
  - phase: 02-python-source-pyextract
    provides: run_pyextract/PythonToolchainMissing/FastApi+Flask Source — the exact twins cloned here
provides:
  - "Lang::TypeScript as a first-class language: 3-way deterministic detect_language → 3-arm dispatch (build_graph AND diagnostics::collect) → run_tsextract → node sidecar"
  - "TypeScriptToolchainMissing typed CoreError (no panic)"
  - "helper::run_tsextract / tsextract_dir subprocess driver (discrete args, no shell)"
  - "NestJs Source built-in (prelude-exported) wrapping the SAME build_graph"
  - "tsextract/ Node package skeleton: pinned typescript 5.9.3 (sole dep), index.js stub emitting an empty-but-valid facts envelope; typescript vendored (committed) for hermetic offline tests"
affects: [04-02-tsextract-extractor, 04-03-nestjs-snapshots, 05-typescript-sdk]

# Tech tracking
tech-stack:
  added: ["typescript@5.9.3 (tsextract sole dep — the documented rule-2 carve-out; gnr8-core adds ZERO crates)"]
  patterns:
    - "Count-based single-decision language classification (no try-then-fallback, rule 3) extended to 3 markers"
    - "Node sidecar twin of goextract/pyextract: stdout=facts JSON only, stderr=tool errors, nonzero-exit-on-failure"
    - "Vendored single dependency committed via .gitignore negation (hermetic, offline tests)"

key-files:
  created:
    - tsextract/package.json
    - tsextract/package-lock.json
    - tsextract/index.js
    - tsextract/node_modules/typescript/** (vendored, committed)
  modified:
    - crates/gnr8-core/src/analyze/mod.rs
    - crates/gnr8-core/src/analyze/helper.rs
    - crates/gnr8-core/src/diagnostics/mod.rs
    - crates/gnr8-core/src/error.rs
    - crates/gnr8-core/src/sdk/builtins.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - .gitignore

key-decisions:
  - "detect_language is a SINGLE count-based classification over {go,python,ts} markers; exactly-1 → that Lang, 0/>1 → typed Config error (rule 3, no fallback chain)"
  - "TS marker = tsconfig.json OR *.ts — *.ts is REQUIRED (the nestjs fixture has no tsconfig); RESEARCH Pitfall 7"
  - "Both dispatch seams (build_graph + diagnostics::collect) are 3-arm exhaustive over the closed Lang enum, NO _ => catch-all"
  - "NestJs Source calls the SAME crate::analyze::build_graph — language detected from the target, never from which Source (rule 3/4)"
  - "VENDORING Option A (hermetic): committed tsextract/node_modules/typescript via a .gitignore negation; package-lock.json committed; tests run offline with no npm ci"
  - "tsextract sole dep is typescript pinned to EXACT 5.9.3 (a floating ^5 could change typeToString formatting; a second dep is a rule-2 defect)"

patterns-established:
  - "3-marker deterministic language dispatch mirrored identically across both seams"
  - "run_tsextract twins run_pyextract: _with(bin) split for the toolchain-missing test, discrete args (no sh -c, threat T-04-01), typed errors"
  - "NestJs Source is a verbatim FastApi/Flask clone differing only in the error proper noun"

requirements-completed: [TSSRC-04]

# Metrics
duration: 6min
completed: 2026-06-25
---

# Phase 4 Plan 01: TypeScript Source Seam (`tsextract` skeleton) Summary

**`Lang::TypeScript` is now first-class end-to-end — a single deterministic 3-way `detect_language`, a typed `TypeScriptToolchainMissing` error, a discrete-arg `run_tsextract` node driver wired into both dispatch seams, a prelude-exported `NestJs` Source, and a hermetic `tsextract/` package (pinned + vendored `typescript`) that emits a valid empty facts envelope.**

## Performance

- **Duration:** ~6 min
- **Started:** 2026-06-25T22:18:09Z
- **Completed:** 2026-06-25T22:24:07Z
- **Tasks:** 3
- **Files modified:** 6 modified + 4 created (excluding the 135 vendored typescript files)

## Accomplishments
- `Lang` gains `TypeScript`; `detect_language` rewritten as a single count-based classification over the three marker booleans (no try-then-fallback, rule 3); mixed trees → typed `Config` ambiguity error; `scan_markers` now sets a `has_ts` flag for `tsconfig.json` OR `*.ts`.
- `TypeScriptToolchainMissing` `CoreError` variant (mirrors `PythonToolchainMissing`) with a Display test; never a panic.
- `helper::run_tsextract` / `tsextract_dir` drive `node index.js <target_dir>` with discrete args (no shell, threat T-04-01) and typed errors; `build_graph` AND `diagnostics::collect` both dispatch `Lang::TypeScript` (3-arm, no `_ =>`).
- `NestJs` Source built-in (clone of `FastApi`/`Flask`) calling the SAME `build_graph`; exported in the prelude; zero/many-input Config-error test.
- `tsextract/` package: `package.json` with exactly one pinned dep (`typescript 5.9.3`); `index.js` stub mirroring pyextract's strict stdout/stderr/exit discipline, emitting `{module,routes,schemas,diagnostics}` with empty arrays; `typescript` vendored (committed) for offline tests.

## Task Commits

Each task was committed atomically:

1. **Task 1: Lang/detect_language/scan_markers + TypeScriptToolchainMissing + run_tsextract + build_graph arm** - `f7f434c` (feat)
2. **Task 2: Lang::TypeScript dispatch arm in diagnostics::collect** - `65a093b` (feat)
3. **Task 3: NestJs Source built-in + tsextract package skeleton + vendoring + .gitignore** - `899e85a` (feat)

_Note: Task 1's commit folds in the `run_tsextract` driver and the `build_graph` dispatch arm (Task 2 scope) because the closed `Lang` enum makes those changes mutually required for the crate to compile — each commit must build green under the pre-commit hooks. Task 2's separate commit lands the SECOND dispatch seam (diagnostics::collect)._

## Files Created/Modified
- `crates/gnr8-core/src/analyze/mod.rs` - `Lang::TypeScript`; count-based 3-way `detect_language`; 3-param `scan_markers` (ts marker); `build_graph` TS arm; detect/ambiguity tests.
- `crates/gnr8-core/src/analyze/helper.rs` - `tsextract_dir`, `run_tsextract`/`run_tsextract_with`; toolchain-missing + dir tests.
- `crates/gnr8-core/src/diagnostics/mod.rs` - `Lang::TypeScript` dispatch arm in `collect`.
- `crates/gnr8-core/src/error.rs` - `TypeScriptToolchainMissing` variant + Display test.
- `crates/gnr8-core/src/sdk/builtins.rs` - `NestJs` Source + zero/many-input test.
- `crates/gnr8-core/src/sdk/mod.rs` - `NestJs` prelude export.
- `tsextract/package.json` - sidecar manifest, sole dep `typescript 5.9.3`.
- `tsextract/package-lock.json` - committed lockfile.
- `tsextract/index.js` - entrypoint stub (argv → target → empty facts JSON on stdout).
- `tsextract/node_modules/typescript/**` - vendored (135 tracked files, Option A).
- `.gitignore` - `node_modules/` ignored with a `!tsextract/node_modules/**` negation.

## Decisions Made
- **Vendoring: Option A (hermetic).** Per the plan default and the toolchain note: the sandbox has `typescript@5.9.3` (zero transitive deps, ~23M, 0 vulnerabilities). Committing it via a `.gitignore` negation means the W3 snapshot tests run offline with no `npm ci`, mirroring the Go test's offline ethos. `git ls-files tsextract/node_modules/typescript/package.json` is non-empty (135 tracked node_modules files). Option B (ignore + `npm ci` Make target) was NOT used; the 23M commit was not rejected.
- **detect_language as a count, not nested boolean tuples.** Counting present languages (`go + python + ts`) keeps the single-decision shape clean at three markers and makes the >1 ambiguity arm trivially correct, with no `_ =>` catch-all that could silently pick a language.
- **Task 1 commit folds in the compile-critical `run_tsextract` + `build_graph` arm** (see Task Commits note) so every commit builds green; Task 2's commit isolates the second dispatch seam.

## Deviations from Plan
None - plan executed exactly as written. (The Task 1/Task 2 file grouping is a commit-boundary detail driven by Rust exhaustiveness, not a scope or behavior change — both seams and the driver are present exactly as specified.)

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required. The sole sidecar dependency (`typescript`) is vendored and committed; `node` (v24.14.1) is present in the sandbox.

## Next Phase Readiness
- The seam is wired end-to-end and exercisable: `node tsextract/index.js fixtures/nestjs-bookstore` emits a valid empty facts envelope; `detect_language` classifies the fixture as `Lang::TypeScript`; both dispatch seams route to `run_tsextract`.
- The 2 NestJS snapshot tests (`snapshot_nestjs_graph`, `snapshot_nestjs_openapi`) remain `#[ignore]` red-by-design — they flip green in 04-02/04-03 when the real Compiler-API extractor replaces the `index.js` stub.
- 04-02 plugs the real extractor (load/routes/types/schemas/diagnostics/facts) into the existing `index.js` stub; no further Rust host changes are needed for the contract.

## Self-Check: PASSED

- Created files verified present: `tsextract/package.json`, `tsextract/package-lock.json`, `tsextract/index.js`, `tsextract/node_modules/typescript/package.json` (vendored, tracked), `04-01-SUMMARY.md`.
- Task commits verified in git history: `f7f434c`, `65a093b`, `899e85a`.
- `cargo test -p gnr8-core` green (174 lib + all integration; the 3 red-by-design snapshots stay `#[ignore]`); `cargo clippy -p gnr8-core --all-targets` clean; `git diff` of any `Cargo.toml` is empty (gnr8-core adds ZERO crates).

---
*Phase: 04-typescript-source-tsextract*
*Completed: 2026-06-25*
