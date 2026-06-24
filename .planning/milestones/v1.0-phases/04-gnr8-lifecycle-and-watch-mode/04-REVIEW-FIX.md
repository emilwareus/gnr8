---
phase: 04-gnr8-lifecycle-and-watch-mode
fixed_at: 2026-06-24T00:00:00Z
review_path: .planning/phases/04-gnr8-lifecycle-and-watch-mode/04-REVIEW.md
verification_path: .planning/phases/04-gnr8-lifecycle-and-watch-mode/04-VERIFICATION.md
iteration: 1
gap_closed: true
findings_in_scope: 8
fixed: 8
deferred: 2
status: all_fixed
---

# Phase 4: Code Review Fix Report

**Fixed at:** 2026-06-24
**Source review:** 04-REVIEW.md (4 warnings, 4 info)
**Source gap:** 04-VERIFICATION.md (criterion 3 / WATCH-01, status `partial`)
**Iteration:** 1

**Principle applied throughout:** gnr8 must not ingest or loop on its OWN generated output. The verification gap and the top warnings are all instances of this; the gap fix and the watch-filter fix now share the same loop-safety logic in two path spaces.

**Summary:**
- Verification gap (criterion 3 / WATCH-01): **CLOSED**
- Warnings fixed: WR-01, WR-02, WR-03, WR-04 (all 4)
- Info fixed: IN-03, IN-04 (cheap + clearly correct)
- Info deferred (documented PoC scope): IN-01, IN-02

---

## Gap Closure (criterion 3 / WATCH-01) — CLOSED

**Commit:** `535455a` — `fix(04): exclude generated output paths from analysis (no-op gap)`
**Files:** `crates/gnr8-core/src/lifecycle/mod.rs`, `crates/gnr8-core/tests/lifecycle.rs`

**The gap:** the shipped default `.gnr8/config.toml` (`inputs = ["."]`, `sdk_dir = "sdk"`, `openapi = "openapi.yaml"`) writes the SDK *inside* the analyzed input tree. A second `gnr8 generate` re-analyzed gnr8's own `sdk/*.go`, doubling every schema (9 → 18) and rewriting larger, duplicated output — so `init → generate → generate` never no-op'd. Verifier reproduced 3/3.

**Architectural fix (robust, output-location-independent):**
- `exclude_output_paths(graph, config)` drops every operation/schema whose **provenance file** is under a configured output anchor (`sdk_dir`, `openapi` path, or `.gnr8/`) BEFORE naming/lowering. Generated files are therefore never ingested regardless of where the user points outputs. This is the graph-side twin of the watch loop-safety filter (`watch::is_under_any_output`); both answer "is this one of gnr8's own outputs?" — the shared principle the task asked for.
- `build_outputs` now resolves `config.inputs` against `project_root` (not the process cwd), so inputs and outputs share one anchor and the exclusion's relative-path comparison is meaningful. (An absolute input is left as-is by `Path::join`, so the existing fixture-pointing tests are unaffected.)

**Regression test added (the missing test):** `default_config_second_regenerate_is_a_noop` stages a self-contained copy of the fixture under a temp root, runs the **real** `regenerate` pipeline twice via the **shipped DEFAULT config**, and asserts the second run writes **0 files**, every output is `unchanged`, and `plan_only` (the `gnr8 check` seam) reports **no drift**. Verified to FAIL without the exclusion (writes `openapi.yaml` + `sdk/models.go`) and PASS with it. A pure `is_under_output` boundary-match unit test was also added.

**Live binary confirmation:** `init → generate → generate → check` on a fresh fixture copy:
- generate #1: `5 written`
- generate #2: `0 written, 5 unchanged`
- check: `outputs are up to date`, exit 0
- 1 `CommandMessage:` component entry (no `sdk.*` duplication).

**Hard constraint honored:** the raw fixture analysis path (`build_graph(fixtures/goalservice)`) is untouched — `snapshot_graph/diagnostics/openapi/sdk` stay GREEN and byte-identical (the exclusion is config-driven in `build_outputs`, which the snapshot tests do not call).

---

## Fixed Warnings

### WR-01: Deleted/renamed output watch event slips the loop filter (macOS)
**Commit:** `8ce99db`
**File:** `crates/gnr8/src/watch.rs`
**Fix:** A delete/rename event names a leaf that no longer exists, so `std::fs::canonicalize` fails on the full path and the old fallback-to-raw left a non-canonical `/var/.../sdk/client.go` that did not match the canonical `/private/.../sdk` in the output set — slipping the filter. `canonicalize_or_keep` now walks up to the nearest **existing** ancestor, canonicalizes it, and re-appends the missing tail, so a just-deleted output still resolves under the canonical output dir and stays filtered — covering delete/rename event kinds, not only create/modify. Added a hermetic test that writes then deletes a generated sdk file and asserts the canonicalized (now-missing) leaf is still under the output dir and is not a trigger.

### WR-02: Chained/colliding `naming.types` rename silently mis-generates
**Commit:** `fd2160c`
**Files:** `crates/gnr8-core/src/lifecycle/mod.rs`, `crates/gnr8-core/tests/lifecycle.rs`
**Fix:** `apply_naming` now resolves every `naming.types` key against the **original** graph in one pass, then — before mutating anything — rejects collapses (two renames → one target), collisions (target equals an un-renamed existing type's bare name), chains (target equals another rename's source, e.g. `A="B"` with `B="C"`), and ambiguous double-matches, returning a typed `CoreError::Config`. The uniqueness invariant is enforced over bare **names** because that is the OpenAPI component key (`lower` maps `schema.id → schema.name`). Signature is now `-> Result<(), CoreError>`; `build_outputs` and the two existing naming tests were updated. Added `naming_type_rename_collision_is_a_typed_error` covering collision, collapse, and chain. **Flagged for human verification** below: this changes generation behavior on conflicting config.

### WR-03: `regenerate_and_report` hard-codes the `single-file-edit` scenario
**Commit:** `fe39b62`
**File:** `crates/gnr8/src/watch.rs`
**Fix:** The debouncer callback now sends the count of **distinct** triggering source files through the channel; the receive loop sums the drained counts and derives the label via `scenario_for_trigger_count` — exactly one ⇒ `single-file-edit`, more ⇒ `multi-file-edit`. The WATCH-03 `--json` record no longer over-claims a single edit for a coalesced/multi-file batch. Pure `count_trigger_paths`/`scenario_for_trigger_count` helpers are unit-tested (single, multi, duplicate-path dedupe, degenerate 0). `LatencyReport` doc updated to the new scenario set.

### WR-04: `build_outputs` silently analyzes only the first of several inputs
**Commit:** `46caf2c`
**Files:** `crates/gnr8-core/src/lifecycle/mod.rs`, `crates/gnr8-core/tests/lifecycle.rs`
**Fix:** `build_outputs` now rejects `config.inputs.len() > 1` with a clear `CoreError::Config` (multi-input fan-in remains out of scope, D-02 / v2) instead of silently analyzing only the first while `watch::run` watches them all — keeping the analyzed set and the watched set in agreement. Added `multi_input_config_is_rejected_loudly` (toolchain-free; the check fires before any Go analysis).

---

## Fixed Info

### IN-03: Redundant `PathBuf::from` round-trip in `workspace::relative`
**Commit:** `e62eb07` — File: `crates/gnr8-core/src/workspace/mod.rs`
**Fix:** Use `Path::to_path_buf` in the `strip_prefix` success arm (both arms already hold a `&Path`) and drop the now-unused `PathBuf` import. Cosmetic, behavior unchanged.

### IN-04: `--debounce-ms 0` accepted with no floor
**Commit:** `e62eb07` — File: `crates/gnr8/src/main.rs`
**Fix:** `run_watch` floors the debounce window at a 10 ms minimum so `--debounce-ms 0` cannot create a zero-window debouncer that defeats burst-coalescing (and would amplify the WR-01 edge case).

---

## Deferred Info (deliberate PoC scope, with reason)

### IN-01: `safe_output_path` lexical-only traversal check (symlink escape)
**Deferred — documented PoC threat model.** Output paths come from the checked-in `config.toml` (the project owner's own file, not untrusted input). T-04-02-01 targets accidental `..` typos, not a malicious local config; the reviewer rated the practical risk low and consistent with the PoC threat model. Canonicalizing the resolved parent and asserting `starts_with(project_root.canonicalize())` is the documented hardening to add **if** output paths are ever sourced from less-trusted input.

### IN-02: Manifest `recorded_hash`/`record`/`prune_to` are O(n)/O(n·m) Vec scans
**Deferred — explicitly out of review scope (performance).** For the PoC's handful of generated files this is irrelevant, and the sorted-`Vec` choice is the right call for deterministic diffs (GRAPH-02). The reviewer flagged it only as a forward note ("None needed for v1; revisit only if output counts grow large"). No change made.

---

## Requirements Traceability

**Commit:** `f0164dc` — File: `.planning/REQUIREMENTS.md`
Marked **WATCH-02** and **WATCH-03** Complete (v1 checklist `[x]` + traceability table) — both are implemented and verified; the table was stale, not the code. WS-01..04 and WATCH-01 were already Complete.

---

## Verification Status (final)

| Gate | Result |
|------|--------|
| `make gates` (gnr8-core lib 82, gnr8 16, determinism 3, lifecycle 22, sdk_compile 3, snapshot graph/diagnostics/openapi/sdk 1 each) | **GREEN** |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | **GREEN** (exit 0) |
| `cargo fmt --all -- --check` | **GREEN** (exit 0) |
| `make goextract-build` (Go side) | **GREEN** |
| opt-in `watch_smoke -- --ignored` (one edit → one regen) | **GREEN** |
| Fixture snapshots byte-identical | **YES** (unchanged) |
| No prod `unwrap`/`expect`/`panic` | **HONORED** (all such calls are `#[cfg(test)]`) |
| Live `init → generate → generate → check` no-op | **0 written, 5 unchanged, check exit 0** |

**Flagged for human verification:** WR-02 (`fd2160c`) changes generation behavior — it now rejects conflicting `naming.types` config that previously silently mis-generated. The collision/collapse/chain detection is unit-tested, but please confirm the rejection semantics (and the bare-name uniqueness invariant) match intent before the phase proceeds.

---

_Fixed: 2026-06-24_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
