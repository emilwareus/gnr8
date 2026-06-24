# Architecture Research

**Generated:** 2026-06-24
**Source material:** `thoughts/ARCHITECTURE.md`, `thoughts/research/native-go-to-openapi.md`, `thoughts/research/sdk-generation-and-structure.md`

## Core Pipeline

The target architecture is a small owned pipeline:

```text
CLI
  -> workspace loader
  -> source analyzer
  -> API graph builder
  -> output planners
  -> OpenAPI writer
  -> SDK writer
  -> reports and diagnostics
```

The first implementation should keep these as direct modules rather than a generalized plugin platform.

## Internal API Graph

The graph should represent HTTP API facts, not OpenAPI syntax.

Minimum nodes:

- Source file.
- Package.
- Route.
- Handler.
- Operation.
- Parameter.
- Request body.
- Response.
- Schema.
- Generated file.

Minimum edges:

- File declares type.
- Route binds handler.
- Handler consumes schema.
- Handler returns schema.
- Operation uses parameter.
- Output file depends on operation or schema.

Stable IDs and plain typed structs are enough for the PoC.

## Source Frontend Boundary

The Go frontend should extract:

- Package facts.
- Route facts.
- Handler facts.
- Schema facts.
- Source spans.
- Diagnostics.

Language-specific facts should stay attached as provenance or extensions. The shared graph should only model API facts needed by OpenAPI and SDK backends.

## Output Backend Boundary

OpenAPI and SDK generation should read from the graph, not from each other.

This prevents the Go SDK from being constrained by whatever OpenAPI version is selected for compatibility. It also preserves source diagnostics and language facts that OpenAPI cannot represent.

## Future Language Direction

The architecture should leave room for additional frontends and backends without implementing them now:

- TypeScript source via TypeScript Compiler API and framework recognizers.
- Python source via Python AST, type hints, and framework-specific patterns.
- Rust source via Rust semantic tooling and Serde/web framework recognizers.
- TypeScript, Python, and Rust SDK backends.

The guardrail is one working source and one working target first.

