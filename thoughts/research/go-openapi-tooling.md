# Go And OpenAPI Tooling Landscape

## Summary

The current Go API generation ecosystem has useful tools, but they do not provide a single fast, incremental, code-first path from Go code to modern OpenAPI to SDKs.

The main split is:

- Code-first documentation tools often lean on comments and older Swagger/OpenAPI models.
- Spec-first tools generate Go from OpenAPI, but require the OpenAPI document to be the maintained source of truth.
- General-purpose SDK generators are broad, but often template-heavy and slower to adapt to language/framework-specific code facts.

## Swaggo

`swaggo/swag` describes itself as converting Go annotations to Swagger Documentation 2.0.

Source: <https://github.com/swaggo/swag>

Implications:

- It validates demand for Go code-first documentation.
- It relies heavily on comments/annotations, which can drift from source behavior.
- Swagger/OpenAPI 2.0 heritage is a strategic limitation when the target product wants modern OpenAPI output.

Relevant signals:

- There are long-running issues and discussions around OpenAPI 3 support, including historical proposals and recent feature gaps.
- These signals should be treated as evidence of demand and friction, not as final proof of current capability.

Examples:

- OpenAPI 3.0 support request: <https://github.com/swaggo/swag/issues/1898>
- Historical OpenAPI 3.0 proposal: <https://github.com/swaggo/swag/issues/548>
- Recent OpenAPI v3 feature request around discriminators: <https://github.com/swaggo/swag/issues/2150>

## Go-Swagger

`go-swagger/go-swagger` describes itself as a Swagger 2.0, also known as OpenAPI 2.0, implementation for Go.

Source: <https://github.com/go-swagger/go-swagger>

Implications:

- It is mature and useful where Swagger 2.0 is acceptable.
- It does not answer the modern OpenAPI 3.1/3.2 code-first requirement.
- It is not positioned as a save-time incremental generator from application code.

## Oapi-Codegen

`oapi-codegen` converts OpenAPI specifications to Go code, including clients, server implementations, and models.

Source: <https://github.com/oapi-codegen/oapi-codegen>

Implications:

- It validates strong demand for Go code generation from OpenAPI.
- It is primarily spec-first, so it does not remove the burden of maintaining a source OpenAPI document.
- It can be a useful benchmark or compatibility reference for Go SDK/server output quality.

Current release signals show active maintenance and security work, including a June 2026 security release.

Source: <https://github.com/oapi-codegen/oapi-codegen/releases>

## Kin-OpenAPI

`kin-openapi` is a Go library for parsing, writing, converting, and validating OpenAPI documents.

Source: <https://github.com/getkin/kin-openapi>

Implications:

- It may be useful for validating emitted OpenAPI during tests.
- It is not itself a code-first incremental generator.
- It is a good comparison point for OpenAPI model coverage and validation behavior.

## Product Gap

The gap worth testing is not "generate code from OpenAPI". That exists.

The gap is:

```text
real Go service code
  -> static and semantic API facts
  -> modern OpenAPI
  -> Go SDK
  -> fast incremental watch loop
```

The research hypothesis is that a Rust engine can win on orchestration, graph invalidation, and generator extensibility while owning the generation pipeline. Go-specific semantic truth can still come from official Go parsing/type-checking APIs, but `gnr8` should not wrap existing OpenAPI/codegen tools as its core engine.

## Non-Wrapper Rule

Existing tools are research inputs and comparison targets:

- Swaggo demonstrates code-first demand and comment-driven limitations.
- go-swagger demonstrates Swagger/OpenAPI 2.0 maturity.
- oapi-codegen demonstrates spec-first Go generation quality.
- kin-openapi demonstrates OpenAPI parsing and validation patterns.

They should not become the implementation substrate for `gnr8`'s extraction, OpenAPI lowering, or SDK generation.

## Competitive Test Criteria

Before implementation, define benchmarks against existing tools:

- Cold generation time on small, medium, and large Go services.
- Warm no-op generation time.
- Single-file route edit to output update latency.
- Single struct field edit to SDK update latency.
- OpenAPI version support.
- Amount of duplicated annotation text required.
- Extensibility cost for a new router/framework.
