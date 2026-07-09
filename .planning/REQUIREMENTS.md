# Requirements: v3.0 Production-ready SDK adoption

## Goal

Make `gnr8` a production-ready SDK publishing pipeline where server source drives OpenAPI 3.1 and
installable, operationally credible SDKs.

## Scope Source

- Research: `thoughts/research/adoption-support.md`
- Confirmed milestone version: `v3.0`
- Explicit non-goals: server stubs, older OpenAPI output profiles, generic template overrides, registry
  publishing automation, docs-site generation, Terraform/CLI/React helpers, and vendor-extension parity.

## Requirements

### SDK Model Foundation

- [x] **SDKM-01**: A gnr8 maintainer can lower `ApiGraph` into a shared SDK planning model that carries
  package, service/group, operation, schema, auth, error, runtime policy, docs metadata, and file-plan facts.
- [x] **SDKM-02**: A gnr8 maintainer can render Go, Python, and TypeScript SDKs from the shared SDK planning
  facts without duplicating semantic decisions in each emitter.
- [x] **SDKM-03**: A gnr8 maintainer can add tests proving the SDK planning model is deterministic and does
  not change existing minimal SDK outputs except where the v3.0 requirements intentionally change them.

### Auth

- [x] **AUTH-01**: A `.gnr8` pipeline author can declare global and per-operation auth requirements that
  lower consistently into OpenAPI 3.1 and generated SDKs.
- [x] **AUTH-02**: An SDK consumer can configure API key header/query auth, bearer token auth, or basic auth
  once at client construction and have protected operations send the correct credentials.
- [x] **AUTH-03**: An SDK consumer can call unauthenticated operations without sending configured credentials
  when the operation is explicitly marked public.
- [x] **AUTH-04**: A gnr8 maintainer can run runtime smoke tests that verify outgoing auth headers/query
  params for Go, Python, and TypeScript SDKs.
- [x] **AUTH-05**: A `.gnr8` pipeline author gets a clear diagnostic when a security scheme is valid OpenAPI
  but unsupported by a generated SDK target.

### Typed Errors

- [x] **ERR-01**: An SDK consumer gets one exported base SDK error type per target language for every non-2xx
  response.
- [x] **ERR-02**: An SDK consumer can inspect status code, response headers/request ID, raw body, and parsed
  JSON body from an SDK error.
- [x] **ERR-03**: An SDK consumer can access a decoded declared error response schema when an operation
  documents a JSON error body.
- [x] **ERR-04**: A gnr8 maintainer can verify explicit status errors resolve before range/default errors
  across OpenAPI-imported and code-first graphs.
- [x] **ERR-05**: A gnr8 maintainer can prove Go, Python, and TypeScript error behavior through compile/type
  checks and runtime smoke tests.

### Stable SDK Surface

- [ ] **SURF-01**: A `.gnr8` pipeline author can assign stable operation IDs and receive collision diagnostics.
- [ ] **SURF-02**: A `.gnr8` pipeline author can group operations by imported OpenAPI tags, path prefix,
  source module/package, or explicit configuration.
- [ ] **SURF-03**: An SDK consumer can use grouped SDK clients where configured, while existing flat clients
  remain available as a fallback/default where appropriate.
- [ ] **SURF-04**: A gnr8 maintainer can verify the same operation/group/name facts drive OpenAPI output,
  SDK methods, generated docs, and `gnr8 compat`.

### SDK Readiness

- [x] **READY-01**: A gnr8 user can run `gnr8 doctor --json` and see an `sdk_readiness` section for each
  configured SDK target.
- [x] **READY-02**: A gnr8 user can see target language, output path, required target toolchain, readiness
  status, and failure reason for each SDK target.
- [x] **READY-03**: `gnr8 doctor` reports actionable failures for generated Go compile/vet checks, Python
  `py_compile` plus import checks, and TypeScript package/typecheck smoke checks where toolchains are available.
- [x] **READY-04**: `gnr8 doctor` reports OpenAPI readiness for emitted OpenAPI 3.1 parseability, local ref
  resolution, operation ID stability, and schema-name stability.
- [x] **READY-05**: Informational source diagnostics remain non-blocking unless they directly block SDK
  generation or readiness.

### Package Metadata

- [x] **PKG-01**: A `.gnr8` pipeline author can configure package/import/module name, registry package name,
  version, description, license/SPDX, repository URL, homepage/docs URL, and keywords.
- [x] **PKG-02**: A generated Go SDK includes coherent `go.mod` metadata.
- [x] **PKG-03**: A generated TypeScript SDK includes valid `package.json`, `exports`, `types`, and runtime
  dependency metadata where needed.
- [x] **PKG-04**: A generated Python SDK can include a valid `pyproject.toml` with build-system and project
  metadata.
- [x] **PKG-05**: A gnr8 user can run local package validation such as `npm pack --dry-run`, Python build/import
  checks, and Go module/list/test checks where reasonable.
- [x] **PKG-06**: A gnr8 user can follow generated or documented publishing recipes without `gnr8` storing
  registry credentials or performing registry uploads.

### Pagination

- [ ] **PAGE-01**: A `.gnr8` pipeline author can explicitly mark cursor-paginated operations with request
  cursor param, response next-cursor field, response items field, and optional page-size param.
- [ ] **PAGE-02**: A `.gnr8` pipeline author can explicitly mark page/offset-paginated operations with
  page/page-size or offset/limit params, response items field, and termination policy.
- [ ] **PAGE-03**: An SDK consumer can use idiomatic iterator/page helpers for configured paginated operations.
- [ ] **PAGE-04**: An SDK consumer can still call the raw operation method for paginated operations.

### SDK Runtime Policy

- [ ] **RUN-01**: An SDK consumer can configure client-level timeout and per-request timeout overrides.
- [ ] **RUN-02**: An SDK consumer can configure client-level max retries and per-request retry overrides.
- [ ] **RUN-03**: Generated SDKs retry only network errors, `408`, `429`, and `5xx` by default and respect
  `Retry-After` where straightforward.
- [ ] **RUN-04**: Generated SDKs do not retry unsafe mutations such as `POST` or `PATCH` unless the operation
  is explicitly marked idempotent.
- [ ] **RUN-05**: Generated SDKs preserve the same idempotency key across retries for explicitly idempotent
  operations.
- [ ] **RUN-06**: An SDK consumer can install request, response, and error hooks/middleware at client
  construction time.
- [ ] **RUN-07**: Hook context includes operation ID, method, path template, resolved URL, headers, request
  metadata, status, and response headers.

### API Metadata And Content Types

- [ ] **META-01**: A `.gnr8` pipeline author can configure operation summary, description, deprecation, tags,
  response descriptions, and documented error responses using selector-based transforms.
- [ ] **META-02**: A `.gnr8` pipeline author can configure named request/response examples for operation media
  types.
- [ ] **META-03**: Generated OpenAPI and SDK docs propagate operation metadata, examples, deprecation notes,
  tags, and documented error responses.
- [ ] **MEDIA-01**: Generated OpenAPI and SDKs support JSON, `text/plain`, `application/x-www-form-urlencoded`,
  `multipart/form-data`, and binary upload/download for common API endpoints.

## Future Requirements

- Streaming/SSE/raw stream SDK surfaces for AI, event, log, or export APIs.
- Rich user-owned extension-file cache/readiness integration beyond current `StaticFiles`.
- Broader `compat` behavior that compares runtime auth behavior, package metadata, and grouped SDK surfaces.
- Additional SDK targets such as Rust.
- Additional source frontends such as Hono, typed Express/Fastify, or Rust source extraction.

## Out of Scope

- Server stub generation.
- Older OpenAPI output profiles.
- Generic template override/custom generator authoring.
- Registry publishing automation and credential management.
- Docs-site generation.
- Terraform, CLI, React hooks, and broad webhook helper generation.
- Vendor-extension parity with OpenAPI Generator, Speakeasy, Stainless, Fern, or Kiota.
- OAuth/OIDC token acquisition flows, mTLS, AWS SigV4, HMAC signing, CSRF/session-cookie workflows.
- XML, protobuf, arbitrary custom media types, and every OpenAPI encoding edge case.

## Traceability

| Requirement IDs | Planned Phase |
|---|---|
| SDKM-01..03 | Phase 1 |
| AUTH-01..05, ERR-01..05 | Phase 2 |
| SURF-01..04, READY-01..05, PKG-01..06 | Phase 3 |
| PAGE-01..04, RUN-01..07 | Phase 4 |
| META-01..03, MEDIA-01 | Phase 5 |
