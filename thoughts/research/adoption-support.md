# Adoption Support For Code-First SDK Publishing

Date: 2026-07-09

## Research Question

What should `gnr8` support for broader adoption when the server source is the source of truth and
`gnr8` emits OpenAPI 3.1 plus SDKs?

Explicit non-goals for this research:

- Do not generate server stubs.
- Do not prioritize older OpenAPI output profiles.
- Do not chase edge-case parity with OpenAPI Generator, Speakeasy, Stainless, Fern, or Kiota.

## Position

The adoption target is not "replace every OpenAPI Generator feature." The credible product claim is:

> gnr8 replaces the fragile chain of framework Swagger generation, OpenAPI Generator, and SDK patches
> for teams that want server source to drive OpenAPI 3.1 and production-ready SDKs.

The previous parity list was too flat. The updated list separates hard adoption blockers from features
that matter for mature SDKs but do not block every API on day one.

## P0: Hard Adoption Blockers

### 1. SDK Semantic Model / Runtime Contract Foundation

`ApiGraph` should remain the source/HTTP truth, but SDK emitters need a shared SDK planning layer before
language rendering. Auth, errors, retries, pagination, grouping, package files, and docs all need the
same operation/package/runtime facts. If every emitter handles those independently, Go/Python/TypeScript
behavior will drift.

Minimum support:

- Add or formalize a language-neutral `SdkModel`.
- Include package metadata, services/groups, operations, schemas, auth, errors, runtime policy, docs
  metadata, and file plan.
- Keep target-language rendering separate from semantic SDK planning.

Do not support yet:

- Full template engine.
- Arbitrary custom generator authoring.
- Vendor-extension compatibility matrix.

### 2. First-Class Auth

Most real APIs are protected. If OpenAPI says an operation is secured but generated SDKs do not accept
credentials and inject them correctly, the SDK is not production-usable without generated-file edits or
wrappers.

Minimum support:

- Generate OpenAPI 3.1 security schemes and operation/global security.
- Propagate the same auth model into each SDK target.
- Support global auth plus per-operation unauthenticated/protected overrides.
- Support API key in header/query, bearer token, and basic auth.
- Accept credentials once through SDK constructor/config.
- Add runtime smoke tests that verify outgoing auth headers/query params.
- Emit clear diagnostics for unsupported security schemes.

Do not support yet:

- OAuth token acquisition/refresh flows.
- OIDC, mTLS, AWS SigV4, HMAC signing, CSRF/session-cookie workflows.
- Full OpenAPI OR/AND multi-scheme SDK UX beyond diagnostics.

### 3. Typed Errors

SDK consumers branch on `401`, `403`, `404`, `409`, `422`, `429`, and `5xx` for auth refresh,
validation display, retry/backoff, idempotency, and support logging. Opaque non-2xx handling makes the
SDK feel like a thin HTTP wrapper.

Minimum support:

- One exported base SDK error type per language.
- Every non-2xx returns/raises that error.
- Always expose status, headers/request ID, raw body, and parsed JSON body when possible.
- If an operation declares a JSON error response schema, decode and expose it.
- Resolve explicit status first, then range/default if represented.
- Keep status helpers minimal; avoid per-status class explosion.

Current repo note: Go already has substantial typed error handling. The adoption work is making this
uniformly complete across SDK targets and OpenAPI import paths.

### 4. Stable Operation Naming and Resource Grouping

Generated SDK method names and import paths are a public contract. Handler renames, route normalization,
or path reshuffling should not randomly break SDK consumers. Large APIs are also hard to use as one flat
method list.

Minimum support:

- Stable unique operation IDs with collision diagnostics.
- First-class operation group/resource metadata in SDK planning.
- Import OpenAPI tags as groups for OpenAPI sources.
- For code-first sources, support deterministic grouping by path prefix/source module and explicit config.
- Preserve flat clients as a fallback/default where appropriate, but allow grouped SDK clients.
- Use the same group/name facts for OpenAPI, SDKs, docs, and `compat`.

Do not support yet:

- Full vendor extension parity such as `x-speakeasy-group` or Fern/Stainless config import.
- Arbitrary deep nested resource hierarchy.
- Every generator's language-specific naming edge case.

### 5. SDK Readiness in `doctor`

This should merge into the existing `doctor`/CI health story, not become a broad standalone lint
product. `gnr8 check` already gates drift, and `doctor` already owns lifecycle/toolchain/pipeline
diagnostics.

Minimum support:

- Add an `sdk_readiness` section to `gnr8 doctor --json` and human output.
- Detect configured SDK targets/output dirs from pipeline artifact metadata.
- For each target, report language, output path, required target toolchain, status, and reason.
- Go: compile/vet-level check.
- Python: `py_compile` plus import smoke.
- TypeScript: package/typecheck smoke when node/typescript are available.
- OpenAPI readiness: parse emitted OpenAPI 3.1, resolve local refs, check operation IDs/schema names.
- Exit non-zero only for actionable readiness failures.

Current repo note: the test suite already has compile/lint gates. The missing product feature is
exposing equivalent readiness signals through `doctor` for user projects.

Do not support yet:

- New `gnr8 lint` command.
- General API style governance.
- Hard dependency on Spectral, Redocly, Speakeasy, or SaaS validators.
- Live-server contract testing.

### 6. Package Metadata and Local Package Validation

Generated SDKs need to be installable through normal language ecosystems. TypeScript needs package
metadata for npm, Python needs `pyproject.toml` project/build metadata, and Go needs coherent `go.mod`
module metadata.

Minimum support:

- Add first-class package metadata fields: package/import/module name, registry package name override,
  version, description, license/SPDX, repository URL, homepage/docs URL, keywords.
- Go: keep/normalize `go.mod`.
- TypeScript: emit valid `package.json`, `exports`, `types`, and runtime dependencies where needed.
- Python: emit optional/default `pyproject.toml`.
- Support deterministic code-as-config overrides.
- Add local package checks such as `npm pack --dry-run`, Python build/import check, and Go module/list/test
  checks where reasonable.
- Provide publishing recipes/workflow snippets.

Do not support yet:

- `gnr8 publish`.
- Registry credentials, token storage, secret creation, package-name availability checks.
- Release orchestration, changelog generation, git tags, dist-tags, rollback.
- Private registry matrix.

## P1: Strong Adoption Features

### 7. Pagination Helpers

For APIs with real list/search endpoints, raw `cursor`, `page`, `limit`, or `offset` parameters force
every SDK consumer to hand-roll loops, termination, and request mutation. That makes generated SDKs feel
thin compared with production SDKs.

Minimum support:

- Explicit code-configured pagination metadata; no broad inference.
- Cursor pagination: request cursor param, response next-cursor field, response items field, optional page
  size.
- Page/offset pagination: page/page_size or offset/limit, response items field, termination by empty/short
  page, total count, or total pages.
- Generate a raw operation plus an idiomatic iterator/page helper.
- Keep OpenAPI standard; optional `x-gnr8-pagination` later.

Do not support yet:

- Vendor-extension compatibility.
- OData/HAL/JSON:API/GraphQL pagination.
- Bidirectional pagination, prefetch, resumable iterators, concurrency/backpressure.

### 8. Timeouts, Conservative Retries, and Idempotency

Production users expect bounded network behavior, but unsafe retries can duplicate side effects. This
must be conservative and explicit.

Minimum support:

- Client-level timeout with per-request override.
- Client-level max retry count with per-request override.
- Retry network errors, `408`, `429`, and `5xx`.
- Respect `Retry-After` where straightforward.
- Retry only safe/idempotent operations by default.
- Do not retry `POST`/`PATCH` unless explicitly marked idempotent in `.gnr8`.
- Preserve idempotency key across retries.

Do not support yet:

- Server-side idempotency implementation.
- Adaptive retries, circuit breakers, hedging, retry budgets.
- Vendor-extension parsing as source of truth.

### 9. Transport Hooks / Middleware

Without hooks, production teams edit or fork generated code for dynamic auth, token refresh, correlation
IDs, tracing, logging/redaction, custom headers, proxies, and error capture. That undermines regeneration.

Minimum support:

- Client-level request hook before network send.
- Response hook after raw response before deserialization.
- Error hook for network/non-2xx errors.
- Context includes operation ID, method, path template, resolved URL, headers, request metadata, status,
  response headers.
- Deterministic ordering.
- Configure at client construction.

Current repo note: TypeScript already has middleware in the fetch-style runtime. The adoption work is
cross-target consistency and a stable documented contract.

Do not support yet:

- Full plugin architecture.
- Model transformation hooks.
- Hook-defined OpenAPI behavior.
- Advanced stream replay/tee behavior.

### 10. Operation Metadata, Examples, and Documented Error Responses

Technically valid OpenAPI is not enough for external adoption. Summaries, descriptions, tags, examples,
deprecation, and explicit error responses feed generated docs, SDK method grouping, examples, and typed
error models.

Minimum support:

- Operation metadata: summary, description, deprecated, tags.
- Response metadata: per-status description, schema/content type, especially error responses.
- Examples: schema/property examples plus named operation request/response examples.
- Top-level tag descriptions.
- SDK docs/README/reference propagation.
- Config API using `OperationSelector`-style selectors.
- Source extraction only where framework metadata is stable and typed.

Do not support yet:

- Full FastAPI/NestJS/Swagger decorator parity.
- Arbitrary comment scraping.
- OpenAPI 3.2 tag hierarchy.
- Auto-generated semantic examples/business error taxonomies.

### 11. Common Content Types

Uploads/downloads and form bodies are common enough that lacking them forces manual clients for otherwise
ordinary APIs.

Minimum support:

- JSON.
- Text/plain.
- `application/x-www-form-urlencoded`.
- `multipart/form-data`.
- Binary download/upload.
- Correct SDK request/response surfaces and docs.

Do not support yet:

- XML, protobuf, arbitrary custom media types.
- Every OpenAPI encoding edge case.

## P2: Customer-Driven / Defer

### Streaming

SSE/raw streaming is increasingly important, especially for AI and event APIs, but it is not universal
enough to outrank auth/errors/package/readiness/pagination. Support it when the target market requires
AI, event, log, or export APIs.

### User-Owned Extension Files

Small hand-written helpers are common. `StaticFiles` is the right lower-risk path before template
overrides. Keep improving cache invalidation, readiness checks, and import-safe extension docs.

### `compat` Beyond OpenAPI Generator

Public SDK diffing is valuable, but broader adoption is better served first by stable naming, package
metadata, and runtime behavior. Let `compat` consume those facts over time rather than making it a
separate roadmap pillar.

## Explicit Deferrals

- Full registry publishing automation.
- Server stubs.
- Older OpenAPI output profiles.
- Docs-site generation.
- Generic template override/custom generator authoring.
- Terraform/CLI generation.
- React hooks.
- Broad webhook tooling.
- Vendor-extension parity.

## Suggested Implementation Order

1. Formalize `SdkModel` enough to carry auth/errors/groups/package/runtime/docs facts.
2. Make auth graph-driven and consistent across all current SDK targets.
3. Finish typed error parity across all SDK targets and imported OpenAPI sources.
4. Add stable operation IDs/groups and duplicate-name diagnostics.
5. Add `doctor.sdk_readiness` using existing compile/typecheck/lint harnesses.
6. Normalize package metadata across Go/TypeScript/Python and add local package checks.
7. Add explicit pagination policy plus SDK helpers.
8. Add timeout/retry/idempotency runtime policy.
9. Add cross-target transport hooks.
10. Add operation metadata/examples/error-response config and docs propagation.
11. Round out common content types.

## Sources

- OpenAPI 3.1 security/error/metadata model: <https://spec.openapis.org/oas/v3.1.0.html>
- OpenAPI 3.1.2 operation metadata: <https://spec.openapis.org/oas/v3.1.2.html>
- Speakeasy auth: <https://www.speakeasy.com/docs/sdks/customize/authentication/configuration>
- Speakeasy errors: <https://www.speakeasy.com/docs/sdks/customize/responses/errors>
- Speakeasy pagination: <https://www.speakeasy.com/docs/sdks/customize/runtime/pagination>
- Speakeasy retries: <https://www.speakeasy.com/docs/sdks/customize/runtime/retries>
- Speakeasy lint/readiness: <https://www.speakeasy.com/docs/sdks/prep-openapi/linting>
- Stainless client settings: <https://www.stainless.com/docs/sdks/configure/client/>
- Stainless config schema: <https://www.stainless.com/docs/reference/config/>
- Stainless diagnostics: <https://www.stainless.com/docs/reference/diagnostics/>
- Kiota auth: <https://learn.microsoft.com/en-us/openapi/kiota/authentication>
- Kiota middleware: <https://learn.microsoft.com/en-us/openapi/kiota/middleware>
- Kiota errors: <https://learn.microsoft.com/en-us/openapi/kiota/errors>
- Fern retries: <https://buildwithfern.com/learn/sdks/deep-dives/retries-with-backoff>
- FastAPI operation metadata: <https://fastapi.tiangolo.com/tutorial/path-operation-configuration/>
- FastAPI advanced operation ID config: <https://fastapi.tiangolo.com/advanced/path-operation-advanced-configuration/>
- npm package metadata: <https://docs.npmjs.com/creating-a-package-json-file/>
- Python `pyproject.toml`: <https://packaging.python.org/en/latest/guides/writing-pyproject-toml/>
- Go module metadata: <https://go.dev/doc/modules/gomod-ref>
