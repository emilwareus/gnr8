---
phase: 05-typescript-target-tssdk
plan: 03
subsystem: testing
tags: [typescript, tsc, tssdk, hermetic-test, supply-chain, determinism, codegen]

# Dependency graph
requires:
  - phase: 05-01
    provides: tssdk::generate + write_to_dir (dependency-free TS SDK bundle)
  - phase: 05-02
    provides: TsSdk Target driving tssdk via the Pipeline (package name source of truth)
  - phase: 04
    provides: tsextract path build_graph routes to (NestJS IR via the typescript Compiler API)
provides:
  - "Hermetic acceptance test crates/gnr8-core/tests/tssdk_compile.rs: build IR -> generate TS SDK -> write to a unique temp dir -> tsc --noEmit --strict --lib es2022,dom (exit 0) over the generated files"
  - "Supply-chain grep gate: every generated .ts proven free of axios/node-fetch/@types/from \"http\""
  - "Two-run byte-identical determinism check + invalid-TS-captured-as-typed-error check"
  - "tssdk_compile wired into the explicit --test list in the green make check/gates gate"
  - "Rule-1 codegen fix: ts_type now namespace-qualifies named refs so client.ts param types resolve through its models import (fixed TS2304)"
affects: [verification, future-sdk-targets, tssdk]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Hermetic SDK typecheck gate (the TS analog of pysdk_compile py_compile+import): generate -> write to a PID+nanosecond stdlib temp dir -> run the language's own reference compiler with discrete Command args + current_dir; reuse the VENDORED typescript, no npm install"
    - "ts_type carries an `ns` namespace prefix so the SAME exhaustive type-mapping serves both models.ts (bare) and client.ts (models.-qualified) with one path (rule 3)"

key-files:
  created:
    - crates/gnr8-core/tests/tssdk_compile.rs
  modified:
    - crates/gnr8-core/src/tssdk/emit.rs
    - Makefile

key-decisions:
  - "ts_type gained an `ns: &str` namespace-prefix arg (not a second code path): models.ts passes \"\" (bare sibling symbols), client.ts passes \"models.\" (reach models.ts through its namespace import). The prefix is the caller's context, not a fallback — one exhaustive match, rule 3."
  - "The non-zero tsc exit reuses the generic captured-stderr CoreError::GoBuild { code, stderr } carrier (no new error variant), folding tsc's stdout (where diagnostics land) + stderr into the message; spawn failure maps to TypeScriptToolchainMissing — exactly the pysdk_compile discipline."
  - "Did NOT port the pysdk round-trip http.server driver (TSSDK-02 asks only for tsc --noEmit, RESEARCH Open Q3); added a determinism (two-run byte-identical) test instead."

patterns-established:
  - "Pattern: --lib es2022,dom is load-bearing in the tsc invocation — lib.dom.d.ts declares the fetch global so the dependency-free SDK needs no @types/node (omit ,dom -> TS2304: Cannot find name 'fetch')."

requirements-completed: [TSSDK-02]

# Metrics
duration: 11min
completed: 2026-06-26
---

# Phase 5 Plan 3: Hermetic TypeScript SDK Typecheck Gate Summary

**A hermetic `tsc --noEmit --strict --lib es2022,dom` acceptance test that generates the NestJS TS SDK, type-checks it against the vendored compiler (exit 0), grep-proves zero runtime deps, and — on first run — caught a real TS2304 codegen bug now fixed and wired into the green `make check` gate.**

## Performance

- **Duration:** ~11 min
- **Started:** 2026-06-26 (phase 05 execution)
- **Completed:** 2026-06-26
- **Tasks:** 2
- **Files modified:** 3 (1 created, 2 modified)

## Accomplishments
- `crates/gnr8-core/tests/tssdk_compile.rs`: builds the IR from `fixtures/nestjs-bookstore` via `build_graph` (the Phase-4 tsextract path), generates the TS SDK, writes it flat into a unique stdlib temp dir, and runs the VERIFIED `node <vendored tsc> --noEmit --strict --target es2022 --module esnext --moduleResolution bundler --lib es2022,dom <each .ts>` (asserts exit 0). Discrete `Command` args + `current_dir` (never a shell string).
- Supply-chain grep gate over every generated `.ts`: no `axios`/`node-fetch`/`@types`/`from "http"`.
- Toolchain-skip guard (node + vendored tsc), invalid-TS-as-captured-`CoreError` test (no panic), and a two-run byte-identical determinism test.
- The gate caught a genuine codegen defect on first run (`client.ts: error TS2304: Cannot find name 'BookFormat'`) — fixed in `tssdk::emit` and locked with a unit test.
- Wired `tssdk_compile` into the explicit `--test` list in the `gates` target; both `make check` and `make gates` are green (exit 0) with go on PATH.

## Task Commits

1. **Task 1: Hermetic tssdk_compile.rs + the TS2304 codegen fix it caught** - `8f0edfb` (test + fix)
2. **Task 2: Wire tssdk_compile into the make check/gates gate** - `7a868da` (test)

## Files Created/Modified
- `crates/gnr8-core/tests/tssdk_compile.rs` - the hermetic generate -> write -> `tsc --noEmit` typecheck + banned-import grep + skip guard + determinism + invalid-input-captured-error.
- `crates/gnr8-core/src/tssdk/emit.rs` - `ts_type` gained an `ns` namespace-prefix arg; client.ts param emission passes `"models."`, models.ts passes `""`; new unit test `named_ref_is_namespace_qualified_for_client_ts_context`.
- `Makefile` - added `--test tssdk_compile` to the explicit `gates` integration-test list + updated the doc comment.

## Decisions Made
- See key-decisions in frontmatter. In short: namespace-prefix arg (not a dual path) for `ts_type`; reuse `GoBuild` as the generic exit-code+stderr carrier; skip the round-trip driver in favor of a determinism check.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Named-enum/model param emitted a BARE symbol in client.ts (TS2304)**
- **Found during:** Task 1 (the new `tsc --noEmit` gate's first run)
- **Issue:** `tssdk::emit::ts_type` resolved a `Type::Named` (and arrays/unions/maps over it) to a bare name like `BookFormat`. Whole-body/response model refs were manually qualified `models.X` in the operations emitter, but a query/path PARAM whose schema is a named enum/model went through `ts_type` and emitted an UNQUALIFIED name. Those symbols live in `models.ts`, not in scope in `client.ts`, so the generated client failed to type-check: `client.ts(57,39): error TS2304: Cannot find name 'BookFormat'` (exit 2). This is exactly the class of bug the new typecheck gate exists to catch — invisible to the string-assertion unit tests.
- **Fix:** Added an `ns: &str` namespace-prefix parameter to `ts_type` (one function, one exhaustive match — NOT a second code path, rule 3). `models.ts` callers pass `""` (bare sibling symbols); `client.ts` param callers pass `"models."` so a named ref resolves through the client's `models` namespace import. The prefix threads through arrays/maps/unions. Locked with a new unit test asserting `models.BookFormat` and `models.BookFormat[]`.
- **Files modified:** crates/gnr8-core/src/tssdk/emit.rs
- **Verification:** `cargo test -p gnr8-core --test tssdk_compile` now exits 0 (the generated SDK type-checks); `cargo test -p gnr8-core --lib tssdk` green (38 passed) incl. the new namespace test; `make check` + `make gates` green.
- **Committed in:** `8f0edfb` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug, Rule 1)
**Impact on plan:** The fix was necessary for the plan's core success criterion (the generated SDK must type-check). It is a minimal, single-path change in the emitter and validates the whole premise of the TSSDK-02 gate. No scope creep.

## Issues Encountered
- None beyond the Rule-1 codegen bug above (which the gate is designed to surface). `cargo`/`go` are not on the default sandbox PATH; sourced `~/.cargo/env` and exported the Go bin dir per the sandbox-toolchains memory.

## User Setup Required
None - no external service configuration required (typescript already vendored at `tsextract/node_modules`, no new install).

## Next Phase Readiness
- Phase 05 (TypeScript Target — TsSdk) is functionally complete: the TS SDK generates, the TsSdk Target drives it, and the hermetic typecheck gate proves it compiles dependency-free and deterministically. Ready for phase verification.
- No new crate, no npm install; `Cargo.toml`/`Cargo.lock` unchanged; `tsextract/node_modules` untouched (rule 2 intact).

---
*Phase: 05-typescript-target-tssdk*
*Completed: 2026-06-26*

## Self-Check: PASSED

- FOUND: crates/gnr8-core/tests/tssdk_compile.rs
- FOUND: crates/gnr8-core/src/tssdk/emit.rs
- FOUND: Makefile
- FOUND: .planning/phases/05-typescript-target-tssdk/05-03-SUMMARY.md
- FOUND commit: 8f0edfb (Task 1)
- FOUND commit: 7a868da (Task 2)
