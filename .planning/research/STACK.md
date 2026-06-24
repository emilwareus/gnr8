# Stack Research

**Generated:** 2026-06-24
**Source material:** `thoughts/ARCHITECTURE.md`, `thoughts/DECISION.md`, `thoughts/research/*`

## Target Stack

`gnr8` should be a Rust CLI and library that owns the code-to-API-graph-to-output pipeline.

First vertical slice:

```text
Go source -> API graph -> OpenAPI -> Go SDK
```

The implementation stack should stay intentionally small:

- Rust binary for CLI orchestration.
- Rust library modules for workspace loading, diagnostics, API graph, OpenAPI lowering, and Go SDK generation.
- Go semantic tooling where source truth requires it, especially package loading and type checking.
- Realistic Go fixtures as the main validation input.
- Snapshot and integration tests for generated reports, OpenAPI, and SDK files.

## Go Analysis Stack

Native does not mean reimplementing Go's compiler in Rust. It means owning the extraction model and product workflow.

Allowed as semantic inputs:

- Go parser and package/type-checking APIs.
- `golang.org/x/tools/go/packages` style package loading.
- `gopls` or related official Go tooling as research references.

Not allowed as core implementation:

- Swaggo as the OpenAPI engine.
- oapi-codegen as the SDK engine.
- OpenAPI Generator as the SDK engine.
- Comment blocks as the normal source of truth.

## OpenAPI Stack

OpenAPI is an output artifact, not the internal model.

The first implementation should likely emit OpenAPI 3.1 by default unless the PoC contract explicitly chooses 3.2. The API graph should retain richer source facts and emit diagnostics when lowering to a specific OpenAPI version loses information.

## SDK Stack

The first SDK target is Go.

The SDK generator should own:

- Client shape.
- Models.
- Operation methods.
- Request/response encoding.
- Error type.
- Transport customization.
- Generated-file ownership.

External generators may be used as quality references, not as the generation backend.

## Rust Guardrails

Future implementation should follow the vendored `thoughts/skills/rust-best-practices/` guidance:

- Prefer borrowing over unnecessary cloning.
- Use typed library errors, likely `thiserror`.
- Use `anyhow` only at the binary boundary.
- Avoid `unwrap` and `expect` in production paths.
- Run `cargo fmt`, `cargo test`, and clippy with warnings denied.
- Benchmark before adding complex performance machinery.

