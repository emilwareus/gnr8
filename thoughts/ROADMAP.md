# PoC Roadmap: Go Source To Go SDK

Rough roadmap to get `gnr8` from discovery to a useful proof of concept.

The PoC target is intentionally narrow:

```text
Go source -> API graph -> OpenAPI -> Go SDK
```

The PoC should prove the product loop, not the final platform.

## PoC Definition

The PoC is successful when `gnr8` can:

- Analyze a realistic Go service fixture.
- Infer routes, request schemas, response schemas, and basic path parameters from code.
- Emit an inspectable internal graph report.
- Emit a valid OpenAPI document.
- Emit a usable Go SDK.
- Regenerate on source changes.
- Explain unsupported or uncertain inference with useful diagnostics.

## Non-Goals

Do not build these for the PoC:

- Multi-language source support.
- Multi-language SDK targets.
- Dynamic plugins.
- Macro APIs.
- Full OpenAPI 3.2 coverage.
- Full Go framework coverage.
- Arbitrary handler body interpretation.
- A graph database.
- A polished public SDK.

## Phase 0: Finalize PoC Contract

Goal:
Make the PoC measurable before writing implementation code.

Deliverables:

- Pick the first supported router style.
- Pick the first fixture service.
- Decide OpenAPI output version for the PoC.
- Decide Go SDK shape for the PoC.
- Decide `.gnr8/` workspace layout for the PoC.
- Define exact acceptance tests.

Recommended defaults:

- Router: `net/http` plus one common router, likely `chi`.
- OpenAPI: 3.1 unless 3.2 compatibility is explicitly required for the first demo.
- SDK shape: simple Go client with flat methods first.
- `.gnr8/`: checked-in generator code plus ignored cache/output.

Exit criteria:

- `thoughts/DECISION.md` records these decisions.
- `thoughts/FEATURE.md` marks PoC features as accepted or deferred.
- Fixture list is concrete enough to implement.

## Phase 1: Skeleton Rust Project

Goal:
Create the smallest Rust workspace that can host the CLI and library.

Deliverables:

- `Cargo.toml`.
- `src/main.rs` thin binary.
- `src/lib.rs` with modules.
- Basic CLI parser.
- Rust lint/test setup.

Proposed modules:

```text
cli
workspace
source_go
graph
openapi
sdk_go
diagnostics
report
```

Quality gates:

- `cargo test`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo fmt --check`

Keep it simple:

- No plugin system.
- No async unless needed.
- No dynamic dispatch unless needed.
- No custom build system.

Exit criteria:

- `gnr8 --help` works.
- `gnr8 init --dry-run` or equivalent can describe the intended `.gnr8/` scaffold.

## Phase 2: Fixture Harness

Goal:
Make realistic fixtures the center of development before extraction logic grows.

Deliverables:

- `fixtures/go-nethttp-basic`.
- `fixtures/go-chi-basic` or chosen router fixture.
- Expected route snapshot.
- Expected schema snapshot.
- Expected OpenAPI snapshot.
- Expected SDK snapshot for selected files.

Fixture requirements:

- More than one package if possible.
- At least one path parameter.
- At least one request body.
- At least one response body.
- Structs with JSON tags.
- Optional field.
- Unsupported pattern that produces a diagnostic.

Testing style:

- Snapshot graph reports, OpenAPI, and generated SDK files.
- Keep snapshots scoped and reviewable.
- Use exact assertions for small type mapping functions.

Exit criteria:

- Fixture tests can run before the analyzer is complete and fail with clear missing-output expectations.

## Phase 3: Go Package And Type Extraction

Goal:
Extract enough Go type information for request/response schemas.

Deliverables:

- Go package/file discovery.
- Struct extraction.
- JSON tag extraction.
- Basic type mapping:
  - string
  - bool
  - integer types
  - float types
  - pointers
  - slices
  - maps
  - named structs
  - `time.Time`
- Source span tracking.
- Diagnostics for unsupported fields.

Preferred implementation direction:

- Use official Go semantics where needed.
- Rust owns the pipeline and graph.
- Do not wrap Go OpenAPI generators.

Open implementation question:

- Whether Phase 3 starts with a Rust syntax parser, a Go sidecar using `go/packages`, or a hybrid.

Exit criteria:

- Fixture schemas are extracted.
- Schema snapshot tests pass.
- Unsupported types produce diagnostics, not panics.

## Phase 4: Route And Handler Extraction

Goal:
Connect HTTP routes to handler functions and infer operation contracts.

Deliverables:

- Recognize chosen router calls.
- Extract method and path.
- Resolve handler symbol for simple direct calls.
- Infer path parameters from route template.
- Infer request schema from typed handler parameter.
- Infer response schema from typed handler result.
- Emit diagnostics for unresolved handlers.

Initial inference level:

```go
func CreateUser(ctx context.Context, req CreateUserRequest) (User, error)
func GetUser(ctx context.Context, id string) (User, error)
```

Defer:

- Complex body analysis.
- Middleware semantics.
- Reflection.
- Dynamic route tables.

Exit criteria:

- Fixture route snapshots pass.
- Graph connects routes to schemas.
- Diagnostics explain unsupported handlers.

## Phase 5: Internal API Graph And Inspect Reports

Goal:
Make the inferred model visible and stable.

Deliverables:

- Plain typed graph structs.
- Stable IDs for operations and schemas.
- JSON inspect output.
- `gnr8 inspect routes`.
- `gnr8 inspect schemas`.
- `gnr8 inspect graph`.

Keep it simple:

- No graph database.
- No query engine.
- No generic plugin layer.

Exit criteria:

- Inspect output is deterministic.
- Fixture graph snapshots pass.
- A human can trace route -> handler -> request/response schema.

## Phase 6: OpenAPI Writer

Goal:
Emit a valid OpenAPI artifact from the internal graph.

Deliverables:

- OpenAPI info object.
- Paths and operations.
- Path parameters.
- JSON request bodies.
- JSON responses.
- Components schemas.
- Source/diagnostic extensions if useful.

PoC limits:

- One success response per operation is acceptable.
- Basic error response support is optional.
- Auth can be deferred.
- Streaming can be deferred.

Exit criteria:

- OpenAPI snapshot passes.
- OpenAPI validates with a chosen validator.
- Lossy or unsupported graph facts become diagnostics.

## Phase 7: Go SDK Writer

Goal:
Generate a minimal but usable Go SDK from the graph.

Deliverables:

- `Client` type.
- Base URL configuration.
- Custom `http.Client`.
- One method per operation.
- Request body JSON encoding.
- Response JSON decoding.
- Typed models.
- Typed API error with status/body.

PoC SDK shape:

```text
client.go
models.go
errors.go
```

Defer:

- Resource service grouping.
- Retries.
- Pagination.
- Auth policy.
- Multi-package SDK layout.
- Hand-written extension boundaries.

Exit criteria:

- Generated SDK compiles with `go test`.
- SDK snapshot tests pass.
- A fixture test can call generated methods against an `httptest` server.

## Phase 8: `.gnr8/` Workspace And Code-As-Config

Goal:
Prove the project-local lifecycle without building a full extension platform.

Deliverables:

- `gnr8 init`.
- `.gnr8/` scaffold.
- Local generator entrypoint.
- Minimal code customization hook.
- Ignored cache/output folders.
- Generated-file manifest.

PoC customization hook:

- Select source package roots.
- Select output paths.
- Override naming for one operation or schema.

Keep it simple:

- Avoid a full plugin registry.
- Avoid proc macros.
- Avoid hot-loading user code if a direct compile/run path is enough.

Exit criteria:

- User can initialize a fixture repo.
- User can edit `.gnr8/` code to change one naming rule.
- `generate` respects that change.

## Phase 9: Incremental Generate And Watch

Goal:
Show the save-time promise in a basic form.

Deliverables:

- Content hashing for inputs and `.gnr8/` generator code.
- No-op skip.
- Write only changed output files.
- `gnr8 watch`.
- Basic watch-event debounce.
- Latency report.

PoC measurements:

- Cold generation time.
- Warm no-op time.
- Single route edit to OpenAPI update.
- Single schema field edit to SDK update.

Exit criteria:

- Warm no-op generation is visibly faster than cold generation.
- Watch mode updates fixture outputs after a save.
- Report includes elapsed time and changed outputs.

## Phase 10: Hardening Pass

Goal:
Make the PoC coherent enough to demo and judge honestly.

Deliverables:

- `gnr8 doctor`.
- Better diagnostics.
- Fixture README.
- Known limitations document.
- Benchmark results.
- PoC demo script.

Quality gates:

- Rust tests pass.
- Clippy passes with warnings denied.
- Go SDK fixtures compile.
- Snapshot changes are reviewed.
- No production `unwrap`/`expect`.

Exit criteria:

- Demo can run from a clean checkout.
- Failure modes are understandable.
- Next-phase gaps are explicit.

## PoC Milestones

### Milestone A: Analyze

Includes phases 1-5.

User-visible outcome:

```text
gnr8 inspect graph
```

shows routes, handlers, schemas, and diagnostics from a Go fixture.

### Milestone B: Generate

Includes phases 6-7.

User-visible outcome:

```text
gnr8 generate
go test ./generated/go-sdk
```

produces OpenAPI and a compiling Go SDK.

### Milestone C: Customize And Watch

Includes phases 8-10.

User-visible outcome:

```text
gnr8 init
gnr8 watch
```

uses `.gnr8/` code customization and updates outputs after source changes.

## Risks To Watch

- Handler inference may require Go semantic analysis earlier than expected.
- `.gnr8/` code execution may add lifecycle complexity.
- SDK structure decisions can sprawl.
- OpenAPI validation can distract from graph correctness.
- Watch mode can become flaky if generated outputs are inside watched roots.

## Immediate Next Steps

1. Choose first router fixture: `net/http`, `chi`, or both.
2. Decide PoC OpenAPI target version.
3. Decide PoC SDK shape: flat methods vs resource services.
4. Decide whether Go semantic analysis starts as a sidecar.
5. Draft the exact fixture repository layout.
6. Convert Phase 0 decisions into `thoughts/DECISION.md`.
