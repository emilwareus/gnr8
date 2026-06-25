# Phase 1: Foundation And Fixtures - Discussion Log (Auto Mode)

> **Audit trail only.** Not consumed by downstream agents. Decisions live in 01-CONTEXT.md.

**Date:** 2026-06-24
**Phase:** 01-foundation-and-fixtures
**Mode:** discuss --auto (autonomous; recommended defaults selected, grounded in PROJECT.md,
REQUIREMENTS.md, ROADMAP.md, and .planning/research/TARGET-API.md)

## Gray Areas & Auto-Selected Decisions

### PoC Contract Lock
- Router target → **Gin first**, generic router-agnostic extraction model. (alt: chi/echo/net-http — deferred)
- OpenAPI target → **3.1.0**. (alt: 3.0.3 for generator compat — deferred behind diagnostics)
- Go SDK shape → **Client(opts) + tag-grouped typed ops + model structs + typed error**. (alt: flat methods; builder pattern — rejected as non-idiomatic)
- `.gnr8/` layout → checked-in code-as-config + ignored cache/output. (detail deferred to Phase 4)

### Rust Workspace Shape
- **Cargo workspace**: `gnr8-core` lib + `gnr8` bin. (alt: single crate — rejected for modularity)
- Edition **2021**, `[workspace.lints]`.
- **thiserror** in libs, **anyhow** at binary boundary only.

### CLI Surface
- **clap** derive; commands init/generate/watch/check/inspect/doctor; inspect routes|schemas|graph.
- Human tables default, `--json` global, `-v` verbose; skeletal commands exit with clear message.

### Fixture & Snapshot Harness
- **insta** snapshots; Go **Gin fixture module** under `fixtures/` per TARGET-API.md §6.
- Expected graph/openapi/sdk/diagnostics snapshots **fail-by-design** until Phases 2–3.

### Quality Gates
- `cargo fmt --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test`.
- Wrapped in **Makefile** + **GitHub Actions** CI.

## Corrections
None — autonomous run, all recommended defaults accepted.
