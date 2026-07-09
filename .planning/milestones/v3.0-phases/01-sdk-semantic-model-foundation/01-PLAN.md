# Phase 1 Plan: SDK Semantic Model Foundation

## Goal

Make the existing SDK planning layer explicit enough to carry the production SDK adoption facts required
by later phases, without changing current minimal SDK output bytes.

## Requirements

- SDKM-01: Lower `ApiGraph` into a shared SDK planning model carrying package, service/group,
  operation, schema, auth, error, runtime policy, docs metadata, and file-plan facts.
- SDKM-02: Ensure Go, Python, and TypeScript SDK targets build and consume the shared planning model
  for shared semantic surfaces.
- SDKM-03: Prove determinism and preserve existing generated SDK output.

## Implementation

1. Extend `crates/gnr8-core/src/sdk/model.rs` with neutral error, runtime-policy, docs-metadata, and
   per-operation auth/success/error response facts.
2. Keep current emitters output-stable by recording the shared facts before rendering, not changing
   language runtime behavior in this phase.
3. Document the `ApiGraph -> SdkModel -> emitter` boundary in `docs/sdk-model.md`.
4. Run focused Rust tests and the existing SDK snapshot/compile gates that catch output drift.

## Verification

- `cargo test -p gnr8-core sdk::model`
- `cargo test -p gnr8-core --test snapshot_sdk --test sdk_pipeline`
- `cargo test -p gnr8-core --test pysdk_compile --test tssdk_compile --test sdk_compile`

## Deferred

- Auth runtime behavior, typed error runtime behavior, retries/timeouts/hooks, package metadata,
  pagination helpers, operation metadata, and media-type behavior remain later v3.0 phases.
