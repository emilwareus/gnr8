---
phase: 04-gnr8-lifecycle-and-watch-mode
verified: 2026-06-24T23:45:00Z
status: passed
score: 3/4 success criteria fully verified (criterion 3 partial); 7/7 requirements implemented
overrides_applied: 0
gaps:
  - truth: "No-op generation avoids rewriting unchanged files (criterion 3 / WATCH-01) — for the DEFAULT-config goalservice workflow"
    status: partial
    reason: >
      The no-op MECHANISM is correct and proven (named tests noop_second_run_writes_nothing /
      noop_preserves_mtime pass; with a non-overlapping config a second `generate` reports
      "0 written, 5 unchanged" and `check` exits 0). BUT the SHIPPED default config
      (`inputs = ["."]`, `sdk_dir = "sdk"`) writes the generated SDK INSIDE the analyzed input tree.
      On the second `gnr8 generate`, `build_graph(".")` re-discovers the just-written `sdk/*.go`
      files as Go source, DOUBLING the schema set (9 → 18, every type duplicated) and producing
      a different, larger, duplicated output (openapi.yaml 6208 → 8666 bytes, models.go 2242 → 4449
      bytes). Reproduced deterministically across 3 trials. Result: a user following the exact
      default `init` → `generate` → `generate` flow on the goalservice fixture sees spurious
      rewrites and corrupted/growing output, not a no-op. `regenerate` has no mechanism to exclude
      the output dir from analysis, and watch mode reuses the same `regenerate`, so a real source
      edit under the default config would also re-analyze gnr8's own SDK output.
    artifacts:
      - path: "crates/gnr8-core/src/lifecycle/mod.rs"
        issue: "build_outputs() calls analyze::build_graph(config.inputs[0]) with no exclusion of config.output paths — the generated SDK under sdk/ is re-analyzed when inputs overlap outputs"
      - path: "crates/gnr8-core/src/workspace/mod.rs"
        issue: "DEFAULT_CONFIG_TOML ships inputs=[\".\"] + sdk_dir=\"sdk\", so outputs land inside the analyzed input by construction — the default workflow self-contaminates"
    missing:
      - "Exclude the configured output paths (openapi file + sdk_dir) from the source set build_graph analyzes (e.g. skip sdk_dir / manifest-tracked files during discovery), OR ship a default config whose sdk_dir/openapi live outside the analyzed input tree (e.g. inputs=[\"internal\"] or a sibling generated/ dir)"
      - "Add a deterministic regression test asserting that two consecutive regenerate() calls on the goalservice fixture WITH THE DEFAULT CONFIG produce byte-identical output (0 written on the second run) — the current noop tests use non-overlapping layouts and miss this"
deferred: []
---

# Phase 4: `.gnr8` Lifecycle And Watch Mode — Verification Report

**Phase Goal:** Prove the code-as-config user workflow and fast regeneration loop.
**Verified:** 2026-06-24T23:45:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `gnr8 init` creates a `.gnr8/` workspace with editable code customization | ✓ VERIFIED | Live e2e on fixture: `gnr8 init` created `.gnr8/{config.toml,.gitignore,cache/}`; `.gnr8/.gitignore` contains `/cache/` and keeps config.toml checked in; re-run over a USER-EDITED config.toml reported "nothing to do — already present", config.toml byte-identical (shasum match), edit marker preserved. 6 named tests green (`init_scaffolds_workspace`, `init_is_idempotent`, `gitignore_splits_lifecycle`, `config_parses_default_body`, `naming_overrides_parse`, `config_rejects_unknown_key`). |
| 2 | Generated outputs are tracked well enough to avoid silent user-file clobbering | ✓ VERIFIED | Live e2e: hand-edited a generated `sdk/client.go` → next `generate` (no --force) printed `warning: ... was hand-edited ... skipped`, count "0 written, 4 unchanged, 1 skipped", edit preserved (shasum match). `gnr8 check` exited 1 (drifted). `gnr8 generate --force` overwrote the edit. Named tests `user_edit_is_protected`, `force_overwrites_user_edit`, `untracked_output_protected` green; `plan_writes_truth_table` covers all 5 arms. blake3 manifest at `.gnr8/cache/manifest.json` records path→hash→source. |
| 3 | No-op generation avoids rewriting unchanged files | ✗ FAILED (partial) | MECHANISM verified: `noop_second_run_writes_nothing` + `noop_preserves_mtime` green; with a non-overlapping config a 2nd `generate` = "0 written, 5 unchanged" and `check` exit 0 (live). BUT the DEFAULT config (`inputs=["."]`, `sdk_dir="sdk"`) writes the SDK inside the analyzed input → 2nd `generate` re-analyzes its own `sdk/*.go`, doubles schemas 9→18, rewrites openapi.yaml + models.go with duplicated/larger content (reproduced 3/3 trials). See Gaps. |
| 4 | Watch mode updates outputs after supported Go source edits and reports latency | ✓ VERIFIED | Pure loop-safety filter tests green (`output_paths_filtered`, `source_wins_over_output`, `go_source_triggers`, `non_go_ignored`). `LatencyReport{scenario,millis,written,unchanged}` shape asserted by `latency_report_json_field_set`; scenarios cold/warm-noop/single-file-edit present in code. `gnr8 watch --help` shows `--debounce-ms` (default 200) + `--json`. Live opt-in smoke `single_edit_one_regen` PASSED: one `.go` edit → exactly one regen signal, output writes produced NONE (no self-loop). Ctrl-C via `ctrlc` (accepted deviation). |

**Score:** 3/4 success criteria fully verified; criterion 3 is partial (mechanism correct, default-config workflow violates it).

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/gnr8-core/src/workspace/mod.rs` | Idempotent `.gnr8/` scaffold + InitOutcome + .gitignore const | ✓ VERIFIED | 142 lines. `init` uses `OpenOptions::create_new(true)` write-if-absent; GITIGNORE_BODY + DEFAULT_CONFIG_TOML consts present; no prod unwrap/expect/panic. Wired from `main.rs::run_init`. |
| `crates/gnr8-core/src/config/mod.rs` | Typed Config (serde+toml) + deny_unknown_fields + load | ✓ VERIFIED | 94 lines. Config/OutputConfig/NamingOverrides with `#[serde(deny_unknown_fields)]`; HONEST scope — no faked plugin field; module docstring frames TOML as PoC stand-in + ADV-02 v2 deferral. `parse`/`load` wired. |
| `crates/gnr8-core/src/manifest/mod.rs` | Manifest (path→blake3+provenance) load/save/record/prune | ✓ VERIFIED | 210 lines. blake3_hex stable; absent/corrupt → empty default (no panic); record keeps sorted; prune_to drops dropped paths; save writes version 1. |
| `crates/gnr8-core/src/lifecycle/mod.rs` | pure plan_writes truth table + apply_writes + regenerate + naming | ✓ VERIFIED | 483 lines. plan_writes pure (5 arms, injected on_disk closure); apply_writes honors force + path-traversal guard; regenerate runs real Phase-3 pipeline; plan_only dry-run seam; apply_naming rewrites $refs. |
| `crates/gnr8/src/watch.rs` | debouncer shell + pure event filter + latency | ✓ VERIFIED | 436 lines. Pure is_trigger_path/batch_should_regenerate; notify-debouncer-full shell watches source dirs only; canonicalize fix for macOS /private; LatencyReport; ctrlc AtomicBool shutdown; no prod unwrap/expect/panic. |
| `crates/gnr8-core/tests/lifecycle.rs` | init/config/manifest/no-op/naming tests | ✓ VERIFIED | 711 lines, 19 tests, ALL GREEN (run live). |
| `crates/gnr8/tests/watch_smoke.rs` | one timing-tolerant smoke | ✓ VERIFIED | 146 lines. `single_edit_one_regen` `#[ignore]`d; PASSES when run opt-in. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `main.rs` | `workspace::init` | `Commands::Init → run_init` | ✓ WIRED | `run_init` calls `gnr8_core::workspace::init(&cwd)`; live `gnr8 init` works. |
| `config/mod.rs` | `toml::from_str` | Config deserialization | ✓ WIRED | `parse` wraps `toml::from_str`; `config_parses_default_body` green. |
| `lifecycle/mod.rs` | `lower::to_openapi` + `sdk::generate` | regenerate runs Phase-3 pipeline | ✓ WIRED | `build_outputs` calls both; live `generate` produced openapi.yaml + 4 SDK files. |
| `main.rs` | `lifecycle::regenerate` / `plan_only` | Generate / Check arms | ✓ WIRED | `run_generate`→regenerate, `run_check`→plan_only (exit 1 on drift, verified live). |
| `watch.rs` | `lifecycle::regenerate` | debounced signal triggers timed regenerate | ✓ WIRED | `regenerate_and_report` + `cold_regenerate` time `lifecycle::regenerate`. |
| `watch.rs` | `notify_debouncer_full::new_debouncer` | watch source dirs, Recursive | ✓ WIRED | `run` builds debouncer, `debouncer.watch(dir, RecursiveMode::Recursive)`. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `gnr8 generate` outputs | openapi.yaml + sdk/*.go | `analyze::build_graph` → `to_openapi`/`sdk::generate` (real Go toolchain) | Yes — live run wrote 5 real files; manifest recorded real blake3 hashes | ✓ FLOWING |
| `gnr8 watch` LatencyReport | scenario/millis/written/unchanged | `lifecycle::regenerate` GenerateOutcome timed by `Instant` | Yes — real counts from real regen | ✓ FLOWING |
| no-op (default config) | re-analyzed graph schemas | `build_graph(".")` re-reads generated sdk/*.go | Yes but CONTAMINATED — output paths re-analyzed as input | ⚠️ HOLLOW (data flows but self-pollutes; see Gaps criterion 3) |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| init scaffolds + idempotent | `gnr8 init` ×2 with user edit | created then "nothing to do"; config preserved | ✓ PASS |
| watch help flags | `gnr8 watch --help` | shows `--debounce-ms` (default 200) + `--json` | ✓ PASS |
| generate writes real outputs | `gnr8 generate` | "5 written"; openapi.yaml + 4 sdk files + manifest | ✓ PASS |
| no-clobber | edit generated file → `gnr8 generate` | warn + skip, edit preserved, "1 skipped" | ✓ PASS |
| check on drift | `gnr8 check` (drifted tree) | "drifted: ...", exit 1 | ✓ PASS |
| force overwrite | `gnr8 generate --force` | edit overwritten, "1 written" | ✓ PASS |
| no-op (non-overlap config) | `gnr8 generate` ×3 | "0 written, 5 unchanged"; check exit 0 | ✓ PASS |
| no-op (DEFAULT config) | `gnr8 generate` ×2 on fixture | "2 written" on 2nd run; schemas 9→18 duplicated | ✗ FAIL (criterion 3 gap) |

### Probe Execution

No conventional `scripts/*/tests/probe-*.sh` and no probe declarations in PLAN/SUMMARY. Phase verification is gate/test-driven (`make gates`), executed below.

| Probe | Command | Result | Status |
|-------|---------|--------|--------|
| make gates (blocking set) | `make gates` | exit 0 — gnr8-core lib + gnr8 (14, incl. watch filter) + 4 contract snapshots + determinism + sdk_compile + lifecycle (19) all green | ✓ PASS |
| lifecycle suite | `cargo test -p gnr8-core --test lifecycle` | 19 passed, 0 failed | ✓ PASS |
| gnr8 (watch filter + CLI) | `cargo test -p gnr8` | 14 passed; watch_smoke 1 ignored | ✓ PASS |
| error display | `cargo test -p gnr8-core --lib error::tests::display` | 11 passed (incl. Workspace/Config/Manifest/Io) | ✓ PASS |
| clippy -D warnings | `cargo clippy --all-targets --locked -- -D warnings` | exit 0 | ✓ PASS |
| fmt check | `cargo fmt --all -- --check` | exit 0 | ✓ PASS |
| live watch smoke (opt-in) | `cargo test -p gnr8 --test watch_smoke -- --ignored` | `single_edit_one_regen` passed | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| WS-01 | 04-01 | `gnr8 init` scaffolds project-local `.gnr8/` | ✓ SATISFIED | Live init + 3 init tests |
| WS-02 | 04-01 | `.gnr8/` separates checked-in vs ignored | ✓ SATISFIED | `.gitignore` `/cache/`, config checked in; `gitignore_splits_lifecycle` |
| WS-03 | 04-01/04-02 | Customize inputs/output/naming through code (honest PoC scope) | ✓ SATISFIED | Typed Config knobs, deny_unknown_fields, naming overrides remap ids+$refs (`naming_overrides_apply`, `naming_type_rename_updates_refs_no_dangling`); no faked plugin field; v2/ADV-02 documented |
| WS-04 | 04-02 | Generated-file ownership tracked, no silent clobber | ✓ SATISFIED | blake3 manifest + plan_writes truth table + live no-clobber/force/check-drift |
| WATCH-01 | 04-02/04-03 | No-op avoids rewriting unchanged outputs | ⚠️ PARTIAL | Mechanism + tests green; default-config goalservice workflow self-contaminates (see gap) |
| WATCH-02 | 04-03 | Watch reacts, debounces, avoids loops | ✓ SATISFIED | Pure filter tests + live smoke (one edit→one regen, output writes→no loop); debouncer watches source dirs only |
| WATCH-03 | 04-03 | Reports cold/warm-noop/single-file-edit latency | ✓ SATISFIED | LatencyReport with all 3 scenarios; human + `--json`; field-set test |

**Note:** REQUIREMENTS.md still lists WATCH-02/WATCH-03 as "Pending" in its traceability table (lines 168-169) — the implementation is present and verified, so the table is stale, not the code. Not a blocker; flag for the table to be updated to Complete.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | — | No prod unwrap/expect/panic in any Phase-4 production file (workspace/config/manifest/lifecycle/watch) — all such calls are inside `#[cfg(test)]` modules with scoped allows | ℹ️ Info | RUST-04 honored; clippy -D warnings green |
| `crates/gnr8-core/src/workspace/mod.rs` | 45 | DEFAULT_CONFIG_TOML ships `inputs=["."]` + `sdk_dir="sdk"` (output inside input) | ⚠️ Warning | Root cause of the criterion-3 no-op gap (see Gaps) |

No TBD/FIXME/XXX debt markers found in Phase-4-modified files. The "PLACEHOLDER"-style language present (e.g. `not_yet`) is pre-existing Phase-1 scaffolding for unimplemented commands (doctor), not Phase-4 stubs.

### Gaps Summary

**One WARNING-level gap blocks a clean pass.** The phase's headline machinery is real and well-tested — init/idempotency, ownership/no-clobber (with `--force`), `check`-on-drift, the pure loop-safe watch filter, and latency reporting all verify end-to-end against the live binary and fixture, and `make gates` is green. The deviation noted (`ctrlc` crate vs std SIGINT) is the accepted, documented one.

The gap is criterion 3 (no-op / WATCH-01) under the **shipped default configuration**: because the default `config.toml` writes the SDK into `sdk/` *inside* the analyzed input `.`, a second `gnr8 generate` on the goalservice fixture re-analyzes gnr8's own generated `sdk/*.go` files, duplicating every schema (9 → 18) and producing larger, duplicated output — so the "no-op = no write" guarantee does NOT hold for the exact default `init → generate → generate` workflow this phase was asked to prove. The no-op *mechanism* is correct (proven with non-overlapping config and by the unit tests), so this is an isolation/default-config defect, not a broken algorithm. It is not covered by any later phase's success criteria (Phase 5 is hardening/demo/benchmarks), so it is not deferrable. Fix options: exclude configured output paths from the analyzed source set, or ship a default config whose outputs live outside the analyzed input tree — plus a regression test that two consecutive default-config regenerations are byte-identical.

---

_Verified: 2026-06-24T23:45:00Z_
_Verifier: Claude (gsd-verifier)_

---

## Gap Closure (post-verification, 2026-06-24)

The single criterion-3 (WATCH-01 no-op) gap was **closed** in commit `535455a` and verified:
`build_outputs` now excludes operations/schemas whose provenance falls under any configured output
anchor (sdk dir, openapi path, `.gnr8/`) — the graph-side twin of the watch loop-safety filter — so
gnr8 never re-ingests its own generated files. Regression test `default_config_second_regenerate_is_a_noop`
added (fails without the fix). Live e2e on the fixture: `init → generate (5 written) → generate (0 written,
5 unchanged) → check (exit 0)`, no schema duplication. All 4 review warnings also fixed (WR-01 deleted/renamed
output filtering, WR-02 colliding-rename rejection, WR-03 scenario label, WR-04 multi-input rejection).
**Status updated gaps_found → passed.** (WR-02 changes behavior only on genuinely conflicting `naming.types`
config — an error case — accepted in autonomous mode as a correctness improvement.)
