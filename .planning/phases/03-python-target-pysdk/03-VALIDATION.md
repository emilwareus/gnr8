---
phase: 3
slug: python-target-pysdk
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-25
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `make check`; generated-SDK hermetic test drives `python3` (py_compile + import + stdlib http.server round-trip) |
| **Config file** | `Cargo.toml` workspace + `Makefile` (existing) |
| **Quick run command** | `cargo test -p gnr8-core` (after `source ~/.bashrc`) |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60-150 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify:** `make check` green AND the hermetic Python SDK test (generate → py_compile → import → round-trip) passes
- **Max feedback latency:** 150 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-W1 | 1x | 1 | PYSDK-01 | — | `pysdk/` emit/bundle: stdlib-urllib client, @dataclass models (required-first ordering), typed ApiError, exhaustive Type match incl. Union/inline-Enum (no `_=>`); 3.9-safe annotations | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 03-W2 | 2x | 2 | PYSDK-03 | — | `PySdk` Target built-in drives the Pipeline; byte-identical deterministic output | unit | `cargo test -p gnr8-core --test sdk_pipeline` | ✅ | ⬜ pending |
| 03-W3 | 3x | 3 | PYSDK-02 | T-sdk-hermetic | Generated SDK compiles + imports + round-trips against stdlib http.server (2xx dataclass, 4xx typed ApiError); no third-party HTTP dep | integration | `cargo test -p gnr8-core --test pysdk_compile` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `pysdk/` module skeleton (emit/bundle/mod) cloned from `gosdk/` (drop the gofmt analog)
- [ ] Exhaustive `Type` match in `pysdk::emit` that handles Union + inline-Enum (the cases gosdk rejects) — no `_=>`
- [ ] Hermetic Python test harness (stdlib temp dir + `python3` skip-if-absent, mirror `sdk_compile.rs`)

*Reuses existing Rust SDK seam + determinism harness; NO new Rust crate; generated SDK + test are stdlib-Python only.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Generated SDK is genuinely dependency-free | PYSDK-01 | "no third-party deps" is a property of emitted code | grep the generated SDK for non-stdlib imports (requests/httpx/pydantic/etc.) — must be absent; only `urllib`/`dataclasses`/`enum`/`json`/`typing` |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have automated verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 150s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-06-25
