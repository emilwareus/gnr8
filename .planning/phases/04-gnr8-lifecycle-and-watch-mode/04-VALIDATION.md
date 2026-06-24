---
phase: 4
slug: gnr8-lifecycle-and-watch-mode
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-24
---

# Phase 4 — Validation Strategy

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` + `insta` (Rust); new `tests/lifecycle.rs`, `tests/watch_smoke.rs` |
| **Quick run command** | `cargo test -p gnr8-core` |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60–120s |

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify-work:** Full suite green; the pure `plan_writes` truth table fully covered.
- **Max feedback latency:** 120s

## Per-Task Verification Map

> Planner/executor populate. Headline guarantees get dedicated tests: no-clobber, no-op=no-write, watch-no-loop.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| (planner to populate) | 04 | — | WS-/WATCH- | unit/integration | `cargo test -p gnr8-core` | ⬜ pending |

## Wave 0 Requirements

- [ ] `tests/lifecycle.rs` — init idempotency; ownership user-edit→warn+skip, --force overwrite; no-op second-generate writes nothing
- [ ] `plan_writes` pure-function unit tests covering the full truth table (incl. present-on-disk/absent-from-manifest → UserEdited)
- [ ] `tests/watch_smoke.rs` — watch filter ignores own outputs (no loop); debounce coalesces a burst
- [ ] add `--test lifecycle` (+ watch tests) to the blocking `gates`

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `gnr8 watch` live regen + latency print | WATCH-02/03 | Real FS watcher timing | `gnr8 watch fixtures/goalservice`, edit a .go file, observe regen + latency line (core is unit-tested; this is the live smoke) |

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 deps
- [ ] No 3 consecutive tasks without automated verify
- [ ] Wave 0 covers plan_writes truth table + watch filter
- [ ] Feedback latency < 120s
- [ ] nyquist_compliant: true

**Approval:** pending
