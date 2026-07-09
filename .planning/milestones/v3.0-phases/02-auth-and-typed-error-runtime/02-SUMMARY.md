# Phase 2 Summary: Auth And Typed Error Runtime

## Status

In progress.

## Completed Plans

- 02-01 Query API-Key Auth: added `ApplySecurity::api_key_query`, OpenAPI `apiKey` query lowering,
  Go/Python/TypeScript query credential emission, shared auth resolver coverage, and Go runtime smoke.

## Verification

- `cargo fmt --all --check`
- `cargo test -p gnr8 sdk::emit_common`
- `cargo test -p gnr8 lower::`
- `cargo test -p gnr8 pysdk::emit::tests::operations::query_api_key_auth_is_appended_to_query_string`
- `cargo test -p gnr8 tssdk::emit::tests::operations::query_api_key_auth_is_appended_to_query_string`
- `cargo test -p gnr8 --test sdk_compile --test pysdk_compile --test tssdk_compile`

All passed for Plan 1 on 2026-07-09.

## Remaining

- Bearer token auth.
- Basic auth.
- Richer typed error runtime payloads across Go/Python/TypeScript.
