# Requirements: gnr8

**Defined:** 2026-06-24
**Core Value:** Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.

## v1 Requirements

### PoC Contract

- [x] **POC-01**: The PoC scope is locked to Go source, OpenAPI output, and Go SDK output.
- [x] **POC-02**: The first supported router set, OpenAPI target version, Go SDK shape, and `.gnr8/` layout are documented before implementation expands.
- [x] **POC-03**: Explicit non-goals prevent dynamic plugins, macro-heavy APIs, graph databases, full framework coverage, and multi-language implementation from entering the PoC.

### Rust Foundation

- [x] **RUST-01**: The repo has a minimal Rust workspace with a thin CLI binary and library modules.
- [x] **RUST-02**: The CLI exposes initial `init`, `generate`, `watch`, `check`, `inspect`, and `doctor` command surfaces, even if early commands are skeletal.
- [x] **RUST-03**: The codebase passes `cargo fmt`, `cargo test`, and clippy with warnings denied.
- [x] **RUST-04**: Library code uses typed errors and avoids production `unwrap` or `expect` paths.

### Fixtures And Validation

- [x] **FIX-01**: Realistic Go service fixtures exist for the selected router patterns.
- [x] **FIX-02**: Fixtures cover path parameters, request bodies, response bodies, JSON tags, optional fields, package boundaries, and at least one unsupported pattern.
- [x] **FIX-03**: Snapshot tests cover graph reports, OpenAPI output, Go SDK output, and diagnostics.
- [x] **FIX-04**: Fixture tests fail clearly before unsupported behavior is implemented.

### Go Analysis

- [x] **GO-01**: The analyzer discovers Go packages and source files for configured inputs.
- [x] **GO-02**: The analyzer extracts structs, fields, JSON tags, source spans, and basic schema facts.
- [x] **GO-03**: The analyzer maps common Go types, including primitives, pointers, slices, maps, named structs, aliases, and `time.Time`.
- [x] **GO-04**: The analyzer recognizes the selected router call patterns and extracts method, path, router family, handler symbol, and source span.
- [x] **GO-05**: The analyzer infers request and response schemas for supported typed handler patterns.
- [x] **GO-06**: Unsupported or uncertain inference produces diagnostics instead of panics or silent omissions.

### API Graph And Inspectability

- [x] **GRAPH-01**: The internal graph models routes, operations, parameters, request bodies, responses, schemas, generated files, and source provenance.
- [x] **GRAPH-02**: Graph node IDs and generated outputs are stable across unchanged runs.
- [x] **GRAPH-03**: `inspect routes`, `inspect schemas`, and `inspect graph` explain inferred facts and diagnostics.

### OpenAPI Output

- [x] **OAPI-01**: The OpenAPI writer emits a valid document for the fixture service.
- [x] **OAPI-02**: The document includes info, paths, operations, parameters, request bodies, responses, and component schemas.
- [x] **OAPI-03**: Lowering emits diagnostics when the selected OpenAPI target cannot represent a graph fact cleanly.

### Go SDK Output

- [x] **SDK-01**: The Go SDK includes a usable client with base URL and custom `http.Client` support.
- [x] **SDK-02**: The SDK exposes typed methods for generated operations.
- [x] **SDK-03**: The SDK includes generated request and response models.
- [x] **SDK-04**: The SDK handles JSON encoding/decoding and typed API errors.
- [x] **SDK-05**: The generated SDK compiles and is exercised by fixture tests.

### Workspace And Lifecycle

- [x] **WS-01**: `gnr8 init` scaffolds a project-local `.gnr8/` workspace with user-owned generator code.
- [x] **WS-02**: `.gnr8/` separates checked-in customization from ignored cache/output lifecycle files.
- [x] **WS-03**: Users can customize source inputs, routing recognition, OpenAPI output, SDK output, naming, and transport behavior through code.
- [x] **WS-04**: Generated-file ownership is tracked well enough to avoid clobbering user-owned files silently.

### Speed And Watch Mode

- [x] **WATCH-01**: No-op generation avoids rewriting unchanged outputs.
- [x] **WATCH-02**: Watch mode reacts to source changes, debounces duplicate events, and avoids loops from generated files.
- [x] **WATCH-03**: The PoC reports cold generation, warm no-op, and single-file edit latency for fixture services.

### Hardening And Demo

- [ ] **HARD-01**: `doctor` or equivalent diagnostics summarize unsupported route patterns, stale outputs, and lifecycle issues.
- [ ] **HARD-02**: A documented demo shows Go source changing, OpenAPI updating, and Go SDK output updating.
- [ ] **HARD-03**: All PoC tests, snapshots, and Rust quality gates pass before the milestone is considered complete.

## v2 Requirements

### Additional Source Frontends

- **TS-01**: Analyze TypeScript framework routes and DTO types.
- **PY-01**: Analyze Python framework routes, type hints, and model definitions.
- **RSRC-01**: Analyze Rust web framework routes and Serde schemas.

### Additional SDK Targets

- **TSDK-01**: Generate an idiomatic TypeScript SDK.
- **PYSDK-01**: Generate an idiomatic Python SDK.
- **RSDK-01**: Generate an idiomatic Rust SDK.

### Advanced Lifecycle

- **ADV-01**: Add deeper graph invalidation or query-level caching if fixture benchmarks prove the need.
- **ADV-02**: Add richer extension APIs after repeated router and SDK customization pressure.

## User Stories

- As a Go service developer, I can run one command and get OpenAPI plus a Go SDK generated from source code.
- As a maintainer, I can save a Go file and see generated outputs update quickly without rerunning a fragmented toolchain manually.
- As a framework-heavy team, I can customize extraction and SDK generation with code instead of maintaining YAML files or duplicated comment strings.
- As a reviewer, I can inspect what `gnr8` inferred and why unsupported cases were skipped.

## Acceptance Criteria

- The PoC fixture produces a stable graph report, OpenAPI document, and compiling Go SDK.
- A single supported route or struct edit updates only the affected generated outputs where practical.
- Unsupported patterns produce actionable diagnostics with source locations.
- The `.gnr8/` workspace contains editable code customization and keeps cache/output lifecycle files separate.

## Definition of Done

- All v1 requirements are mapped to roadmap phases.
- Phase tests and fixture snapshots pass.
- Rust formatting, tests, and clippy gates pass.
- The demo workflow is documented and reproducible from a fresh checkout.
- No implementation scope exceeds the Go-to-Go PoC without an explicit roadmap update.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Multi-language source implementation | Go must prove the graph model first. |
| Multi-language SDK generation | The first Go SDK must be high quality before targets multiply. |
| Dynamic plugin loading | Too much lifecycle and stability surface before repeated extension pressure exists. |
| Macro-heavy configuration API | Plain Rust code should be tested first. |
| Graph database | Stable IDs and typed structs are sufficient for the PoC. |
| Full Go framework coverage | One or two router styles are enough to validate the loop. |
| Full OpenAPI 3.2 coverage | Useful modern output matters before complete spec coverage. |
| Arbitrary handler body interpretation | Static analysis should start with explicit supported patterns and diagnostics. |
| Wrapping existing generators as the core | The product promise is an owned native pipeline. |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| POC-01 | Phase 1 | Complete |
| POC-02 | Phase 1 | Complete |
| POC-03 | Phase 1 | Complete |
| RUST-01 | Phase 1 | Complete |
| RUST-02 | Phase 1 | Complete |
| RUST-03 | Phase 1 | Complete |
| RUST-04 | Phase 1 | Complete |
| FIX-01 | Phase 1 | Complete |
| FIX-02 | Phase 1 | Complete |
| FIX-03 | Phase 1 | Complete |
| FIX-04 | Phase 1 | Complete |
| GO-01 | Phase 2 | Complete |
| GO-02 | Phase 2 | Complete |
| GO-03 | Phase 2 | Complete |
| GO-04 | Phase 2 | Complete |
| GO-05 | Phase 2 | Complete |
| GO-06 | Phase 2 | Complete |
| GRAPH-01 | Phase 2 | Complete |
| GRAPH-02 | Phase 2 | Complete |
| GRAPH-03 | Phase 2 | Complete |
| OAPI-01 | Phase 3 | Complete |
| OAPI-02 | Phase 3 | Complete |
| OAPI-03 | Phase 3 | Complete |
| SDK-01 | Phase 3 | Complete |
| SDK-02 | Phase 3 | Complete |
| SDK-03 | Phase 3 | Complete |
| SDK-04 | Phase 3 | Complete |
| SDK-05 | Phase 3 | Complete |
| WS-01 | Phase 4 | Complete |
| WS-02 | Phase 4 | Complete |
| WS-03 | Phase 4 | Complete |
| WS-04 | Phase 4 | Complete |
| WATCH-01 | Phase 4 | Complete |
| WATCH-02 | Phase 4 | Complete |
| WATCH-03 | Phase 4 | Complete |
| HARD-01 | Phase 5 | Pending |
| HARD-02 | Phase 5 | Pending |
| HARD-03 | Phase 5 | Pending |

**Coverage:**
- v1 requirements: 37 total
- Mapped to phases: 37
- Unmapped: 0

---
*Requirements defined: 2026-06-24*
*Last updated: 2026-06-24 — marked WATCH-02/WATCH-03 Complete (Phase 4 implemented + verified)*

