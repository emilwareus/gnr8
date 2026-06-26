---
phase: 03-python-target-pysdk
plan: 02
subsystem: sdk
tags: [config-as-code, target, python, sdk, determinism]
requires:
  - "crate::pysdk::generate / pysdk::split_bundle (the Wave-1 IR->Python bundle + framing split)"
  - "sdk::builtins::sdk_package (the single-source package-name derivation GoSdk uses)"
  - "crate::graph::ApiGraph.base_path (the URL prefix SetBasePath sets / OpenAPI lowering reads)"
provides:
  - "sdk::builtins::PySdk — a Target built-in configurable via PySdk::new().module(..).to(..)"
  - "sdk::prelude::PySdk — PySdk in the `use gnr8_core::sdk::prelude::*;` composition surface"
affects:
  - "crates/gnr8-core/src/pysdk/mod.rs (split_bundle now consumed — #[allow(dead_code)] removed)"
tech-stack:
  added: []
  patterns:
    - "Target built-in as the config-as-code enablement surface (rule 4) — a .gnr8/ Pipeline composes PySdk"
    - "single source of truth: sdk_package(&module) + ir.base_path, no re-derivation / no fallback (rules 3/4)"
    - "output_anchors() returns the output dir for loop-safety (generated *.py excluded from re-analysis)"
    - "identical path-traversal name guard reused at the Target write seam (no second guard)"
key-files:
  created: []
  modified:
    - crates/gnr8-core/src/sdk/builtins.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/src/pysdk/mod.rs
decisions:
  - "PySdk is a verbatim structural clone of GoSdk (struct + builder + impl Target) — only the generator (crate::pysdk::generate/split_bundle) and the proper noun differ; no behavioural divergence."
  - "No new CoreError variant: empty module/dir reuse Config; unsafe frame name reuses SdkGen (parity with GoSdk)."
  - "fn sdk_package stays singular (one definition) — PySdk reuses it; the second `sdk_package` grep hit is the pre-existing test name, not a derivation."
metrics:
  duration: ~10m
  completed: 2026-06-25
  tasks: 2
  files: 3
  unit_tests_added: 2
---

# Phase 3 Plan 02: PySdk Target built-in Summary

Added the `PySdk` `Target` — the config-as-code enablement surface (rule 4) that lets a `.gnr8/`
Pipeline compose `PySdk::new().module("example.com/bookstore/sdk").to("generated/sdk-py")` to drive the
Wave-1 `pysdk::generate` and write the Python SDK files under the output dir. A verbatim structural
clone of `GoSdk` reusing the single `sdk_package` name derivation and `ir.base_path` (one source of
truth, rules 3/4), the identical unsafe-name guard, and `output_anchors` loop-safety.

## What was built

- **`sdk/builtins.rs` — `PySdk` Target** (cloned from `GoSdk`, placed immediately after the `GoSdk`
  block): `pub struct PySdk { module, dir }` + `new()`/`module(..)`/`to(..)` builder + `Default` +
  `impl Target`. `generate` guards empty `module`/`dir` → `CoreError::Config`; derives the package via
  the existing `sdk_package(&self.module)` (no second derivation, rule 3); calls
  `crate::pysdk::generate(ir, &package, &ir.base_path)` (the SAME `ir.base_path` source of truth GoSdk
  uses, never re-derived); loops `crate::pysdk::split_bundle(&bundle)` applying the identical
  `is_empty()/'/'/'\\'/".."` name guard → `CoreError::SdkGen` before `out.write("<dir>/<name>", …)`.
  `output_anchors()` returns the trimmed output dir (loop-safety) or `[]` when unconfigured. No new
  `CoreError` variant added.
- **`sdk/builtins.rs` — tests:** `pysdk_target_writes_under_the_output_dir_and_is_deterministic`
  (configured run emits ≥1 Artifact, every path under the trimmed dir, `output_anchors` correct, two
  fresh `Artifacts` byte-identical); plus `PySdk` added to the test `use super::{…}` import list and two
  new arms in `targets_error_when_unconfigured` (no-module + no-dir → `Config`).
- **`sdk/mod.rs` — prelude:** `PySdk` added to the `pub use super::builtins::{…}` re-export (alpha-near
  `OpenApi31`), so `use gnr8_core::sdk::prelude::*;` brings it into scope.
- **`pysdk/mod.rs`:** removed the scoped `#[allow(dead_code)]` on `split_bundle` (now consumed by the
  `PySdk` target) and updated its doc note to point at `sdk::builtins`.

## Verification results

- `cargo test -p gnr8-core` — 163 lib tests + all integration suites green (lib up from 162: one new
  determinism test; the unconfigured-error test gained two PySdk arms).
- `cargo test -p gnr8-core --lib pysdk_target_writes_…` and `…targets_error_when_unconfigured` — pass.
- `cargo clippy -p gnr8-core --all-targets -- -D warnings` — clean, exit 0 (the removed
  `#[allow(dead_code)]` does not trip the dead-code lint because `split_bundle` is now reachable from
  `builtins.rs`).
- `git diff HEAD~2 -- Cargo.toml crates/gnr8-core/Cargo.toml Cargo.lock` — empty (zero new Rust crates,
  rule 2).
- Acceptance greps: `pub struct PySdk` ×1; `crate::pysdk::generate` ×2; `ir.base_path` ×8 (≥2);
  `fn sdk_package` definition singular (line 650; the second hit is the test name
  `sdk_package_derives_last_segment`); `unsafe name` ×2 (GoSdk + PySdk); prelude `PySdk` present.
  Production `unwrap/expect/panic`: none — every hit is inside `#[cfg(test)]`.

## Deviations from Plan

None — plan executed exactly as written. (The Task-1 acceptance criterion `grep -c 'fn sdk_package' == 1`
counts a substring; the file has exactly one `fn sdk_package` *definition* (line 650) — the second hit is
the pre-existing test function name `sdk_package_derives_last_segment`. The intent, "no second
derivation," is satisfied: PySdk reuses the single helper.)

## Threat surface

All plan `<threat_model>` mitigations applied: PySdk reuses the identical GoSdk path-traversal frame-name
guard (T-03-02-01); `output_anchors()` returns the output dir so the generated `*.py` are excluded from
re-analysis (T-03-02-02); the package name comes only from the single `sdk_package` derivation and the
URL prefix only from `ir.base_path` — no fallback, no re-derivation (T-03-02-03); empty module/dir →
typed `CoreError::Config` with no production `unwrap/expect/panic` (T-03-02-04); the determinism test
asserts two runs are byte-identical (T-03-02-05); zero packages added (T-03-02-SC).

No new threat surface beyond the plan's `<threat_model>`.

## Known Stubs

None. `PySdk` is fully wired to `pysdk::generate`/`split_bundle` and writes real Artifacts; the Wave-1
`split_bundle` is now consumed (its dead-code allow removed).

## Self-Check: PASSED

- Files: `crates/gnr8-core/src/sdk/builtins.rs`, `crates/gnr8-core/src/sdk/mod.rs`,
  `crates/gnr8-core/src/pysdk/mod.rs` all present and modified.
- Commits: `b2c31e9` (Task 1) and `8e35163` (Task 2) both FOUND in git log.
