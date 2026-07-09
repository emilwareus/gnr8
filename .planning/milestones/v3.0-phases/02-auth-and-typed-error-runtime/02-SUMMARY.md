# Phase 2 Summary: Auth And Typed Error Runtime

## Status

In progress.

## Completed Plans

- 02-01 Query API-Key Auth: added `ApplySecurity::api_key_query`, OpenAPI `apiKey` query lowering,
  Go/Python/TypeScript query credential emission, shared auth resolver coverage, and Go runtime smoke.
- 02-02 Bearer And Basic Auth: added `ApplySecurity::bearer` / `ApplySecurity::basic`, OpenAPI
  `http` security lowering/import, shared operation-scoped HTTP auth resolution, Go/Python/TypeScript
  client auth options, Authorization header emission, and Go bearer/basic runtime smoke.
- 02-03 Typed SDK Error Runtime: added shared non-2xx response-body resolution, enriched Go/Python/
  TypeScript base SDK error objects with status, headers/request ID, raw body, parsed JSON body, and
  decoded declared error-body access, plus Go/Python runtime smoke and target generator assertions.

## Verification

- `cargo fmt --all --check`
- `cargo test -p gnr8 sdk::emit_common`
- `cargo test -p gnr8 lower::`
- `cargo test -p gnr8 pysdk::emit::tests::operations::query_api_key_auth_is_appended_to_query_string`
- `cargo test -p gnr8 tssdk::emit::tests::operations::query_api_key_auth_is_appended_to_query_string`
- `cargo test -p gnr8 --test sdk_compile --test pysdk_compile --test tssdk_compile`

All passed for Plan 1 on 2026-07-09.

Plan 2 verification passed on 2026-07-09:

- `cargo test -p gnr8 auth -- --nocapture`
- `cargo test -p gnr8 security_is_emitted -- --nocapture`
- `cargo test -p gnr8 imports_openapi31_security_schemes_and_operation_security -- --nocapture`
- `cargo test -p gnr8 generated_sdk -- --nocapture`
- `cargo clippy -p gnr8 --all-targets -- -D warnings`

Plan 3 verification passed on 2026-07-09:

- `cargo test -p gnr8 error -- --nocapture`
- `cargo test -p gnr8 generated_sdk -- --nocapture`
- `cargo test -p gnr8 body_op_has -- --nocapture`
- `cargo clippy -p gnr8 --all-targets -- -D warnings`

## Remaining

- Cross-target auth runtime smoke coverage beyond the existing Go smoke tests where local toolchains are
  available.
