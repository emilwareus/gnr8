---
phase: 1
slug: language-neutral-ir-facts-contract-fixtures
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-25
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` (workspace) + `make check` (fmt, clippy -D warnings, snapshots, determinism) |
| **Config file** | `Cargo.toml` workspace + `Makefile` (existing) |
| **Quick run command** | `cargo test -p gnr8-core` |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60-120 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p gnr8-core` (quick)
- **After every plan wave:** Run `make check` (full suite)
- **Before `/gsd:verify-work`:** Full suite must be green EXCEPT intentionally-RED fixture snapshots (the red-by-design acceptance contract for Phases 2-5)
- **Max feedback latency:** 120 seconds

> NOTE: Phase 1's success criterion #4 requires committed snapshots that VISIBLY FAIL. These red tests are the acceptance contract — they are expected red and must be clearly marked (e.g. `#[ignore]` with a documented reason, or a dedicated red-by-design test module excluded from the green gate) so `make check` stays green for everything Phase 1 actually delivers while the red contract remains visible.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-01-xx | 01 | 1 | IR-01 | — | Neutral Type vocab (objects/arrays/enums/optional/nullable/unions), no language terms | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 01-01-xx | 01 | 1 | IR-02 | T-02-05 (strict deserialize) | Facts contract rejects unknown fields | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 01-02-xx | 02 | 2 | IR-03 | — | Lowering + SDK gen consume IR with no per-language branches (exhaustive match, no `_=>`) | snapshot | `cargo test -p gnr8-core --test snapshot_openapi` | ✅ | ⬜ pending |
| 01-03-xx | 03 | 3 | IR-04 | — | FastAPI/Flask/NestJS fixtures exist; red-by-design snapshots committed & visibly failing | snapshot | `cargo test -p gnr8-core` (red module) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Neutral `Type` enum round-trip unit tests (facts ⇄ JSON) — stubs for IR-01, IR-02
- [ ] Exhaustive-match compile guard in lowering + SDK (no `_ =>` catch-all on `Type`) — IR-03
- [ ] Red-by-design fixture snapshot harness wired for FastAPI/Flask/NestJS — IR-04
- [ ] Existing Go-fixture snapshots deliberately re-accepted (stay GREEN) after any facts-field churn

*Existing Rust test infrastructure (`cargo test`, `insta` snapshots, `tests/determinism.rs`) covers the harness; no new framework install.*

> Rule-2 note: `insta` is on the known-debt list. Phase 1 may continue using the existing snapshot harness (do not add NEW OSS deps); a hand-rolled snapshot replacement is out of scope for this phase unless a plan explicitly scopes it.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Red-by-design snapshots are "visibly failing" for the right reason (no extractor yet, not a harness bug) | IR-04 | Distinguishing intended-red from accidentally-broken needs human read of the diff | Run the red module; confirm each failure is "expected facts vs empty/absent extraction", documented as intentional |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
