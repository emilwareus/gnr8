---
phase: 01-language-neutral-ir-facts-contract-fixtures
fixed_at: 2026-06-25T00:00:00Z
review_path: .planning/phases/01-language-neutral-ir-facts-contract-fixtures/01-REVIEW.md
iteration: 1
findings_in_scope: 8
fixed: 8
skipped: 0
status: all_fixed
---

# Phase 1: Code Review Fix Report

**Fixed at:** 2026-06-25
**Source review:** .planning/phases/01-language-neutral-ir-facts-contract-fixtures/01-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 8 (CR-01..CR-04 blockers, WR-01..WR-04 warnings)
- Fixed: 8
- Skipped: 0

All fixes preserve the phase invariants: the 6 fixture snapshot tests remain RED-BY-DESIGN
(still `#[ignore]`-marked, still panic honestly via `.expect("...lands in Phase N")` because
no extractor exists yet, still excluded from the green `make check` gate). After the fixes,
`make check` exits 0 (GREEN), and `cargo test -p gnr8-core --no-fail-fast -- --ignored`
reports exactly **6 FAILED** (fastapi/flask/nestjs × graph+openapi), each failing via the
honest `.expect()` panic — NOT a snapshot mismatch. The existing GREEN goalservice OpenAPI
and SDK snapshots are unaffected by the CR-03 writer change (goalservice has no response-less
operation).

## Fixed Issues

### CR-01: Nullable-union acceptance snapshots asserted a flat 3-member `oneOf` the lowering cannot emit

**Files modified:** `crates/gnr8-core/tests/snapshots/snapshot_fastapi_openapi__fastapi_openapi.snap`, `crates/gnr8-core/tests/snapshots/snapshot_flask_openapi__flask_openapi.snap`
**Commit:** 37148c5
**Applied fix:** Corrected the SNAPSHOT to the **nested** `oneOf` shape `lower::lower_field_schema`
provably emits (the source of truth for the rendered shape), NOT the flat form. A nullable union
lowers to `oneOf: [ <union oneOf>, {type: null} ]`. Both `Book.rating` (FastAPI) and
`OrderInput.discount` (Flask) now read:
```yaml
oneOf:
- oneOf:
  - type: integer
  - type: number
- type: null
```
Did NOT change lowering to match the snapshot — lowering is authoritative. The exact byte form
was verified empirically by the new `nullable_union_field_lowers_to_nested_oneof_with_null` unit
test (see WR-01), which asserts the byte-identical nested rendering against real
`to_openapi` output.

**Note on file grouping:** The FastAPI openapi `.snap` carries both CR-01 (`rating`) and CR-02
(`sort`) edits; since git commits whole files and they share one file, the FastAPI CR-02 edit
rode along in this CR-01 commit. The CR-02 commit (8fcc3a9) carries the NestJS snapshot.

### CR-02: Nullable-enum acceptance snapshots asserted a `oneOf` wrapper the lowering cannot emit

**Files modified:** `crates/gnr8-core/tests/snapshots/snapshot_nestjs_openapi__nestjs_openapi.snap` (and the FastAPI snapshot edit landed in 37148c5 — same file as CR-01)
**Commit:** 8fcc3a9 (NestJS); FastAPI portion in 37148c5
**Applied fix:** Corrected the SNAPSHOT to the `type: [string, null]` + `enum` form the lowering
emits. A `Type::Enum` lowers to `type: string` + `enum` (neither `$ref` nor `one_of`), so the
nullable wrap falls through to the `nullable: true, ..lowered` branch, rendering the 3.1
type-array form (valid JSON-Schema-2020-12):
```yaml
sort:
  type: [string, null]
  enum: [asc, desc]
```
`BookFilters.sort` corrected in both FastAPI and NestJS snapshots. Byte form locked by the new
`nullable_enum_field_lowers_to_type_array_with_enum` unit test (WR-01).

### CR-03: Flask OpenAPI snapshot asserted `responses: {}` but the writer emitted a bare null `responses:`

**Files modified:** `crates/gnr8-core/src/lower/yaml.rs`
**Commit:** 1de5c2b
**Applied fix:** This was a genuine LOWERING bug. Fixed `write_responses` to emit the explicit
empty map `responses: {}` (valid, deterministic OpenAPI — the `responses` object is REQUIRED and
a YAML null is invalid) for an empty response list, returning early before the loop. The Flask
OpenAPI snapshot already encoded `responses: {}`, so it now matches the corrected writer output
(no snapshot edit needed for CR-03). Verified the goalservice OpenAPI + SDK snapshots are
unchanged (they have no response-less operation, so the new branch is never taken for them);
both still pass in `make check`. Byte form locked by the new
`response_less_operation_renders_empty_responses_map` unit test (WR-01).

### CR-04: Flask graph acceptance snapshot listed diagnostics in an order the graph re-sorts away

**Files modified:** `crates/gnr8-core/tests/snapshots/snapshot_flask_graph__flask_graph.snap`
**Commit:** aaa0411
**Applied fix:** Corrected the SNAPSHOT to the deterministic `(file, line, message)` sort
`ApiGraph::from_facts` applies (`graph/mod.rs:256-261`). All three diagnostics share file
`app/routes.py`, so they reorder by line ascending: **42, 69, 78** (was 78, 42, 69).

### WR-01: No unit test covered nullable-union or nullable-enum lowering

**Files modified:** `crates/gnr8-core/src/lower/mod.rs`
**Commit:** dbbab89
**Applied fix:** Added three lowering unit tests to the GREEN `lower::tests` module (not the
ignored red snapshots), each asserting the byte-exact rendered form so the chosen shapes are
locked by the green gate:
- `nullable_union_field_lowers_to_nested_oneof_with_null` (locks CR-01's nested-oneOf contract)
- `nullable_enum_field_lowers_to_type_array_with_enum` (locks CR-02's `type:[string,null]`+enum)
- `response_less_operation_renders_empty_responses_map` (locks CR-03's `responses: {}`)
All pass under `cargo test -p gnr8-core --lib`.

### WR-02: Go extractor mapped all unsigned integer widths to signed `IntPrim(64, true)`

**Files modified:** `goextract/internal/types/extract.go`
**Commit:** 2c1319f
**Applied fix:** Split the integer arm in `mapBasic` so the unsigned kinds
(`Uint, Uint8..Uint64`) map to `facts.IntPrim(64, false)`, faithfully carrying the `signed` axis
the neutral IR provides (one source of truth per fact — CLAUDE.md rule 3). Signed kinds keep
`IntPrim(64, true)`. `go build`/`go vet`/`go test ./...` all clean.

### WR-03: `mapBasic` default arm silently coerced unknown basic kinds to `string`

**Files modified:** `goextract/internal/types/extract.go`, `goextract/internal/diag/diag.go`
**Commit:** 2c1319f
**Applied fix:** The `default:` arm now emits a diagnostic and returns the HONEST free-form
`facts.AnyType()` instead of fabricating a `string` fact with no evidence (GO-06 / CLAUDE.md
rule 3: diagnose, never guess). Added a dedicated `(*Accumulator).UnsupportedType` helper to
`diag.go`, mirroring the machine-stable rule + struct.field + declared-type identity pattern of
`Floatf`/`FreeFormMap`. `go vet` clean (no `_ =>`-style catch-all that swallows the kind).

### WR-04: `typeString` wrapped `gotypes.TypeString` in a no-op `fmt.Sprintf("%s", ...)`

**Files modified:** `goextract/internal/types/extract.go`
**Commit:** 2c1319f
**Applied fix:** Returned the `gotypes.TypeString(...)` value directly (it is already a string;
the `interface{}`→`any` normalization is done by `TypeString` itself, not the wrapper). Removed
the now-unused `"fmt"` import. This clears the `go vet` simplify (S1025) signal and the redundant
allocation. `go build`/`go vet`/`go test ./...` clean.

---

_Fixed: 2026-06-25_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
