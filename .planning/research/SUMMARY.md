# Research Summary

**Generated:** 2026-06-24

## Conclusion

`gnr8` should start as a Rust CLI that proves one narrow but meaningful loop:

```text
Go source -> API graph -> OpenAPI -> Go SDK
```

The product should own extraction, graph construction, OpenAPI lowering, SDK generation, diagnostics, lifecycle, and watch-mode behavior. Existing generators are useful comparison points, not the core engine.

## Product Bet

Developers will prefer a fast code-first generator when it:

- Infers API facts from real source code.
- Avoids duplicated comment-driven API definitions.
- Generates useful OpenAPI and SDK outputs.
- Updates quickly on save.
- Explains unsupported cases.
- Lets users customize behavior with code.

## First Build Bias

The PoC should be deliberately small:

- Rust workspace and CLI.
- Realistic Go fixtures.
- Go route/schema/handler extraction for simple supported patterns.
- Stable API graph and inspect reports.
- OpenAPI writer.
- Go SDK writer.
- `.gnr8/` workspace scaffold.
- Incremental generation and watch mode.

## Key Decisions To Carry Forward

- Code is configuration.
- OpenAPI is an artifact, not the internal model.
- Comments are escape hatches.
- Simpler is better.
- Multi-language support is a design constraint, not first implementation scope.
- Rust quality gates and realistic fixture tests are non-negotiable.

## Next Planning Step

Start Phase 1 planning with the GSD command:

```text
$gsd-plan-phase 1
```

