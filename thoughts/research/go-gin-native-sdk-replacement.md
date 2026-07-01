# Native Go/Gin Contract Extraction Research

Date: 2026-07-01

## Goal

Make `gnr8` replace Swaggo/openapi-generator for Gin services from native Go/Gin source only:

- source of truth is Go route registration plus handler code
- no Swagger/OpenAPI source input or OpenAPI-generator pass for SDKs
- no Swagger UI hosting
- no inferred behavior beyond the API already exposed by the service

The existing architecture already fits this direction: `GoGin` loads Go source into a language-neutral `ApiGraph`, and `GoSdk`/`TsSdk` generate directly from that graph. OpenAPI lowering is an optional target, not an SDK-generation intermediate.

## Current Pipeline Shape

Relevant implementation points:

- Go source extraction: `goextract/internal/routes/routes.go`, `goextract/internal/handlers/handlers.go`, `goextract/internal/types/extract.go`
- host facts/IR: `crates/gnr8-core/src/analyze/facts.rs`, `crates/gnr8-core/src/graph/mod.rs`
- OpenAPI lowering: `crates/gnr8-core/src/lower/*`
- SDK emitters: `crates/gnr8-core/src/gosdk/*`, `crates/gnr8-core/src/tssdk/*`
- public pipeline: `crates/gnr8-core/src/sdk/builtins.rs`, `crates/gnr8-core/tests/sdk_pipeline.rs`

Validation commands run during this research:

```sh
cd goextract && go test ./internal/routes ./internal/handlers ./internal/types
cargo test -p gnr8 pipeline_emits_openapi_and_sdk_artifacts_with_key_facts --test sdk_pipeline -- --nocapture
cargo test -p gnr8 lower:: --lib -- --nocapture
cargo test -p gnr8 tssdk::emit::tests::operations --lib -- --nocapture
cargo test -p gnr8 gosdk::emit::tests::operations --lib -- --nocapture
```

All passed.

## Findings By Requirement

### 1. Path Params From Gin Routes

Current state: partial support.

The route recognizer already normalizes Gin `:param` tokens to OpenAPI-style `{param}` in route paths. SDK emitters already validate that path template tokens exactly match declared path params. That validation is correct, but today declared path params come only from handler `c.Param("name")` calls.

Gap: helper-based parsing such as `parsePathUUID(c, "itemId")` does not register `itemId`, so SDK generation can fail with `templated tokens ... do not match its path params`.

Recommended support:

- Add route-template param extraction in `buildRoutes` or a shared route helper.
- For every `{name}` in `Route.Path`, ensure a required path param exists.
- Preserve handler-derived params when present; synthesize missing route-template params as `string`, `required=true`.
- Use route registration span for synthesized params.

This is extractor-only and does not require an IR change.

### 2. PATCH Routes

Current state: mixed.

Already supported:

- `goextract/internal/routes/routes.go` includes `PATCH` in `httpMethods`.
- TS/Go SDK dispatch paths use `op.method` directly, so a graph operation with `PATCH` will emit HTTP PATCH.

Gap:

- OpenAPI lowering `PathItem` has only `get`, `post`, `put`, `delete`.
- `place_operation` rejects any method outside those four.
- YAML writer only emits those four methods.

Recommended support:

- Add `patch: Option<Operation>` to `lower::model::PathItem`.
- Add `PATCH` to `place_operation`.
- Add `patch` to YAML/JSON OpenAPI writers in canonical method order.
- Add tests at route, OpenAPI, TS SDK, and Go SDK levels.

### 3. No-Content Responses

Current state: graph/lowering/SDKs can represent it, extractor cannot infer the requested patterns yet.

Already supported:

- `Response.body: Option<SchemaRef>` can represent `body: null`.
- OpenAPI writer omits `content` for body-less responses.
- Go SDK has tests for body-less 2xx success, including 204.
- TS SDK handles body-less success as `Promise<void>`.

Gap:

- Handler analyzer only recognizes `c.JSON(...)` response calls.
- It does not currently infer `c.Status(http.StatusNoContent)` or `c.AbortWithStatus(http.StatusNoContent)`.

Recommended support:

- Add handler cases for Gin `Status` and `AbortWithStatus`.
- Resolve the status with existing `statusOf`.
- Append `ResponseFact{Status: status, Body: nil}`.
- Deduplicate with the existing `seenStatus` policy.

This is extractor-only.

### 4. Array JSON Responses

Current state: IR type vocabulary supports arrays, but response facts only point to named schema refs.

Already supported:

- `facts.Type`, `graph::Type`, OpenAPI lowering, TS models, and Go models all support named array schema aliases.
- OpenAPI lowering has a passing test for named schema array bodies.

Gap:

- `analyzeJSON` resolves response bodies only through `namedTypeID`.
- A value of type `[]SavedViewResponse` is a slice, not a named type, so today it becomes a body-less response plus a dynamic-response diagnostic.

Recommended support:

- Do not add a broad response-shape change unless exact inline response schemas are required.
- Minimal approach: synthesize a response schema alias per operation/status for anonymous array responses:
  - ID: `__synthetic.<Handler><Status>Response`
  - Name: `<Handler><Status>Response` or exported handler-based equivalent
  - Body: `Type::Array(Box::new(Type::Named("SavedViewResponse id")))`
  - Response body: ref to that synthetic schema
- Extend `CodeFacts.Schemas` reuse path, already used for synthetic form schemas.

This keeps SDK generation simple: TS emits `type ListSavedViews200Response = SavedViewResponse[]`; Go emits `type ListSavedViews200Response = []SavedViewResponse`.

If the graph JSON must literally show an inline array under `response.body`, then `Response.body` must change from `Option<SchemaRef>` to `Option<Type>`, which is a larger host/SDK/OpenAPI contract change. The synthetic schema route is lower risk.

### 5. File/Binary Responses

Current state: not supported end to end.

Gaps:

- Handler analyzer does not recognize `c.FileAttachment`, `c.File`, `c.Data`, or response `Content-Type` headers.
- `ResponseFact` carries only status and body ref; it does not carry media type or response kind.
- TS SDK assumes success bodies are JSON and returns typed models or `void`.
- Go SDK assumes success bodies are JSON and decodes with `json.NewDecoder`.

Recommended support:

- Add a response media/kind field to facts and graph, for example:
  - `content_type: Option<String>`
  - `body: Option<ResponseBody>`, where `ResponseBody` can be `Schema(TypeRef/Type)` or `Binary`
- Recognize:
  - `c.FileAttachment(path, filename)` as 200 binary, default media type `application/octet-stream`
  - `c.File(path)` as 200 binary, media type unknown or `application/octet-stream`
  - `c.Data(status, contentType, []byte)` as binary when payload is `[]byte`
  - `c.Header("Content-Type", "...")` as a media hint for following file/data response in the handler
- OpenAPI lowering should emit `content: <media-type>: schema: { type: string, format: binary }`.
- TS SDK should return `Blob` in browser/fetch-compatible mode. If Node compatibility is a target, make the response policy explicit.
- Go SDK should return `[]byte` or `io.ReadCloser`; for parity with current dependency-light design, `[]byte` is simpler and testable.

This is the largest change because it affects extraction, graph schema, OpenAPI, and both SDK emitters.

### 6. `gin.H` Responses

Current state: likely failing as a dangling external ref.

Why: `gin.H` is a named external type. `analyzeJSON` can produce a ref like `github.com/gin-gonic/gin.H`, but schema extraction intentionally only emits target-module schemas. OpenAPI/SDK lowering then treats it as dangling.

Recommended support:

- Special-case `github.com/gin-gonic/gin.H` in `analyzeJSON`.
- If the expression is a composite literal with simple string keys and scalar literal values, synthesize an inline object schema:
  - fields from literal keys
  - primitive field types from literal values
  - optional/required policy: required for literal keys present in the response shape
- If keys or values are dynamic, synthesize a free-form object schema using `Type::Any` or `Type::Map { string -> Any }`.
- Never emit a `gin.H` named ref.

This can reuse the synthetic schema path recommended for arrays.

### 7. Operation Grouping

Current state: partial and profile-dependent.

Already supported:

- Gin static groups produce `Route.Group` from the deepest static route prefix.
- Graph operations preserve `group`.
- OpenAPI lowering emits tags from `op.group`.
- TS axios-compatible output groups operations into classes via `grouped_operations`.
- Split SDK layouts can use `{service}` file templates.

Gaps:

- Compact Go SDK intentionally emits one `operations.go` with methods on `Client`; it does not produce `AuthApi`/`FilesApi` groups by default.
- Compact TS fetch SDK emits one `Client` class by default.
- `GroupOperations` is deterministic but clears source-derived groups before applying configured rules.

Recommended support:

- Treat source-derived Gin groups as the default operation grouping.
- Keep `GroupOperations` as an override, but consider preserving existing groups when no rule matches.
- Add a grouped surface validation using either:
  - TS axios compatibility profile, which already emits grouped API classes, or
  - split file layout with `operation_file_template("{service_kebab}/{operation_kebab}.ts")`
- For Go, decide whether grouped services are required in the minimal profile or only in the OpenAPI-generator compatibility profile. The compatibility client surface already has grouped-service concepts and should be the lowest-risk path.

## Regression Fixture Plan

Add one fixture, for example `fixtures/gin-contract-regression`, containing:

- route groups: `/v1/auth`, `/v1/files`, `/v1/items`
- route-template params: `/v1/items/:itemId/children/:childId`
- helper-based param parsing for `itemId`
- direct `c.Param("childId")`
- PATCH route with `ShouldBindJSON`
- `c.Status(http.StatusNoContent)`
- `c.AbortWithStatus(http.StatusNoContent)`
- array JSON response `[]SavedViewResponse`
- file responses via `FileAttachment`, `File`, and `Data`
- `gin.H` response

Use a local `replace github.com/gin-gonic/gin => ./ginstub` for fast deterministic tests if full Gin runtime behavior is not needed. The stub must include every method signature the semantic analyzer resolves.

Required public validation test:

```rust
Pipeline::new()
    .source(GoGin::new().inputs(["."]))
    .target(TsSdk::new().module("@example/sdk").to("generated/ts"))
    .target(GoSdk::new().module("example.com/sdk").to("generated/go"))
```

Assertions:

- pipeline succeeds
- generated TS contains `method: "PATCH"`
- generated Go contains `http.NewRequestWithContext(..., "PATCH", ...)`
- no path-token mismatch error for helper-derived params
- 204 responses have no JSON decode
- array response returns the generated array alias or direct array type
- binary response returns `Blob`/`[]byte` and does not JSON-decode
- no generated artifact references `gin.H`
- grouped output does not collapse all operations to `default`

## Implementation Order

1. Path params from route templates.
2. No-content responses.
3. PATCH OpenAPI lowering.
4. Synthetic response schemas for arrays and `gin.H`.
5. Binary/file response IR and SDK changes.
6. Grouping policy/profile validation.
7. Full regression fixture and public pipeline test.

This order front-loads extractor-only fixes, then handles the broader IR/SDK work for binary responses after the easy graph-compatible cases are green.

## Risk Summary

Lowest risk:

- route-template params
- `Status`/`AbortWithStatus`
- PATCH lowering

Medium risk:

- array JSON responses through synthetic schemas
- `gin.H` literal/free-form object handling
- preserving source groups while allowing deterministic overrides

Highest risk:

- file/binary responses, because the current response model assumes JSON/schema-ref responses and both SDKs currently decode success bodies as JSON.
