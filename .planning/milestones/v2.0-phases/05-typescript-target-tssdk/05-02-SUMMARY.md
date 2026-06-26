---
phase: 05-typescript-target-tssdk
plan: 02
subsystem: api
tags: [typescript, sdk, codegen, target, builtins, prelude, dependency-free]

# Dependency graph
requires:
  - phase: 05-typescript-target-tssdk (plan 01)
    provides: the crate::tssdk module (generate / split_bundle / write_to_dir) the Target consumes
  - phase: 03-python-target-pysdk
    provides: the PySdk Target analog cloned verbatim (struct/builders/impl Target/tests)
provides:
  - "TsSdk Target built-in in sdk::builtins (struct + builders + impl Default + impl Target)"
  - "TsSdk re-exported from sdk::prelude (use gnr8_core::sdk::prelude::*; resolves TsSdk)"
  - "TsSdk Target tests: unconfigured -> typed Config error; writes-under-dir + output_anchors + two-run byte-identical"
affects: [05-03 hermetic tsc typecheck test consumes tssdk::generate/write_to_dir]

# Tech tracking
tech-stack:
  added: []  # zero new crates (CLAUDE.md rule 2); generated SDK stays dependency-free
  patterns:
    - "Target built-in as a verbatim structural clone of the PySdk twin (only crate::pysdk:: -> crate::tssdk:: and PySdk -> TsSdk labels change)"
    - "Single source of truth: sdk_package(self.module) + ir.base_path reused verbatim — no second derivation (rule 3)"
    - "output_anchors() returns the trimmed output dir as the loop-safety anchor so a re-run never re-ingests the generated .ts"

key-files:
  created: []
  modified:
    - crates/gnr8-core/src/sdk/builtins.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/src/tssdk/mod.rs

key-decisions:
  - "[05-02] TsSdk Target is a verbatim structural clone of PySdk reusing sdk_package + ir.base_path as the single source of truth; no new CoreError variant, no TS-specific sanitizer, no second name/base-path derivation (rule 3)"
  - "[05-02] The scoped #[allow(dead_code)] on tssdk::split_bundle is removed now that the TsSdk Target consumes it in-crate (its dedicated Plan-02 caller has landed)"
  - "[05-02] TsSdk sorts after SetTitle in the alphabetized prelude builtins re-export (Se < Ts)"

requirements-completed: [TSSDK-03]

# Metrics
duration: 9min
completed: 2026-06-26
---

# Phase 05 Plan 02: TsSdk Target built-in Summary

**The rule-4 enablement seam: a `TsSdk` `Target` built-in (a verbatim PySdk clone wired to `crate::tssdk`) that a developer composes into a `.gnr8/` Pipeline as code, deriving the package via `sdk_package` and the base path via `ir.base_path` from the single sources of truth, with a typed Config error when unconfigured, an unsafe-name write guard, deterministic byte-identical output, and an `output_anchors` loop-safety anchor — re-exported from `sdk::prelude`.**

## Performance

- **Duration:** ~9 min
- **Completed:** 2026-06-26
- **Tasks:** 2
- **Files modified:** 3 (0 created, 3 modified)

## Accomplishments
- `TsSdk` struct + `new`/`module`/`to` builders + `impl Default` + `impl Target` added to `sdk/builtins.rs`, a verbatim structural clone of `PySdk` — the only changes are `crate::pysdk::` -> `crate::tssdk::` and the `PySdk` -> `TsSdk` Config/SdkGen error-message labels.
- `TsSdk::generate` derives the package via `sdk_package(&self.module)` and generates via `crate::tssdk::generate(ir, &package, &ir.base_path)` — the SAME single sources of truth (rule 3), with no second derivation and no TS-specific sanitizer.
- Empty module / empty dir each return a typed `CoreError::Config` (asserted, not a panic); the unsafe-name guard rejects `/`, `\`, `..`, empty frame names with `CoreError::SdkGen`; `output_anchors()` returns the trimmed dir.
- `TsSdk` re-exported from `sdk::prelude` (alphabetized, after `SetTitle`) so `use gnr8_core::sdk::prelude::*;` resolves it in a `.gnr8/` crate.
- The scoped `#[allow(dead_code)]` on `tssdk::split_bundle` removed — the TsSdk Target is now its in-crate consumer.
- New tests: `tssdk_target_errors_when_unconfigured` (empty module + module-but-no-dir each -> Config) and `tssdk_target_writes_under_the_output_dir_and_is_deterministic` (writes-under-dir + `output_anchors` + two-run byte-identical).

## Task Commits

Each task was committed atomically:

1. **Task 1: TsSdk Target built-in (clone of PySdk) + tests + remove the split_bundle allow** - `104110c` (feat)
2. **Task 2: Re-export TsSdk from sdk::prelude** - `af3e116` (feat)

_Task 1 was marked tdd="true"; the Target and its clone-and-adapt tests were ported together (mirroring the Plan-01 approach) and verified green at the task boundary._

## Files Created/Modified
- `crates/gnr8-core/src/sdk/builtins.rs` - added the `TsSdk` struct + `impl Target` (verbatim PySdk clone wired to `crate::tssdk`, reusing `sdk_package` + `ir.base_path`); added `TsSdk` to the test-module `use` list; added the two `tssdk_target_*` tests.
- `crates/gnr8-core/src/sdk/mod.rs` - added `TsSdk` to the alphabetized `builtins::{...}` prelude re-export.
- `crates/gnr8-core/src/tssdk/mod.rs` - removed the now-obsolete `#[allow(dead_code)]` on `split_bundle` (the TsSdk Target consumes it) and trimmed its dead-code doc note.

## Decisions Made
See key-decisions frontmatter. Headline: TsSdk is a verbatim PySdk clone reusing `sdk_package` + `ir.base_path` (no new error variant, no second derivation, no TS-specific sanitizer); the `split_bundle` `#[allow(dead_code)]` is retired now that its Plan-02 caller exists.

## Deviations from Plan

None - plan executed exactly as written. (The plan anticipated removing the `tssdk::split_bundle` `#[allow(dead_code)]`; that was carried out as the documented consequence of wiring the Target, not as an out-of-scope change.)

## Threat Surface
All four mitigations from the plan's `<threat_model>` are implemented:
- T-05-02-01 (path traversal): unsafe-name write guard rejects `/`, `\`, `..`, empty before `out.write`.
- T-05-02-02 (derivation drift): `sdk_package(self.module)` + `ir.base_path` are the only sources.
- T-05-02-03 (loop): `output_anchors()` returns the output dir.
- T-05-02-04 (unconfigured): empty module/dir -> typed `CoreError::Config`, no panic.

No new security surface beyond the plan's threat register.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `TsSdk` is composable in a `.gnr8/` Pipeline and reachable via `sdk::prelude`.
- Plan 03 can drive the hermetic `tsc --noEmit --strict --lib es2022,dom` typecheck over the emitted SDK (the Target + `tssdk::generate`/`write_to_dir` path is fully wired).
- No blockers.

## Self-Check: PASSED
- FOUND: `struct TsSdk` in crates/gnr8-core/src/sdk/builtins.rs
- FOUND: `TsSdk` in the prelude re-export of crates/gnr8-core/src/sdk/mod.rs
- FOUND: `crate::tssdk::` usage in crates/gnr8-core/src/sdk/builtins.rs
- FOUND commit 104110c (Task 1)
- FOUND commit af3e116 (Task 2)

---
*Phase: 05-typescript-target-tssdk*
*Completed: 2026-06-26*
