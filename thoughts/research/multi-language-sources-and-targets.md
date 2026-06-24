# Multi-Language Sources And Targets

## Research Question

How can `gnr8` support more source languages and SDK target languages without overengineering the first version?

Target languages to account for early:

- Source frontends: Go first, then TypeScript, Python, Rust.
- SDK targets: Go first, then TypeScript, Python, Rust.

## Position

Design for multiple languages, but do not build a multi-language platform first.

The first implementation should prove one vertical path:

```text
Go source -> API graph -> OpenAPI -> Go SDK
```

The research should still make sure that path does not bake in Go-only assumptions that would make TypeScript, Python, or Rust impossible later.

## Simplicity Guardrails

Avoid these until repeated use cases justify them:

- Dynamic plugin loading.
- Macro-heavy extension APIs.
- A graph database.
- A full compiler framework.
- A universal type system that tries to model every language perfectly.
- Multiple source frontends before one source frontend is credible.
- Multiple SDK targets before one SDK backend feels excellent.

Prefer:

- A small internal API graph.
- Plain data structures.
- Direct source frontend modules.
- Direct SDK backend modules.
- Explicit compatibility gaps.
- Concrete fixtures.

## Language-Neutral Core

The core model should stay small:

- Operation.
- Path.
- Method.
- Parameters.
- Request body.
- Response variants.
- Schema.
- Auth requirement.
- Source span.
- Diagnostics.

The model should avoid storing language syntax directly. Language-specific facts can live beside the core graph as provenance or extensions.

Example:

```text
schema User
  field id: string
  source: go package ./internal/api, type User
```

Later:

```text
schema User
  field id: string
  source: ts file src/api.ts, interface User
```

The core schema should be the same. The source provenance differs.

## Source Frontend Matrix

| Source | Native analysis path | Strength | Risk |
| --- | --- | --- | --- |
| Go | Go parser/type checker, `go/packages`, optional syntax fast path | Strong package/type model, common backend target | Framework handler contracts are often implicit in function bodies |
| TypeScript | TypeScript Compiler API `Program` and `TypeChecker` | Rich type information for interfaces, handlers, framework adapters | Runtime routing can be highly dynamic |
| Python | Standard `ast` module, optional framework-specific introspection | Easy syntax parsing, strong FastAPI/Pydantic conventions | Dynamic typing makes semantic inference weaker outside typed frameworks |
| Rust | rust-analyzer or rustc-derived analysis, syntax parsing | Strong static types and macro-heavy web frameworks | Macros make route extraction difficult without deeper compiler integration |

Sources:

- TypeScript Compiler API: <https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API>
- Python `ast`: <https://docs.python.org/3/library/ast.html>
- rust-analyzer: <https://rust-analyzer.github.io/>
- Rust compiler development guide: <https://rustc-dev-guide.rust-lang.org/about-this-guide.html>
- Go `go/packages`: <https://pkg.go.dev/golang.org/x/tools/go/packages>

## TypeScript Source Frontend

Candidate frameworks:

- Express.
- Fastify.
- Hono.
- NestJS.
- Next.js route handlers.

Analysis needs:

- Parse source files.
- Resolve handler symbols.
- Use TypeChecker for request/response DTO types.
- Understand framework-specific route registration.
- Support schema libraries such as Zod later, but do not require them as the only path.

Do not overbuild:

- Start with one framework and one idiom.
- Avoid supporting every decorator framework up front.
- Avoid writing a TypeScript type checker; use TypeScript's own compiler API.

## Python Source Frontend

Candidate frameworks:

- FastAPI.
- Flask.
- Django REST Framework.

Analysis needs:

- Parse Python AST.
- Recognize decorators and route registration calls.
- Extract type hints.
- Understand Pydantic models where present.
- Fall back to explicit code configuration for dynamic cases.

Do not overbuild:

- Start with typed FastAPI-like functions if Python is added.
- Do not pretend arbitrary Flask code has statically knowable request/response schemas.

## Rust Source Frontend

Candidate frameworks:

- Axum.
- Actix Web.
- Poem.

Analysis needs:

- Resolve route macros/builders.
- Resolve handler function types.
- Map Serde structs to schemas.
- Understand extractor and response types.

Risk:

Rust web frameworks often use macros and trait-heavy response types. A syntax-only parser may not be enough. rust-analyzer or rustc-derived semantic data may be needed earlier than for Go.

Do not overbuild:

- Treat Rust source support as later research until the Go path proves the graph model.

## SDK Target Matrix

| Target SDK | Default shape to research | First concern |
| --- | --- | --- |
| Go | Client struct with options, resource services or flat methods, typed models | Idiomatic `context.Context`, `http.Client`, errors, pagination |
| TypeScript | Typed client with fetch transport, ESM-first package, optional runtime validators | Browser vs Node, tree-shaking, async ergonomics |
| Python | Sync/async clients, typed models, context managers | Pydantic vs dataclasses, httpx/aiohttp dependency tradeoff |
| Rust | Typed client with reqwest-like transport, builders, serde models | Async runtime compatibility, error types, feature flags |

## Minimal Abstraction Boundary

Do not start with a broad plugin system. Start with simple traits only after the second frontend or backend creates real duplication.

Possible future boundary:

```text
SourceFrontend -> emits SourceFacts
GraphBuilder -> normalizes facts into API graph
SdkBackend -> emits files from API graph
```

For now, this is a research shape, not an implementation requirement.

## First-Slice Rule

The first implementation must remain:

- One source language.
- One source framework or very small framework set.
- One SDK target.
- One OpenAPI output path.
- One code-as-config workspace.

Multi-language support should influence names and data boundaries, not expand the first milestone.

## Research Tasks

1. Pick the first Go framework pattern.
2. Define the minimum language-neutral API graph.
3. Identify Go-specific fields that should stay out of the core graph.
4. Compare TS, Python, and Rust source extraction risks.
5. Compare TS, Python, and Rust SDK target structure.
6. Decide what must be proven before adding a second source language.
