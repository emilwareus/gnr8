---
phase: 2
slug: go-analysis-and-api-graph
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-24
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` + `insta` (Rust); `go build`/`go vet`/`go test` for the `goextract/` helper |
| **Config file** | `Cargo.toml` workspace; `goextract/go.mod` |
| **Quick run command** | `cargo test -p gnr8-core` |
| **Full suite command** | `make check` (fmt/clippy/test + go build of goextract + fixture) |
| **Estimated runtime** | ~30–90 seconds |

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify-work:** Full suite green. The `snapshot_graph` + `snapshot_diagnostics` contract tests flip from red-by-design to GREEN this phase; `snapshot_openapi` + `snapshot_sdk` remain red-by-design until Phase 3.
- **Max feedback latency:** 90 seconds

## Per-Task Verification Map

> Planner/executor populate. Each task gets a `cargo test` / `go test` / `go build` assertion or Wave 0 dep.
> Determinism (GRAPH-02) gets a dedicated check: run analyze twice, assert byte-identical output.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| (planner to populate) | 02 | — | GO-/GRAPH- | unit/snapshot/build | `cargo test -p gnr8-core` / `go test ./...` | ⬜ pending |

## Wave 0 Requirements

- [ ] `goextract/go.mod` builds (`go build ./...`) and the helper emits JSON for the fixture
- [ ] Rust serde DTOs deserialize the helper JSON (round-trip unit test)
- [ ] Determinism harness: analyze fixture twice → identical bytes (GRAPH-02)

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `inspect routes/schemas/graph` human-table readability | GRAPH-03 | Visual table check | `cargo run -p gnr8 -- inspect routes fixtures/goalservice` (also assertable via `--json` snapshot) |

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] No 3 consecutive tasks without automated verify
- [ ] Wave 0 covers MISSING references (goextract build, serde round-trip, determinism)
- [ ] Feedback latency < 90s
- [ ] `nyquist_compliant: true` set after planner populates the task map

**Approval:** pending
