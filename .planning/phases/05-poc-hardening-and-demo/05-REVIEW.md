---
phase: 05-poc-hardening-and-demo
reviewed: 2026-06-24T22:41:45Z
depth: deep
files_reviewed: 4
files_reviewed_list:
  - crates/gnr8/src/doctor.rs
  - crates/gnr8/src/main.rs
  - scripts/bench.sh
  - docs/demo.md
findings:
  critical: 0
  warning: 2
  info: 3
  total: 5
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-06-24T22:41:45Z
**Depth:** deep
**Files Reviewed:** 4 (doctor.rs, main.rs, scripts/bench.sh, docs/demo.md; docs/evidence.md cross-read)
**Status:** issues_found

## Summary

Phase 5 adds the read-only `gnr8 doctor` aggregator (`doctor.rs` + `run_doctor` in `main.rs`), a
benchmark harness (`scripts/bench.sh`), and demo/evidence docs. The headline correctness claims hold
up well under adversarial tracing:

- **`assemble` is genuinely pure.** It takes already-collected facts (booleans, `Option<Vec<Diagnostic>>`,
  `Option<&WritePlan>`) and only groups/classifies them. No filesystem, no subprocess, no `build_graph`.
  The impure half (`probe_go`, `config::load`, `build_graph`, `plan_only`) lives in `run_doctor`. Clean split.
- **`run_doctor` only CALLS existing seams** — no new analysis or codegen. It reuses `build_graph`,
  `plan_only`, and the `config`/workspace probes verbatim.
- **The exit policy is correct.** `has_actionable_problem()` excludes the analysis `diagnostics` entirely
  (verified against `actionable_problem_count()` and the 9 unit tests). Informational WARNs cannot force
  exit 1 — Pitfall 1 is respected and unit-tested. `informational_diagnostics_alone_are_not_actionable`
  directly proves it.
- **Graceful Go-toolchain degradation confirmed.** `probe_go()` uses `.output().is_ok()` (no `?`/unwrap),
  a missing toolchain becomes `go_toolchain=false` (an actionable finding, not a crash), and the
  diagnostics/drift harvest is gated on `(Ok(cfg), true)` so it never runs without a toolchain.
- **No production `unwrap`/`expect`/`panic`.** The only such calls are in the `#[cfg(test)]` module with
  the scoped `allow`.
- **`bench.sh` is safe.** `set -euo pipefail`, `mktemp -d` scratch dir, `trap 'rm -rf "$WORK"' EXIT`
  cleanup, `cp -R` of the fixture, all mutation confined to the scratch copy, no `eval`/`sh -c`, no
  unquoted destructive `rm`, and timings are reported (never asserted as thresholds). The off-by-one
  requirement-count fix (38 vs 37) is correctly handled in the docs and is not a defect.

Two real defects remain, both in `run_doctor`'s handling of the `None` (unavailable) drift path, plus
three low-severity quality items. None are BLOCKERs: the documented single-input `inputs=["."]` flow
works correctly and safely.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: `doctor` reports a project that cannot analyze/generate as "healthy / outputs up to date" (silent green)

**File:** `crates/gnr8/src/main.rs:321-344`, rendered by `crates/gnr8/src/doctor.rs:172-182,314-316`

**Issue:** When the drift computation is unavailable, `doctor` masks the failure as a clean, healthy
project. In `run_doctor`, `drift = gnr8_core::lifecycle::plan_only(&root, cfg).ok()` swallows ANY error
into `None`. `assemble` then turns `drift: None` into a default (all-empty) `OutputHealth`, and
`render_human` prints `(all outputs up to date)` (doctor.rs:314-316) while `has_actionable_problem()`
returns `false` → exit 0, verdict "healthy".

This is reachable in normal use, not just an exotic edge case:
- **Multi-input config.** `config::load` does NOT reject `inputs.len() > 1` (it only validates TOML;
  the rejection lives in `lifecycle::build_outputs`, mod.rs:486-494). So with two configured inputs,
  `config_valid=true`, `build_graph(cfg.inputs.first())` silently analyzes only the first input, and
  `plan_only` errors out (multi-input rejected) → `drift=None` → doctor renders "healthy / all outputs
  up to date" for a project that `gnr8 generate` would refuse to build.
- **Any analysis error in the user's source.** If the project has a Go compile/parse error,
  `plan_only` fails → `None` → doctor still says "healthy / up to date / exit 0".

`doctor` is explicitly positioned as a CI gate ("Deliberate non-zero exit so `gnr8 doctor` is a usable
CI gate", main.rs:352-355). A gate that returns exit 0 + "healthy" when the project fails to analyze is
a false negative — the inverse failure of Pitfall 1. The comment at main.rs:319-320 even CLAIMS this is
"reported as 'diagnostics/drift unavailable'", but no such finding exists anywhere in the report shape
or render path — the claim is not implemented.

**Fix:** Distinguish "drift unavailable" from "drift clean". Track availability and treat unavailability
as either an actionable finding or at minimum a visibly-rendered non-clean state. Sketch:

```rust
// run_doctor: when config+go are present but plan_only fails, this is NOT "up to date".
let drift_available = drift.is_some();
// ... pass an explicit availability signal into assemble, OR
// gate the harvest so a multi-input/analysis failure surfaces as a lifecycle finding.
```

In `assemble`/`render_human`, render an unavailable drift as e.g.
`OUTPUTS (drift unavailable — analysis/plan failed; run `gnr8 check` for details)` and count it toward
`has_actionable_problem()` (or at least never render it as `(all outputs up to date)`). At a minimum,
add a `doctor` test for the `drift=None && config_valid && go_present` case asserting it is NOT rendered
as a clean "all outputs up to date" healthy report.

### WR-02: `doctor` diagnostics analyze gnr8's own generated output, diverging from what `generate` acts on

**File:** `crates/gnr8/src/main.rs:323-328`

**Issue:** `run_doctor` harvests diagnostics with a bare `build_graph(&resolved)` and does NOT apply
`lifecycle::exclude_output_paths` — the filter `build_outputs` uses (mod.rs:452-465,508-512) to drop
gnr8's own generated `sdk/*.go`/`openapi.yaml` from the analyzed graph. As a result `doctor`'s
diagnostic set is computed over a DIFFERENT graph than the one `generate`/`check` actually use: under
the default `inputs=["."]`, doctor re-analyzes the generated SDK and surfaces spurious
"duplicate handler name" WARNs (the "20 vs 7" the demo doc explains at docs/demo.md:260-266).

Exit-policy-wise this is benign — all goextract diagnostics are `WARN` (goextract/internal/diag/diag.go
emits only `severityWarn`), and `diagnostics` are excluded from `has_actionable_problem()` regardless of
severity. But it is a correctness/consistency defect in the INFORMATIONAL output: doctor tells the user
about diagnostics on files gnr8 itself wrote and would never ingest during generation, which is
misleading "what I can't represent" guidance (D-02 says doctor explains, it does not re-analyze a
different graph). It also means the diagnostic count is non-deterministic with respect to whether the
project happens to contain previously-generated output (e.g. the fixture's committed `expected/sdk/`).

**Fix:** Make doctor harvest diagnostics from the SAME graph the lifecycle uses. The cleanest path is to
expose the post-`exclude_output_paths` graph (or its diagnostics) from `gnr8-core` and call that from
`run_doctor`, rather than re-deriving via a raw `build_graph` that skips the exclusion:

```rust
// e.g. add a gnr8_core::lifecycle::diagnostics_only(&root, cfg) that runs
// build_graph + exclude_output_paths and returns graph.diagnostics, so doctor
// and generate agree on the analyzed set.
```

## Info

### IN-01: `assemble` computes the actionable verdict twice via two parallel predicates

**File:** `crates/gnr8/src/doctor.rs:214,219,226-257`

**Issue:** `assemble` calls `actionable_problem_count()` (lines 226-243) to populate the summary, then
separately calls `has_actionable_problem()` (lines 250-257) to set `healthy`. The two functions encode
the SAME truth table independently. They agree today, but a future edit to one (e.g. adding a new
actionable condition) that misses the other would silently desync `summary.actionable_problems` from
`healthy`. Duplicated policy is a maintenance hazard.

**Fix:** Derive one from the other — e.g. `has_actionable_problem()` can be
`self.actionable_problem_count() > 0`, eliminating the parallel boolean expression:

```rust
pub(crate) fn has_actionable_problem(&self) -> bool {
    self.actionable_problem_count() > 0
}
```

### IN-02: Inline comment in `inputs_overlap_outputs` misdescribes the `"."`-trim mechanism

**File:** `crates/gnr8/src/main.rs:272-279`

**Issue:** The comment claims a `"."` input short-circuits because it is `"."`-trimmed-to-empty
(`return false` on the `a.is_empty()` branch). That is factually wrong: `".".trim_matches('/')` is
still `"."`, not empty — the `is_empty()` guard never fires for `"."`. The OUTCOME (no false positive
for the default `inputs=["."]`) is still correct, but for a different reason (`"openapi.yaml"` does not
`==` or `starts_with("./")` against `"."`). The misleading rationale could cause a future maintainer to
"fix" the wrong branch.

**Fix:** Correct the comment to state the real reason the default config is not flagged (no path equals
or is separator-nested under `"."`), or drop the inaccurate `is_empty()` justification.

### IN-03: `bench.sh` single-file-edit scenario silently degrades to a warm no-op if the awk anchor misses

**File:** `scripts/bench.sh:67-77`

**Issue:** The awk edit inserts `BenchField` only on a line matching `^type CreateGoalInput struct \{`.
If the fixture's struct formatting ever changes (e.g. a trailing comment, different whitespace), the
pattern never matches, awk emits the file unchanged, `mv` succeeds, and the "single-file-edit" scenario
silently becomes a second warm no-op — the benchmark reports a misleading `single-file-edit` number with
no error. This is a benchmark-reliability gap, not a safety issue (it stays entirely on the scratch
copy).

**Fix:** Verify the edit actually landed before timing the third scenario:

```bash
grep -q 'BenchField' "$GOAL_GO" || { echo "bench: failed to inject BenchField into $GOAL_GO" >&2; exit 1; }
```

---

_Reviewed: 2026-06-24T22:41:45Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
