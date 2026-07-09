# Phase 4 Summary — SDK Runtime Ergonomics

Status: Complete
Completed: 2026-07-09

## Delivered

- Added shared runtime and pagination metadata to `ApiGraph` and the SDK model.
- Added `.gnr8` transforms for runtime defaults, idempotent operations, and explicit pagination policies.
- Generated Go, Python, and TypeScript SDK runtime controls:
  - client timeout and max-retry defaults
  - per-request timeout and retry overrides
  - conservative retry behavior for network errors, `408`, `429`, and `5xx`
  - unsafe-method retry gating unless an operation is explicitly idempotent
  - idempotency-key preservation across retries
  - request, response, and error hooks carrying operation context
- Generated pagination helpers for configured operations:
  - Go page collector and item visitor helpers
  - Python page and item generators
  - TypeScript async page and item generators
- Preserved raw operation methods for paginated operations.

## Verification

- `cargo test -p gnr8 generated_sdk -- --nocapture`
- `cargo test -p gnr8 tssdk::emit::tests::operations -- --nocapture`
- `cargo clippy -p gnr8 --all-targets -- -D warnings`
- `cargo clippy -p gnr8-cli --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --check`

## Notes

- TypeScript `tsc` smoke tests still skip when the local Node/TypeScript toolchain is unavailable, but
  emitter tests lock the generated async pagination helper shape.
- Pagination remains explicit by design; no broad inference from field names or route names was added.
