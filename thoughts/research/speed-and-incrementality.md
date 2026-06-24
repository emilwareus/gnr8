# Speed And Incrementality

## Performance Promise

The user-facing promise is not just "faster than existing tools". It is:

> Save a file and see OpenAPI and SDK outputs update almost immediately.

That requires measuring watch-event-to-output latency, not only total CLI runtime.

## Why Rust

Rust is a good fit for:

- Long-running watch services.
- Low-overhead graph management.
- Parallel parsing and generation.
- Deterministic file emission.
- Native plugin APIs.

Rust should not be used to reimplement Go semantics from scratch. The architecture should use Go-native semantic tooling where correctness requires it.

The customization and lifecycle implications are tracked separately:

- [Code-as-config and CLI UX](code-as-config-and-ux.md)
- [Generation lifecycle](generation-lifecycle.md)

## Incremental Layers

### Layer 1: Content Hashing

Hash files and config to skip no-op generation.

Pros:

- Simple.
- Good baseline.
- Easy to test.

Cons:

- Does not isolate which graph facts changed.
- Still too coarse for the final save-time promise.

### Layer 2: Per-File Fact Cache

Parse changed files and persist extracted facts per file.

Pros:

- Avoids reparsing the world.
- Enables diagnostics by source file.

Cons:

- Cross-file dependencies still need graph invalidation.

### Layer 3: API Graph Invalidation

Represent route, handler, schema, and output dependencies as graph edges.

A changed struct field should invalidate:

- That schema node.
- Operations that reference that schema.
- OpenAPI components and paths using that schema.
- SDK model files and methods that depend on that schema.

It should not invalidate unrelated operations.

### Layer 4: Incremental Parsing

In long-running watch mode, use parser edit deltas where available so syntax trees are updated rather than rebuilt.

Tree-sitter is designed for incremental parsing and can update syntax trees as source edits occur.

Sources:

- <https://tree-sitter.github.io/>
- <https://github.com/tree-sitter/tree-sitter>

### Layer 5: Query-Level Invalidation

Salsa is a Rust framework for efficient incremental recomputation, inspired by compiler query systems and used in compiler-like tooling.

Sources:

- <https://salsa-rs.github.io/salsa/overview.html>
- <https://github.com/salsa-rs/salsa>

Open question:

- Use Salsa directly, or implement a smaller graph invalidation layer first and migrate only if the query model becomes complex.

## File Watching

Rust `notify` provides cross-platform filesystem notifications. Its docs point users to debouncer crates when debounced events are desired.

Source: <https://docs.rs/notify/latest/notify/>

Watch mode must account for:

- Multiple events per save.
- Atomic-save editors.
- Rename events.
- Generated files causing recursive watch loops.
- Monorepo root size.
- Platform-specific filesystem behavior.

## Benchmarks Required

Before implementation, define a benchmark harness with these metrics:

- Cold CLI generation.
- Warm CLI no-op.
- Watch startup.
- Watch no-op save.
- Single route edit to OpenAPI path update.
- Single struct field edit to OpenAPI schema update.
- Single struct field edit to Go SDK model update.
- Large repository discovery time.
- Semantic analysis cache hit and miss time.

Target thresholds should be explicit before coding.

Initial working targets:

- Warm no-op: under 50 ms for small projects.
- Single-file syntax-only change: under 100 ms for small projects.
- Medium project watch update: under 250 ms.
- Large project watch update: budget to be set after fixture selection.

These are hypotheses, not commitments.
