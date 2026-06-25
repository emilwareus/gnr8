---
phase: 05-poc-hardening-and-demo
plan: 01
subsystem: cli
tags: [rust, doctor, diagnostics, benchmark, exit-code, serde-json, bash]

# Dependency graph
requires:
  - phase: 02-graph-and-analysis
    provides: build_graph().diagnostics (structured Diagnostic{severity,message,file,line})
  - phase: 04-lifecycle-and-watch
    provides: lifecycle::plan_only/has_drift/WriteAction, config::load, manifest, is_under_output semantics, run_check exit+json pattern
provides:
  - "gnr8 doctor — read-only health aggregator: lifecycle facts + stale/drift + unsupported-pattern diagnostics, human report + --json, exit 0 healthy / 1 actionable"
  - "DoctorReport::assemble PURE grouping + has_actionable_problem exit policy (Pitfall 1: informational WARNs excluded)"
  - "scripts/bench.sh — reproducible cold/warm-no-op/single-file-edit wall-clock benchmark on a scratch fixture copy"
affects: [05-02-demo-and-evidence, ci, milestone-evidence]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Read-only CLI aggregator: pure decision (DoctorReport::assemble) + thin impure shell (run_doctor), mirroring run_check shell-vs-decision split"
    - "Exit-policy split: informational analysis WARNs excluded from has_actionable_problem so they never force non-zero exit (Pitfall 1)"
    - "Graceful-absence harvesting: build_graph/plan_only via .ok() -> Option, missing Go toolchain reported as a finding not a crash (Pitfall 4)"
    - "Hermetic bench: mktemp -d scratch copy + trap rm -rf EXIT, committed fixture never mutated (Pitfall 2)"

key-files:
  created:
    - crates/gnr8/src/doctor.rs
    - scripts/bench.sh
  modified:
    - crates/gnr8/src/main.rs

key-decisions:
  - "doctor is a read-only aggregator in the binary (run_doctor + doctor module), reusing build_graph().diagnostics + lifecycle::plan_only + config::load + a go-version probe; NO new core analysis (D-02)"
  - "Exit policy mirrors run_check: 0 = healthy, 1 = actionable; the fixture's informational unsupported-pattern WARNs are EXCLUDED from actionable so doctor is never permanently red (Pitfall 1)"
  - "LifecycleHealth keeps 4 independent bools (initialized/config_valid/go_toolchain/inputs_overlap_outputs) as the documented --json field set; struct_excessive_bools allowed locally rather than collapsing to enums and breaking the contract"
  - "inputs_overlap_outputs checks both directions on a path-separator boundary (is_under_output semantics); inputs=['.'] is not a false positive"
  - "bench.sh times external wall-clock (date +%s%N) including go run/build/gofmt subprocess cost — the honest end-to-end number, no asserted thresholds (Pitfall 3)"

patterns-established:
  - "Pure-report + impure-shell: DoctorReport::assemble takes already-collected facts (no I/O), making the full exit-policy truth table unit-testable without a filesystem or Go toolchain"
  - "Per-diagnostic why/fix enrichment by message-kind matching (free-form map / float64 narrowing / untyped query param / generic default) — doctor explains, does not re-analyze (D-02)"

requirements-completed: [HARD-01, HARD-03]

# Metrics
duration: 7min
completed: 2026-06-24
---

# Phase 5 Plan 01: doctor diagnostics aggregator + benchmark harness Summary

**`gnr8 doctor` read-only health aggregator (lifecycle + stale/drift + unsupported-pattern diagnostics, human report + `--json`, exit 0 healthy / 1 actionable with informational WARNs excluded) plus a hermetic `scripts/bench.sh` producing cold/warm-no-op/single-file-edit wall-clock numbers on a scratch fixture copy.**

## Performance

- **Duration:** 7 min
- **Started:** 2026-06-24T22:19:32Z
- **Completed:** 2026-06-24T22:26:24Z
- **Tasks:** 3
- **Files modified:** 3 (2 created, 1 modified)

## Accomplishments

- Implemented `gnr8 doctor` (the last unimplemented CLI arm) as a READ-ONLY aggregator over existing green subsystems — no new analysis or codegen (D-02).
- Defined and unit-tested the exit-code contract (Pitfall 1): the fixture's informational unsupported-pattern WARNs do NOT make doctor red; only lifecycle/staleness problems do.
- Added per-diagnostic `why`/`fix` explanations (free-form map, float64 narrowing, untyped query param) so the report explains "what I can't represent and why."
- `scripts/bench.sh` drives the real release binary on a `mktemp -d` scratch copy of the fixture (trap-cleaned), printing the three honest wall-clock numbers and leaving the committed fixture untouched.

## Task Commits

Each task was committed atomically:

1. **Task 1: DoctorReport types, grouping, exit-policy, renders (TDD)** - `63eeffc` (feat) — implementation + 9 unit tests landed together (GREEN); module declared in main.rs
2. **Task 2: Wire run_doctor in main.rs (read-only aggregator, exit policy, --json)** - `4cc57f6` (feat)
3. **Task 3: scripts/bench.sh reproducible 3-scenario benchmark** - `4a5c3fd` (chore)

**Plan metadata:** _this commit_ (docs: complete plan)

_Note: Task 1 is a `tdd="true"` task; the failing-test RED step and the passing GREEN step were authored in one file and committed together as a single GREEN commit (tests verified passing before commit). No separate RED commit exists — see TDD Gate Compliance below._

## Files Created/Modified

- `crates/gnr8/src/doctor.rs` (created) - `DoctorReport` + sub-structs (`LifecycleHealth`, `OutputHealth`, `DoctorDiagnostic`, `DoctorSummary`), PURE `assemble`, `has_actionable_problem` exit policy, `render_human`, `explain` why/fix mapping, and 9 unit tests covering the full exit-policy truth table + `--json` field-set stability.
- `crates/gnr8/src/main.rs` (modified) - `mod doctor;`, `Commands::Doctor => return run_doctor(cli.json)` early-return arm, `run_doctor` impure shell (project_paths -> lifecycle facts -> graceful diagnostics/drift harvest -> assemble -> human/--json -> exit 1 on actionable), `probe_go`, `inputs_overlap_outputs` helper.
- `scripts/bench.sh` (created) - hermetic 3-scenario benchmark over the release binary on a scratch fixture copy.

## Decisions Made

- **doctor in the binary, not core (D-02):** the report shape + exit policy is CLI UX (like `render.rs`/`run_check`); core stays read-only with no new analysis. `doctor` only CALLS `build_graph`/`plan_only`/`config::load`.
- **Exit policy excludes informational WARNs (Pitfall 1):** `has_actionable_problem` is true only for `.gnr8/` missing, invalid config, missing Go toolchain, input/output overlap, or any stale/drifted output. The 20 diagnostics observed on a generated scratch project (and the fixture's 7) keep exit 0.
- **Four-bool LifecycleHealth kept as the published `--json` field set:** `struct_excessive_bools` allowed locally with rationale rather than refactoring to enums (which would change the documented JSON contract a CI gate reads).
- **bench.sh honest wall-clock, no thresholds (Pitfall 3):** external `date +%s%N` timing includes the dominant `go run`/`go build`/`gofmt` subprocess cost; numbers are labeled environment-dependent.

## Deviations from Plan

None - plan executed exactly as written.

The plan's authoritative 7-arg `assemble` signature (incl. `inputs_overlap_outputs`) was used verbatim at both the definition and the `run_doctor` call site (PLAN-CHECK W2 honored — the abbreviated 5-arg form in the Task-1 `<behavior>` prose was NOT used).

---

**Total deviations:** 0 auto-fixed.
**Impact on plan:** None — all three tasks implemented as specified; every `<acceptance_criteria>` and the plan-level `<verification>` passed without auto-fixes.

## TDD Gate Compliance

Task 1 (`tdd="true"`) authored the 9 unit tests and the implementation in a single file and committed them together as one `feat(05-01)` GREEN commit after verifying the tests pass. There is no separate `test(...)` RED commit in the git log for this plan. The RED→GREEN intent was satisfied (tests were run and observed passing before commit, and exercise every documented behavior), but the gate-sequence convention of a distinct preceding `test(...)` commit was not followed — flagged here per the TDD gate-validation rule. All 9 tests are green and asserted at verification time.

## Issues Encountered

None. Observation (not an issue): running `doctor` on a scratch copy of `fixtures/goalservice` after `init`+`generate` reports 20 informational diagnostics rather than the fixture's canonical 7, because the scratch tree contains BOTH the committed `expected/sdk/*.go` golden files and the freshly-generated `sdk/*.go`, and `build_graph` (with `inputs=["."]`) analyzes both, surfacing extra "duplicate handler name" WARNs. This is correct read-only behavior — `doctor` faithfully reports `build_graph().diagnostics` for the current project — and all of them remain informational, so the exit-0 healthy verdict is unaffected.

## Verification

- `cargo test -p gnr8` green (25 passed, 1 ignored watch_smoke); the 9 new `doctor::tests` pass (healthy→0, missing-init/invalid-config/no-go/overlap/stale/drift→1, `--json` field set).
- `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `cargo fmt --all -- --check` clean.
- `cargo build -p gnr8 --locked` clean; `gnr8 doctor --json` in an uninitialized scratch dir emits `"healthy": false` and exits 1; on an initialized+generated scratch project emits `"healthy": true` and exits 0.
- `scripts/bench.sh` runs end-to-end on a scratch copy, prints `cold=…ms warm-no-op=…ms single-file-edit=…ms` (e.g. `cold=673ms warm-no-op=686ms single-file-edit=710ms`), and `git status` shows NO changes under `fixtures/goalservice/` afterward.
- `make gates` green (exit 0; the blocking wave-merge subset per 05-VALIDATION.md).
- No production `unwrap`/`expect`/`panic`; no new runtime crates.

## Next Phase Readiness

- HARD-01 (doctor diagnostics) and the benchmark half of HARD-03 are complete and gate-green.
- Ready for **05-02** (demo + evidence docs): the demo can drive `gnr8 doctor` and `scripts/bench.sh` on a scratch fixture copy. This plan deliberately did NOT write the demo/evidence docs (05-02 scope).
- No blockers.

---
*Phase: 05-poc-hardening-and-demo*
*Completed: 2026-06-24*

## Self-Check: PASSED

- FOUND: crates/gnr8/src/doctor.rs
- FOUND: scripts/bench.sh
- FOUND: .planning/phases/05-poc-hardening-and-demo/05-01-SUMMARY.md
- FOUND commit 63eeffc (Task 1: DoctorReport + tests)
- FOUND commit 4cc57f6 (Task 2: run_doctor wiring)
- FOUND commit 4a5c3fd (Task 3: scripts/bench.sh)
- FOUND: live `Commands::Doctor => return run_doctor` arm in main.rs (dispatch no longer returns not_yet on the live path)
