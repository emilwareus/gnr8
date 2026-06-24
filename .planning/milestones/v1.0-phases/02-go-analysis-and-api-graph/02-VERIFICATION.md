---
phase: 02-go-analysis-and-api-graph
verified: 2026-06-24T00:00:00Z
status: passed
score: 4/4 success criteria verified (16/16 plan truths verified)
overrides_applied: 0
re_verification:
  previous_status: none
  previous_score: n/a
---

# Phase 2: Go Analysis And API Graph Verification Report

**Phase Goal:** Build the native Go extraction path and produce inspectable API graph reports.
**Verified:** 2026-06-24
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A developer can inspect discovered routes and schemas from the fixture services | ✓ VERIFIED | `cargo run -p gnr8 -- inspect routes fixtures/goalservice` renders a 4-row table (POST /, GET /list, DELETE /{uuid}, PUT /{uuid}). `inspect schemas` renders 9 schemas (8 DTO + TargetDirection enum). `inspect graph` renders 4 ops / 9 schemas / 7 diagnostics. All three support `--json` (valid JSON parsed). |
| 2 | Supported handlers connect routes to request and response schemas | ✓ VERIFIED | POST request_body → `dto.CreateGoalInput`; 201 → `CommandMessageWithUUID`, 400 → `HttpError` (resolved via go/constant). PUT request_body → `UpdateGoalInput`; responses 200/400/404. All routes `secured=true` (group `api.Use(AuthMiddleware)`). Verified through both the helper JSON and the rendered ApiGraph. |
| 3 | Unsupported patterns produce diagnostics with source locations | ✓ VERIFIED | 7 WARN diagnostics carry file:line: float64-narrowing ×3 (goal.go:32/43/57), free-form-map ×1 (goal.go:62), untyped-query ×3 (handlers.go:57/58/59). Relativized to module-relative paths in the CLI/snapshot. |
| 4 | Graph IDs and report output stay stable across unchanged runs | ✓ VERIFIED | Helper `go run` twice → byte-identical. CLI `--json inspect graph` twice → byte-identical (24155 bytes). `tests/determinism.rs` passes (two build_graph runs serialize byte-identical). `snapshot_graph` + `snapshot_diagnostics` GREEN with committed reviewed `.snap` files. |

**Score:** 4/4 success criteria verified.

### Plan Frontmatter Truths (16/16 verified)

All `must_haves.truths` across 02-01, 02-02, 02-03 verified against the codebase: DTO/field/tag extraction with embedded flattening and well-known types (helper JSON), Gin route recognition via go/types Selections (routes_test passes), request/response inference via go/constant, swaggo annotation escape hatch (goalUuidPut + aggregation Enums), 7 diagnostics, deny_unknown_fields serde mirror, typed CoreError on all subprocess failure modes, populated ApiGraph with from_facts + stable IDs + sorted serialization, inspect renderers, GREEN snapshots, determinism.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `goextract/` module (main.go, internal/{load,types,facts,diag,routes,handlers}) | Go sidecar extractor | ✓ VERIFIED | `go build ./... && go vet ./... && go test ./...` all pass; emits deterministic sorted JSON facts (9 schemas, 4 routes, 7 diagnostics) |
| `crates/gnr8-core/src/analyze/facts.rs` | serde mirror, deny_unknown_fields | ✓ VERIFIED | deny_unknown_fields present; round-trip + reject-unknown tests in lib suite (22 pass) |
| `crates/gnr8-core/src/analyze/helper.rs` | subprocess driver, typed errors | ✓ VERIFIED | `Command::new("go")`, resolve_target, run_goextract; toolchain-missing → typed error |
| `crates/gnr8-core/src/error.rs` | GoToolchainMissing/HelperExit/FactsParse | ✓ VERIFIED | All three variants present + NotYetImplemented kept; Display tests pass |
| `crates/gnr8-core/src/graph/mod.rs` | populated ApiGraph + from_facts | ✓ VERIFIED | 684-line snapshot proves full graph; from_facts sorts all collections; router-agnostic (no Gin fields) |
| `crates/gnr8-core/src/diagnostics/mod.rs` | collect() → 7-line text | ✓ VERIFIED | snapshot_diagnostics GREEN; 7 WARN lines reconciled with expected/diagnostics.txt |
| `crates/gnr8/src/render.rs` | table + JSON renderers | ✓ VERIFIED | render_routes/schemas/graph; tables + --json both work end-to-end |
| `crates/gnr8-core/tests/snapshots/*.snap` | real reviewed snapshots | ✓ VERIFIED | graph .snap 684 lines, diagnostics .snap 7 WARN lines — both real, GREEN |
| `crates/gnr8-core/tests/determinism.rs` | two-run byte-identical | ✓ VERIFIED | test passes; runs on this machine (go 1.26 present) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| helper.rs | go subprocess | `Command::new("go") run . <dir>` (discrete arg) | ✓ WIRED | run_goextract invokes go run with current_dir=goextract; no shell |
| helper.rs | facts.rs | serde_json::from_slice → GoFacts | ✓ WIRED | parse failure → FactsParse |
| analyze/mod.rs | graph/mod.rs | build_graph → ApiGraph::from_facts | ✓ WIRED | build_graph implemented (not NotYetImplemented) |
| main.rs | render.rs | inspect arms → render(graph, json) | ✓ WIRED | CLI renders routes/schemas/graph, table + --json |
| snapshot_graph.rs | build_graph | assert_yaml_snapshot of real graph | ✓ WIRED | GREEN; no longer red-by-design |
| routes.go | go/types Selections | gin method identity by receiver pkg path | ✓ WIRED | routes_test (incl. aliased-import) passes |
| handlers.go | go/constant | http.StatusXxx → numeric status | ✓ WIRED | 201/400/200/404 resolved, not hardcoded |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| inspect graph --json | operations | build_graph → from_facts → goextract | Yes — 4 ops with handlers, params, request/response refs, provenance | ✓ FLOWING |
| inspect graph --json | schemas | same | Yes — 9 schemas; CreateGoalInput has 6 real fields + provenance goal.go:28; TargetDirection enum [gte,lte] | ✓ FLOWING |
| inspect graph --json | diagnostics | diagnostics::collect | Yes — 7 WARN with file:line | ✓ FLOWING |
| GET /list aggregation | enum_values | swaggo annotation merge | Yes — [avg,count,max,min,sum], required=true | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| go helper builds/vets/tests | `cd goextract && go build/vet/test ./...` | all pass | ✓ PASS |
| helper emits facts | `go run . ../fixtures/goalservice` | 4 routes, 9 schemas, 7 diags, exit 0 | ✓ PASS |
| helper determinism | two `go run` invocations | byte-identical | ✓ PASS |
| inspect routes table | `cargo run -p gnr8 -- inspect routes fixtures/goalservice` | 4-route table + 7 diagnostics | ✓ PASS |
| inspect schemas --json | `--json inspect schemas` | valid JSON, 9 entries | ✓ PASS |
| inspect graph --json | `--json inspect graph` | valid JSON, 4 ops/9 schemas/7 diags | ✓ PASS |
| CLI graph determinism | `--json inspect graph` ×2 | byte-identical (24155 bytes) | ✓ PASS |
| fmt | `cargo fmt --all -- --check` | clean | ✓ PASS |
| clippy -D warnings | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0, clean | ✓ PASS |
| workspace tests | `cargo test --workspace` | gnr8 9, gnr8-core lib 22, determinism 1, snapshot_graph 1, snapshot_diagnostics 1 all pass; snapshot_openapi + snapshot_sdk FAIL (red-by-design, correct) | ✓ PASS |
| no prod unwrap/expect/panic | grep gnr8-core/src | all occurrences inside `#[cfg(test)]` modules only | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` probes declared for this phase (Rust/Go cargo+go test phase). Behavioral spot-checks above serve as the runnable verification.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| GO-01 | 02-01 | Discovers Go packages/files | ✓ SATISFIED | go/packages LoadAllSyntax+NeedModule; module resolved, 9 schemas extracted |
| GO-02 | 02-01 | Extracts structs/fields/json tags/spans | ✓ SATISFIED | helper JSON shows fields, json names, spans; snapshot 684 lines |
| GO-03 | 02-01 | Maps primitives/pointers/slices/maps/structs/time.Time | ✓ SATISFIED | uuid.UUID→string(uuid), time.Time→string(date-time), *float64 optional+number, []uuid→array, map→object |
| GO-04 | 02-02 | Recognizes router patterns (method/path/handler/span) | ✓ SATISFIED | 4 routes via go/types Selections (gin pkg path); :uuid→{uuid}; routes_test incl. aliased import |
| GO-05 | 02-02 | Infers request/response schemas | ✓ SATISFIED | ShouldBindJSON→request, c.JSON(status,_)→response via go/constant; swaggo escape hatch |
| GO-06 | 02-01/02/03 | Diagnostics instead of panics/silent drops | ✓ SATISFIED | 7 diagnostics with file:line; typed CoreError on all subprocess failures; zero prod unwrap/expect/panic; clippy deny gate green |
| GRAPH-01 | 02-03 | Graph models routes/ops/params/bodies/responses/schemas/provenance | ✓ SATISFIED | ApiGraph with Operation/Param/Response/Schema/Field + SourceSpan on every node |
| GRAPH-02 | 02-03 | Stable IDs + outputs across unchanged runs | ✓ SATISFIED | determinism.rs passes; helper + CLI byte-identical; sorted collections; @ID-else-handler ids |
| GRAPH-03 | 02-03 | inspect routes/schemas/graph explain facts + diagnostics | ✓ SATISFIED | all 3 inspect commands work table + --json, each appending diagnostics |

No orphaned requirements: REQUIREMENTS.md maps GO-01..06 + GRAPH-01..03 to Phase 2; every one is claimed by a plan and verified.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none in production code) | — | — | — | All `.unwrap()`/`.expect()`/`unreachable!` occurrences are confined to `#[cfg(test)]` modules (lib.rs:29, graph/mod.rs:391, facts.rs:191). No debt markers (TBD/FIXME/XXX), no placeholder returns, no hollow stubs in production code. |

### Red-By-Design (Correct, NOT a gap)

`snapshot_openapi` and `snapshot_sdk` FAIL by design — they invoke the Phase-3 seams `lower::to_openapi` and `sdk::generate`, which still return `NotYetImplemented`. `snapshot_openapi` now passes `build_graph` (proving it works) and fails only at `to_openapi`. These remain in the **non-blocking** `contract` CI job; the now-green `snapshot_graph` + `snapshot_diagnostics` + `determinism` were correctly promoted into the **blocking** `gates` job. This is the expected, documented Phase-2 boundary.

### Notable Intentional Deviations (verified acceptable)

1. **Group-relative paths** (`/`, `/list`, `/{uuid}`) rather than absolute `/goal/...`. The `/goal/` prefix is the dynamic `"/" + basePath` group prefix the helper cannot constant-fold; deferred to Phase-3 lowering per CONTEXT D-03 and 02-03 decisions. `expected/diagnostics.txt` (an "eventually" acceptance target) shows `/goal/list`, but the phase-locked `.snap` uses the group-relative form — a documented, planned reconciliation (CONTEXT D-10: snapshot locks exact text).
2. **Normalized diagnostic templates** (one template per rule) rather than reproducing `expected/diagnostics.txt`'s per-line trailing clauses (e.g. the aggregation line). Explicit 02-03 Open-Q2 decision; the `.snap` is the locked contract.
3. **Operation id = @ID else handler symbol** (createGoal/listGoals/goalUuidPut/deleteGoal) — matches `expected/openapi.yaml` operationIds; deterministic. Honors D-08.

These are recorded in the plan/SUMMARY decisions and verified to match the locked CONTEXT decisions; they reduce no scope.

### Human Verification Required

None. All four success criteria are programmatically observable through the CLI, the helper, the test suite, and source inspection — all verified above.

### Gaps Summary

No gaps. The native Go extraction path (goextract sidecar via go/packages + go/types + go/constant) is built and tested; it produces a deterministic JSON facts document that the Rust analyzer deserializes into a router-agnostic, stably-IDed ApiGraph; the three `inspect` reports render that graph (table + --json) with diagnostics carrying source locations; the two Phase-1 contract tests `snapshot_graph` + `snapshot_diagnostics` are GREEN and in the blocking gate; determinism is proven at the helper, graph, and CLI levels. All 9 requirements (GO-01..06, GRAPH-01..03) are satisfied with codebase evidence. The two still-red snapshots (openapi, sdk) are correctly red-by-design for Phase 3.

---

_Verified: 2026-06-24_
_Verifier: Claude (gsd-verifier)_
