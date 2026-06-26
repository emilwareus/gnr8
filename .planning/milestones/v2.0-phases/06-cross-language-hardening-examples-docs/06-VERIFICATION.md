---
phase: 06-cross-language-hardening-examples-docs
verified: 2026-06-26T06:28:21Z
status: passed
score: 5/5 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
re_verification:
  previous_status: none
  note: initial verification
---

# Phase 6: Cross-Language Hardening + Examples + Docs Verification Report

**Phase Goal:** A developer can drive complete FastAPI and NestJS projects end-to-end from a `.gnr8/` lifecycle with real committed output, trust an honest per-language supported-envelope, and rely on doctor/check/watch parity and cross-language determinism — with the `typescript` toolchain dependency recorded.
**Verified:** 2026-06-26T06:28:21Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

Truths are the 5 phase requirement IDs (XLANG-01..05), each cross-referenced against ROADMAP success criteria and the merged PLAN must_haves.

| #   | Truth (requirement) | Status     | Evidence       |
| --- | ------------------- | ---------- | -------------- |
| 1   | XLANG-01: FastAPI end-to-end → OpenAPI 3.1 + Python SDK from a `.gnr8/` lifecycle with real committed output | ✓ VERIFIED | `examples/fastapi-bookstore/` git-tracked: `.gnr8/{Cargo.toml,Cargo.lock,.gitignore,src/main.rs}` + `generated/openapi.yaml` (`openapi: 3.1.0`) + `generated/sdk/{__init__,client,errors,models}.py`. main.rs composes `FastApi::new()` + `PySdk::new()` (no GoSdk/TsSdk). `make examples-check` ran `gnr8 generate && gnr8 check` in the dir → `0 written, 5 unchanged` (byte-identical). |
| 2   | XLANG-02: NestJS end-to-end → OpenAPI 3.1 + TS SDK from a `.gnr8/` lifecycle with real committed output, no node_modules | ✓ VERIFIED | `examples/nestjs-bookstore/` git-tracked: `.gnr8/` crate + `generated/openapi.yaml` (`openapi: 3.1.0`) + `generated/sdk/{client,errors,index,models}.ts`. main.rs composes `NestJs::new()` + `TsSdk::new()` (no GoSdk/PySdk). `git ls-files examples/nestjs-bookstore/node_modules` = empty; README states "No `node_modules` needed". `gnr8 check` in dir → `0 written, 5 unchanged`. |
| 3   | XLANG-03: docs/USAGE.md documents honest per-language envelope (FastAPI full; Flask typed-only with gaps; NestJS class DTOs) with limits | ✓ VERIFIED | `## Supported source frontends (the honest envelope)` table present; FastAPI/Flask/NestJS named (6 hits); Flask row states "untyped `request.json` / unannotated `request.args` / missing return annotation → diagnostic, NEVER inferred"; NestJS class-DTO scope + bright line; Source/Target tables add FastApi/Flask/NestJs + PySdk/TsSdk; "dependency-free in every language" line; both new examples referenced; Go/Gin content preserved. |
| 4   | XLANG-04: doctor/check/watch toolchain detection, drift, loop-safety across language sidecars via single decision (no fallback) | ✓ VERIFIED | `gnr8_core::analyze::source_toolchain` (pub) delegates to single `detect_language` (one `scan_markers`, no second classifier). `probe_source_lang_toolchain` calls it ONCE; TS→`typescript_toolchain_present` (node+typescript probe, shared resolution), Go/Python→discrete binary with success-exit. `go_toolchain` fully removed from CLI. watch threads `source_ext` + `gnr8_root` exclusion; `.gnr8/src/**.rs` trigger + output-set loop-safety intact. Tests green: `doctor_json_field_set`, `py/ts_source_triggers`, `dot_gnr8_source_language_file_does_not_trigger`. |
| 5   | XLANG-05: sidecars stdlib-only per language (Python ast; TS = user's typescript, gnr8 ships none); gnr8-core zero new OSS; deterministic byte-identical; typescript toolchain recorded in CLAUDE.md + PROJECT.md | ✓ VERIFIED | gnr8-core deps unchanged (thiserror/serde/serde_json/blake3 — the 4 v1 debt crates, none added, none retired); NO phase-06 commit touched Cargo.toml/Cargo.lock. tsextract un-vendored (`git ls-files tsextract/node_modules` empty; `refactor: stop vendoring 23MB` commit). `probe.js`/`ts.js` node-builtins only, single `resolveTypescript`. CLAUDE.md `### TypeScript toolchain (required, not shipped)` + bright line (@nestjs/swagger/zod/class-validator excluded); PROJECT.md l.119 agrees ("required user toolchain, gnr8 ships none"). `make examples-check`: all 3 languages `0 written, 5 unchanged`. |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected    | Status | Details |
| -------- | ----------- | ------ | ------- |
| `crates/gnr8-core/src/analyze/mod.rs` | pub source_toolchain delegating to detect_language | ✓ VERIFIED | `pub enum SourceToolchain` (l.100), `pub fn source_toolchain` (l.158), `pub fn typescript_toolchain_present` (l.176); body delegates `match detect_language(dir)?` (l.159); 1 `scan_markers`; accessors `probe_binary`/`source_extension`/`language`. |
| `crates/gnr8/src/main.rs` | run_doctor probing source toolchain via core API | ✓ VERIFIED | `probe_source_lang_toolchain` (l.223) calls `source_toolchain` once (l.224), TS→node+typescript probe, Go/Python→discrete success-exit (WR-05). No production unwrap/expect/panic. |
| `crates/gnr8/src/doctor.rs` | LifecycleHealth with source_toolchain + language + pinned --json test | ✓ VERIFIED | `source_toolchain: bool` (l.46), `language: String` (l.50); pinned field-set asserts `{initialized, source_toolchain, language, pipeline_runs}` with drift message (l.580); `missing_source_toolchain_is_actionable` test passes. |
| `crates/gnr8/src/watch.rs` | is_trigger_path generalized to source_ext + .gnr8/ exclusion | ✓ VERIFIED | `source_ext: &str` + `gnr8_root` params; `path.extension() == source_ext` (l.126); `.gnr8/src/**.rs` trigger (l.112), output-set loop-safety (l.116), `gnr8_root` exclusion (l.122) all present. |
| `examples/fastapi-bookstore/.gnr8/src/main.rs` | FastApi→OpenApi31+PySdk Pipeline (config is code) | ✓ VERIFIED | FastApi::new() + PySdk::new() present; no GoSdk/TsSdk. |
| `examples/fastapi-bookstore/generated/openapi.yaml` | REAL committed OpenAPI 3.1 | ✓ VERIFIED | `openapi: 3.1.0`; regenerates byte-identical (examples-check). |
| `examples/nestjs-bookstore/.gnr8/src/main.rs` | NestJs→OpenApi31+TsSdk Pipeline | ✓ VERIFIED | NestJs::new() + TsSdk::new() present; no GoSdk/PySdk. |
| `examples/nestjs-bookstore/generated/openapi.yaml` | REAL committed OpenAPI 3.1, no node_modules | ✓ VERIFIED | `openapi: 3.1.0`; no node_modules tracked; byte-identical regen. |
| `Makefile` | examples-check regen-and-diff wired into check | ✓ VERIFIED | examples-check covers all 3 examples; prerequisite of `check:`; `make check` exit 0. |
| `docs/USAGE.md` | per-language honest envelope | ✓ VERIFIED | See truth #3. |
| `CLAUDE.md` | typescript toolchain recorded, rule 2 not loosened | ✓ VERIFIED | See truth #5; `## 2.` + `## Known debt` both intact; 4 debt crates not removed. |
| `.planning/PROJECT.md` | carve-out reworded to "required user toolchain" | ✓ VERIFIED | l.119 reworded; agrees with CLAUDE.md. |
| `.planning/ROADMAP.md` | backlog WR-02/WR-04; integrity preserved | ✓ VERIFIED | 6 `### Phase` headings, 1 `## Progress`, 1 `## Backlog (deferred)`, WR-02/WR-04 present. |
| `tsextract/probe.js` | doctor TS health probe (WR-02 fix) | ✓ VERIFIED | Exists; node-builtins only; shares `resolveTypescript`. |

### Key Link Verification

| From | To  | Via | Status | Details |
| ---- | --- | --- | ------ | ------- |
| main.rs run_doctor | gnr8-core source_toolchain | single detect call → discrete probe | ✓ WIRED | `gnr8_core::analyze::source_toolchain` called once (l.224), exactly 1 occurrence. |
| watch.rs | gnr8-core source_toolchain | source_extension threaded into is_trigger_path | ✓ WIRED | exactly 1 `source_toolchain(` call; `source_ext` compared in is_trigger_path. |
| fastapi .gnr8/main.rs | examples/fastapi-bookstore source | FastApi::new().inputs([...]) | ✓ WIRED | generate produces real output that gnr8 check confirms byte-identical. |
| nestjs .gnr8/main.rs | examples/nestjs-bookstore/src | NestJs::new().inputs(["src"]) | ✓ WIRED | generate→check byte-identical. |
| Makefile examples-check | examples/*/generated | gnr8 generate && gnr8 check per example | ✓ WIRED | All 3 `0 written, 5 unchanged`. |
| CLAUDE.md carve-out | PROJECT.md decision | wording audited for agreement | ✓ WIRED | Both say "required user toolchain, gnr8 ships none". |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Full green gate incl. cross-language examples-check | `make check` | exit 0; all 3 examples `0 written, 5 unchanged` | ✓ PASS |
| gnr8 CLI test suite | `cargo test -p gnr8` | 30 + e2e passed, 0 failed | ✓ PASS |
| Core + CLI suites incl. WR regression tests | `cargo test -p gnr8 -p gnr8-core` | doctor_json_field_set, missing_source_toolchain_is_actionable, dot_gnr8_source_language_file_does_not_trigger, typescript_toolchain probes, scan_markers skip — all ok | ✓ PASS |
| No node_modules tracked | `git ls-files examples/nestjs-bookstore/node_modules tsextract/node_modules` | empty | ✓ PASS |
| No new OSS dep across phase | `git log --since 2026-06-26 -- crates/*/Cargo.toml Cargo.lock` | no commits | ✓ PASS |
| Working tree clean after gate | `git status --short` | only pre-existing STATE.md; no example drift | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ---------- | ----------- | ------ | -------- |
| XLANG-01 | 06-02 | FastAPI end-to-end + committed output | ✓ SATISFIED | Truth #1 |
| XLANG-02 | 06-02 | NestJS end-to-end + committed output, no node_modules | ✓ SATISFIED | Truth #2 |
| XLANG-03 | 06-03 | docs/USAGE.md honest envelope | ✓ SATISFIED | Truth #3 |
| XLANG-04 | 06-01 | doctor/check/watch parity, single toolchain decision | ✓ SATISFIED | Truth #4 |
| XLANG-05 | 06-02, 06-03 | stdlib-only sidecars; zero new OSS; deterministic; typescript recorded | ✓ SATISFIED | Truth #5 |

All 5 phase requirement IDs are present in REQUIREMENTS.md (lines 52-56), mapped to Phase 6 (lines 111-115), and declared across the three plans' frontmatter. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| (none) | — | No production `unwrap`/`expect`/`panic` in changed CLI/core files (all matches are inside `#[cfg(test)]` modules) | ℹ️ Info | CLAUDE.md no-panic constraint upheld |

No TBD/FIXME/XXX debt markers introduced. No stubs (all artifacts wired to live `source_toolchain` decision; generated output is real bytes proven by byte-identical regen). Single deterministic toolchain dispatch confirmed (no `_=>` masking a language; one `detect_language` → mapped arms, no fallback — CLAUDE.md rule 3).

### Human Verification Required

None. All truths are programmatically verifiable: the determinism/byte-identity is enforced by `gnr8 check` (machine), toolchain dispatch by unit tests, and the docs/invariant prose was auto-checkable against grep-able structural claims. The PLAN 06-03 `checkpoint:human-verify` (Task 4) was auto-approved in autonomous mode; its substance (envelope honesty, carve-out boundedness) was re-confirmed here: the Flask gaps and NestJS class-DTO scope match the documented extractor behavior, the bright line excludes @nestjs/swagger/zod/class-validator, and rule 2 + the Known-debt schedule are intact — no residual human-only item remains.

### Gaps Summary

No gaps. All 5 requirement IDs satisfied with codebase evidence. The phase goal is observably achieved:

- A developer can drive both FastAPI and NestJS end-to-end from a `.gnr8/` lifecycle — both examples are self-contained, git-tracked, with real committed `generated/` output that regenerates byte-identically.
- The honest per-language envelope is documented in docs/USAGE.md with stated limits and no overclaiming.
- doctor/check/watch parity is real: one deterministic `detect_language`-backed `source_toolchain` decision drives the doctor probe (Go/Python discrete binary + TS node+typescript probe) and the watch trigger extension, with `.gnr8/` excluded and loop-safety intact.
- Cross-language determinism is enforced and green (`make check` incl. `examples-check`).
- The `typescript` toolchain dependency is recorded in CLAUDE.md and PROJECT.md as a required user toolchain (un-vendored; gnr8 ships zero OSS), and gnr8-core adds zero new OSS deps (the 4 v1 debt crates unchanged, correctly not retired).

The 5 code-review findings (WR-01..WR-05) are fixed and committed, with regression tests green. WR-02/WR-04 hardening items are correctly backlogged (ROADMAP 999.1/999.2) rather than folded into the green snapshots.

---

_Verified: 2026-06-26T06:28:21Z_
_Verifier: Claude (gsd-verifier)_
