---
phase: 1
slug: foundation-and-fixtures
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-24
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` + `insta` (Rust snapshot tests); `go build`/`go vet` for the Gin fixture module |
| **Config file** | `Cargo.toml` workspace; `fixtures/<svc>/go.mod` |
| **Quick run command** | `cargo test` |
| **Full suite command** | `make check` (`cargo fmt --check` + `cargo clippy --all-targets --all-features --locked -- -D warnings` + `cargo test` + fixture `go build`) |
| **Estimated runtime** | ~30–60 seconds (greenfield) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `make check`
- **Before `/gsd:verify-work`:** Full suite must be green EXCEPT the four red-by-design contract snapshot tests (graph/openapi/sdk/diagnostics), which MUST remain red until Phases 2–3 (FIX-04).
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

> Filled by the planner/executor as tasks are created. Each task gets an automated `cargo test`
> or `go build` assertion, or a Wave 0 dependency. The four contract snapshot tests are tracked
> as **red-by-design** (expected ❌ until Phase 3) per the RESEARCH.md Validation Architecture.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| (planner to populate) | 01 | — | RUST-/FIX-/POC- | unit/snapshot/build | `cargo test` / `go build ./...` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red (incl. red-by-design) · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `Cargo.toml` workspace + `gnr8-core` lib + `gnr8` bin compile (RUST-01)
- [ ] `insta` wired as dev-dependency; snapshot dir established (FIX-03)
- [ ] `fixtures/<svc>/go.mod` builds with `go build ./...` (FIX-01)

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CLI `--help` shows planned command surface | RUST-02 | Human-readable surface check | `cargo run -- --help` and `cargo run -- inspect --help` show init/generate/watch/check/inspect/doctor + routes/schemas/graph |

*Most phase behaviors have automated verification; the help-surface check is also assertable via a CLI snapshot test where practical.*

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter (after planner populates the task map)

**Approval:** pending
