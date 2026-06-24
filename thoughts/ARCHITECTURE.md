# Target Architecture

Discovery-phase architecture for `gnr8`.

This document is intentionally concrete but not an implementation plan. The goal is to keep the first build small while making sure the shape can later support more source languages and SDK targets.

## Principles

1. Simpler is better.
2. Own the core pipeline.
3. Code is configuration.
4. OpenAPI is an artifact, not the internal model.
5. Comments are escape hatches, not the normal source of truth.
6. One vertical slice before generalized abstractions.
7. Realistic fixtures drive design.
8. Measure before optimizing.

## First Vertical Slice

The first implementation should prove exactly this path:

```text
Go source -> API graph -> OpenAPI -> Go SDK
```

Scope:

- One Go source frontend.
- One or two Go router families.
- One Go SDK backend.
- One `.gnr8/` code-as-config workspace.
- One OpenAPI output path.
- Watch mode with measured latency.

Not in the first slice:

- Dynamic plugins.
- Macro-heavy extension APIs.
- A graph database.
- Universal source-language abstractions.
- Multiple SDK languages.
- Full framework coverage.
- Full OpenAPI 3.2 feature coverage.

## System Shape

```text
CLI
  -> project workspace loader
  -> source analyzer
  -> API graph builder
  -> output planners
  -> OpenAPI writer
  -> SDK writer
  -> reports and diagnostics
```

### CLI

The CLI is the primary user experience.

Candidate commands:

```text
gnr8 init
gnr8 generate
gnr8 watch
gnr8 check
gnr8 inspect routes
gnr8 inspect schemas
gnr8 inspect graph
gnr8 new route-recognizer <name>
gnr8 new sdk <target>
gnr8 cache status
gnr8 cache clean
gnr8 doctor
```

Command responsibilities:

- `init`: scaffold `.gnr8/` with user-owned Rust code and ignored lifecycle folders.
- `generate`: run source analysis, update graph, emit OpenAPI/SDK outputs.
- `watch`: keep state warm and regenerate on save.
- `check`: verify generated outputs are current without writing them.
- `inspect`: show what was inferred and why.
- `doctor`: report unsupported route patterns, stale state, and lifecycle issues.

### `.gnr8/` Workspace

The project-local workspace should hold user-owned generator code and lifecycle state.

Candidate layout:

```text
.gnr8/
  generator/
    Cargo.toml
    src/
      main.rs
      routes.rs
      schemas.rs
      sdk_go.rs
  tests/
    fixtures/
    snapshots/
  cache/
  output/
    latest.json
    diagnostics.json
  .gitignore
```

Checked in:

- Generator source code.
- Fixture inputs.
- Approved snapshots.
- Maybe a generator lock file, if needed.

Ignored:

- Cache.
- Last-run output reports.
- Local build artifacts.

Open question:

- Whether generated OpenAPI/SDK outputs live inside `.gnr8/generated/` or in user-chosen project paths with a manifest.

### Source Analyzer

The first source analyzer is Go.

Responsibilities:

- Discover packages and files.
- Extract route facts.
- Extract handler facts.
- Extract schema/type facts.
- Attach source spans.
- Report uncertain inference.

Native does not mean reimplementing Go's compiler in Rust. Go parser/type-checker APIs and `go/packages` are acceptable because they provide language semantics, not generation behavior.

The analyzer should prefer code facts:

- Function signatures.
- Struct declarations.
- JSON and validation tags.
- Route registration calls.
- Handler body decode/encode calls where simple.

Comments should only fill gaps that code cannot express.

### API Graph

The API graph is the internal model.

Minimum node types:

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
- Output file depends on operation/schema.

Keep this graph plain at first. A set of typed structs and stable IDs is enough until a real need for more appears.

### OpenAPI Lowerer

The lowerer converts the API graph into OpenAPI.

Initial target:

- Emit one modern OpenAPI version selected during research, likely 3.1 or 3.2.

Compatibility behavior:

- If a target version cannot represent a graph fact, emit a diagnostic.
- Do not shape the graph around OpenAPI version quirks.

### SDK Backend

The first backend is Go SDK generation.

Candidate structure:

```text
client.go
option.go
error.go
transport.go
models/
operations/
```

Research decision still needed:

- Flat client methods vs resource services.
- Models in same package vs subpackage.
- Retry defaults.
- Pagination shape.
- Error type shape.
- Generated-file ownership model.

## Multi-Language Direction

The architecture should leave room for more source and target languages without building them now.

Likely future source frontends:

- TypeScript: TypeScript Compiler API and framework recognizers.
- Python: Python `ast`, type hints, framework-specific extraction.
- Rust: rust-analyzer or rustc-derived semantic facts for macro-heavy frameworks.

Likely future SDK targets:

- TypeScript.
- Python.
- Rust.

Design rule:

- The core graph should model HTTP API facts.
- Language-specific source facts should stay attached as provenance/extensions.
- SDK-specific behavior belongs in SDK backends.

## Rust Implementation Guardrails

The repo vendors Apollo's `rust-best-practices` skill under `thoughts/skills/rust-best-practices`.

Implementation rules derived from that skill:

- Prefer borrowing over cloning.
- Accept `&str`, `&[T]`, and borrowed views where ownership is not needed.
- Return `Result` for fallible work; no `unwrap`/`expect` outside tests.
- Use `thiserror` for library errors.
- Use `anyhow` only at binary boundaries.
- Run `cargo clippy --all-targets --all-features --locked -- -D warnings`.
- Use `#[expect(...)]` with justification instead of broad `#[allow(...)]`.
- Benchmark with release builds before optimizing.
- Prefer static dispatch until runtime heterogeneity is required.
- Avoid type-state patterns unless they prevent real invalid states.
- Prefer public API docs and doc tests over explanatory comments.
- Keep pointer/concurrency choices explicit: `Arc` only for real shared ownership, `Mutex/RwLock` only for real shared mutation.

## Testing Strategy

Testing should prove extraction and generation against realistic fixtures, not just unit-sized snippets.

### Unit Tests

Purpose:

- Small parser helpers.
- Type mapping.
- Path template normalization.
- Operation ID generation.
- Diagnostic construction.
- OpenAPI lowering edge cases.

Style:

- Descriptive test names.
- One behavior per test.
- Exact assertions for small data.
- Error paths tested directly.

### Fixture Integration Tests

Fixtures should look like real repositories.

Initial fixture set:

```text
fixtures/
  go-nethttp-basic/
  go-chi-basic/
  go-gin-basic/
  go-gorilla-methods/
  go-adapter-functions/
  go-package-separated-handlers/
  go-validation-tags/
  go-generics-and-aliases/
  go-streaming-response/
```

Each fixture should include:

- Source service code.
- Expected inferred route list.
- Expected schema graph.
- Expected diagnostics.
- Expected OpenAPI output.
- Expected SDK shape or selected SDK files.

### Snapshot Tests

Use snapshots for:

- OpenAPI output.
- SDK generated files.
- Graph inspection JSON.
- CLI `inspect` output.

Keep snapshots small:

- Snapshot one output file or one graph section at a time.
- Redact unstable fields.
- Avoid giant "whole world" snapshots unless the fixture is intentionally tiny.

### CLI Tests

CLI tests should cover:

- `init` creates expected `.gnr8/` structure.
- `generate` writes expected outputs.
- `check` fails on stale outputs.
- `watch` can be tested with a bounded event harness.
- `inspect` returns stable machine-readable JSON.
- `doctor` reports unsupported patterns.

### Performance Tests

Benchmarks must measure the promise:

- Cold generation time.
- Warm no-op time.
- Watch startup time.
- Save-to-output latency.
- Single route edit latency.
- Single schema field edit latency.
- SDK-only invalidation latency.

Rules:

- Run performance tests with release builds.
- Do not optimize without measurements.
- Record fixture size and machine context.

### Regression Tests

Every supported framework pattern should have:

- A positive fixture.
- A negative/unsupported fixture with useful diagnostics.
- A fixture for adapter/wrapper behavior if supported.

## Diagnostics And Inspectability

The tool must explain what it inferred.

Diagnostics should answer:

- Which route was found?
- Which handler was attached?
- Which request schema was inferred?
- Which response schema was inferred?
- Which facts were uncertain?
- Which OpenAPI facts were lossy for the selected version?
- Which SDK files changed and why?

Machine-readable reports should live under `.gnr8/output/`.

## Incrementality

Start simple:

- Content hash source files.
- Skip no-op generation.
- Write only changed output files.

Then add:

- Per-file fact cache.
- API graph dependencies.
- Output dependency tracking.

Avoid:

- A graph database.
- Query engine complexity before fixtures prove the need.

## First Implementation Readiness Checklist

Do not implement until these are accepted:

- First router/framework fixture chosen.
- Type mapping matrix drafted.
- Handler inference levels scoped.
- OpenAPI default version decided.
- Go SDK structure decided.
- `.gnr8/` layout decided.
- Fixture list committed.
- CLI command shape accepted.
- Rust crate quality gates accepted.
