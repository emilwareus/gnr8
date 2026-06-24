# Roadmap: gnr8

## Overview

The PoC moves from a minimal Rust CLI and realistic Go fixtures to a complete Go source to OpenAPI to Go SDK loop, then adds `.gnr8/` code-as-config, watch mode, and hardening. The roadmap is intentionally coarse to protect the project from premature platform work.

## Phases

- [ ] **Phase 1: Foundation And Fixtures** - Create the Rust project skeleton, lock the PoC contract, and establish realistic validation fixtures.
- [ ] **Phase 2: Go Analysis And API Graph** - Extract Go route, schema, and handler facts into a stable inspectable graph.
- [ ] **Phase 3: OpenAPI And Go SDK Generation** - Emit valid OpenAPI and a compiling usable Go SDK from the graph.
- [ ] **Phase 4: `.gnr8` Lifecycle And Watch Mode** - Add code-as-config workspace flow, generated-file ownership, and save-time regeneration.
- [ ] **Phase 5: PoC Hardening And Demo** - Tighten diagnostics, performance evidence, docs, and milestone verification.

## Phase Details

### Phase 1: Foundation And Fixtures
**Goal**: Establish the smallest Rust workspace and fixture harness that can drive all later implementation.
**Depends on**: Nothing (first phase)
**Requirements**: POC-01, POC-02, POC-03, RUST-01, RUST-02, RUST-03, RUST-04, FIX-01, FIX-02, FIX-03, FIX-04
**Success Criteria** (what must be TRUE):
  1. A developer can run the Rust CLI help and see the planned command surface.
  2. The selected Go fixture services exist and encode the PoC acceptance cases.
  3. Tests and snapshots define expected graph, OpenAPI, SDK, and diagnostic behavior before the analyzer is complete.
  4. Rust quality gates are wired into the project.
**Plans**: 3 plans

Plans:
- [ ] 01-01: Lock PoC contract and scaffold the Rust workspace.
- [ ] 01-02: Add realistic Go fixtures and expected snapshot structure.
- [ ] 01-03: Wire quality gates, fixture harness, and baseline failing expectations.

### Phase 2: Go Analysis And API Graph
**Goal**: Build the native Go extraction path and produce inspectable API graph reports.
**Depends on**: Phase 1
**Requirements**: GO-01, GO-02, GO-03, GO-04, GO-05, GO-06, GRAPH-01, GRAPH-02, GRAPH-03
**Success Criteria** (what must be TRUE):
  1. A developer can inspect discovered routes and schemas from fixture services.
  2. Supported handlers connect routes to request and response schemas.
  3. Unsupported patterns produce diagnostics with source locations.
  4. Graph IDs and report output stay stable across unchanged runs.
**Plans**: 3 plans

Plans:
- [ ] 02-01: Implement Go package discovery, struct extraction, and type mapping.
- [ ] 02-02: Implement selected router and handler contract extraction.
- [ ] 02-03: Build the API graph and inspect reports with diagnostics.

### Phase 3: OpenAPI And Go SDK Generation
**Goal**: Generate real artifacts from the graph: a valid OpenAPI document and a compiling Go SDK.
**Depends on**: Phase 2
**Requirements**: OAPI-01, OAPI-02, OAPI-03, SDK-01, SDK-02, SDK-03, SDK-04, SDK-05
**Success Criteria** (what must be TRUE):
  1. The fixture graph lowers to a valid OpenAPI document.
  2. OpenAPI output includes paths, operations, parameters, request bodies, responses, and schemas.
  3. The generated Go SDK compiles and can call fixture operations through tests.
  4. OpenAPI compatibility gaps are reported as diagnostics.
**Plans**: 3 plans

Plans:
- [ ] 03-01: Implement OpenAPI lowering and validation snapshots.
- [ ] 03-02: Implement Go SDK models, client, operations, and errors.
- [ ] 03-03: Add generated SDK compile tests and end-to-end artifact snapshots.

### Phase 4: `.gnr8` Lifecycle And Watch Mode
**Goal**: Prove the code-as-config user workflow and fast regeneration loop.
**Depends on**: Phase 3
**Requirements**: WS-01, WS-02, WS-03, WS-04, WATCH-01, WATCH-02, WATCH-03
**Success Criteria** (what must be TRUE):
  1. `gnr8 init` creates a `.gnr8/` workspace with editable code customization.
  2. Generated outputs are tracked well enough to avoid silent user-file clobbering.
  3. No-op generation avoids rewriting unchanged files.
  4. Watch mode updates outputs after supported Go source edits and reports latency.
**Plans**: 3 plans

Plans:
- [ ] 04-01: Implement `.gnr8/` initialization and lifecycle layout.
- [ ] 04-02: Add code customization hooks and generated-file ownership tracking.
- [ ] 04-03: Implement no-op detection, watch mode, debounce, and latency reporting.

### Phase 5: PoC Hardening And Demo
**Goal**: Make the PoC coherent, measured, diagnosable, and ready for review.
**Depends on**: Phase 4
**Requirements**: HARD-01, HARD-02, HARD-03
**Success Criteria** (what must be TRUE):
  1. A developer can run a documented demo from source edit to updated OpenAPI and SDK output.
  2. `doctor` or equivalent diagnostics explain unsupported patterns and lifecycle issues.
  3. Benchmark numbers exist for cold generation, warm no-op, and single-file edits.
  4. All tests, snapshots, and Rust quality gates pass.
**Plans**: 2 plans

Plans:
- [ ] 05-01: Harden diagnostics, doctor output, and performance reporting.
- [ ] 05-02: Finalize demo documentation, verification evidence, and milestone audit.

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation And Fixtures | 0/3 | Not started | - |
| 2. Go Analysis And API Graph | 0/3 | Not started | - |
| 3. OpenAPI And Go SDK Generation | 0/3 | Not started | - |
| 4. `.gnr8` Lifecycle And Watch Mode | 0/3 | Not started | - |
| 5. PoC Hardening And Demo | 0/2 | Not started | - |

