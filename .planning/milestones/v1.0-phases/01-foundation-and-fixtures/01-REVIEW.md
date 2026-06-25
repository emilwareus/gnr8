---
phase: 01-foundation-and-fixtures
reviewed: 2026-06-24T00:00:00Z
depth: deep
files_reviewed: 27
files_reviewed_list:
  - Cargo.toml
  - Makefile
  - .github/workflows/ci.yml
  - docs/poc-contract.md
  - crates/gnr8-core/Cargo.toml
  - crates/gnr8-core/src/lib.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/analyze/mod.rs
  - crates/gnr8-core/src/graph/mod.rs
  - crates/gnr8-core/src/lower/mod.rs
  - crates/gnr8-core/src/sdk/mod.rs
  - crates/gnr8-core/src/diagnostics/mod.rs
  - crates/gnr8-core/tests/snapshot_graph.rs
  - crates/gnr8-core/tests/snapshot_openapi.rs
  - crates/gnr8-core/tests/snapshot_sdk.rs
  - crates/gnr8-core/tests/snapshot_diagnostics.rs
  - crates/gnr8/Cargo.toml
  - crates/gnr8/src/main.rs
  - crates/gnr8/src/cli.rs
  - fixtures/goalservice/go.mod
  - fixtures/goalservice/internal/common/dto/common.go
  - fixtures/goalservice/internal/common/dto/goal.go
  - fixtures/goalservice/internal/goal/ports/handlers.go
  - fixtures/goalservice/internal/goal/ports/http.go
  - fixtures/goalservice/expected/sdk/models.go
  - fixtures/goalservice/expected/openapi.yaml
  - fixtures/goalservice/expected/diagnostics.txt
findings:
  critical: 0
  warning: 0
  info: 3
  total: 3
status: clean
---

# Phase 1: Code Review Report

**Reviewed:** 2026-06-24
**Depth:** deep
**Files Reviewed:** 27
**Status:** clean

## Summary

This is a greenfield scaffolding phase whose explicit job is to lock a contract and stand up
quality gates *before* any real analysis/lowering/SDK logic exists. I reviewed the full Phase 1
diff (`f8f46e3..HEAD`): the `gnr8-core` library seams and typed error, the `gnr8` binary CLI, the
four red-by-design contract tests, the Go Gin fixture, the expected-output acceptance targets, and
the `Makefile` / CI quality gates.

I did not stop at reading. I executed every gate to verify the claims rather than trust the
narrative:

- `cargo fmt --all -- --check` — clean (exit 0).
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean after `cargo clean`
  (no warnings, exit 0), confirming the `pedantic = warn` / `all = deny` / `unwrap_used`/
  `expect_used`/`panic = deny` workspace lint stack does not trip on any production path.
- Green gate tests (`cargo test -p gnr8-core --lib`, `cargo test -p gnr8`) — 3 + 4 pass, 0 ignored.
- The four contract tests (`snapshot_graph/openapi/sdk/diagnostics`) — each FAILS individually via a
  panicking `.expect()` on a `NotYetImplemented` seam. Verified **none** carry `#[ignore]` and **no**
  pre-authored `.snap` files exist, so the redness is genuine and FIX-04-compliant (not a silent skip
  or a missing-file race).
- `go build ./...` + `go vet ./...` in `fixtures/goalservice/` — clean; `gofmt -l` reports no
  unformatted files.
- Ran the binary: `gnr8 generate` and `gnr8 --json doctor` both print a clean message and exit `2`,
  confirming the non-panicking exit-code mapping (D-12 / RUST-04).

Convention checks all pass: no production `unwrap`/`expect`/`panic` (the only `unwrap`/`expect` live
under `#[cfg(test)]` or test-target-scoped `#![allow(...)]`); `anyhow` appears only in the binary
(`main.rs` / `gnr8/Cargo.toml`) and never in `gnr8-core`; the library uses `thiserror`; `Cargo.lock`
is committed (required for `--locked`) and `.gitignore` explicitly protects it.

No correctness, security, or actionable quality defects were found. The three items below are
informational observations about the CI/fixture scaffolding, not defects, and require no action to
ship the phase. The "unsupported patterns" in the fixture (`map[string]any`, untyped `c.Query`,
`*float64`→`float32` narrowing) are intentional FIX-02 diagnostic triggers and are correctly
mirrored in the expected `openapi.yaml` / `diagnostics.txt` / SDK `models.go`; they are not flagged.

## Info

### IN-01: Blocking `gates` CI job runs `cargo test` without `--locked`

**File:** `.github/workflows/ci.yml:44-46`
**Issue:** The blocking `gates` job enforces `--locked` for clippy (line 39) but the test step
(`cargo test -p gnr8-core --lib` / `cargo test -p gnr8`) omits `--locked`. If a future change edits a
`Cargo.toml` dependency without regenerating `Cargo.lock`, clippy would already catch it — so this is
not exploitable today — but the test step alone would silently resolve a fresh lockfile rather than
failing on drift. This is a defense-in-depth gap, not a current bug (clippy runs first in the same
job and would fail the drift).
**Fix:** Add `--locked` to the test invocations for consistency with the clippy gate and the
`Makefile` intent:
```yaml
cargo test --locked -p gnr8-core --lib
cargo test --locked -p gnr8
```
(Same applies to `make gates`/`make test` in the `Makefile` if strict lockfile parity is desired.)

### IN-02: `contract` CI job installs unused `rustfmt`/`clippy` components

**File:** `.github/workflows/ci.yml:71`
**Issue:** The non-blocking `contract` job only runs `cargo test ... --test snapshot_*`, but its
toolchain step requests `components: rustfmt, clippy`. These components are never used in that job,
so they add unnecessary install/cache time. Harmless, purely cosmetic.
**Fix:** Drop the `with: components:` block from the `contract` job's `dtolnay/rust-toolchain@stable`
step; the default toolchain is sufficient to compile and run the tests.

### IN-03: Fixture exports (`NewHttpServer`, `RegisterGoalRoutes`) have no in-module caller

**File:** `fixtures/goalservice/internal/goal/ports/http.go:25,65`
**Issue:** The fixture module has no `main` and no caller of `NewHttpServer` /
`RegisterGoalRoutes` / `setupRoutes`. This is intentional and correct for an analyzer-input fixture
(it is parsed, never run), and `go build`/`go vet` pass because the methods are exported and
`setupRoutes` is reachable through `RegisterGoalRoutes`. Noting it only so a future reviewer does not
mistake the absent caller for dead code: the route-registration call chain
(`RegisterGoalRoutes` → `setupRoutes` → `api.METHOD(...)`) is the exact surface Phase 2's analyzer
must walk, so keeping it exported-but-uncalled is the right shape.
**Fix:** None required. Optionally add a one-line `// analyzer entrypoint; intentionally has no
runtime caller` note near `RegisterGoalRoutes` to preempt the question (the package doc already
implies it).

---

_Reviewed: 2026-06-24_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
