---
phase: 06-cross-language-hardening-examples-docs
plan: 01
subsystem: cli-cross-language-parity
tags: [doctor, watch, toolchain-detection, source-language, cli]
requires:
  - "gnr8-core analyze::detect_language (single deterministic classifier, Phase 02/04)"
  - "gnr8-core CoreError::{Go,Python,TypeScript}ToolchainMissing variants"
provides:
  - "gnr8_core::analyze::source_toolchain (pub) — the single CLI-facing source-language/toolchain identity"
  - "gnr8 doctor probes the DETECTED source toolchain (go/python3/node) reported with a language field"
  - "gnr8 watch triggers regen on the DETECTED source extension (.go/.py/.ts)"
affects:
  - "crates/gnr8/src/doctor.rs (LifecycleHealth field set + pinned --json contract)"
  - "crates/gnr8/src/main.rs (run_doctor probe)"
  - "crates/gnr8/src/watch.rs (is_trigger_path extension)"
tech-stack:
  added: []
  patterns:
    - "Single detect decision mapped onto a discrete-args subprocess probe (rule 3, no fallback)"
    - "Pinned --json field-set contract updated in lockstep with the field rename (T-06-03)"
key-files:
  created: []
  modified:
    - crates/gnr8-core/src/analyze/mod.rs
    - crates/gnr8/src/main.rs
    - crates/gnr8/src/doctor.rs
    - crates/gnr8/src/watch.rs
decisions:
  - "New pub SourceToolchain enum + source_toolchain fn delegate to the existing pub(crate) detect_language — one source of truth, never a second classifier (rule 3); Lang/detect_language stay pub(crate)."
  - "LifecycleHealth.go_toolchain renamed to source_toolchain AND a language field added so the report is honest about WHICH toolchain was probed (RESEARCH A6)."
  - "Source dir resolved as the project root with .gnr8/ excluded inside scan_markers (Open Q2 / Pitfall 2) so a vendored other-language file under .gnr8/target cannot spoof detection."
  - "go probes with the bare `version` subcommand; python3/node with `--version` — discrete literal args, never sh -c (T-06-01)."
metrics:
  duration: "~7min (3 atomic feat commits)"
  tasks: 3
  files: 4
  completed: "2026-06-26"
---

# Phase 6 Plan 1: CLI Cross-Language Parity (doctor/watch) Summary

Generalized `gnr8 doctor` and `gnr8 watch` off the Go-only assumption: both now follow the SOURCE
language's toolchain and file extension via a single new `pub gnr8_core::analyze::source_toolchain`
decision that delegates to the existing `detect_language` classifier (XLANG-04) — no fallback, no new dep.

## What Was Built

**Task 1 — pub source-toolchain API (gnr8-core):** Added a `pub enum SourceToolchain { Go, Python,
TypeScript }` with `probe_binary()` (`go`/`python3`/`node`), `source_extension()` (`go`/`py`/`ts`), and
`language()` (`go`/`python`/`typescript`) accessors, plus `pub fn source_toolchain(dir) -> Result<SourceToolchain, CoreError>`
that is a PURE MAPPING over the existing `detect_language` decision. `Lang`/`detect_language` stay
`pub(crate)` — the new fn is the only public surface, so there is exactly one classifier and no second
source of truth (rule 3). The classifier's typed `CoreError::Config` ambiguity/none errors propagate
unchanged. `scan_markers` was extended to skip a nested `.gnr8/` directory (Open Q2 / Pitfall 2).
Unit tests cover single-language fixtures → the right arm with the right accessors, and mixed/empty
trees → `CoreError::Config` (no panic, no guess).

**Task 2 — generalized doctor probe + LifecycleHealth (gnr8):** Replaced the hardcoded `probe_go` with
`probe_source_lang_toolchain`, which calls `source_toolchain` ONCE over the project root, then spawns
`Command::new(tc.probe_binary())` with the discrete version arg (`version` for go, `--version` for
python3/node) — preserving the no-`sh -c` shape (T-06-01). On a `CoreError::Config` (ambiguous/none) it
reports `"unknown"` + a false health bool rather than crashing (Pitfall 4). `LifecycleHealth.go_toolchain`
was renamed to `source_toolchain` and a `language` field added; every coupled site was updated in the
same task — `assemble`, the human renderer label, the `actionable_problem_count`/`has_actionable_problem`
references, the pinned `doctor_json_field_set` test (now `{initialized, source_toolchain, language,
pipeline_runs}` with the updated drift message, T-06-03), and the second test (renamed
`missing_source_toolchain_is_actionable`). A new test asserts a Python/TS source yields the matching
`language` + probe binary via the core decision.

**Task 3 — generalized watch trigger (gnr8):** Threaded a `source_ext: &str` parameter through
`is_trigger_path`, `batch_should_regenerate`, and `count_trigger_paths`, derived ONCE from
`source_toolchain(project_root).source_extension()` at the watch entry point. The pure function now
compares against the passed extension instead of the hardcoded `ext == "go"` — no per-extension match
inside it (no second source of truth). The `.gnr8/src/**.rs` pipeline-edit trigger and the output-set
loop-safety (WATCH-02) are untouched and remain language-agnostic. Added pure `py_source_triggers` /
`ts_source_triggers` tests and a non-source-language-ignored case.

`check` (drift) was verified already language-agnostic — not rewritten.

## Tasks Completed

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | Expose pub source_toolchain API delegating to detect_language | a789042 | crates/gnr8-core/src/analyze/mod.rs |
| 2 | Generalize doctor probe to the detected source toolchain | 6a2aacc | crates/gnr8/src/main.rs, crates/gnr8/src/doctor.rs |
| 3 | Generalize watch trigger to the detected source extension | b20d8ea | crates/gnr8/src/watch.rs |

## Verification

- `cargo test -p gnr8-core --lib` — 220 passed, 0 failed.
- `cargo test -p gnr8` — all passed (incl. the updated `doctor_json_field_set`, `missing_source_toolchain_is_actionable`, `py_source_triggers`, `ts_source_triggers`).
- `make check` — exit 0 (fmt, clippy `-D warnings`, all Rust crates, tssdk hermetic typecheck, goextract build/vet/test).
- `! grep -rn 'go_toolchain' crates/gnr8/src/` — clean (rename complete).
- Single-decision invariant: exactly one `source_toolchain(` call in `main.rs` and one in `watch.rs`; no surviving `probe_go` / hardcoded `ext == "go"` outside test fixtures.
- Discrete-args preserved: no `sh -c` / `"sh"` in `main.rs`.
- Zero new OSS deps: `git diff --exit-code crates/gnr8/Cargo.toml crates/gnr8-core/Cargo.toml` clean.

## Deviations from Plan

None — plan executed exactly as written. The three tasks map 1:1 to the three feat commits; all
acceptance criteria and the threat-register mitigations (T-06-01 discrete args, T-06-02 single decision,
T-06-03 pinned-contract update in lockstep, T-06-SC zero installs) were satisfied.

## Authentication Gates

None.

## Known Stubs

None — `doctor`/`watch`/`check` are fully wired to the live `source_toolchain` decision.

## Self-Check: PASSED

- crates/gnr8-core/src/analyze/mod.rs — FOUND (pub fn source_toolchain present)
- crates/gnr8/src/main.rs — FOUND (probe_source_lang_toolchain present)
- crates/gnr8/src/doctor.rs — FOUND (source_toolchain + language fields present)
- crates/gnr8/src/watch.rs — FOUND (source_ext threaded; py/ts trigger tests present)
- Commit a789042 — FOUND
- Commit 6a2aacc — FOUND
- Commit b20d8ea — FOUND
