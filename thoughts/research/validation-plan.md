# Validation Plan

## Purpose

This plan defines what must be proven before implementing the first Rust slice.

## Research Questions

1. Which Go routing frameworks should define the first slice?
2. Can simple syntax analysis cover enough real services to be useful?
3. Where does Go semantic analysis become mandatory?
4. Which OpenAPI version should be the default output?
5. How fast must watch-mode generation be to feel instant?
6. What extension API is necessary before framework support becomes unmaintainable?

## Fixture Set

Build or select fixtures in increasing complexity:

- Minimal net/http service.
- Chi service.
- Gin service.
- Gorilla/mux service.
- Service with adapter functions.
- Service with package-separated handlers and models.
- Service with generics or aliases.
- Service with validation tags.
- Service with streaming responses.

Do not implement against only toy examples. Each fixture should preserve a real pattern the tool must support.

## Tooling Comparison

Compare against:

- `swaggo/swag` for code-first annotation workflow.
- `go-swagger` for Swagger 2.0 workflow.
- `oapi-codegen` for spec-first Go generation.
- OpenAPI Generator or another broad SDK generator for multi-language output expectations.

Metrics:

- Required source annotations.
- Generated spec version.
- Generated Go SDK quality.
- Cold runtime.
- Warm no-op runtime.
- Watch support.
- Framework extensibility.
- Failure diagnostics.

## Acceptance Criteria For Starting Implementation

Implementation should not begin until these artifacts exist:

- Feature log with accepted first-slice features.
- Decision log with accepted configuration and lifecycle direction.
- Simplicity guardrails for what is explicitly out of scope.
- Framework route-pattern matrix.
- Type mapping matrix.
- Handler contract inference matrix.
- SDK structure decision.
- `.gnr8/` lifecycle layout decision.
- Minimum language-neutral graph sketch that leaves room for TS, Python, and Rust later.
- OpenAPI compatibility matrix.
- Benchmark fixture list.
- First-slice scope document.

## Known Risks

- Real Go services hide request/response contracts inside handler bodies.
- Latest OpenAPI output may be ahead of downstream SDK tooling.
- Fully correct static analysis may require Go package loading, which can dominate runtime if not cached.
- Plugin APIs can become unstable if designed before enough framework adapters are studied.
- Watch mode can become unreliable if generated outputs are inside watched roots.

## Recommended Next Research Tasks

1. Inventory route patterns for chi, gin, gorilla/mux, echo, fiber, and net/http.
2. Build a Go type mapping matrix covering primitives, aliases, pointers, slices, maps, embedded structs, enums, and validation tags.
3. Decide whether OpenAPI 3.1 or 3.2 should be the default user-facing output.
4. Define benchmark fixtures and target latency budgets.
5. Draft the extension API as a design document before writing macros.
6. Define the "second language" threshold: what evidence justifies adding TS, Python, or Rust as a source or target.
