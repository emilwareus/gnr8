# Pitfalls Research

**Generated:** 2026-06-24
**Source material:** `thoughts/DECISION.md`, `thoughts/research/go-static-analysis.md`, `thoughts/research/speed-and-incrementality.md`, `thoughts/research/validation-plan.md`

## Overengineering Risks

- Building a plugin runtime before one vertical slice works.
- Introducing macros before plain Rust customization has failed.
- Designing a universal type system before real frontends require it.
- Adding a graph database before stable IDs and typed structs are insufficient.
- Supporting several languages before Go-to-Go is credible.

Mitigation:

- Keep the PoC to Go source, OpenAPI output, and Go SDK output.
- Generalize only when a second concrete use case creates pressure.

## Static Analysis Risks

Real Go services may hide API contracts behind:

- Arbitrary handler body control flow.
- Adapter functions.
- Reflection.
- Middleware.
- Dynamic route tables.
- Map-based JSON responses.
- Framework-specific context objects.

Mitigation:

- Define explicit inference levels.
- Support typed handlers and simple route calls first.
- Emit useful diagnostics for unresolved cases.
- Use escape-hatch annotations only where source code cannot encode intent.

## Performance Risks

Save-time generation can fail if the system reparses and regenerates everything on each change.

Known risks:

- Package type checking can dominate runtime.
- Generated file changes can trigger recursive watch loops.
- Atomic-save editors can emit duplicate events.
- Monorepos can make naive file discovery expensive.

Mitigation:

- Start with content hashing and unchanged-file write avoidance.
- Add per-file fact caches before complex query-level invalidation.
- Measure cold generation, warm no-op, and single-file edit latency.
- Keep generated outputs outside watched inputs or explicitly ignore them.

## OpenAPI Compatibility Risks

Latest OpenAPI output may be ahead of downstream tool support.

Mitigation:

- Keep OpenAPI as a lowerer target.
- Prefer OpenAPI 3.1 for the PoC unless 3.2 is chosen explicitly.
- Emit compatibility diagnostics when lowering loses information.

## SDK Quality Risks

Poor SDKs are easy to generate and hard to use.

Mitigation:

- Start with a simple idiomatic Go client.
- Keep generated files readable.
- Support custom `http.Client` and base URL early.
- Add typed API errors early.
- Defer retries, pagination helpers, and resource services until the first SDK shape is validated.

