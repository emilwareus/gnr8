---
phase: 05-poc-hardening-and-demo
plan: 02
subsystem: docs
tags: [demo, evidence, milestone, traceability, make-check, benchmark, openapi, go-sdk]

# Dependency graph
requires:
  - phase: 05-poc-hardening-and-demo (05-01)
    provides: "gnr8 doctor read-only health aggregator + scripts/bench.sh (the demo drives doctor; evidence references the bench numbers)"
  - phase: 03-openapi-and-go-sdk-generation
    provides: "to_openapi + Go SDK codegen (the artifacts the demo shows updating)"
  - phase: 04-lifecycle-and-watch
    provides: "gnr8 init/generate/watch + no-op skip (WATCH-01) the demo exercises"
provides:
  - "docs/demo.md — reproducible fresh-checkout walkthrough (build -> scratch copy -> init -> generate -> doctor -> single-field edit -> re-generate, only affected OpenAPI + SDK outputs update); real captured output"
  - "docs/evidence.md — HARD-03 milestone sign-off: make check GREEN captured live this session + every v1 requirement mapped to concrete file/test + representative scripts/bench.sh numbers"
affects: [milestone-audit, complete-milestone, verify-work]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Verified-reproducible docs: every command + output block in docs/demo.md was actually run on a mktemp/cp -R scratch fixture copy and the real output pasted (not authored from memory)"
    - "Evidence-from-real-gate: docs/evidence.md gate-status table is captured from a live `make check` run (exit 0) this session, never asserted from memory (Pitfall 6)"
    - "Hermetic demo/bench: all gnr8 runs operate on a scratch copy of fixtures/goalservice; the committed fixture + its expected/ golden + CI module stay pristine (Pitfall 2)"

key-files:
  created:
    - docs/demo.md
    - docs/evidence.md
  modified:
    - .planning/REQUIREMENTS.md

key-decisions:
  - "Demo + evidence are docs-only; zero code changes, zero new dependencies (RESEARCH Package Legitimacy Audit: nothing to install this phase)"
  - "docs/demo.md uses the exact single-field CreateGoalInput.BenchField edit that scripts/bench.sh applies, so the demo's headline edit and the benchmark's single-file-edit scenario are the same reproducible change"
  - "Documented that the scratch copy reports 20 informational doctor diagnostics (not the canonical 7) because it contains BOTH expected/sdk/*.go and freshly-generated sdk/*.go under inputs=['.'] — correct read-only behavior, verdict stays healthy/exit 0 (matches 05-01 observation)"
  - "Corrected an off-by-one: the v1 requirement set is 38 IDs, not 37 (the REQUIREMENTS.md coverage note said 37 but its own checklist enumerates 38); evidence.md maps all 38 and proves exact set-equality with REQUIREMENTS.md"

patterns-established:
  - "Reproducible-demo discipline: build the release binary, cp -R the fixture into mktemp -d, run the full loop, paste the REAL captured output, then prove git status shows no fixture changes"
  - "Milestone-evidence discipline: run the authoritative gate live, capture per-sub-gate PASS with the producing command, and map every requirement to a concrete artifact"

requirements-completed: [HARD-02, HARD-03]

# Metrics
duration: 12min
completed: 2026-06-25
---

# Phase 5 Plan 02: Demo And Evidence Summary

**`docs/demo.md` — a verified-reproducible fresh-checkout walkthrough (build → scratch-copy the fixture → init → generate → doctor → add one Go field → re-generate, with only the affected OpenAPI + SDK outputs updating, all real captured output) — plus `docs/evidence.md`, the HARD-03 milestone sign-off that captures `make check` GREEN live (exit 0) and maps every v1 requirement to the concrete file/test where it is satisfied.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-06-25T00:22:00Z
- **Completed:** 2026-06-25T00:35:00Z
- **Tasks:** 2
- **Files modified:** 3 (2 created: docs/demo.md, docs/evidence.md; 1 modified: .planning/REQUIREMENTS.md)

## Accomplishments

- Wrote `docs/demo.md` (408 lines) — a copy-pasteable, fresh-checkout-reproducible demo. I actually ran every step on a `mktemp -d` / `cp -R` scratch copy of `fixtures/goalservice` and pasted the real output: `gnr8 init` (scaffolds `.gnr8/`), `gnr8 generate` (5 written → `openapi.yaml` + 4 SDK files), `gnr8 doctor` (grouped report, healthy/exit 0), then the headline edit (add `BenchField` to `CreateGoalInput`) → `gnr8 generate` (2 written, 3 unchanged — WATCH-01 no-op) showing `benchField` now present in BOTH `openapi.yaml` and `sdk/models.go`.
- Wrote `docs/evidence.md` (HARD-03 sign-off) — GENERATED from a LIVE `make check` run this session (exit 0): per-sub-gate PASS table (fmt-check, clippy `-D warnings`, the full test suite — 25 bin tests incl. 9 doctor tests + 82 lib + 3 determinism + 22 lifecycle + 3 sdk_compile + 4 contract snapshots, fixture-build, goextract-build) plus a complete requirement-traceability table mapping every v1 ID to a concrete file/test.
- Captured representative benchmark numbers from `scripts/bench.sh` (3 runs, cold/warm-no-op/single-file-edit ≈ 700–770 ms each), labeled environment-dependent and reproducible — never asserted as thresholds (Pitfall 3).
- Verified the committed fixture stayed pristine throughout (demo dry-run + bench + `make check`): `git status --short fixtures/goalservice/` is empty.
- **Milestone gate ends GREEN:** final `make check` exit 0.

## Task Commits

Each task was committed atomically:

1. **Task 1: docs/demo.md — reproducible fresh-checkout → source-edit → updated-outputs walkthrough** - `6c1f082` (docs)
2. **Task 2: docs/evidence.md — make check GREEN capture + v1-requirement traceability sign-off** - `401b29a` (docs; amended to include the 37→38 count correction)

**Plan metadata:** _this commit_ (docs: complete plan — SUMMARY + STATE + ROADMAP + REQUIREMENTS)

## Files Created/Modified

- `docs/demo.md` (created) — fresh-checkout demo: prerequisites, build, scratch copy (mktemp/cp -R), init, cold generate, doctor, the single-field `CreateGoalInput.BenchField` edit, re-generate (only `openapi.yaml` + `sdk/models.go` rewritten), watch (optional), benchmark, and a closing `git status` clean-tree check. Real captured output throughout.
- `docs/evidence.md` (created) — milestone v1.0 sign-off: `make check` gate-results table (all PASS, captured live), full per-test breakdown, the requirement→satisfied-by traceability table (all v1 IDs), representative benchmark numbers, and the ready-for-review sign-off.
- `.planning/REQUIREMENTS.md` (modified) — corrected the coverage note from "37 total" to "38 total" (the checklist always enumerated 38 IDs; the headline was an off-by-one).

## Decisions Made

- **Docs-only, zero new dependencies:** Per D-04/D-05 and the RESEARCH Package Legitimacy Audit (no installs this phase). No code touched.
- **Demo edit == bench edit:** `docs/demo.md` uses the exact `CreateGoalInput.BenchField` field that `scripts/bench.sh` appends, so the headline "source edit → updated outputs" change and the benchmark single-file-edit scenario are one and the same, reproducible change.
- **Faithfully documented the 20-vs-7 diagnostics behavior:** On the scratch copy, `doctor` reports 20 informational diagnostics (not the fixture's canonical 7) because the tree carries both `expected/sdk/*.go` and the freshly-generated `sdk/*.go` and `inputs=["."]` analyzes both. This is correct read-only behavior; the verdict stays healthy/exit 0. Documented in the demo so readers aren't surprised (matches the 05-01 SUMMARY observation).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Corrected the v1 requirement count (37 → 38)**
- **Found during:** Task 2 (evidence traceability table)
- **Issue:** The plan, `.planning/REQUIREMENTS.md`'s coverage note, and the interfaces block all state "37 v1 requirements", but the REQUIREMENTS.md "## v1 Requirements" checklist enumerates **38** distinct IDs (POC×3, RUST×4, FIX×4, GO×6, GRAPH×3, OAPI×3, SDK×5, WS×4, WATCH×3, HARD×3 = 38). A `comm -3` set-difference between the evidence table and the REQUIREMENTS.md checklist is empty (exact coverage), and there are 38, not 37. The headline "37" is an off-by-one in the planning summary.
- **Fix:** Wrote `docs/evidence.md` to map all 38 actual v1 IDs (the plan's authoritative `<automated>` id-presence gate still passes — it spot-checks 12 representative IDs, all present), added an explicit count note explaining the off-by-one, and corrected the `.planning/REQUIREMENTS.md` coverage note from "37 total" to "38 total".
- **Files modified:** docs/evidence.md, .planning/REQUIREMENTS.md
- **Verification:** `comm -3 <(evidence IDs) <(REQUIREMENTS IDs)` prints nothing (exact set-equality); per-prefix tally sums to 38; Task 2 `<automated>` gate prints `all-ids-present`.
- **Committed in:** docs/evidence.md in `401b29a` (Task 2); the REQUIREMENTS.md note in the plan-metadata commit.

---

**Total deviations:** 1 auto-fixed (1 bug — a factual count error in the planning docs).
**Impact on plan:** None to scope. The substantive requirement ("map every v1 requirement to where satisfied") is fully met — the doc covers the complete, real requirement set and proves exact coverage. The only change is correcting an off-by-one label so the evidence is internally consistent and accurate for the milestone audit. No code changed; no new dependencies.

## Issues Encountered

None. (Observation, not an issue: a shell-quoting quirk made an interactive verification loop report "0/37" while the underlying content was correct — confirmed via direct `grep`, `comm -3` set-difference, and per-prefix tally that all v1 IDs are present exactly once. The authoritative `<automated>` gate passed throughout.)

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **This is the final plan of the final phase (Phase 5, 2 of 2) — the v1.0 milestone build is complete.**
- HARD-02 (demo) and HARD-03 (evidence + all gates green) are satisfied; `make check` exits 0; every v1 requirement is mapped to a concrete artifact.
- The committed fixture and all tracked source files are clean (demo/bench ran hermetically on scratch copies).
- Ready for `/gsd:complete-milestone` (the milestone audit consumes `docs/evidence.md`) and `/gsd:verify-work`.
- No blockers.

---
*Phase: 05-poc-hardening-and-demo*
*Completed: 2026-06-25*

## Self-Check: PASSED

- FOUND: docs/demo.md
- FOUND: docs/evidence.md
- FOUND: .planning/phases/05-poc-hardening-and-demo/05-02-SUMMARY.md
- FOUND commit 6c1f082 (Task 1: docs/demo.md)
- FOUND commit 401b29a (Task 2: docs/evidence.md)
- `make check` exit 0 (milestone gate GREEN, captured live this session)
- `git status --short fixtures/goalservice/` empty (committed fixture pristine)
