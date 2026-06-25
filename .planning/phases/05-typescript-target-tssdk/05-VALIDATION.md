---
phase: 5
slug: typescript-target-tssdk
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-25
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `make check`; generated-SDK hermetic test runs `tsc --noEmit` via the vendored typescript |
| **Config file** | `Cargo.toml` workspace + `Makefile`; vendored `tsextract/node_modules/typescript` (no new install) |
| **Quick run command** | `cargo test -p gnr8-core` (after `source ~/.bashrc`; node v24 on PATH) |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60-180 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify:** `make check` green AND the hermetic `tsc --noEmit` typecheck of the generated TS SDK passes
- **Max feedback latency:** 180 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-01 | 01 | 1 | TSSDK-01 | — | `tssdk/` emit/bundle/mod: fetch Client, interface models, string-literal-union enums, typed ApiError; exhaustive Type match (no `_=>`); optional/nullable independent axes | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 05-02 | 02 | 2 | TSSDK-03 | — | `TsSdk` Target built-in drives the Pipeline; byte-identical deterministic output | unit | `cargo test -p gnr8-core builtins` | ✅ | ⬜ pending |
| 05-03 | 03 | 3 | TSSDK-02 | T-sdk-typecheck | Generated SDK type-checks under `tsc --noEmit --strict --lib es2022,dom` (fetch resolved via vendored lib.dom.d.ts, no @types/node); zero runtime deps (no axios) | integration | `cargo test -p gnr8-core --test tssdk_compile` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tssdk/` module skeleton (emit/bundle/mod) cloned from `pysdk/` (no formatter)
- [ ] Exhaustive `Type` match in `tssdk::emit` (Union/inline-Enum/Map/optional/nullable) — no `_=>`
- [ ] Hermetic `tsc --noEmit` test harness (vendored typescript, `--lib es2022,dom` for fetch, skip-if-node/typescript-absent)

*Reuses the existing Rust SDK seam + determinism harness; NO new Rust crate; NO new npm dep (typescript already vendored); generated SDK is dependency-free.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Generated SDK is genuinely dependency-free | TSSDK-01 | "no runtime deps" is a property of emitted code | grep the generated SDK for non-builtin imports (axios/node-fetch/etc.) — must be absent; only built-in fetch + local types |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have automated verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 180s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-06-25
