# Milestones

## v1.0 PoC: Go to OpenAPI to Go SDK (Shipped: 2026-06-24)

**Phases completed:** 5 phases, 14 plans, 38 tasks

**Key accomplishments:**

- Two-crate Cargo workspace (gnr8-core lib + gnr8 clap CLI) with a thiserror typed-error boundary, five stubbed router-agnostic module seams returning NotYetImplemented, a clippy-clean six-command CLI that exits cleanly without panicking, and a committed PoC contract locking Go->OpenAPI->Go-SDK scope.
- 1. [Rule 3 - Blocking] Re-pinned gin in go.mod after `go mod tidy` pruned it
- Four insta contract tests (graph/openapi/sdk/diagnostics) that fail clearly today via a panicking `.expect()` on the NotYetImplemented gnr8-core seams, plus a Makefile and a GitHub Actions CI that reconcile RUST-03 (blocking gates green) with FIX-04 (contract suite visibly red) by splitting them into separate jobs — the red suite promoted to blocking in Phase 3.
- A go/packages-based `goextract` helper that emits a deterministic sorted JSON facts document (8 DTO schemas + TargetDirection enum + float64/free-form-map diagnostics) for the goalservice fixture, plus the Rust serde mirror, a typed-error subprocess driver, and the Makefile/CI gate.
- goextract now emits 4 Gin route facts (method, normalized path, handler, secured, request/response/param refs, source spans) recognized via go/types Info.Selections, with handler request/response inference (go/constant status mapping) and a swaggo doc-comment escape hatch (operationId goalUuidPut, aggregation Enums, ApiKeyAuth security, @Router override) merged code-primary — plus exactly the 3 untyped-query WARN diagnostics.
- Router-agnostic Rust `ApiGraph` built from the goextract facts with deterministic stable operation/schema ids and fully sorted serialization, `build_graph` + `diagnostics::collect` implemented, three `inspect` renderers (table + `--json`), and the two Phase-1 contract tests (`snapshot_graph` + `snapshot_diagnostics`) flipped GREEN with reviewed `.snap` files plus an end-to-end determinism test — `snapshot_openapi`/`snapshot_sdk` stay red-by-design for Phase 3.
- `lower::to_openapi` lowers the Phase-2 ApiGraph into a valid OpenAPI 3.1.0 YAML document — typed Rust structs + a deterministic key-ordered hand-rolled YAML writer, absolute `/goal/...` paths joined from a const, free-form maps surfaced as `additionalProperties: true`, dangling `$ref`/unknown-kind as typed `CoreError::Lowering` — flipping `snapshot_openapi` GREEN.
- `sdk::generate` emits a deterministic, gofmt-clean Go SDK from the Phase-2 ApiGraph — a functional-options `Client`, tag-grouped `context.Context`-first operation methods with path/query/body handling and a typed `APIError`, and model structs with json tags/optional pointers/enum newtypes/uuid·time·float32 mapping — serialized as one file-marker-framed String that flips `snapshot_sdk` GREEN and `go build`s clean.
- 1. [Rule 3 - Blocking] `sdk::write_to_dir` was not callable from an out-of-crate integration test
- `gnr8 init` now idempotently scaffolds a `.gnr8/` workspace (checked-in `config.toml` + auto-written `.gitignore` ignoring `/cache/`) and a typed `toml`-backed `Config` reads the documented PoC knobs (inputs/output paths/go module/naming overrides) with `deny_unknown_fields`, plus four new lifecycle `CoreError` variants for 04-02/04-03.
- A blake3-hashed ownership manifest plus a PURE `plan_writes` truth table now drive `gnr8 generate`/`gnr8 check` through the real Phase-3 pipeline, delivering the two headline guarantees — no silent clobbering (a hand-edited generated file is warned + skipped unless `--force`) and no-op = no write (a byte-identical second generate touches zero files and zero mtimes) — with naming overrides that rename referenced types without dangling any `$ref`.
- Phase:
- `gnr8 doctor` read-only health aggregator (lifecycle + stale/drift + unsupported-pattern diagnostics, human report + `--json`, exit 0 healthy / 1 actionable with informational WARNs excluded) plus a hermetic `scripts/bench.sh` producing cold/warm-no-op/single-file-edit wall-clock numbers on a scratch fixture copy.
- `docs/demo.md` — a verified-reproducible fresh-checkout walkthrough (build → scratch-copy the fixture → init → generate → doctor → add one Go field → re-generate, with only the affected OpenAPI + SDK outputs updating, all real captured output) — plus `docs/evidence.md`, the HARD-03 milestone sign-off that captures `make check` GREEN live (exit 0) and maps every v1 requirement to the concrete file/test where it is satisfied.

---
