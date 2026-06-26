---
phase: 6
slug: cross-language-hardening-examples-docs
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-26
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `make check`; CLI integration tests; example regen-and-diff (determinism) |
| **Config file** | `Cargo.toml` workspace + `Makefile`; `examples/*/.gnr8/` Pipeline crates |
| **Quick run command** | `cargo test -p gnr8-core -p gnr8` (after `source ~/.bashrc`; go+python3+node on PATH) |
| **Full suite command** | `make check` (incl. the new cross-language `examples-check` regen-diff) |
| **Estimated runtime** | ~90-240 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test` for the touched crate
- **After every plan wave:** `make check`
- **Before verify:** `make check` green AND `gnr8 generate` for both examples regenerates byte-identical committed output
- **Max feedback latency:** 240 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-W1 | 1x | 1 | XLANG-04 | T-toolchain-detect | doctor/check/watch dispatch on detect_language per source language (Go/Python/TS); minimal pub API; single deterministic decision (no fallback); loop-safe | unit/integration | `cargo test -p gnr8 -p gnr8-core` | ❌ W0 | ⬜ pending |
| 06-W2 | 2x | 2 | XLANG-01, XLANG-02 | — | FastAPI + NestJS `.gnr8/` example lifecycles → OpenAPI 3.1 + Py/TS SDK, real committed output; byte-identical regen | integration | `make examples-check` (regen + diff committed output) | ✅ | ⬜ pending |
| 06-W3 | 3x | 3 | XLANG-03, XLANG-05 | T-supply-chain | docs/USAGE.md honest per-language envelope; record typescript carve-out in CLAUDE.md + PROJECT.md; zero NEW OSS deps in gnr8-core (cargo tree unchanged); cross-language determinism gate in make check | docs/gate | `make check` (+ a no-new-dep assertion) | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Minimal `pub` API to expose `detect_language`/`Lang` (or an equivalent) to the `gnr8` CLI crate (currently `pub(crate)`)
- [ ] `doctor`/`watch` toolchain dispatch generalized off the detected source language (reuse the `*ToolchainMissing` errors)
- [ ] `examples/fastapi-bookstore/` + `examples/nestjs-bookstore/` skeletons (source copied from fixtures + a `.gnr8/` Pipeline crate)
- [ ] `make examples-check` regen-and-diff target wired into the green gate

*Reuses the v1 example pattern + lifecycle/manifest/doctor/check/watch infra; gnr8-core adds NO new OSS crate (the v1 serde/blake3/thiserror known-debt is separately tracked, not retired here); TS sidecar's sole OSS is the recorded `typescript` carve-out.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| docs/USAGE.md envelope is HONEST (limits stated, no overclaiming) | XLANG-03 | Truthfulness of prose vs. actual extractor behavior needs human read | Cross-check the USAGE.md FastAPI/Flask/NestJS rows against the Phase 2/4 SUMMARYs + actual diagnostics |
| The typescript carve-out wording is bounded (does NOT loosen rule 2 generally) | XLANG-05 | Invariant-doc edit needs human judgment | Read the CLAUDE.md carve-out note: it must scope to the TS sidecar toolchain only, behind the JSON-facts boundary, bright-line excluding swagger/zod/class-validator |

---

## Validation Sign-Off

- [x] All tasks have automated verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags (the `watch` work is tested via loop-safety integration tests, not a long-running watcher)
- [x] Feedback latency < 240s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-06-26
