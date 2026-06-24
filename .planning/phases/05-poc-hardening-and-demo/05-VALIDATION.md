---
phase: 5
slug: poc-hardening-and-demo
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-25
---

# Phase 5 — Validation Strategy

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` + `insta` (Rust); `scripts/bench.sh` (smoke); doc/evidence are reviewed artifacts |
| **Quick run command** | `cargo test -p gnr8 -p gnr8-core` |
| **Full suite command** | `make check` (THE final v1 gate — fmt/clippy/all tests/4 snapshots/sdk_compile/determinism/lifecycle/watch) |
| **Estimated runtime** | ~90–150s |

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8`
- **After every plan wave:** `make check`
- **Before milestone audit:** `make check` GREEN is the HARD-03 sign-off.
- **Max feedback latency:** 150s

## Per-Task Verification Map

> Planner/executor populate. doctor gets unit tests (healthy vs actionable exit, --json shape); bench is a smoke; demo+evidence are reviewed.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| (planner to populate) | 05 | — | HARD- | unit/smoke/review | `cargo test -p gnr8` | ⬜ pending |

## Wave 0 Requirements

- [ ] `doctor` unit tests: healthy project → exit 0; drift/missing-config/no-go → exit 1; `--json` field set asserted
- [ ] `scripts/bench.sh` runs on a scratch fixture copy and prints cold/warm-noop/single-file-edit numbers

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| docs/demo.md reproducible from fresh checkout | HARD-02 | Doc walkthrough | Follow docs/demo.md exactly on a scratch fixture copy; OpenAPI+SDK appear, a Go edit updates outputs |
| Final evidence: all v1 reqs satisfied | HARD-03 | Review artifact | `make check` GREEN + evidence maps 37 v1 reqs → satisfied-by |

## Validation Sign-Off

- [ ] doctor has automated tests; bench has a smoke
- [ ] make check GREEN is the milestone HARD-03 gate
- [ ] Feedback latency < 150s
- [ ] nyquist_compliant: true

**Approval:** pending
