---
phase: 01-foundation-and-fixtures
plan: 03
subsystem: quality-gates
tags: [rust, insta, snapshot, red-by-design, makefile, github-actions, ci, clippy, go-fixture]

# Dependency graph
requires:
  - phase: 01-foundation-and-fixtures (plan 01-01)
    provides: gnr8-core seams (analyze::build_graph, lower::to_openapi, sdk::generate, diagnostics::collect) returning NotYetImplemented; docs/poc-contract.md
  - phase: 01-foundation-and-fixtures (plan 01-02)
    provides: fixtures/goalservice Go Gin module + expected/ acceptance targets (the test input)
provides:
  - "Four red-by-design insta contract tests (graph/openapi/sdk/diagnostics) that FAIL CLEARLY today via a panicking .expect() on the NotYetImplemented seams (FIX-03/FIX-04)"
  - "Makefile gates: fmt/fmt-check/clippy(--locked,-D warnings)/test/gates/contract/fixture-build/check/all (RUST-03, D-16)"
  - ".github/workflows/ci.yml — blocking gates + go-fixture jobs, separate NON-BLOCKING contract job (continue-on-error) for the red-by-design suite (Open Q1 option d)"
  - "docs/poc-contract.md §5 CI policy: blocking-vs-non-blocking split + Phase-3 promotion of the contract job to blocking"
affects: [02-go-analysis-and-api-graph, 03-openapi-and-go-sdk-generation, ci]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Red-by-design contract test: .expect() on a NotYetImplemented seam panics BEFORE insta::assert_*, so the test fails clearly with a phase-naming message and needs no pre-authored .snap (RESEARCH Pattern 4 mechanism 1)"
    - "RUST-03 vs FIX-04 reconciliation (Open Q1 option d): blocking gates run only genuinely-green tests (lib + bin, excluding the integration tests/ dir); the four red tests run in a separate continue-on-error CI job"
    - "Per-test-target #![allow(clippy::unwrap_used, clippy::expect_used)] keeps RUST-04 deny intact for production code while letting tests unwrap/expect (Pitfall 2)"
    - "Go fixture compiled/vetted in a dedicated CI job + make target so the standalone module (cargo never builds it) cannot rot (Pitfall 5)"

key-files:
  created:
    - crates/gnr8-core/tests/snapshot_graph.rs
    - crates/gnr8-core/tests/snapshot_openapi.rs
    - crates/gnr8-core/tests/snapshot_sdk.rs
    - crates/gnr8-core/tests/snapshot_diagnostics.rs
    - Makefile
    - .github/workflows/ci.yml
  modified:
    - docs/poc-contract.md

key-decisions:
  - "Reconciled RUST-03 (gates must pass) with FIX-04 (contract tests visibly red) via Open Q1 option (d): blocking gates run lib+bin tests only; the four red tests run as a separate non-blocking continue-on-error CI job, promoted to blocking in Phase 3"
  - "Used the .expect()-on-NotYetImplemented panic as the PRIMARY redness mechanism (fires before insta asserts) so no .snap is pre-authored and `cargo insta accept` cannot falsely green the suite (T-03-03)"
  - "Each .expect() message names the responsible phase (build_graph/collect -> phase 2; to_openapi/generate -> phase 3) so the red failure is self-documenting"
  - "Added a `gates` make target (lib+bin tests only) mirroring the blocking CI job, distinct from `test` (full suite, shows red) and `contract` (the four red tests alone)"

patterns-established:
  - "One behavioral assertion per snapshot test (skill ch.5); openapi/sdk/diagnostics use assert_snapshot! (String output), graph uses assert_yaml_snapshot! (Serialize-derived ApiGraph)"
  - "Third-party GitHub Actions pinned to major-version tags from well-known publishers (actions/*, dtolnay, Swatinem) per threat register T-03-02"

requirements-completed: [RUST-03, FIX-03, FIX-04]

# Metrics
duration: 7min
completed: 2026-06-24
---

# Phase 1 Plan 03: Quality Gates + Red-by-Design Contract Tests Summary

**Four insta contract tests (graph/openapi/sdk/diagnostics) that fail clearly today via a panicking `.expect()` on the NotYetImplemented gnr8-core seams, plus a Makefile and a GitHub Actions CI that reconcile RUST-03 (blocking gates green) with FIX-04 (contract suite visibly red) by splitting them into separate jobs — the red suite promoted to blocking in Phase 3.**

## Performance

- **Duration:** ~7 min
- **Completed:** 2026-06-24
- **Tasks:** 3 (2 auto + 1 human-verify checkpoint, self-verified under AUTO_MODE)
- **Files modified:** 7 (6 created, 1 modified)

## Accomplishments
- Added the four red-by-design contract tests under `crates/gnr8-core/tests/`. Each resolves the fixture via `const FIXTURE_DIR = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice")`, calls its gnr8-core seam, and `.expect()`s the result — which panics today because the seams return `CoreError::NotYetImplemented`, so each test FAILS CLEARLY (FIX-04). `graph` uses `assert_yaml_snapshot!` (the `ApiGraph` derives `serde::Serialize`); `openapi`/`sdk`/`diagnostics` use `assert_snapshot!` (the seams return `String`). No `#[ignore]`, no pre-authored `.snap` — the redness comes from the stubbed seam, and each `.expect()` message names the responsible phase.
- Authored a `Makefile` exposing `fmt`/`fmt-check`/`clippy` (`--all-targets --all-features --locked -- -D warnings`)/`test`/`gates`/`contract`/`fixture-build`/`check`/`all`. `check` is the full local gate mirroring CI; `gates` runs only the genuinely-green lib+bin tests; `contract` runs the four red tests on their own; `fixture-build` compiles + vets the standalone Go module.
- Authored `.github/workflows/ci.yml` implementing Open Q1 option (d) — three jobs: **`gates`** (BLOCKING: fmt-check + clippy + `cargo test -p gnr8-core --lib` + `cargo test -p gnr8`, excluding the red integration tests), **`go-fixture`** (BLOCKING: `go build`/`go vet` in `fixtures/goalservice`), and **`contract`** (NON-BLOCKING, `continue-on-error: true`: the four red-by-design snapshot tests). Actions pinned to major-version tags from well-known publishers (T-03-02).
- Appended a §5 "CI policy" to `docs/poc-contract.md` documenting the blocking-vs-non-blocking split, the `.expect()`-driven redness rationale, local `make check` parity, and the Phase-3 promotion of the `contract` job to blocking — preserving all of 01-01's existing content.

## Task Commits

Each task was committed atomically:

1. **Task 1: Four red-by-design contract snapshot tests** - `51139aa` (test)
2. **Task 2: Makefile + CI workflow + appended CI policy** - `d0ea396` (feat)
3. **Task 3: Phase-gate human-verify checkpoint** - self-verified under AUTO_MODE (no code change; evidence below)

**Plan metadata:** committed with this SUMMARY (docs: complete plan)

## Files Created/Modified
- `crates/gnr8-core/tests/snapshot_graph.rs` - Calls `analyze::build_graph(FIXTURE_DIR).expect(...)` + `assert_yaml_snapshot!("goalservice_graph", graph)`. Red-by-design (phase 2).
- `crates/gnr8-core/tests/snapshot_openapi.rs` - Builds graph then `lower::to_openapi(&graph).expect(...)` + `assert_snapshot!("goalservice_openapi", openapi)`. Red-by-design (phase 2 then 3).
- `crates/gnr8-core/tests/snapshot_sdk.rs` - Builds graph then `sdk::generate(&graph).expect(...)` + `assert_snapshot!("goalservice_sdk", sdk)`. Red-by-design (phase 2 then 3).
- `crates/gnr8-core/tests/snapshot_diagnostics.rs` - Calls `diagnostics::collect(FIXTURE_DIR).expect(...)` + `assert_snapshot!("goalservice_diagnostics", diags)`. Red-by-design (phase 2+).
- `Makefile` - fmt/fmt-check/clippy/test/gates/contract/fixture-build/check/all targets (RUST-03, D-16).
- `.github/workflows/ci.yml` - Blocking `gates` + `go-fixture` jobs, non-blocking `contract` job (Open Q1 option d).
- `docs/poc-contract.md` - Appended §5 CI policy (blocking-vs-non-blocking + Phase-3 promotion); 01-01 content preserved.

## Decisions Made
- **RUST-03/FIX-04 reconciliation via Open Q1 option (d):** the blocking gate runs only the genuinely-green tests (`cargo test -p gnr8-core --lib && cargo test -p gnr8`), which exclude the integration `tests/` dir; the four red tests live in a separate `continue-on-error` CI job (`contract`) so they are visibly red but never block merges, and never `#[ignore]`d. Promotion to blocking is scheduled for Phase 3 and documented in `docs/poc-contract.md` §5.
- **`.expect()`-on-NotYetImplemented as the primary red mechanism:** the panic fires before any `insta::assert_*`, so `cargo insta accept` / `INSTA_UPDATE=always` cannot falsely green the suite (mitigates T-03-03) and no `.snap` needs pre-authoring. The `expected/` files from 01-02 remain the human-readable acceptance target.
- **Separate `gates` make target:** distinct from `test` (full suite, surfaces the red contract failures locally) and `contract` (the four red tests alone), so a developer can run the blocking gate set green locally just as CI does.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Reworded test doc comments to satisfy the `#[ignore` acceptance grep and clippy::doc_markdown**
- **Found during:** Task 1 (after writing the four tests)
- **Issue:** Two blocking problems surfaced against the plan's own acceptance gate: (a) the doc comments contained the literal token `#[ignore]` (used in prose to say the tests are NOT ignored), which the acceptance check `grep -rn '#\[ignore' crates/gnr8-core/tests/` matched — so the "returns NOTHING" criterion failed on a false positive; (b) `cargo clippy --tests -- -D warnings` failed `clippy::doc_markdown` on the bare word "OpenAPI" in `snapshot_openapi.rs` doc comments.
- **Fix:** Reworded every test's doc comment to describe the constraint as "never marked ignored" (removing the literal `#[ignore` token) and backticked `OpenAPI` where clippy flagged it. No change to test logic, assertions, or the `#![allow(...)]` attributes.
- **Files modified:** crates/gnr8-core/tests/snapshot_graph.rs, snapshot_openapi.rs, snapshot_sdk.rs, snapshot_diagnostics.rs
- **Verification:** `grep -rn '#\[ignore' crates/gnr8-core/tests/` returns nothing; `cargo clippy -p gnr8-core --tests --all-features --locked -- -D warnings` exits 0; all four tests still fail red.
- **Committed in:** `51139aa` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking).
**Impact on plan:** Cosmetic doc-comment wording only — necessary to pass the plan's own `#[ignore`-grep and clippy acceptance criteria. No change to test behavior, gate structure, or CI. No scope creep, no new dependencies, no architectural change.

## Checkpoint Evidence (Task 3 — phase-gate human-verify, self-verified under AUTO_MODE)

AUTO_MODE was active, so the blocking `checkpoint:human-verify` was pre-approved. It was self-verified by running the exact `<how-to-verify>` commands and capturing the evidence (treated as "approved"):

| Step | Command | Expected | Result |
|------|---------|----------|--------|
| 1 | `make fmt-check` | exit 0 | **exit 0 (green)** |
| 2 | `make clippy` | exit 0 (no warnings) | **exit 0 (green)** |
| 3 | `make fixture-build` | Go build+vet clean, exit 0 | **exit 0 (green)** |
| 4 | `cargo test -p gnr8-core --lib && cargo test -p gnr8` | green (3 + 4 tests) | **3 passed / 4 passed, exit 0 (green)** |
| 5 | four `--test snapshot_*` | FAIL (panic: not yet implemented) | **All 4 FAIL at exit 101; panic = `NotYetImplemented { command: "analyze::build_graph", phase: 2 }` etc. — visibly RED (FIX-04)** |
| 6 | `grep -rn '#\[ignore' crates/gnr8-core/tests/` | returns nothing | **nothing (no silent skips)** |
| 7 | `cargo run -p gnr8 -- --help` and `inspect --help` | full command surface | **6 commands (init/generate/watch/check/inspect/doctor) + inspect routes/schemas/graph + global --json/-v shown** |

Gates 1-4 green, step 5 visibly red (4 failing tests with NotYetImplemented panics, NOT via `#[ignore]`), steps 6-7 as described. Checkpoint condition satisfied → treated as **approved**.

## Known Stubs
None introduced by this plan. The four contract tests are red-by-design against the gnr8-core seams stubbed in Plan 01-01 (documented there as intentional, contract-defined stubs resolved in Phases 2-3). The redness is the FIX-04 contract, not an unfinished deliverable; the tests turn green on snapshot review once the seams are implemented.

## Issues Encountered
None beyond the single auto-fixed doc-comment deviation above. The optional `pyyaml` lint of `ci.yml` was unavailable (`No module named 'yaml'`); validated the workflow YAML with Ruby's `YAML.load_file` instead (valid), and proved the workflow's commands behave by running the equivalent `make` targets locally (gates green, contract red).

## User Setup Required
None - no external service configuration, auth gates, or env vars. (Rust 1.96.0 / clippy / rustfmt and Go 1.26.2 are present locally; CI installs pinned toolchains.)

## Next Phase Readiness
- **Phase 1 is complete (3/3 plans).** RUST-03 (fmt/clippy/test gates wired locally + CI, clippy denying warnings), FIX-03 (snapshot tests cover graph/OpenAPI/Go SDK/diagnostics), and FIX-04 (tests fail clearly before behavior exists) are all satisfied.
- All four ROADMAP Phase 1 success criteria are TRUE: (1) CLI help shows the planned surface; (2) the Go fixture encodes the PoC acceptance cases; (3) tests/snapshots define expected graph/OpenAPI/SDK/diagnostic behavior before the analyzer exists; (4) Rust quality gates are wired.
- Phase 2 (Go Analysis And API Graph) implements `analyze::build_graph` + `diagnostics::collect` against `fixtures/goalservice`; turning `snapshot_graph`/`snapshot_diagnostics` green on review. Phase 3 implements `lower::to_openapi` + `sdk::generate`, turns the remaining two green, and promotes the `contract` CI job to blocking.

## Self-Check: PASSED

- All 6 created files + 1 modified file verified present on disk.
- Both task commits verified in git history (51139aa test, d0ea396 feat).
- Plan-level verification re-run: blocking gates GREEN (`make fmt-check`/`make clippy`/`make gates`/`make fixture-build` all exit 0); the four contract tests RED (exit 101, NotYetImplemented panic); no `#[ignore]`; CI YAML valid; `docs/poc-contract.md` §5 CI policy present with 01-01 content preserved.

---
*Phase: 01-foundation-and-fixtures*
*Completed: 2026-06-24*
