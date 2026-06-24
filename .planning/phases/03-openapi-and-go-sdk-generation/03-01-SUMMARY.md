---
phase: 03-openapi-and-go-sdk-generation
plan: 01
subsystem: api
tags: [rust, openapi, lowering, yaml, serde, insta, determinism, diagnostics, thiserror]

# Dependency graph
requires:
  - phase: 02-go-analysis-and-api-graph
    provides: "byte-stable ApiGraph (operations/schemas/params/responses/diagnostics, all pre-sorted) + the lower::to_openapi seam stub + the red-by-design snapshot_openapi contract test"
provides:
  - "lower::to_openapi(&ApiGraph) -> Result<String, CoreError> — graph lowered to a valid OpenAPI 3.1.0 YAML document"
  - "Typed OpenAPI 3.1 Rust model (lower/model.rs) reusable by any future emitter (JSON form is one serde_json call away)"
  - "Deterministic key-ordered YAML writer (lower/yaml.rs) — no YAML crate, byte-stable"
  - "Four new typed CoreError variants (Lowering/SdkGen/GoFmt/GoBuild) so 03-02 and 03-03 need no error.rs edit"
  - "Reviewed, committed snapshot_openapi .snap reconciled with expected/openapi.yaml"
affects: [03-02-go-sdk, 03-03-compile-smoke, 04-gnr8-workspace-lifecycle]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Hand-rolled typed OpenAPI 3.1 structs + a deterministic key-ordered YAML writer (no openapiv3/serde_yaml crate)"
    - "Vec<(String, T)> for every map-like construct (never a HashMap) so output is byte-stable"
    - "Typed CoreError on every un-representable graph fact (dangling $ref, unknown kind) — no prod unwrap/expect/panic"
    - "Cross-plan file-disjoint error variants: all four Phase-3 CoreError variants land in 03-01 so later plans stay edit-free in error.rs"

key-files:
  created:
    - crates/gnr8-core/src/lower/model.rs
    - crates/gnr8-core/src/lower/yaml.rs
    - crates/gnr8-core/tests/snapshots/snapshot_openapi__goalservice_openapi.snap
  modified:
    - crates/gnr8-core/src/error.rs
    - crates/gnr8-core/src/lower/mod.rs
    - crates/gnr8-core/tests/snapshot_openapi.rs

key-decisions:
  - "Open Q A3 resolved: the absolute /goal base prefix is joined in lowering from a private const BASE_PATH = \"/goal\" with slash-collapse, NOT by reshaping the Phase-2 graph (single-group PoC; multi-group deferred)"
  - "expected/openapi.yaml is a reference target, not the literal snapshot — the .snap was authored from real generated output, validated as parseable OpenAPI 3.1 (Ruby psych), and reviewed for semantic equivalence (Pitfall 2)"
  - "Schema-property descriptions added to the model + writer for a faithful document; descriptions are never emitted on a bare $ref node (JSON Schema sibling-key rule)"
  - "All four Phase-3 CoreError variants (Lowering/SdkGen/GoFmt/GoBuild) defined in 03-01 so 03-02/03-03 stay file-disjoint for parallel execution"

patterns-established:
  - "Typed OpenAPI model + hand-rolled deterministic YAML writer (key-ordered, two-space block, quoted $ref, no nullable, additionalProperties:true for free-form maps)"
  - "Manual insta accept flow (no cargo-insta): run with INSTA_UPDATE=new, review .new vs reference, strip assertion_line, rename to .snap"

requirements-completed: [OAPI-01, OAPI-02, OAPI-03]

# Metrics
duration: 13min
completed: 2026-06-24
---

# Phase 3 Plan 1: OpenAPI Lowering + Validation Snapshot Summary

**`lower::to_openapi` lowers the Phase-2 ApiGraph into a valid OpenAPI 3.1.0 YAML document — typed Rust structs + a deterministic key-ordered hand-rolled YAML writer, absolute `/goal/...` paths joined from a const, free-form maps surfaced as `additionalProperties: true`, dangling `$ref`/unknown-kind as typed `CoreError::Lowering` — flipping `snapshot_openapi` GREEN.**

## Performance

- **Duration:** 13 min
- **Started:** 2026-06-24T18:39:55Z (Phase 03 execution start)
- **Completed:** 2026-06-24T18:53:00Z
- **Tasks:** 3
- **Files modified:** 6 (3 created, 3 modified)

## Accomplishments
- `lower::to_openapi(&ApiGraph) -> Result<String, CoreError>` implemented as a pure graph→typed-doc transform (no re-analysis, D-02), emitting a valid OpenAPI 3.1.0 YAML document for the goalservice fixture (paths `/goal/`, `/goal/list`, `/goal/{uuid}` with PUT+DELETE coexisting; operations with operationId/summary/tags; path+query params with enum/required; requestBody; responses by status; `components.schemas` with `$ref`/required/format/additionalProperties; `components.securitySchemes` ApiKeyAuth).
- Typed OpenAPI 3.1 Rust model (`lower/model.rs`) + a ~180-line deterministic key-ordered YAML writer (`lower/yaml.rs`) — no `openapiv3`/`serde_yaml` crate; `Vec<(K,V)>` everywhere so output is byte-stable.
- Open Q A3 resolved: the absolute `/goal` prefix is joined in lowering from a private `const BASE_PATH` with slash-collapse, without reshaping the Phase-2 graph.
- OAPI-03 diagnostics surfaced non-fatally: `to_openapi` never re-derives, drops, or panics on the Phase-2 diagnostics; the representational decision (free-form map → `additionalProperties: true`) is applied.
- Four new typed `CoreError` variants (`Lowering`/`SdkGen`/`GoFmt`/`GoBuild`) so 03-02/03-03 need no `error.rs` edit.
- `snapshot_openapi` flipped red→GREEN with a reviewed, committed `.snap`; `snapshot_sdk` stays red-by-design; `snapshot_graph`/`snapshot_diagnostics`/`determinism` unchanged (no regression).

## Task Commits

Each task was committed atomically:

1. **Task 1: Add the four Phase-3 CoreError variants + resolve Open Q A3** - `12cb6cc` (feat)
2. **Task 2: Typed OpenAPI 3.1 model + deterministic YAML writer** - `6310bcd` (feat)
3. **Task 3: Implement to_openapi mapping + flip snapshot_openapi GREEN** - `88dec70` (feat)

_TDD note: variants/model/writer/mapper were each developed RED→GREEN (failing tests first, then implementation); each task is squashed to one atomic feat commit._

## Files Created/Modified
- `crates/gnr8-core/src/lower/model.rs` (created) - Typed, serde-derivable OpenAPI 3.1 structs (`OpenApiDoc`, `Info`, `PathItem`, `Operation`, `Parameter`, `RequestBody`, `ResponseObj`, `Components`, `SchemaObject`, `SecurityScheme`); all map-like fields are `Vec<(String, T)>`.
- `crates/gnr8-core/src/lower/yaml.rs` (created) - Hand-rolled block-style YAML writer with fixed spec key order, quoted JSON-pointer `$ref`, `additionalProperties: true`, no `nullable`, `format` alongside `type`; total fn returning `String` + 5 unit tests.
- `crates/gnr8-core/src/lower/mod.rs` (modified) - `to_openapi` graph→doc mapping, `/goal` base-path join (`join_base`), `$ref` resolution to bare component names, security-scheme collection, A3 + OAPI-03 doc comments, 9 unit tests.
- `crates/gnr8-core/src/error.rs` (modified) - Added `Lowering`/`SdkGen`/`GoFmt`/`GoBuild` variants + four Display tests.
- `crates/gnr8-core/tests/snapshot_openapi.rs` (modified) - Header flipped red-by-design → GREEN contract test.
- `crates/gnr8-core/tests/snapshots/snapshot_openapi__goalservice_openapi.snap` (created) - Reviewed OpenAPI 3.1 snapshot reconciled with `expected/openapi.yaml`.

## Decisions Made
- **Open Q A3 (base path):** Join `/goal` in lowering from a private `const BASE_PATH = "/goal"` with slash-collapse (`/goal/`, `/goal/list`, `/goal/{uuid}`), per RESEARCH recommendation (a) — no Phase-2 graph reshape. Single-group PoC; multi-group generalization deferred.
- **Snapshot reconciliation (Pitfall 2):** Authored the `.snap` from real generated output, validated it parses as OpenAPI 3.1 (Ruby `psych` — PyYAML unavailable), and reviewed it semantically against `expected/openapi.yaml` rather than byte-copying. Incidental differences are all faithful to the graph (the source of truth): response descriptions default to `Response <status>` where the graph carries none; params render in graph (name-sorted) order; `deleteGoal` carries the 200 + 400 responses the real graph extracted.
- **Property descriptions:** Added a `description` field to `SchemaObject` and the writer so the document carries the field docs the graph already holds — never emitted beside a bare `$ref`.
- **Cross-plan error variants:** All four Phase-3 `CoreError` variants land here so 03-02/03-03 stay file-disjoint.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added `description` to `SchemaObject` model + YAML writer**
- **Found during:** Task 3 (to_openapi mapping)
- **Issue:** The Task-2 model omitted a property-level `description` field, but the graph carries field descriptions and the reference target (`expected/openapi.yaml`) includes them; the mapper referenced `prop.description`, failing to compile, and the document would have been less faithful without it.
- **Fix:** Added `description: Option<String>` to `SchemaObject`, emitted it after `type`/`format` in the writer's fixed key order (never on a bare `$ref` node), and updated the writer's key-order doc comment.
- **Files modified:** crates/gnr8-core/src/lower/model.rs, crates/gnr8-core/src/lower/yaml.rs
- **Verification:** Full lib suite + clippy green; generated `.snap` shows correct per-field descriptions; YAML still parses as valid OpenAPI 3.1.
- **Committed in:** 88dec70 (Task 3 commit)

**2. [Rule 3 - Blocking] Clippy `-D warnings` fixes in new code**
- **Found during:** Tasks 2 and 3
- **Issue:** `-D warnings` flagged: `dead_code` on the model/writer (unused until Task 3 wired them), `doc-markdown` (`PoC`, `JSON-Schema-2020-12`), `struct_field_names` (`Operation.operation_id`), `items-after-statements` (a const mid-function), `single_match_else`, and `clippy::panic` in a test module.
- **Fix:** Scoped `#[allow(dead_code)]` on the two submodules with a "wired in Task 3" comment (removed once Task 3 consumed them); backticked the doc terms; scoped `#[allow(clippy::struct_field_names)]` on `Operation` (keeps the spec term `operationId`); hoisted the const to module scope and replaced an index `[0]` with a non-panicking `.first()`; converted the accumulator `match` to `if let ... else`; added `clippy::panic` to the test-module scoped allow.
- **Files modified:** crates/gnr8-core/src/lower/{mod.rs, model.rs, yaml.rs}
- **Verification:** `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `cargo fmt --all -- --check` clean.
- **Committed in:** 6310bcd and 88dec70 (task commits)

---

**Total deviations:** 2 auto-fixed (1 missing-critical, 1 blocking). **Impact on plan:** Both were necessary for correctness/faithfulness and the workspace lint gate; no scope creep — the OpenAPI document is richer (field descriptions) and all gates pass.

## Issues Encountered
- **No `cargo-insta` and no PyYAML in the environment.** Used the documented manual insta accept flow (`INSTA_UPDATE=new` → review `.new` → strip `assertion_line` → rename to `.snap`) and validated the generated YAML with Ruby `psych` (present) instead of PyYAML (absent) to confirm it parses as valid OpenAPI 3.1 with the expected paths/schemas/security/refs. Resolved within the task.

## User Setup Required
None - no external service configuration required (the Go toolchain, used by the snapshot test to build the graph, is already a hard project dependency and present at go1.26.2).

## Next Phase Readiness
- **Ready for 03-02 (Go SDK):** `CoreError::SdkGen`/`GoFmt` are defined, so 03-02 needs no `error.rs` edit; the typed model + writer patterns are a template for the SDK bundle emitter; `snapshot_sdk` remains red-by-design awaiting `sdk::generate`.
- **Ready for 03-03 (compile/smoke + CI promotion):** `CoreError::GoBuild` is defined; `snapshot_openapi` is now green and can be promoted into the blocking CI gate alongside `snapshot_sdk` once 03-02 lands.
- No blockers. Three of the four contract tests (`snapshot_graph`, `snapshot_diagnostics`, `snapshot_openapi`) are green; only `snapshot_sdk` remains red-by-design.

## Self-Check: PASSED

- Created files exist: `crates/gnr8-core/src/lower/model.rs`, `crates/gnr8-core/src/lower/yaml.rs`, `crates/gnr8-core/tests/snapshots/snapshot_openapi__goalservice_openapi.snap`, `.planning/phases/03-openapi-and-go-sdk-generation/03-01-SUMMARY.md` — all FOUND.
- Task commits exist: `12cb6cc`, `6310bcd`, `88dec70` — all FOUND.
- `snapshot_openapi` GREEN; `snapshot_sdk` red-by-design; `snapshot_graph`/`snapshot_diagnostics`/`determinism` green (no regression); `cargo fmt --check` + `cargo clippy -D warnings` clean; 40 lib tests pass.

---
*Phase: 03-openapi-and-go-sdk-generation*
*Completed: 2026-06-24*
