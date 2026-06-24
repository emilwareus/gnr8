# Phase 3: OpenAPI And Go SDK Generation - Discussion Log (Auto Mode)

> Audit trail only. Decisions in 03-CONTEXT.md.

**Date:** 2026-06-24 · **Mode:** discuss --auto (recommended defaults; grounded in PROJECT/REQUIREMENTS/ROADMAP/TARGET-API + Phase-1 expected/ targets + Phase-2 graph).

## Gray Areas & Auto-Selected Decisions
- **OpenAPI lowering** → typed Rust structs → OpenAPI 3.1.0 YAML (insta snapshot; reconcile expected/openapi.yaml). (alt: serde_json::Value blobs — rejected, less type-safe)
- **Lowering diagnostics (OAPI-03)** → emit on un-representable facts; never panic/drop.
- **Go SDK shape** → Client(opts: baseURL+*http.Client) + tag-grouped typed ops (ctx first) + model structs + typed API error; per Phase-1 expected/sdk. (alt: builder pattern — rejected as non-idiomatic)
- **SDK codegen** → deterministic Rust string emission (no heavy template engine); gofmt-clean.
- **SDK-05 compile test** → materialize generated SDK to temp dir + go.mod, run `go build` + httptest smoke test calling a fixture op.
- **Snapshots/gates** → flip snapshot_openapi + snapshot_sdk green; promote all 4 contract tests to blocking CI.

## Corrections
None — autonomous run, all recommended defaults accepted.
