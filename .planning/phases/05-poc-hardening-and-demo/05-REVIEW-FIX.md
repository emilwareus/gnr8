---
phase: 05-poc-hardening-and-demo
fixed_at: 2026-06-25T00:00:00Z
review_path: .planning/phases/05-poc-hardening-and-demo/05-REVIEW.md
iteration: 1
findings_in_scope: 2
fixed: 2
skipped: 0
status: all_fixed
---

# Phase 5: Code Review Fix Report

**Fixed at:** 2026-06-25
**Source review:** `.planning/phases/05-poc-hardening-and-demo/05-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope (warnings): 2
- Fixed: 2 (WR-01, WR-02)
- Skipped: 0
- Also applied: 1 cosmetic verifier/Info item (demo.md footnote 37 -> 38)
- Deferred (out of scope, Info): IN-01, IN-02, IN-03

**Milestone gate:** `make check` ends **exit 0** (green). All four fixture snapshots
(`snapshot_graph` / `snapshot_diagnostics` / `snapshot_openapi` / `snapshot_sdk`) stay byte-identical
(0 changes vs the base commit). doctor stays read-only; no production `unwrap`/`expect`/`panic`;
`cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo fmt --all --check` both green.

## Fixed Issues

### WR-02: doctor diagnostics analyze gnr8's own generated output, diverging from what generate acts on

**Files modified:** `crates/gnr8-core/src/lifecycle/mod.rs`, `crates/gnr8/src/main.rs`, `crates/gnr8-core/tests/lifecycle.rs`
**Commit:** `a5306df`
**Status:** fixed

**Applied fix:** Added a new read-only `gnr8_core::lifecycle::diagnostics_only(&root, cfg)` seam that runs
the SAME front half of `build_outputs` (resolve the first input against the project root → `build_graph`
→ `exclude_output_paths`) and returns the resulting diagnostics. `run_doctor` now calls
`diagnostics_only` instead of a bare `build_graph(&resolved)`, so doctor harvests diagnostics over the
same graph `generate`/`check` act on and never re-analyzes gnr8's own generated `sdk/*.go` / `openapi.yaml`.

**Deeper fix found during implementation:** `exclude_output_paths` only filters `graph.operations` and
`graph.schemas` — the `graph.diagnostics` list is provenance-tagged independently and was NOT filtered.
So reusing `exclude_output_paths` alone still left diagnostics on generated files (a test caught a WARN on
`sdk/client.go`). `diagnostics_only` therefore also applies the same output anchors (`is_under_output`)
to `graph.diagnostics`. This keeps the change scoped to the doctor path and leaves
`exclude_output_paths`'s documented operations/schemas contract — and thus the generation output —
unchanged. `diagnostics_only` also enforces the same single-input PoC rejection as `build_outputs`.

**Tests added:** `diagnostics_only_excludes_generated_output` (stages the fixture under the default
layout, runs a cold `generate` to write `sdk/`, then asserts no doctor diagnostic points at a generated
output file) and `diagnostics_only_rejects_multi_input` (multi-input -> typed `CoreError::Config`).

**Snapshot safety:** The fixture snapshot path is the RAW `build_graph(fixtures/goalservice)` (never calls
`exclude_output_paths`), so `snapshot_diagnostics` and the other three snapshots are unchanged. Verified
byte-identical.

### WR-01: doctor reports a project that cannot analyze/generate as "healthy / outputs up to date"

**Files modified:** `crates/gnr8/src/doctor.rs`, `crates/gnr8/src/main.rs`
**Commit:** `f324651`
**Status:** fixed (logic change — see verification note)

**Applied fix:** Distinguished "no drift" from "drift could not be computed". `run_doctor` now derives an
explicit `drift_computable = config.is_ok() && go_present` signal (drift was ATTEMPTED) and passes it into
`DoctorReport::assemble`. When `drift_computable` is true but `plan_only` returned `None` (e.g. a
multi-input config `generate` would reject, or a Go source build/parse error), `assemble` sets a new
`outputs.drift_unknown` flag instead of leaving an empty partition that renders as clean.

- `render_human` renders a DISTINCT line — `drift: UNKNOWN — could not compute (run gnr8 check / gnr8
  generate for the error)` — instead of `(all outputs up to date)`.
- `drift_unknown` is counted as ACTIONABLE in BOTH `actionable_problem_count()` and
  `has_actionable_problem()`, so doctor exits non-zero (a usable CI gate) and `healthy` is false. This keeps
  the Phase-5 exit policy intact: informational analysis WARNs remain exit-0 (Pitfall 1 unchanged).
- Boundary respected: when config is invalid OR the toolchain is absent, drift is also unavailable, but
  that is already carried by the lifecycle findings, so `drift_computable` is false and `drift_unknown`
  stays off (no double-counting).

**JSON shape:** `drift_unknown` was added to the `outputs` sub-object only; the locked top-level
`doctor --json` field set (`healthy`, `lifecycle`, `outputs`, `diagnostics`, `summary`) and the `lifecycle`
sub-object key set are unchanged, so `doctor_json_field_set` stays green.

**Tests added:** `uncomputable_drift_is_actionable_not_healthy` (config valid + go present + drift None ->
`drift_unknown` true, actionable, `!healthy`, render does NOT contain "all outputs up to date" and DOES
contain the "could not compute" finding) and `unavailable_drift_from_bad_config_does_not_set_drift_unknown`
(invalid config -> `drift_unknown` stays false, no double-count). All 12 existing `assemble` call sites
updated for the new parameter.

**Verification note:** This finding involves exit-policy / state-classification logic. Syntax + unit tests
pass and the new tests assert the intended behavior directly, but a human should confirm the policy choice
(treating uncomputable drift as ACTIONABLE / non-zero exit) matches the intended CI-gate semantics before
the phase proceeds.

## Also Applied (cosmetic — verifier Info)

### demo.md footnote: "all 37 v1 requirements" -> 38

**Files modified:** `docs/demo.md`
**Commit:** `4b25c77`
**Status:** fixed

**Applied fix:** Corrected the `docs/demo.md` line-407 footnote from "all 37 v1 requirements" to "all 38 v1
requirements" to match `docs/evidence.md` (which canonically maps "all 38 actual v1 requirement IDs",
"38 / 38") and the `.planning/REQUIREMENTS.md` actual ID set. Verified 38 is the correct count before
editing (evidence.md explicitly documents the 37-vs-38 prose discrepancy).

## Deferred Issues

These are Info-tier (out of the `critical_warning` fix scope) and were intentionally NOT applied:

### IN-01: assemble computes the actionable verdict twice via two parallel predicates

**File:** `crates/gnr8/src/doctor.rs`
**Reason:** Info-tier, out of scope. NOTE: the WR-01 fix added the `drift_unknown` condition to BOTH
`actionable_problem_count()` and `has_actionable_problem()` consistently, so they remain in sync; the
underlying duplication IN-01 flags is unchanged but was not made worse. A future refactor can still derive
`has_actionable_problem() == actionable_problem_count() > 0`.

### IN-02: inline comment in inputs_overlap_outputs misdescribes the "."-trim mechanism

**File:** `crates/gnr8/src/main.rs:272-279`
**Reason:** Info-tier, out of scope. Comment-only correctness issue; the code behavior is already correct.

### IN-03: bench.sh single-file-edit scenario silently degrades if the awk anchor misses

**File:** `scripts/bench.sh:67-77`
**Reason:** Info-tier, out of scope. Benchmark-reliability hardening, not a safety/correctness defect; stays
entirely on the scratch copy.

---

_Fixed: 2026-06-25_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
