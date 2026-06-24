---
phase: 02-go-analysis-and-api-graph
plan: 03
subsystem: api
tags: [rust, serde, insta, api-graph, stable-ids, determinism, clap, diagnostics, snapshot-testing]

# Dependency graph
requires:
  - phase: 02-go-analysis-and-api-graph (02-01)
    provides: "analyze::facts::GoFacts serde mirror + analyze::helper::run_goextract typed-error subprocess driver + CoreError::{GoToolchainMissing,HelperExit,FactsParse}"
  - phase: 02-go-analysis-and-api-graph (02-02)
    provides: "Populated GoFacts.routes (4 fixture routes) with params/request/response/annotation facts; 7 diagnostics (float64x3, free-form-map x1, untyped-query x3)"
provides:
  - "Router-agnostic Rust ApiGraph (Operation/Param/Response/Schema/Field/SchemaType/SchemaRef + graph-owned SourceSpan) with provenance on every node (D-07)"
  - "ApiGraph::from_facts: stable operation ids (@ID else handler symbol), package-qualified schema ids, fully sorted collections so unchanged source => byte-identical output (GRAPH-02)"
  - "analyze::build_graph implemented (run_goextract -> ApiGraph::from_facts); diagnostics::collect implemented (7-line canonical WARN text reconciled with expected/diagnostics.txt)"
  - "inspect routes|schemas|graph CLI renderers (human aligned tables + machine --json), each listing diagnostics (GRAPH-03/D-09); optional path arg defaulting to the fixture"
  - "snapshot_graph + snapshot_diagnostics GREEN with committed reviewed .snap files; tests/determinism.rs (two build_graph runs byte-identical)"
  - "helper::resolve_target: canonical absolute target resolution so relative inspect paths work AND span/diagnostic paths relativize portably"
  - "CI gates job promoted: graph/diagnostics/determinism now blocking (with setup-go); only snapshot_openapi+sdk remain non-blocking"
affects: [03-openapi-sdk-generation]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Graph determinism via sorted Vec everywhere + sort-before-store in from_facts (never serialize a HashMap) — GRAPH-02"
    - "Stable operation id = @ID annotation when present (goalUuidPut) else the handler function symbol (createGoal/listGoals/deleteGoal) — both source-derived, matching expected/openapi.yaml"
    - "Path-prefix relativization of provenance + diagnostic file paths against the canonical module root → portable, machine-independent, byte-stable snapshots"
    - "DISPATCH CONTRACT: inspect arms build+render directly to stdout in main (anyhow boundary); other commands stay on the NotYetImplemented Report path"
    - "Snapshots authored from REAL output then accepted (cargo-insta unavailable → .snap.new renamed, assertion_line stripped); CI runs INSTA_UPDATE=no so a mismatch hard-fails"

key-files:
  created:
    - "crates/gnr8/src/render.rs"
    - "crates/gnr8-core/tests/determinism.rs"
    - "crates/gnr8-core/tests/snapshots/snapshot_graph__goalservice_graph.snap"
    - "crates/gnr8-core/tests/snapshots/snapshot_diagnostics__goalservice_diagnostics.snap"
  modified:
    - "crates/gnr8-core/src/graph/mod.rs"
    - "crates/gnr8-core/src/analyze/mod.rs"
    - "crates/gnr8-core/src/diagnostics/mod.rs"
    - "crates/gnr8-core/src/analyze/helper.rs"
    - "crates/gnr8-core/src/lib.rs"
    - "crates/gnr8/src/main.rs"
    - "crates/gnr8/src/cli.rs"
    - "crates/gnr8-core/tests/snapshot_graph.rs"
    - "crates/gnr8-core/tests/snapshot_diagnostics.rs"
    - "Makefile"
    - ".github/workflows/ci.yml"

key-decisions:
  - "Operation id = @ID annotation else the handler symbol (NOT a method+path hash) — deterministic and matches expected/openapi.yaml's operationIds exactly (createGoal, listGoals, goalUuidPut, deleteGoal)"
  - "Graph stores the code-derived group-relative path + the @Router override (router_path); the absolute /goal/... prefix is dynamic ('/' + basePath in the fixture) and is deferred to Phase-3 lowering — the facts cannot supply it"
  - "Graph-owned public SourceSpan (not the crate-private facts::SourceSpan) so the public ApiGraph surface is self-contained and passes the private-interfaces lint"
  - "Diagnostics canonical phrasing: one normalized template per rule carrying field/route identity (Open Q2 recommendation), rendered as 'WARN  <message> (<file>:<line>)', sorted by (file,line,message)"
  - "resolve_target canonicalizes the target so the module root matches the helper's canonical span paths — keeps relativized snapshots portable (no machine-absolute paths leak into .snap)"

patterns-established:
  - "Pattern: build_graph/collect resolve the target once and pass the same canonical root to both run_goextract and from_facts/render, so relativization is consistent"
  - "Pattern: insta snapshots authored from real output then reviewed/accepted; CI INSTA_UPDATE=no hard-fails on drift (FIX-04)"
  - "Pattern: blocking gates job runs green snapshot tests with setup-go; non-blocking contract job holds only the still-red openapi+sdk snapshots until their phase lands"

requirements-completed: [GRAPH-01, GRAPH-02, GRAPH-03, GO-06]

# Metrics
duration: 14min
completed: 2026-06-24
---

# Phase 2 Plan 03: ApiGraph, Stable IDs, Inspect Reports & Diagnostics Snapshot Summary

**Router-agnostic Rust `ApiGraph` built from the goextract facts with deterministic stable operation/schema ids and fully sorted serialization, `build_graph` + `diagnostics::collect` implemented, three `inspect` renderers (table + `--json`), and the two Phase-1 contract tests (`snapshot_graph` + `snapshot_diagnostics`) flipped GREEN with reviewed `.snap` files plus an end-to-end determinism test — `snapshot_openapi`/`snapshot_sdk` stay red-by-design for Phase 3.**

## Performance

- **Duration:** 14 min
- **Started:** 2026-06-24T17:37:24Z
- **Completed:** 2026-06-24T17:51:53Z
- **Tasks:** 3
- **Files modified:** 15 (4 created, 11 modified)

## Accomplishments

- **ApiGraph + from_facts (Task 1):** Fleshed out the empty placeholder into a router-agnostic graph — `ApiGraph { module, operations, schemas, diagnostics }` over `Operation`/`Param`/`Response`/`Schema`/`Field`/`SchemaType`/`SchemaRef` plus a graph-owned `SourceSpan` on every operation, param, and schema (D-07). `from_facts` derives the stable operation id (the `@ID` annotation when present — `goalUuidPut` — else the handler symbol — `createGoal`/`listGoals`/`deleteGoal`), keeps package-qualified schema ids, and sorts every collection (operations by `(path, method)`, schemas by id, params by name, responses by status, fields by json name, enum values + tags + schemes lexically) so two runs serialize byte-identically (GRAPH-02). No Gin/framework field leaks into the graph (D-03).
- **build_graph + diagnostics::collect (Task 1):** `analyze::build_graph` = `run_goextract` → `ApiGraph::from_facts`; `diagnostics::collect` renders the 7-line canonical WARN text (float64×3, free-form-map×1, untyped-query×3) reconciled with `expected/diagnostics.txt`, sorted by `(file, line, message)`. Every subprocess failure path returns a typed `CoreError` (no panic, GO-06); the lib test was updated to assert the typed-error behavior rather than `NotYetImplemented`.
- **inspect renderers (Task 2):** New `render.rs` renders `routes` (METHOD/PATH/OPERATION/SECURED/REQUEST/RESPONSES), `schemas` (ID/KIND/FIELDS/ENUM), and `graph` (compact combined view) as human aligned tables (no table crate — plain `{:<width}`) and as machine `--json` (serde straight off the graph), each appending the diagnostics list (D-09). `main.rs` builds the graph and renders directly to stdout, with a real `CoreError` (e.g. toolchain missing) flowing through the anyhow boundary as a clean message + exit 1. `cli.rs` gained an optional `path` arg per `inspect` variant defaulting to the fixture, so `gnr8 inspect routes` and `gnr8 inspect routes <dir>` both work; parse tests updated + extended.
- **Snapshots + determinism + gates (Task 3):** Authored the two reviewed `.snap` files from real output — `snapshot_graph` shows all 4 operations (with `goalUuidPut` from `@ID`, request/response schema refs, the `aggregation` enum `[avg,count,max,min,sum]`, `secured=true`, relativized provenance spans, all 8 object schemas + the `TargetDirection` enum), `snapshot_diagnostics` shows the 7 WARN lines. Both tests flipped GREEN; their headers updated from red-by-design to green. Added `tests/determinism.rs` (two `build_graph` runs serialize byte-identical). Promoted `snapshot_graph` + `snapshot_diagnostics` + `determinism` into the blocking `make gates` / CI `gates` job (added `setup-go`); only `snapshot_openapi` + `snapshot_sdk` remain in the non-blocking `contract` job (red-by-design until Phase 3).

## Task Commits

Each task was committed atomically:

1. **Task 1: Populate ApiGraph + from_facts + stable IDs + sorted serialization; implement build_graph + diagnostics::collect** - `05bf639` (feat)
2. **Task 2: Wire inspect routes|schemas|graph renderers (table + --json)** - `3e4af19` (feat)
3. **Task 3: Flip snapshot_graph + snapshot_diagnostics GREEN; add determinism; promote gates** - `7e57e12` (test)

**Plan metadata:** committed separately with this SUMMARY + STATE + ROADMAP + REQUIREMENTS.

_Task 1 was a TDD task; test + implementation landed in one commit (graph/diagnostics/analyze unit tests co-developed against the live fixture)._

## Files Created/Modified

- `crates/gnr8-core/src/graph/mod.rs` - Router-agnostic ApiGraph + child structs + graph-owned SourceSpan + `from_facts` (stable ids, sorted collections, path relativization) + 6 unit tests
- `crates/gnr8-core/src/analyze/mod.rs` - `build_graph` = resolve_target → run_goextract → from_facts; typed-error unit test
- `crates/gnr8-core/src/diagnostics/mod.rs` - `collect` + pure `render` (canonical WARN lines, sorted, relativized) + 3 unit tests
- `crates/gnr8-core/src/analyze/helper.rs` - `resolve_target` (canonical absolute target resolution; Rule 1 fix)
- `crates/gnr8-core/src/lib.rs` - updated `build_graph` lib test (typed error, not NotYetImplemented)
- `crates/gnr8/src/render.rs` - NEW: render_routes/render_schemas/render_graph (table + --json) + 4 render tests
- `crates/gnr8/src/main.rs` - `run_inspect` dispatch (build_graph + render to stdout via anyhow boundary)
- `crates/gnr8/src/cli.rs` - InspectAction path arg + DEFAULT_INSPECT_TARGET + parse tests
- `crates/gnr8-core/tests/snapshot_graph.rs` / `snapshot_diagnostics.rs` - headers updated to GREEN
- `crates/gnr8-core/tests/determinism.rs` - NEW: two-run byte-identical assertion (GRAPH-02)
- `crates/gnr8-core/tests/snapshots/*.snap` - NEW: the two reviewed locked snapshots
- `Makefile` / `.github/workflows/ci.yml` - moved graph/diagnostics/determinism into the blocking gate (setup-go added); contract job now openapi+sdk only

## ApiGraph struct layout (the Phase-3 consumer contract)

```
ApiGraph { module: String, operations: Vec<Operation>, schemas: Vec<Schema>, diagnostics: Vec<Diagnostic> }
Operation { id, method, path, router_path: Option, handler, summary: Option, tags, secured,
            security_schemes, params: Vec<Param>, request_body: Option<SchemaRef>,
            responses: Vec<Response>, provenance: SourceSpan }
Param { name, location, required, schema: SchemaType, enum_values, description: Option, provenance: SourceSpan }
Response { status: u16, body: Option<SchemaRef>, description: Option }
Schema { id, name, kind, fields: Vec<Field>, enum_values, provenance: SourceSpan }
Field { json_name, required, optional, schema: SchemaType, description: Option, example: Option }
SchemaType { kind, format: Option, items: Option<Box<SchemaType>>, ref_id: Option, additional_properties: Option<bool> }
SchemaRef { ref_id }   Diagnostic { severity, message, file, line }   SourceSpan { file, start_line, end_line }
```

- **Stable-ID derivation actually used:** `operation_id = facts.operation_id.unwrap_or(handler)`; `schema_id` = the package-qualified name straight from the helper. Both deterministic; the four fixture operation ids (`createGoal`, `listGoals`, `goalUuidPut`, `deleteGoal`) match `expected/openapi.yaml`.
- **Path storage:** code-derived group-relative `path` (`/`, `/list`, `/{uuid}`) + `router_path` (the `@Router` override). The absolute `/goal/...` prefix is the dynamic `"/" + basePath` group prefix the helper cannot constant-fold, so it is intentionally NOT folded here — Phase-3 lowering joins it.

## Diagnostics canonical phrasing / order chosen

`collect` renders one line per diagnostic as `WARN  <message> (<file>:<line>)`, where `<message>` is the helper's machine-stable rule+identity template (one template per rule) and `<file>` is relativized against the analyzed module. Order is sorted by `(file, line, message)`, which yields exactly the `expected/diagnostics.txt` sequence: float64-narrowing ×3 (CreateGoalInput / UpdateGoalInput / GoalResponse `.TargetValue`, `internal/common/dto/goal.go` lines 32/43/57), free-form-map ×1 (`GoalResponse.Metadata`, goal.go:62), then untyped-query ×3 (cursor / page_size / aggregation on GET /list, `internal/goal/ports/handlers.go` lines 57/58/59). Per Open Q2, this normalizes to one template per rule rather than reproducing the file's slightly-divergent per-line trailing clauses; the `.snap` locks the final exact text.

## inspect path-arg behavior

Each `InspectAction` variant carries `path: String` with `#[arg(default_value = DEFAULT_INSPECT_TARGET)]` (the goalservice fixture, resolved from `CARGO_MANIFEST_DIR`). So `gnr8 inspect routes` analyzes the fixture out of the box and `gnr8 inspect routes <dir>` analyzes any module. Relative paths work because `helper::resolve_target` canonicalizes the target before the subprocess runs (the helper's cwd is the goextract dir, so a relative path would otherwise be misresolved).

## Decisions Made

See `key-decisions` in the frontmatter. The load-bearing ones: operation id = `@ID`-else-handler (matches expected openapi), path stays group-relative + `@Router` override (absolute prefix deferred to Phase-3), graph-owned public `SourceSpan`, and canonical target resolution for portable snapshots.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Relative `inspect` target paths were resolved against the wrong directory**
- **Found during:** Task 2 (running the plan's verify `cargo run -p gnr8 -- inspect routes fixtures/goalservice`)
- **Issue:** `run_goextract` runs the helper with `current_dir(goextract_dir())`, so a relative `target_dir` like `fixtures/goalservice` was interpreted relative to `goextract/` → `chdir: no such file or directory`. The plan's own verify commands pass a relative path.
- **Fix:** Added `helper::resolve_target`, which canonicalizes the target to an absolute path; `build_graph` and `collect` resolve once and pass the same canonical root to both the helper and `from_facts`/`render`.
- **Files modified:** `crates/gnr8-core/src/analyze/helper.rs`, `crates/gnr8-core/src/analyze/mod.rs`, `crates/gnr8-core/src/diagnostics/mod.rs`
- **Verification:** `cargo run -p gnr8 -- inspect routes fixtures/goalservice` prints the route table; `--json inspect schemas` emits valid JSON.
- **Committed in:** `3e4af19` (Task 2 commit)

**2. [Rule 1 - Bug] Machine-absolute span/diagnostic paths leaked into the snapshots (non-portable, would fail CI)**
- **Found during:** Task 3 (first snapshot generation)
- **Issue:** The contract tests resolve `FIXTURE_DIR` as `<manifest>/../../fixtures/goalservice` (contains `..`), but the helper emits CANONICAL absolute file paths in spans/diagnostics. Prefix-stripping with the non-canonical root failed, so the full machine path (`/Users/.../tripoli-v7/fixtures/...`) leaked into the `.snap` — different on CI ⇒ guaranteed snapshot mismatch.
- **Fix:** `resolve_target` now canonicalizes (resolving `..`/symlinks) so the module root handed to relativization matches the helper's canonical output; file paths strip to module-relative (`internal/common/dto/goal.go`).
- **Files modified:** `crates/gnr8-core/src/analyze/helper.rs`
- **Verification:** Both `.snap` files contain only module-relative paths; `snapshot_graph`/`snapshot_diagnostics` GREEN; determinism test passes.
- **Committed in:** `7e57e12` (Task 3 commit)

**3. [Rule 2 - Missing Critical] Graph-owned public `SourceSpan` instead of leaking the crate-private facts type**
- **Found during:** Task 1 (clippy `-D warnings` → `private_interfaces`)
- **Issue:** Reusing the `pub(crate) facts::SourceSpan` inside the public `Operation`/`Param`/`Schema` structs exposed a more-private type than the item (clippy `private_interfaces`, a `-D warnings` failure), and would have leaked the contract DTO into the public graph surface.
- **Fix:** Defined a public graph-owned `SourceSpan` and mapped `facts::SourceSpan -> graph::SourceSpan` in `relativize_span`; made `ApiGraph::from_facts` `pub(crate)` (the public entry is `build_graph`).
- **Files modified:** `crates/gnr8-core/src/graph/mod.rs`
- **Verification:** `cargo clippy --all-targets --all-features --locked -- -D warnings` clean.
- **Committed in:** `05bf639` (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 missing-critical). **Impact on plan:** All three were necessary for correctness (relative paths working, portable/deterministic snapshots) or to satisfy the existing clippy gate. No scope creep — the graph shape, stable ids, diagnostics text, inspect behavior, and snapshot/determinism gates all match the plan; `snapshot_openapi`/`snapshot_sdk` were left untouched (red-by-design for Phase 3).

## Issues Encountered

- `cargo-insta` is not installed in this environment (`cargo insta` → "no such command"). Authored the two snapshots by running the tests (which write `.snap.new` on first run), reviewing the captured output against `expected/diagnostics.txt` + `expected/openapi.yaml`, then accepting by renaming `.snap.new` → `.snap` and stripping the transient `assertion_line:` metadata line (exactly what `cargo insta accept` does). The accepted snapshots now pass; CI's `INSTA_UPDATE=no` hard-fails on any future drift.

## User Setup Required

None - no external service configuration required. The Go toolchain (`go 1.26`) remains the only dependency; a missing toolchain surfaces as `CoreError::GoToolchainMissing` (clean message + exit 1), never a panic.

## Next Phase Readiness

- **Phase 2 complete.** The router-agnostic `ApiGraph` (with stable ids, sorted/deterministic serialization, provenance) + `diagnostics::collect` + the three `inspect` reports are the deliverable; `snapshot_graph` + `snapshot_diagnostics` are GREEN and in the blocking gate.
- **Ready for Phase 3 (03-openapi-sdk-generation):** the `ApiGraph` struct (documented above) is the consumable source of truth for `lower::to_openapi` (which joins the `/goal` base prefix and uses the operation ids/params/responses) and `sdk::generate` (which uses the schemas/fields). The two remaining red-by-design tests (`snapshot_openapi`, `snapshot_sdk`) are the Phase-3 gate; promote them to the blocking `gates`/CI job once green.

---
*Phase: 02-go-analysis-and-api-graph*
*Completed: 2026-06-24*

## Self-Check: PASSED

- All 4 created files verified present on disk (`[ -f ]`): render.rs, determinism.rs, the two `.snap` files, plus this SUMMARY.
- All 3 task commits verified in `git log` (`05bf639`, `3e4af19`, `7e57e12`).
- Plan `<verification>` re-run green: `snapshot_graph` + `snapshot_diagnostics` + `determinism` GREEN; `snapshot_openapi` + `snapshot_sdk` still RED-by-design; `cargo fmt --all -- --check` + `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `cargo test -p gnr8-core --lib` (22) + `cargo test -p gnr8` (9) pass; `make gates` (blocking) passes; `gnr8 inspect routes fixtures/goalservice` prints the table, `--json inspect schemas` emits valid JSON, `--json inspect graph` reports 4 ops / 9 schemas / 7 diagnostics.
- Determinism confirmed: two `build_graph` runs serialize byte-identically (GRAPH-02).
