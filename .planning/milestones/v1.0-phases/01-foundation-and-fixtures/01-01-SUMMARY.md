---
phase: 01-foundation-and-fixtures
plan: 01
subsystem: infra
tags: [rust, cargo-workspace, clap, thiserror, anyhow, serde, clippy, cli, openapi, go-sdk]

# Dependency graph
requires: []
provides:
  - "Two-crate Cargo workspace (gnr8-core lib + gnr8 bin) with [workspace.lints] + [workspace.dependencies] single-source inheritance"
  - "CoreError thiserror enum with NotYetImplemented variant + generic not_yet<T> helper (RUST-04 typed-error boundary)"
  - "Five gnr8-core module seams (analyze::build_graph, lower::to_openapi, sdk::generate, diagnostics::collect, graph::ApiGraph) returning typed NotYetImplemented — the exact names/signatures Plan 01-03 red-by-design tests call"
  - "Router-agnostic ApiGraph placeholder (D-03) — no router-framework-specific fields"
  - "clap derive CLI surface: init/generate/watch/check/inspect/doctor + inspect routes|schemas|graph + global --json/-v (RUST-02)"
  - "anyhow boundary confined to gnr8/src/main.rs (D-09); skeletal commands exit code 2 with a clean non-panicking message (D-12)"
  - "docs/poc-contract.md — locked scope, surface, and non-goals (POC-01/02/03)"
affects: [02-foundation-and-fixtures, 03-foundation-and-fixtures, go-analysis, openapi-lowering, sdk-generation, quality-gates]

# Tech tracking
tech-stack:
  added: [clap 4.6 (derive), thiserror 2.0, anyhow 1.0, serde 1.0 (derive), serde_json 1.0, insta 1.48 (dev)]
  patterns:
    - "Workspace-wide lint policy via [workspace.lints]: unsafe_code=forbid, clippy unwrap_used/expect_used/panic=deny (RUST-04)"
    - "Typed-error/anyhow split: thiserror CoreError in lib, anyhow only at the binary main boundary (D-09)"
    - "Module seams return typed NotYetImplemented instead of unimplemented!()/panic! for clean non-panicking skeletal exits (D-12)"
    - "Scoped #![allow(clippy::unwrap_used, expect_used, panic)] in #[cfg(test)] modules so RUST-04 deny stays intact for production code"

key-files:
  created:
    - Cargo.toml
    - rust-toolchain.toml
    - crates/gnr8-core/Cargo.toml
    - crates/gnr8-core/src/lib.rs
    - crates/gnr8-core/src/error.rs
    - crates/gnr8-core/src/analyze/mod.rs
    - crates/gnr8-core/src/graph/mod.rs
    - crates/gnr8-core/src/lower/mod.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/src/diagnostics/mod.rs
    - crates/gnr8/Cargo.toml
    - crates/gnr8/src/main.rs
    - crates/gnr8/src/cli.rs
    - docs/poc-contract.md
    - Cargo.lock
  modified:
    - .gitignore

key-decisions:
  - "Routed skeletal-command path through typed CoreError::NotYetImplemented + exit code 2 rather than unimplemented!()/panic! (D-12 / RUST-04)"
  - "Added serde (not just serde_json) to the gnr8 binary so the Report --json render path can derive Serialize"
  - "Used scoped #[allow(clippy::doc_markdown)] on the two doc comments that double as clap help text, so 'OpenAPI' (a proper noun) stays clean in --help instead of leaking backticks"
  - "Used pub(crate) on Cli/Commands/InspectAction to satisfy the unreachable_pub workspace lint in a binary crate"

patterns-established:
  - "Cargo workspace with crates/ prefix; member manifests inherit edition/rust-version/license/lints/deps from the root"
  - "TDD per task: RED (test commit) -> GREEN (feat commit) with conventional-commit messages tagged 01-01"
  - "Library code is clippy-pedantic-clean: every Result-returning pub fn carries a # Errors doc section"

requirements-completed: [POC-01, POC-02, POC-03, RUST-01, RUST-02, RUST-04]

# Metrics
duration: 8min
completed: 2026-06-24
---

# Phase 1 Plan 01: Foundation Scaffold Summary

**Two-crate Cargo workspace (gnr8-core lib + gnr8 clap CLI) with a thiserror typed-error boundary, five stubbed router-agnostic module seams returning NotYetImplemented, a clippy-clean six-command CLI that exits cleanly without panicking, and a committed PoC contract locking Go->OpenAPI->Go-SDK scope.**

## Performance

- **Duration:** 8 min
- **Started:** 2026-06-24T16:03:26Z
- **Completed:** 2026-06-24T16:12:12Z
- **Tasks:** 3 (2 TDD)
- **Files modified:** 16 (15 created, 1 modified)

## Accomplishments
- Stood up the `gnr8-core` lib + `gnr8` bin Cargo workspace with `[workspace.lints]` (RUST-04 clippy denies on `unwrap_used`/`expect_used`/`panic`, `unsafe_code = "forbid"`) and `[workspace.dependencies]` pinning the verified crate versions (thiserror **2.0**, clap 4.6, anyhow 1.0, serde/serde_json 1.0, insta 1.48).
- Implemented `CoreError` (thiserror enum, `NotYetImplemented` variant), the `not_yet<T>` helper, the router-agnostic `ApiGraph` placeholder (D-03), and the five module seams (`analyze::build_graph`, `lower::to_openapi`, `sdk::generate`, `diagnostics::collect`) with the exact names/signatures Plan 01-03's red-by-design tests will call — all clippy-pedantic-clean, 3 unit tests green.
- Built the clap derive CLI exposing `init`/`generate`/`watch`/`check`/`inspect`/`doctor` + `inspect routes|schemas|graph` + global `--json`/`-v`; skeletal commands dispatch to `gnr8-core` seams and exit with code 2 and a clean message (`gnr8: 'generate' is not yet implemented (arrives in phase 3)`) instead of panicking — anyhow confined to `main.rs` (D-09), 4 parse tests green.
- Committed `docs/poc-contract.md` locking scope (Go->OpenAPI->Go SDK), the recognized Gin patterns, OpenAPI 3.1.0, the Go SDK shape, the `.gnr8/` split, and the verbatim non-goals table before any analyzer work (POC-01/02/03).

## Task Commits

Each task was committed atomically (TDD tasks have test -> feat pairs):

1. **Task 1: Cargo workspace skeleton, lints, deps, housekeeping** - `050f74b` (chore)
2. **Task 2: gnr8-core CoreError + not_yet + five module seams**
   - RED: `f8a8e4e` (test)
   - GREEN: `50c0451` (feat)
3. **Task 3: gnr8 CLI surface, dispatch, anyhow boundary + PoC contract doc**
   - RED: `b18efb6` (test)
   - GREEN: `da70d6b` (feat)

**Plan metadata:** committed with this SUMMARY (docs: complete plan)

## Files Created/Modified
- `Cargo.toml` - Workspace manifest: members, resolver 2, `[workspace.package]` (edition 2021 / rust-version 1.85 / MIT), `[workspace.lints]`, `[workspace.dependencies]`
- `rust-toolchain.toml` - Pins `channel = "stable"` + rustfmt/clippy for CI reproducibility
- `crates/gnr8-core/Cargo.toml` - lib manifest; thiserror + serde deps, insta dev-dep, no anyhow (D-09)
- `crates/gnr8-core/src/error.rs` - `CoreError` thiserror enum with `NotYetImplemented { command, phase }`
- `crates/gnr8-core/src/lib.rs` - Module declarations, `pub use error::CoreError`, `not_yet<T>` helper, 3 unit tests
- `crates/gnr8-core/src/graph/mod.rs` - Router-agnostic `ApiGraph` placeholder (D-03)
- `crates/gnr8-core/src/analyze/mod.rs` - `build_graph` seam (phase 2)
- `crates/gnr8-core/src/lower/mod.rs` - `to_openapi` seam (phase 3)
- `crates/gnr8-core/src/sdk/mod.rs` - `generate` seam (phase 3)
- `crates/gnr8-core/src/diagnostics/mod.rs` - `collect` seam (phase 2+)
- `crates/gnr8/Cargo.toml` - bin manifest; gnr8-core + clap + serde + serde_json + anyhow
- `crates/gnr8/src/cli.rs` - clap derive `Cli`/`Commands`/`InspectAction` + global flags, 4 parse tests
- `crates/gnr8/src/main.rs` - anyhow boundary, `dispatch`, non-panicking NotYetImplemented exit (code 2), `Report` Serialize placeholder
- `docs/poc-contract.md` - PoC scope lock, locked surface, non-goals table (POC-01/02/03)
- `Cargo.lock` - Committed lockfile (binary project; required for `clippy --locked`)
- `.gitignore` - Appended insta leftovers (`*.snap.new`, `*.pending-snap`) + Cargo.lock-must-stay note

## Decisions Made
- **Skeletal exits via typed error, not panic:** Every unimplemented command/seam returns `CoreError::NotYetImplemented`; the binary prints `gnr8: <message>` to stderr and `std::process::exit(2)`. This keeps `--help`/`--version`/arg validation fully working while honoring RUST-04 (no panic backtraces) — matches RESEARCH Pattern 2/3 and D-12.
- **`serde` added to the binary:** The plan specified a `Report` type deriving `serde::Serialize` for the `--json` path; the binary had only `serde_json`, so `serde` (already pinned in `[workspace.dependencies]`) was added. (Tracked as a deviation below.)
- **`doc_markdown` scoped allows:** Two doc comments double as clap help text; `OpenAPI` is a proper noun, and backticks would leak into `--help`. Used a narrowly-scoped, commented `#[allow(clippy::doc_markdown)]` per skill ch.2.4 (documented genuine false positive) rather than weakening the workspace policy.
- **`pub(crate)` on CLI types:** Binary-crate `pub` items tripped the `unreachable_pub` workspace lint; narrowed to `pub(crate)` since they are only used within the crate.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Member manifests needed source targets to parse**
- **Found during:** Task 1 (workspace scaffold)
- **Issue:** `cargo metadata --no-deps` (a Task 1 acceptance criterion) failed with "no targets specified in the manifest" because the member crates had no `src/lib.rs`/`src/main.rs` yet (the plan creates source in Tasks 2-3).
- **Fix:** Created minimal placeholder `crates/gnr8-core/src/lib.rs` and `crates/gnr8/src/main.rs` so the workspace parses; Tasks 2 and 3 replaced them with the real implementations.
- **Files modified:** crates/gnr8-core/src/lib.rs, crates/gnr8/src/main.rs
- **Verification:** `cargo metadata --no-deps` exits 0 after the stubs exist.
- **Committed in:** `050f74b` (Task 1 commit)

**2. [Rule 3 - Blocking] `serde` missing from the gnr8 binary for the Report derive**
- **Found during:** Task 3 (CLI dispatch / --json render path)
- **Issue:** The plan-specified `Report` type derives `serde::Serialize`, but the binary depended only on `serde_json`; the GREEN build failed with "cannot find module or crate `serde`".
- **Fix:** Added `serde = { workspace = true }` to `crates/gnr8/Cargo.toml` (version already pinned in `[workspace.dependencies]`; no new package introduced).
- **Files modified:** crates/gnr8/Cargo.toml, Cargo.lock
- **Verification:** `cargo test -p gnr8` and `cargo clippy -p gnr8 --locked -- -D warnings` both pass.
- **Committed in:** `da70d6b` (Task 3 commit)

**3. [Rule 1 - Bug] clap help text leaked literal backticks**
- **Found during:** Task 3 (after adding `doc_markdown` backticks to satisfy clippy pedantic)
- **Issue:** Backticking `OpenAPI` in command doc comments (to satisfy `clippy::doc_markdown`) made `gnr8 --help` render the literal "Generate \`OpenAPI\` + Go SDK", an ugly user-facing string.
- **Fix:** Replaced the backticks with a narrowly-scoped, commented `#[allow(clippy::doc_markdown)]` on the two doc comments that double as help text, restoring clean `--help` output while keeping clippy green.
- **Files modified:** crates/gnr8/src/cli.rs
- **Verification:** `gnr8 --help` shows "Generate OpenAPI + Go SDK from Go source"; clippy still exits 0.
- **Committed in:** `da70d6b` (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 blocking, 1 bug)
**Impact on plan:** All three were small, mechanical, and necessary to satisfy the plan's own acceptance criteria (workspace parse, `--json` render path, clean help UX). No scope creep, no new dependencies beyond the already-pinned `serde`, no architectural change.

## Known Stubs

These stubs are **intentional and contract-defined** (D-12 / D-15) — they are the seams Plan 01-03's red-by-design tests and Phases 2-3 will fill in. They are documented here for the verifier, not as gaps:

| Stub | File | Behavior | Resolved by |
|------|------|----------|-------------|
| `analyze::build_graph` | crates/gnr8-core/src/analyze/mod.rs | returns `NotYetImplemented` | Phase 2 |
| `diagnostics::collect` | crates/gnr8-core/src/diagnostics/mod.rs | returns `NotYetImplemented` | Phase 2+ |
| `lower::to_openapi` | crates/gnr8-core/src/lower/mod.rs | returns `NotYetImplemented` | Phase 3 |
| `sdk::generate` | crates/gnr8-core/src/sdk/mod.rs | returns `NotYetImplemented` | Phase 3 |
| `graph::ApiGraph` (empty struct) | crates/gnr8-core/src/graph/mod.rs | router-agnostic placeholder, no fields | Phase 2 |
| CLI command dispatch (all 6 + 3 subcommands) | crates/gnr8/src/main.rs | each calls a seam -> `NotYetImplemented`, exit 2 | Phases 2-5 |

The plan's objective explicitly scopes this plan to "seams only — NO analysis/lowering/SDK logic", so these stubs are the intended deliverable, not unfinished work.

## Issues Encountered
- `clippy::pedantic` (escalated to error by `-D warnings`) initially failed on `missing_errors_doc` and `doc_markdown` across the seam modules. Resolved by adding proper `# Errors` doc sections to every `Result`-returning pub fn and backticking/rewording type names — fixing warnings rather than silencing them (skill ch.2.4). The only retained `#[allow]` is the documented `doc_markdown` false positive on help-text doc comments.

## User Setup Required
None - no external service configuration required. (Rust toolchain 1.96.0 / clippy / rustfmt are present; no auth gates or env vars.)

## Next Phase Readiness
- Workspace builds, is fmt-clean and clippy-clean (`--all-targets --all-features --locked -- -D warnings`), and all 7 real unit/parse tests pass. `cargo run -p gnr8 -- --help` and `inspect --help` show the full planned command surface (RUST-02 / ROADMAP Phase 1 success criterion 1).
- The five module seams are named and signed exactly per the plan's `<interfaces>` block, so Plan **01-03**'s red-by-design snapshot tests can compile against them.
- Ready for **01-02** (the realistic Go Gin fixture module + expected/ acceptance scaffolds).

## Self-Check: PASSED

- All 15 created files + 1 modified file verified present on disk.
- All 5 task commits verified in git history (050f74b, f8a8e4e, 50c0451, b18efb6, da70d6b).
- Plan-level verification re-run green: `cargo build --workspace`, `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test --workspace` (7 tests ok), `cargo run -p gnr8 -- generate` exits 2 with a clean message.

---
*Phase: 01-foundation-and-fixtures*
*Completed: 2026-06-24*
