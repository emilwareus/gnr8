---
phase: 03-openapi-and-go-sdk-generation
plan: 03
subsystem: testing
tags: [rust, go-build, httptest, hermetic, ci, determinism, integration-test, sdk-compile]

# Dependency graph
requires:
  - phase: 03-openapi-and-go-sdk-generation
    provides: "03-02 built sdk::generate (file-marker-framed bundle String) + write_to_dir; 03-01 added CoreError::GoBuild and lower::to_openapi"
  - phase: 02-go-analysis-and-api-graph
    provides: "analyze::build_graph (byte-stable ApiGraph) consumed by the compile/determinism tests"
provides:
  - "tests/sdk_compile.rs — SDK-05 proof: materializes the generated SDK to a hermetic stdlib-only temp module (zero-require go.mod), runs `go build ./...` (exit 0), and an httptest `go test` smoke (CreateGoal POST /goal/ + decoded CommandMessageWithUUID + DeleteGoal 404 -> *APIError, SDK-04)"
  - "Public sdk::write_to_dir(bundle: &str, dir: &Path) — string-based, test-callable disk writer with frame-name safety checks (T-03-03)"
  - "tests/determinism.rs — extended: to_openapi + sdk::generate each byte-identical across two runs (idempotent generation)"
  - "All four contract tests (graph/diagnostics/openapi/sdk) + sdk_compile promoted to the BLOCKING CI gates job; the non-blocking contract (continue-on-error) job retired (D-07)"
affects: [04-gnr8-workspace-lifecycle, 05-poc-hardening-and-demo]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Hermetic Go compile gate from Rust: zero-dependency std::env::temp_dir() subdir (PID + nanosecond, no tempfile crate) + a generated zero-require go.mod (module gnr8sdktest, go 1.26) so `go build`/`go test` never touch the module proxy (GOPROXY=off-safe, RESEARCH Pitfall 5)"
    - "Subprocess harness mirrors helper.rs: discrete-arg Command::args + current_dir (no shell, T-03-03-01); a non-zero exit maps to CoreError::GoBuild (never an unwrap/panic on the subprocess Result, T-03-03-04)"
    - "Smoke *_test.go written by the Rust harness (NOT in the snapshot-ed SDK bundle) so the bundle stays production-SDK-only; the smoke shares the SDK's package read from the materialized files (source of truth, not hardcoded)"
    - "Graceful-skip on a missing Go toolchain (early return) so non-Go envs never hard-fail; CI (go 1.26) runs it for real"

key-files:
  created:
    - crates/gnr8-core/tests/sdk_compile.rs
  modified:
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/tests/determinism.rs
    - .github/workflows/ci.yml
    - Makefile

key-decisions:
  - "write_to_dir made public + retyped to take the generated bundle String (not the crate-private SdkBundle) so the out-of-crate integration test can materialize the SDK without exposing SdkBundle — keeps the type private while satisfying the plan's test-callable seam."
  - "Declined the tempfile crate (RESEARCH [ASSUMED]); used the zero-dependency std::env::temp_dir() + a PID/nanosecond-unique subdir (threat T-03-03-SC) — the plan stays autonomous, no package-legitimacy checkpoint needed."
  - "go build/go test run with GOPROXY=off + GOFLAGS=-mod=mod as belt-and-braces hermeticity on top of the zero-require go.mod, so a stray import can never silently reach the network in CI."
  - "Smoke test reads the package clause from the written models.go rather than hardcoding `goalservice`, so it can never drift from the generated SDK's package name."

patterns-established:
  - "SDK-05 acceptance = the real Go compiler + a real HTTP round-trip, not a string snapshot (RESEARCH Pitfall 3): go build proves it compiles, httptest proves it answers a request and decodes the typed response + 4xx APIError."
  - "Defense-in-depth on a program-controlled disk writer: reject empty / separator-bearing / `..` frame names so a malformed bundle can never traverse out of the temp dir (T-03-03)."

requirements-completed: [SDK-05]

# Metrics
duration: 9min
completed: 2026-06-24
---

# Phase 3 Plan 3: SDK Compile/Smoke + Determinism + CI Promotion Summary

**The generated Go SDK is proven end-to-end — it `go build`s clean in a hermetic stdlib-only temp module and answers a real `httptest` round-trip (CreateGoal POST /goal/ -> decoded `CommandMessageWithUUID`, plus a 404 -> `*APIError`) — `to_openapi` and `sdk::generate` are asserted byte-identical across two runs, and all four contract tests + the new `sdk_compile` gate now run as BLOCKING CI gates with the non-blocking `contract` job retired.**

## Performance

- **Duration:** 9 min
- **Started:** 2026-06-24T19:14:00Z
- **Completed:** 2026-06-24T19:23:15Z
- **Tasks:** 3
- **Files modified:** 5 (1 created, 4 modified)

## Accomplishments
- `tests/sdk_compile.rs` (265 lines): `build_graph` -> `sdk::generate` -> `write_to_dir` into a unique `std::env::temp_dir()` subdir + a generated zero-require `go.mod` (`module gnr8sdktest`, `go 1.26`); `go build ./...` exits 0 (SDK-05 compiles). A harness-written `smoke_test.go` spins up `httptest` servers and asserts: `CreateGoal` sends `POST /goal/` with the marshaled body and decodes the 201 `CommandMessageWithUUID.UUID`; `DeleteGoal` against a 404 returns a `*APIError` with `StatusCode == 404` and `IsNotFound()` (SDK-04 typed error). A third test proves a `go build` of invalid Go maps to `CoreError::GoBuild` (carrying captured stderr), never a panic.
- The compile test is fully hermetic — zero-require `go.mod` plus `GOPROXY=off` means `go build`/`go test` never reach the network; it skips gracefully (early return) when the Go toolchain is absent.
- `tests/determinism.rs` extended from one to **three** passing tests: graph, `to_openapi`, and `sdk::generate` are each byte-identical across two `build_graph` runs (idempotent generation, RESEARCH Pitfall 4 / TARGET-API §5.6).
- D-07 CI promotion: the BLOCKING `gates` job now runs `snapshot_graph/diagnostics/openapi/sdk` + `determinism` + `sdk_compile`; the non-blocking `contract` (continue-on-error) job is removed entirely (no `continue-on-error` remains in `ci.yml`). The `Makefile` `gates` target folds in the four contract tests + `sdk_compile`, and the `contract` target is retired from `.PHONY` and recipes.
- `make gates` and `make check` pass end-to-end (full suite green — no red-by-design failures remain); `cargo fmt --check` + `cargo clippy --all-targets --all-features --locked -- -D warnings` clean.

## Task Commits

Each task was committed atomically:

1. **Task 1: tests/sdk_compile.rs — hermetic temp dir + go build + httptest smoke (SDK-05)** - `481bdae` (test)
2. **Task 2: extend tests/determinism.rs with to_openapi + sdk::generate two-run byte-identical asserts** - `c00c614` (test)
3. **Task 3: promote all four contract tests + sdk_compile to blocking gates; retire contract job (D-07)** - `b3f8357` (ci)

**Plan metadata:** committed with this SUMMARY + STATE.md + ROADMAP.md + REQUIREMENTS.md.

## Files Created/Modified
- `crates/gnr8-core/tests/sdk_compile.rs` (created) — SDK-05 compile + httptest smoke gate: unique temp dir, zero-require go.mod, `go build ./...`, harness-written `smoke_test.go` (`CreateGoal` POST + `DeleteGoal` 404 -> `*APIError`), and a `go build`-failure -> `CoreError::GoBuild` (no-panic) test. Skips gracefully without the Go toolchain.
- `crates/gnr8-core/src/sdk/mod.rs` (modified) — `write_to_dir` made `pub` and retyped to take the generated bundle **String** (keeps `SdkBundle` crate-private while the integration test can call it); rejects empty / separator-bearing / `..` frame names (T-03-03 temp-dir hygiene).
- `crates/gnr8-core/tests/determinism.rs` (modified) — added `to_openapi_is_byte_identical_across_two_runs` and `sdk_generate_is_byte_identical_across_two_runs`, each following the graceful-skip pattern; module doc updated to cover both downstream artifacts.
- `.github/workflows/ci.yml` (modified) — `gates` job test step now runs all four contract tests + `determinism` + `sdk_compile` (blocking); the `contract` (continue-on-error) job removed; header rewritten (red-by-design era over).
- `Makefile` (modified) — `gates` target folds in `snapshot_openapi`/`snapshot_sdk`/`sdk_compile`; `contract` target retired from `.PHONY` + recipes; header + `check` comment updated.

## Decisions Made
- **`write_to_dir` public + String-based:** The plan's `<interfaces>` referenced `sdk::write_to_dir`, but it was `pub(crate)` and took the crate-private `SdkBundle` — uncallable from an out-of-crate integration test. Retyped it to take the public `generate` output (the framed bundle String) and made it `pub`, so the test materializes the SDK without exposing `SdkBundle`. The shared `bundle::parse` framing is still the single source of truth.
- **No `tempfile` crate (T-03-03-SC):** Used the zero-dependency `std::env::temp_dir()` + a PID/nanosecond-unique subdir, exactly as the plan directs — keeps the plan autonomous with no package-legitimacy checkpoint.
- **Belt-and-braces hermeticity:** Set `GOPROXY=off` + `GOFLAGS=-mod=mod` on the subprocess in addition to the zero-require `go.mod` so a stray import can never silently fetch from the network in CI.
- **Smoke test reads the package clause:** Rather than hardcoding `goalservice`, the harness reads `package <name>` from the written `models.go` so the smoke `*_test.go` can never drift from the generated SDK's package.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `sdk::write_to_dir` was not callable from an out-of-crate integration test**
- **Found during:** Task 1 (writing tests/sdk_compile.rs)
- **Issue:** The plan's `<interfaces>` listed `sdk::write_to_dir(bundle: &SdkBundle, dir: &Path)`, but the existing fn was `pub(crate)` and took the crate-private `SdkBundle` — an integration test (a separate crate) cannot name `SdkBundle` or call a `pub(crate)` fn, so the test could not materialize the SDK as written.
- **Fix:** Made `write_to_dir` `pub` and retyped it to take the generated bundle **String** (the public `generate` output), splitting it through the existing shared `bundle::parse` framing. Added frame-name safety checks (reject empty / `/` / `\\` / `..`) so a malformed bundle can never traverse out of the temp dir (T-03-03 defense-in-depth). `SdkBundle`/`SdkFile` stay crate-private.
- **Files modified:** crates/gnr8-core/src/sdk/mod.rs
- **Verification:** `tests/sdk_compile.rs` calls `gnr8_core::sdk::write_to_dir(&bundle, &dir)` and the four SDK files materialize; `go build`/`go test` pass; clippy -D warnings + fmt clean; no regression in lib/snapshot tests.
- **Committed in:** 481bdae (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking).
**Impact on plan:** Necessary to make the planned SDK-05 integration test compile and run; no scope creep — the change keeps `SdkBundle` private and adds a path-safety guard. All gates pass.

## Issues Encountered
- **Clippy `-D warnings` on the new test targets:** the test scopes initially allowed only `unwrap_used`/`expect_used`, while the harness uses `panic!` (in `package_clause` and the error-path match) and clippy flagged a `map(..).unwrap_or(..)` on a `Result` plus a few `doc_markdown` items. Fixed by adding `clippy::panic` to the scoped allow (matching the `sdk/mod.rs` tests), switching to `map_or`, and backticking the doc terms. Resolved within the tasks; final `cargo clippy --all-targets --all-features --locked -- -D warnings` is clean.
- **Untracked `.planning/HANDOFF.json`:** present in the working tree (a planning artifact unrelated to this plan); left untouched and never staged.

## User Setup Required
None - no external service configuration required (the Go toolchain, used by `build_graph`/`gofmt`/`go build`, is already a hard project dependency at go 1.26).

## Next Phase Readiness
- **Phase 3 complete (3/3 plans):** the core source -> OpenAPI -> Go-SDK loop is proven end-to-end on the fixture. `to_openapi` + `sdk::generate` are deterministic; the generated SDK genuinely compiles and answers an HTTP round-trip (SDK-05); all four contract tests + `sdk_compile` are GREEN and BLOCKING in CI. Ready for `/gsd:verify-work 3` and Phase 4 (`.gnr8/` lifecycle + watch mode), which consumes these artifacts for no-op detection.
- No blockers.

## Self-Check: PASSED

- Created/modified files exist: `crates/gnr8-core/tests/sdk_compile.rs` (265 lines), `crates/gnr8-core/src/sdk/mod.rs`, `crates/gnr8-core/tests/determinism.rs`, `.github/workflows/ci.yml`, `Makefile` — all FOUND.
- Task commits exist: `481bdae`, `c00c614`, `b3f8357` — all FOUND in `git log`.
- `cargo test -p gnr8-core` GREEN (64 lib + 9 bin + determinism 3 + sdk_compile 3 + snapshot_graph/diagnostics/openapi/sdk 1 each); `make gates` + `make check` exit 0; `cargo fmt --check` + `cargo clippy -D warnings` clean.
- `ci.yml` contains `sdk_compile` + `snapshot_openapi`; no `continue-on-error: true` remains; `Makefile` `gates` contains `sdk_compile`; the `contract` job/target are gone.

---
*Phase: 03-openapi-and-go-sdk-generation*
*Completed: 2026-06-24*
