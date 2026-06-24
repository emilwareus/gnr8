---
phase: 04-gnr8-lifecycle-and-watch-mode
plan: 01
subsystem: infra
tags: [workspace, scaffold, toml, config, serde, blake3, notify-debouncer-full, lifecycle, cli]

# Dependency graph
requires:
  - phase: 03-openapi-and-go-sdk-generation
    provides: "deterministic pipeline (analyze::build_graph → lower::to_openapi + sdk::generate/write_to_dir) + CoreError thiserror surface the lifecycle wraps"
  - phase: 01-foundation-and-fixtures
    provides: "gnr8 clap CLI skeleton (Commands::Init seam) + Report dispatch path + workspace lint policy"
provides:
  - "gnr8 init: idempotent .gnr8/ workspace scaffold (config.toml + .gitignore + cache/) wired to the real CLI arm"
  - "gnr8_core::workspace::{init, InitOutcome, GITIGNORE_BODY, DEFAULT_CONFIG_TOML}"
  - "gnr8_core::config::{Config, OutputConfig, NamingOverrides, parse, load} — typed TOML knobs, deny_unknown_fields"
  - "CoreError::Workspace/Config (consumed now) + Manifest/Io (reserved for 04-02/04-03)"
  - "blake3 1.8 / toml 1.1 / notify-debouncer-full 0.7 pinned in [workspace.dependencies]; Cargo.lock updated"
  - "tests/lifecycle.rs — the shared Phase-4 core test file (04-02 extends it)"
affects: [04-02-manifest-ownership-noop, 04-03-watch-latency]

# Tech tracking
tech-stack:
  added: [toml 1.1, blake3 1.8 (pinned only), notify-debouncer-full 0.7 (pinned only)]
  patterns:
    - "write-if-absent via OpenOptions::create_new(true) — TOCTOU-safe idempotent scaffold (D-01)"
    - "owned-message CoreError variants (no #[source] coupling) mirroring Lowering/SdkGen"
    - "serde deny_unknown_fields typed config — reject typo'd keys with CoreError::Config (V5)"
    - "honest WS-03 scope — documented knobs only, no faked plugin field, v2 deferral stated in docs"
    - "errors defined one plan ahead (Manifest/Io) so 04-02/04-03 stay file-disjoint from error.rs"

key-files:
  created:
    - crates/gnr8-core/src/workspace/mod.rs
    - crates/gnr8-core/src/config/mod.rs
    - crates/gnr8-core/tests/lifecycle.rs
  modified:
    - Cargo.toml
    - crates/gnr8-core/Cargo.toml
    - crates/gnr8-core/src/error.rs
    - crates/gnr8-core/src/lib.rs
    - crates/gnr8/src/main.rs
    - Cargo.lock

key-decisions:
  - "blake3 pinned in workspace deps but NOT added to gnr8-core/Cargo.toml in 04-01 (PLAN-CHECK W1): unused until 04-02's manifest, so adding it would not yet trip a denial — but deferring keeps 04-01 free of a dead dependency and lets 04-02 add the line when the manifest consumes it. toml IS added (config uses it now)."
  - "Idempotency uses OpenOptions::create_new(true) (not exists()+write) for the atomic write-if-absent guarantee (RESEARCH Pattern 1 TOCTOU note)."
  - "WS-03 modeled with exactly the documented knobs (inputs/output.openapi/output.sdk_dir/output.go_module/naming.operations/naming.types); NO plugin/seam field — module docs state TOML is a PoC stand-in and v2 (ADV-02) customization is deliberately absent."
  - "All four new CoreError variants (Workspace/Config/Manifest/Io) landed in 04-01 so 04-02/04-03 never edit error.rs (mirrors 03-01 pre-defining SdkGen/GoFmt/GoBuild)."

patterns-established:
  - "Pattern 1: idempotent .gnr8/ scaffold — create_dir_all(cache) + write_if_absent(config.toml,.gitignore), AlreadyExists → skipped"
  - "Pattern 2: typed TOML config layer — toml::from_str → CoreError::Config, deny_unknown_fields on every struct"

requirements-completed: [WS-01, WS-02, WS-03]

# Metrics
duration: 8min
completed: 2026-06-24
---

# Phase 4 Plan 01: `.gnr8/` Workspace Lifecycle And Static Config Summary

**`gnr8 init` now idempotently scaffolds a `.gnr8/` workspace (checked-in `config.toml` + auto-written `.gitignore` ignoring `/cache/`) and a typed `toml`-backed `Config` reads the documented PoC knobs (inputs/output paths/go module/naming overrides) with `deny_unknown_fields`, plus four new lifecycle `CoreError` variants for 04-02/04-03.**

## Performance

- **Duration:** 8 min
- **Started:** 2026-06-24T20:36:58Z
- **Completed:** 2026-06-24T20:45:37Z
- **Tasks:** 3 (Task 2 + Task 3 are TDD: RED → GREEN)
- **Files modified:** 9 (3 created, 6 modified)

## Accomplishments

- `gnr8 init` wired to the real scaffold: creates `.gnr8/config.toml`, `.gnr8/.gitignore`, `.gnr8/cache/`, reports created vs skipped, exits 0, `--json` works. Verified end-to-end on a temp cwd (first run creates; second run after a user edit reports "nothing to do" and preserves the edit byte-for-byte).
- Typed `config` module (`Config`/`OutputConfig`/`NamingOverrides`, `parse`/`load`) — the documented WS-03 knobs ONLY; unknown keys are rejected via `deny_unknown_fields` as `CoreError::Config`; `DEFAULT_CONFIG_TOML` round-trips through the parser.
- Honest WS-03 scope: NO faked plugin field; module docstrings state TOML is a PoC code-as-config stand-in (not the long-term UX) and that "through code" routing/transport/emitter customization is a deliberately-absent v2 direction (ADV-02).
- Four new `CoreError` variants added once in 04-01: `Workspace`/`Config` (consumed now) + `Manifest`/`Io` (reserved for 04-02/04-03), keeping `error.rs` owned by one plan.
- `blake3 1.8` / `toml 1.1` / `notify-debouncer-full 0.7` pinned in `[workspace.dependencies]` at the researched stable versions (no release candidates); `Cargo.lock` updated and committed.

## Task Commits

Each task was committed atomically (tagged `04-01`):

1. **Task 1: Pin lifecycle crates + add CoreError variants** — `018c60d` (feat)
2. **Task 2 (TDD): idempotent `.gnr8/` workspace scaffold** — `8712477` (test, RED) → `76574d1` (feat, GREEN)
3. **Task 3 (TDD): typed config + wire `gnr8 init` CLI arm** — `957a912` (feat, GREEN; config tests were RED in `8712477`)

**Plan metadata:** committed separately (docs: complete plan) after this SUMMARY.

_Note: the RED commit `8712477` carries the failing tests for BOTH TDD tasks (the shared `tests/lifecycle.rs`); Task 2 GREEN (`76574d1`) flips the three workspace tests, Task 3 GREEN (`957a912`) flips the three config tests._

## Files Created/Modified

- `crates/gnr8-core/src/workspace/mod.rs` (created) — `init` idempotent scaffold, `InitOutcome`, `GITIGNORE_BODY`, `DEFAULT_CONFIG_TOML`, `write_if_absent` (create_new).
- `crates/gnr8-core/src/config/mod.rs` (created) — typed `Config`/`OutputConfig`/`NamingOverrides`, `parse`/`load`, honest-scope docs.
- `crates/gnr8-core/tests/lifecycle.rs` (created) — hermetic temp-dir tests: init scaffold/idempotency/gitignore-split + config default-body/naming/unknown-key.
- `crates/gnr8-core/src/error.rs` (modified) — `Workspace`/`Config`/`Manifest`/`Io` variants + display tests.
- `crates/gnr8-core/src/lib.rs` (modified) — registered `pub mod config; pub mod workspace;`.
- `crates/gnr8/src/main.rs` (modified) — `Commands::Init` → `run_init()` (real scaffold + Report).
- `Cargo.toml` (modified) — pinned blake3/toml/notify-debouncer-full in `[workspace.dependencies]`.
- `crates/gnr8-core/Cargo.toml` (modified) — added `toml = { workspace = true }` (blake3 deferred to 04-02).
- `Cargo.lock` (modified) — locked the new crates.

## Constants emitted (for 04-02/04-03)

`GITIGNORE_BODY` (written to `.gnr8/.gitignore`):

```gitignore
# gnr8 lifecycle state — regenerated, do not commit.
/cache/
```

`DEFAULT_CONFIG_TOML` (written to `.gnr8/config.toml`):

```toml
# gnr8 PoC configuration — a code-as-config STAND-IN, not the long-term UX (see docs / D-03).
# Programmatic ("through code") customization of routing recognition / transport / emitters is a
# documented v2 direction (ADV-02) and is deliberately NOT a knob here.
inputs = ["."]                              # Go source dir(s) to analyze (project-relative)

[output]
openapi   = "openapi.yaml"                  # OpenAPI artifact path (project-relative)
sdk_dir   = "sdk"                           # generated Go SDK directory
go_module = "example.com/yourservice/sdk"   # Go module path for the generated SDK

# [naming.operations]                        # optional: remap operation ids, e.g.
# goalUuidPut = "UpdateGoal"
# [naming.types]                             # optional: remap generated type names, e.g.
# CreateGoalInput = "NewGoal"
```

New `CoreError` variant names (consumers): `Workspace` (04-01), `Config` (04-01), `Manifest` (04-02), `Io` (04-02/04-03).

## Decisions Made

- **blake3 dependency line:** landed in the workspace pin only, NOT in `crates/gnr8-core/Cargo.toml` (PLAN-CHECK W1). 04-02 adds `blake3 = { workspace = true }` to gnr8-core when the ownership manifest first consumes it. `toml` is added to gnr8-core now because `config::parse` uses it this plan. Verified clean: `cargo build -p gnr8-core --locked` and `cargo clippy -p gnr8-core --all-targets -- -D warnings` both pass with `toml` present and no unused-dependency denial (the workspace lint set does not include `unused_crate_dependencies`).
- **Idempotency mechanism:** `OpenOptions::create_new(true)` (atomic write-if-absent) over `exists()` + `write` per RESEARCH Pattern 1's TOCTOU note; `AlreadyExists` → recorded in `skipped`, never overwrites.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `doc_markdown` (clippy pedantic) denied the acronym-dense module/test docs**
- **Found during:** Task 2 / Task 3 (clippy `-D warnings` gate)
- **Issue:** `cargo clippy --all-targets -- -D warnings` failed on `doc_markdown` for prose proper-nouns/acronyms (PoC, OpenAPI, TOML, JSON, TOCTOU, BTreeMap, `deny_unknown_fields`) in the new module/test doc comments — the workspace lint set treats clippy `all` as deny.
- **Fix:** Added a scoped `#![allow(clippy::doc_markdown)]` to `workspace/mod.rs`, `config/mod.rs`, and the `tests/lifecycle.rs` allow block, each with an explanatory comment — mirroring the existing scoped allow in `gnr8/src/cli.rs` (skill ch.2.4). No backticking of user-facing prose.
- **Files modified:** crates/gnr8-core/src/workspace/mod.rs, crates/gnr8-core/src/config/mod.rs, crates/gnr8-core/tests/lifecycle.rs
- **Verification:** `cargo clippy --all-targets --locked -- -D warnings` clean.
- **Committed in:** `76574d1` (workspace allow) and `957a912` (config + test allow).

**2. [Rule 3 - Blocking] `cargo fmt --check` flagged long lines in 04-01's own new code**
- **Found during:** Task 3 (fmt gate)
- **Issue:** `cargo fmt --all --check` reported line-wrap diffs in two of the new error-display tests (`error.rs`) and the `write_if_absent`/`init` bodies (`workspace/mod.rs`) — both files introduced in earlier 04-01 commits.
- **Fix:** Ran `cargo fmt --all` and folded the normalization into the Task 3 commit (rather than amending prior commits, which would rewrite committed history). All 04-01-touched files are now fmt-clean.
- **Files modified:** crates/gnr8-core/src/error.rs, crates/gnr8-core/src/workspace/mod.rs
- **Verification:** `cargo fmt --all --check` clean (for 04-01 files).
- **Committed in:** `957a912` (Task 3 commit).

---

**Total deviations:** 2 auto-fixed (both Rule 3 — blocking lint/format gates).
**Impact on plan:** Both were quality-gate satisfactions on 04-01's own code, no behavior change, no scope creep. All success criteria met as written.

## Issues Encountered

- **Pre-existing fmt drift in an out-of-scope file (`crates/gnr8-core/src/sdk/emit.rs`):** `cargo fmt --all` also reformats two long lines in this Phase-3 file that were already unformatted before 04-01 started. Per the scope boundary, this was **reverted** (not committed) and logged to `.planning/phases/04-gnr8-lifecycle-and-watch-mode/deferred-items.md`. It is unrelated to 04-01 and belongs in a separate `style:` chore. Consequence: a workspace-wide `cargo fmt --all --check` will report this one pre-existing file until it is addressed; all files 04-01 touched are clean.

## User Setup Required

None - no external service configuration required. (Three new crates are standard, pinned, and legitimacy-audited in 04-RESEARCH; no auth/secrets involved.)

## Next Phase Readiness

- **04-02 (ownership manifest + no-op):** `config::load` resolves input/output paths; `CoreError::Manifest`/`Io` are defined and ready to consume; `blake3` is pinned in workspace deps awaiting its `{ workspace = true }` line in gnr8-core; `tests/lifecycle.rs` is the shared file to extend.
- **04-03 (watch + latency):** `notify-debouncer-full` is pinned; `CoreError::Io` is available for the watch shell.
- No blockers. The `.gnr8/` layout, the typed config surface, and the new error variants are the foundation the rest of Phase 4 builds on.

---

## Self-Check: PASSED

- Created files exist on disk: `workspace/mod.rs`, `config/mod.rs`, `tests/lifecycle.rs`, `04-01-SUMMARY.md` — all FOUND.
- Task commits exist: `018c60d`, `8712477`, `76574d1`, `957a912` — all FOUND.
- Gates green: `cargo test --workspace` (all suites 0 failed), `cargo clippy --all-targets --locked -- -D warnings` clean, `cargo build --locked` succeeds, `cargo fmt --all --check` clean for all 04-01-touched files.
- Manual `gnr8 init` verified: scaffold + idempotency (edit preserved) + `--json`, exit 0.

---
*Phase: 04-gnr8-lifecycle-and-watch-mode*
*Completed: 2026-06-24*
