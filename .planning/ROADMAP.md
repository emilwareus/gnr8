# Roadmap: gnr8

## Overview

The PoC moves from a minimal Rust CLI and realistic Go fixtures to a complete Go source to OpenAPI to Go SDK loop, then adds `.gnr8/` code-as-config, watch mode, and hardening. The roadmap is intentionally coarse to protect the project from premature platform work.

## Phases

- [x] **Phase 1: Foundation And Fixtures** - Create the Rust project skeleton, lock the PoC contract, and establish realistic validation fixtures. (completed 2026-06-24)
- [x] **Phase 2: Go Analysis And API Graph** - Extract Go route, schema, and handler facts into a stable inspectable graph. (completed 2026-06-24)
- [x] **Phase 3: OpenAPI And Go SDK Generation** - Emit valid OpenAPI and a compiling usable Go SDK from the graph. (completed 2026-06-24)
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
- [x] 01-01-PLAN.md — Lock the PoC contract (docs/poc-contract.md) and scaffold the Cargo workspace: gnr8-core lib (CoreError + module seams) + gnr8 clap CLI.
- [x] 01-02-PLAN.md — Add the realistic Go Gin fixture module (CRUD + list-with-filters, full DTO coverage incl. map[string]any) and expected/ acceptance scaffolds.
- [x] 01-03-PLAN.md — Wire fmt/clippy/test gates (Makefile + CI), the insta harness, and the four red-by-design contract tests; blocking checkpoint on phase-gate state.

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
- [x] 02-01-PLAN.md — Create the goextract Go sidecar (go/packages LoadAllSyntax): struct/field/tag/type-mapping extraction, well-known uuid/time, enum const sets, float64+free-form-map diagnostics, sorted JSON facts; Rust serde DTOs + subprocess driver + extended CoreError; Makefile/CI goextract gate (wave 1).
- [x] 02-02-PLAN.md — Extend goextract with Gin route recognition (types.Info.Selections), handler request/response/param inference (go/constant), and the swaggo annotation escape hatch (@ID/@Router/@Param/Enums/@Security); untyped-query diagnostics (wave 2).
- [x] 02-03-PLAN.md — Build the router-agnostic Rust ApiGraph from facts (stable IDs, sorted serialization, provenance), implement build_graph + diagnostics::collect, wire inspect routes|schemas|graph (table + --json), flip snapshot_graph + snapshot_diagnostics to real green snapshots (wave 3).

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
- [x] 03-01-PLAN.md — OpenAPI lowering: typed OpenAPI 3.1 structs + deterministic YAML writer, to_openapi graph→doc mapping (join the /goal base prefix, surface OAPI-03 diagnostics), four new CoreError variants; flip snapshot_openapi GREEN (wave 1).
- [x] 03-02-PLAN.md — Go SDK codegen: format!-based emitters (models/functional-options Client/tag-grouped ctx-first ops/typed APIError) + gofmt normalization + SdkBundle file-marker String + write_to_dir; flip snapshot_sdk GREEN (wave 1).
- [x] 03-03-PLAN.md — Generated-SDK compile + smoke: hermetic stdlib-only temp-dir go build + httptest smoke (SDK-05), to_openapi/generate determinism asserts, promote all four contract tests + sdk_compile to the blocking CI gate and retire the non-blocking contract job (wave 2).

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
- [x] 04-01-PLAN.md — `.gnr8/` init + lifecycle layout + static TOML config: idempotent `workspace::init` (config.toml + auto `.gitignore` split), typed `Config` (inputs/outputs/go-module/naming knobs, deny_unknown_fields, honest PoC stand-in), new CoreError lifecycle variants, pin blake3/toml/notify-debouncer-full, wire `gnr8 init` (wave 1).
- [x] 04-02-PLAN.md — Ownership manifest + pure write decision + customization: blake3 manifest (load/save/prune, graceful on absent/corrupt), pure `plan_writes` truth table + `apply_writes` (warn+skip / `--force`, no-op skip), `apply_naming` overrides, `regenerate`, wire `gnr8 generate --force` + `gnr8 check` dry-run (wave 2).
- [ ] 04-03-PLAN.md — No-op detection + watch + debounce + latency: pure event filter (loop-safe, output-path drop), `notify-debouncer-full` shell, Ctrl-C, cold/no-op/single-edit latency (human + --json), `watch_smoke` test, add `--test lifecycle` to blocking make/CI gates (wave 3).

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
| 1. Foundation And Fixtures | 3/3 | Complete   | 2026-06-24 |
| 2. Go Analysis And API Graph | 3/3 | Complete   | 2026-06-24 |
| 3. OpenAPI And Go SDK Generation | 3/3 | Complete   | 2026-06-24 |
| 4. `.gnr8` Lifecycle And Watch Mode | 2/3 | In Progress|  |
| 5. PoC Hardening And Demo | 0/2 | Not started | - |
