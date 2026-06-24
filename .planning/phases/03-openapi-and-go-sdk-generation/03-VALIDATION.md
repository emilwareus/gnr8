---
phase: 3
slug: openapi-and-go-sdk-generation
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-24
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` + `insta` (Rust); `go build`/`gofmt` for the generated SDK compile test (hermetic, stdlib-only, GOPROXY=off-safe) |
| **Config file** | `Cargo.toml` workspace; generated temp `go.mod` (zero requires) |
| **Quick run command** | `cargo test -p gnr8-core` |
| **Full suite command** | `make check` (fmt/clippy/all tests incl. 4 now-green contract tests + goextract build) |
| **Estimated runtime** | ~60–120 seconds (incl. one `go build` of the generated SDK) |

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify-work:** Full suite green. ALL FOUR contract tests (graph/diagnostics/openapi/sdk) green by end of phase; promote to blocking CI.
- **Max feedback latency:** 120 seconds

## Per-Task Verification Map

> Planner/executor populate. Each task gets cargo test / go build assertions. Determinism: lower twice / generate twice → byte-identical.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| (planner to populate) | 03 | — | OAPI-/SDK- | unit/snapshot/go-build | `cargo test -p gnr8-core` | ⬜ pending |

## Wave 0 Requirements

- [ ] OpenAPI typed structs + YAML writer round-trip/determinism unit test
- [ ] Generated-SDK temp-dir + go.mod scaffolding helper (hermetic, GOPROXY=off-safe)
- [ ] `go build` of generated SDK succeeds (SDK-05); httptest smoke test calls a fixture op

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Generated OpenAPI semantically matches expected/openapi.yaml | OAPI-01/02 | Reviewed snapshot accept | Review the generated openapi `.snap` vs fixtures/goalservice/expected/openapi.yaml for semantic equivalence |

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 deps
- [ ] No 3 consecutive tasks without automated verify
- [ ] Wave 0 covers generated-SDK go build + smoke test
- [ ] Feedback latency < 120s
- [ ] nyquist_compliant: true

**Approval:** pending
