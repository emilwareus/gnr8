---
phase: 02-python-source-pyextract
plan: 01
subsystem: api
tags: [python, pyextract, fastapi, flask, subprocess, language-dispatch, facts-contract]

# Dependency graph
requires:
  - phase: 01-language-neutral-ir
    provides: "neutral facts contract (GoFacts DTO, deny_unknown_fields), ApiGraph::from_facts, GoGin Source, run_goextract subprocess-driver pattern"
provides:
  - "CoreError::PythonToolchainMissing typed error variant"
  - "helper::pyextract_dir() + run_pyextract()/run_pyextract_with() python3 subprocess driver"
  - "analyze::Lang enum + detect_language() single deterministic language classifier"
  - "language-aware build_graph + diagnostics::collect dispatch (Go vs Python by one detector)"
  - "FastApi + Flask Source built-ins (prelude-exported) driving build_graph"
affects: [pyextract-sidecar, fastapi-green, flask-green, python-snapshot-flip]

# Tech tracking
tech-stack:
  added: []  # rule 2: zero new crates (Cargo.toml unchanged)
  patterns:
    - "Language dispatch = ONE deterministic detect_language scan, then a single match per language (never try-Go-then-fallback-Python)"
    - "A new-language Source is a verbatim GoGin clone differing only in the Config error proper noun; ALL Sources call the SAME build_graph (language detected from the target, not the Source)"

key-files:
  created: []
  modified:
    - crates/gnr8-core/src/error.rs
    - crates/gnr8-core/src/analyze/helper.rs
    - crates/gnr8-core/src/analyze/mod.rs
    - crates/gnr8-core/src/diagnostics/mod.rs
    - crates/gnr8-core/src/sdk/builtins.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/src/lib.rs

key-decisions:
  - "pyextract_dir() = repo root (CARGO_MANIFEST_DIR/../..) because invocation is `python3 -m pyextract`; carries v1 compile-time-path debt forward without deepening it"
  - "detect_language: when both Go and Python markers are present, prefer Go (documented deterministic order, defensive only — fixtures are single-language); empty/ambiguous -> typed Config error, never a guess"
  - "run_pyextract deserializes into the SAME facts::GoFacts DTO (the contract is language-agnostic; the `Go` name is historical)"

patterns-established:
  - "Pattern E (rule 3): single deterministic source/path — language dispatch is one detector + one match arm per language, no fallback chain"
  - "Pattern D (RUST-04): every Rust seam ?-propagates a typed CoreError; no production unwrap/expect/panic; test allows scoped per-module"

requirements-completed: [PYSRC-05]

# Metrics
duration: ~18min
completed: 2026-06-25
---

# Phase 2 Plan 1: Python Source Host Seam Summary

**Language-aware `build_graph`/`collect` dispatch (one deterministic `detect_language`) plus the `python3 -m pyextract` subprocess driver, `PythonToolchainMissing` typed error, and `FastApi`/`Flask` Source built-ins — the host switch that routes a Python target through the SAME facts → ApiGraph → lowering path the Go sidecar uses, with zero new crates.**

## Performance

- **Duration:** ~18 min
- **Started:** 2026-06-25
- **Completed:** 2026-06-25
- **Tasks:** 3
- **Files modified:** 7

## Accomplishments
- `CoreError::PythonToolchainMissing` variant (verbatim clone of `GoToolchainMissing`, `python3` message) + display test.
- `helper::pyextract_dir()` + `run_pyextract()`/`run_pyextract_with()`: spawns `python3 -m pyextract <target>` with DISCRETE args (no shell — threat T-02-01-py), reuses `HelperExit`/`FactsParse`, maps spawn failure to `PythonToolchainMissing`.
- `analyze::Lang` enum + `detect_language()`: a SINGLE tree scan classifying `go.mod`/`*.go` → Go, `*.py` → Python, neither → typed `Config` error — one decision, never a fallback chain (CLAUDE.md rule 3 / T-02-04-py).
- `build_graph` and `diagnostics::collect` rewritten to dispatch through `detect_language` into one `match` arm per language; Go path regression-free (`snapshot_diagnostics` still green).
- `FastApi` + `Flask` Source built-ins (GoGin clones, prelude-exported) — both call the SAME `build_graph`; reject 0/many inputs with typed `Config`.

## Task Commits

Each task was committed atomically:

1. **Task 1: PythonToolchainMissing error + run_pyextract/pyextract_dir driver** - `5288529` (feat)
2. **Task 2: Single deterministic language dispatch in build_graph and collect** - `c6a7c61` (feat)
3. **Task 3: FastApi + Flask Source built-ins** - `91ed74f` (feat)

## Files Created/Modified
- `crates/gnr8-core/src/error.rs` - Added `CoreError::PythonToolchainMissing` variant + display test.
- `crates/gnr8-core/src/analyze/helper.rs` - Added `pyextract_dir()`, `run_pyextract()`/`run_pyextract_with()`, and their unit tests.
- `crates/gnr8-core/src/analyze/mod.rs` - Added `Lang` enum + `detect_language()` + `scan_markers()`; rewrote `build_graph` to dispatch; added classification/empty-target tests.
- `crates/gnr8-core/src/diagnostics/mod.rs` - Applied the identical single-detection dispatch in `collect`.
- `crates/gnr8-core/src/sdk/builtins.rs` - Added `FastApi` + `Flask` Source structs/builders/impls + unconfigured-error tests.
- `crates/gnr8-core/src/sdk/mod.rs` - Re-exported `FastApi` + `Flask` from `sdk::prelude`.
- `crates/gnr8-core/src/lib.rs` - Updated the `build_graph_no_longer_returns_not_yet_implemented` test to accept `Config` (dispatch deviation, see below).

## Decisions Made
- **`pyextract_dir()` = repo root** (`CARGO_MANIFEST_DIR/../..`), one level shallower than `goextract_dir()`, because the invocation is `python3 -m pyextract` (run from the dir that *holds* the package). Carries the v1 compile-time-path debt forward without deepening it (CONTEXT / RESEARCH A6).
- **Ambiguity rule:** both-markers-present prefers Go (documented order, defensive only — fixtures are single-language); empty/ambiguous is a typed `Config` error, never a guessed language.
- **Reuse `facts::GoFacts`** as the Python deserialization target — the neutral contract is language-agnostic; no separate DTO.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] lib.rs `build_graph_no_longer_returns_not_yet_implemented` test broke under the new dispatch**
- **Found during:** Task 3 (full `cargo test --workspace --locked` gate)
- **Issue:** A second copy of the bad-target assertion lived in `crates/gnr8-core/src/lib.rs` (not just `analyze/mod.rs`, which the plan called out). The Task 2 dispatch change made a non-existent target classify as ambiguous → `Config` *before* any spawn, so the test's `GoToolchainMissing | HelperExit | FactsParse` matcher no longer held.
- **Fix:** Extended the matcher to also accept `CoreError::Config` and `CoreError::PythonToolchainMissing` and updated the comment — identical to the plan-sanctioned update of the `analyze/mod.rs` twin test.
- **Files modified:** `crates/gnr8-core/src/lib.rs`
- **Verification:** `cargo test --workspace --locked` green (128 lib tests + all integration tests).
- **Committed in:** `91ed74f` (Task 3 commit)

**2. [Rule 1 - Bug] clippy `-D warnings` rejected the initial Task 2 code**
- **Found during:** Task 2 (`cargo clippy -p gnr8-core --all-targets -- -D warnings`)
- **Issue:** (a) `scan_markers` used `name.ends_with(".go"/".py")` → `case_sensitive_file_extension_comparisons`; (b) a doc comment said `FastAPI` without backticks → `doc_markdown`; (c) the new empty-target test used `.map().unwrap_or()` → `map_unwrap_or`.
- **Fix:** Switched `scan_markers` to `Path::extension().eq_ignore_ascii_case(...)` (mirrors the existing `is_go_file` helper); backticked `FastAPI`; used `map_or`.
- **Files modified:** `crates/gnr8-core/src/analyze/mod.rs`
- **Verification:** `cargo clippy --workspace --all-targets --locked -- -D warnings` clean.
- **Committed in:** `c6a7c61` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 Rule 1 bugs)
**Impact on plan:** Both necessary for the green test + clippy gates the plan mandates. No scope creep — both are mechanical consequences of the planned dispatch change, fixed within the directly-affected files.

## Issues Encountered
- None beyond the two auto-fixed deviations above. The Go toolchain and `python3 3.9.25` were both present; the Go-dependent `snapshot_diagnostics` test passed (PATH sourced per the toolchain note).

## CLAUDE.md Compliance
- **Rule 2 (no new deps):** `git diff crates/gnr8-core/Cargo.toml` is empty — zero new crates.
- **Rule 3 (no fallback / one path):** language dispatch is a single `detect_language` scan feeding one `match` arm per language; `grep` confirms `run_goextract` sits in a `Lang::Go` match arm, not an `.or_else(`/fallback after `run_pyextract`.
- **Rule 4 (config is code):** Python extraction is enabled via the `FastApi`/`Flask` Source built-ins (Rust), not a data file.

## Next Phase Readiness
- The host seam is the switch every downstream wave needs: `build_graph(<python-dir>)` now routes to `pyextract`. The `pyextract/` sidecar itself is NOT built here (Plan 02) — until it lands, a Python target reaches `run_pyextract` and surfaces a typed `HelperExit`/`PythonToolchainMissing`/`FactsParse`, never a panic or garbage.
- The four FastAPI/Flask snapshot tests remain `#[ignore]` (red-by-design) and flip green in later waves once the sidecar emits real facts — zero snapshot edits required here.

## Self-Check: PASSED

- `02-01-SUMMARY.md` exists.
- Task commits `5288529`, `c6a7c61`, `91ed74f` all present in git history.
- No unexpected file deletions across the three task commits.

---
*Phase: 02-python-source-pyextract*
*Completed: 2026-06-25*
