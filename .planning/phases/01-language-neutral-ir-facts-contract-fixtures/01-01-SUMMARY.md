---
phase: 01-language-neutral-ir-facts-contract-fixtures
plan: 01
subsystem: api
tags: [ir, facts-contract, serde, type-system, go-sidecar, narrow-waist, deny-unknown-fields]

# Dependency graph
requires:
  - phase: v1.0 (Phases 1-5)
    provides: stringly-typed SchemaType/Schema.kind facts contract + ApiGraph IR + goextract sidecar
provides:
  - Closed, language-neutral Type enum (Primitive/WellKnown/Array/Map/Named/Object/Enum/Union/Any) in facts.rs and the IR
  - Independent optional + nullable field axes (all four combinations representable, tested)
  - Byte-identical Rust serde DTO <-> Go json-tag facts contract with an in-plan contract-drift guard
  - A single shared neutral vocabulary re-used by both the wire DTO and the IR (no duplicate definition)
affects: [01-02 (consumers lower/gosdk forced to update via compile errors), 01-03 (fixtures + red snapshots), Phase 2-5 (pyextract/tsextract sidecars emit this contract)]

# Tech tracking
tech-stack:
  added: []  # No new deps (CLAUDE.md rule 2). Rust + Go toolchains installed in-sandbox to verify.
  patterns:
    - "Adjacently-tagged Type enum ({\"type\":..,\"of\":..}); internally-tagged Prim; Any = empty struct variant for buffered-deserialize safety"
    - "IR re-exports the facts vocabulary (one definition, zero drift) rather than mirroring it"
    - "Two-bool optional/nullable axes parallel to the existing required/optional pair"
    - "In-plan contract-drift guard: Go marshals every variant + both axes; asserts key set == canonical Rust field-name list"

key-files:
  created:
    - .planning/phases/01-language-neutral-ir-facts-contract-fixtures/01-01-SUMMARY.md
  modified:
    - crates/gnr8-core/src/analyze/facts.rs
    - crates/gnr8-core/src/graph/mod.rs
    - goextract/internal/facts/facts.go
    - goextract/internal/facts/facts_test.go
    - goextract/internal/types/extract.go
    - goextract/internal/types/extract_test.go
    - goextract/internal/handlers/handlers.go
    - goextract/internal/handlers/handlers_test.go

key-decisions:
  - "Type enum tagging: adjacently-tagged ({\"type\":<variant>,\"of\":<payload>}) — handles the mixed newtype/struct/seq payloads internal tagging cannot; rejects unknown sibling keys."
  - "Prim is internally-tagged ({\"prim\":\"int\",\"bits\":64,\"signed\":true}) — flat, sidecar-friendly; only unit + struct variants so it round-trips when buffered."
  - "Type::Any is an empty struct variant (Any {}) -> {\"type\":\"any\",\"of\":{}} — a bare unit variant fails to deserialize when buffered inside a deny_unknown_fields struct."
  - "optional + nullable are parallel bool flags (RESEARCH Open Q2), mirroring required/optional."
  - "The IR re-exports facts::{Type,Prim,WellKnown,FieldFact} (graph::Field = FieldFact) — one definition for wire + IR."
  - "Type::Ext omitted this phase (no extension runtime is built — phase guardrails)."
  - "SecurityScheme.kind retained — it is rule-4 OpenAPI security metadata, not a type discriminator."

patterns-established:
  - "Closed neutral Type vocabulary replaces every kind: String discriminator (exhaustive match, no _ => catch-all)."
  - "Atomic dual-contract edit: facts.rs serde fields == facts.go json tags, guarded by a marshaled-key-set test."

requirements-completed: [IR-01, IR-02]

# Metrics
duration: 23min
completed: 2026-06-25
---

# Phase 1 Plan 01: Language-Neutral Type Enum + Facts Contract Summary

**Promoted the stringly-typed `SchemaType { kind: String }` to a closed, adjacently-tagged neutral `Type` enum (objects, arrays, enums, unions, maps, $ref, well-knowns, any) with independent optional/nullable axes, threaded byte-identically across the Rust serde facts DTO, the Go json-tag DTO, and the IR.**

## Performance

- **Duration:** 23 min
- **Started:** 2026-06-25T14:20:10Z
- **Completed:** 2026-06-25
- **Tasks:** 3
- **Files modified:** 8 (+1 summary created)

## Accomplishments
- A closed `Type` enum (`Primitive(Prim)`, `WellKnown(WellKnown)`, `Array`, `Map{key,value}`, `Named`, `Object`, `Enum`, `Union`, `Any{}`) replaces the `kind: String` discriminator in both `facts.rs` and the IR (`graph/mod.rs`) — a new variant is now a compile error in every consumer, not a runtime wrong-output bug.
- `optional` and `nullable` are independent bool axes on every field; all four combinations round-trip distinctly (unit-tested in both Rust and Go).
- The Go facts DTO mirrors the Rust serde field/variant names byte-for-byte (stdlib `encoding/json` only), with an in-plan contract-drift guard test that fails inside 01-01 if any tag drifts.
- The IR re-uses the facts vocabulary (one definition), preserves sort-once/store-sorted determinism (object fields by name, enum members lexically), and carries no Go-ism prose.

## Task Commits

Each task was committed atomically:

1. **Task 1: Neutral Type enum + optional/nullable axes in the Rust facts DTO** - `40557b1` (feat)
2. **Task 2: Mirror the neutral Type enum + nullable axis into the IR (graph/mod.rs)** - `fe6ef16` (feat)
3. **Task 3: Mirror the neutral Type enum into the Go facts DTO (stdlib only)** - `81b2340` (feat)

_TDD note: Tasks 1 & 2 are `tdd="true"` but reshape an existing contract whose implementation and tests change together; they were committed as single feat commits (test + impl inseparable for a contract migration), each with the new round-trip/reject/determinism tests green before commit._

## Files Created/Modified
- `crates/gnr8-core/src/analyze/facts.rs` - Neutral `Type`/`Prim`/`WellKnown` enums; `FieldFact.nullable`; `SchemaFact.body: Type`; strict deserialize preserved; full round-trip + reject + axes tests.
- `crates/gnr8-core/src/graph/mod.rs` - IR re-exports the neutral vocabulary; recursive `normalize_type` with exhaustive match; nullable carried through; neutralized prose; determinism tests.
- `goextract/internal/facts/facts.go` - Go `Type{type,of}` + `Prim` + `MapType` + kind/well-known constants + constructor helpers; recursive deterministic sort; stdlib only.
- `goextract/internal/facts/facts_test.go` - Contract-drift guard (canonical key-set), Any/Prim wire-form pins, updated determinism/sort tests.
- `goextract/internal/types/extract.go` - Producer lowered to the neutral vocabulary (Rule 3); pointer fields set optional+nullable; free-form maps -> Any.
- `goextract/internal/handlers/handlers.go` - Path/query param schema -> `PrimitiveType(StringPrim())` (Rule 3).
- `goextract/internal/types/extract_test.go`, `goextract/internal/handlers/handlers_test.go` - Producer tests updated to the neutral accessors (Rule 3).

## Decisions Made
- **Adjacent tagging for `Type`, internal tagging for `Prim`.** `Type` holds mixed payload shapes (newtype, struct, sequence) that internal tagging cannot encode; adjacent tagging (`{"type","of"}`) handles all of them and rejects unknown sibling keys. `Prim` has only unit + struct variants, so internal tagging gives a flatter, sidecar-friendlier form (`{"prim":"int","bits":64,"signed":true}`).
- **`Type::Any` is an empty struct variant (`Any {}`).** A bare unit variant in an adjacently-tagged enum fails to deserialize when the surrounding `deny_unknown_fields` struct buffers (serde demands the content key). The empty `{}` payload keeps `any` strict and round-trippable everywhere; Go mirrors it with an `emptyObject{}`.
- **optional/nullable as parallel bool flags** (RESEARCH Open Q2), mirroring the existing required/optional pair — lowest-risk, all four combinations reachable.
- **IR re-uses the facts enum** (re-export) instead of owning a mirror copy, eliminating drift between the wire DTO and the IR.
- **`Type::Ext` omitted** — no extension runtime is built this phase (phase guardrails); the §2a sketch's `Ext` variant is intentionally absent.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated the Go sidecar producers + their tests to the neutral shape**
- **Found during:** Task 3 (Go facts DTO)
- **Issue:** The plan's `files_modified` listed only `facts.go`, but the Task 3 acceptance gate requires `cd goextract && go build ./... && go vet ./...` to exit 0. The Go *producers* (`internal/types/extract.go`, `internal/handlers/handlers.go`) and their unit tests construct/assert the old `facts.SchemaType`/`SchemaFact.Kind/Fields/EnumValues` shape, which no longer compiles after the DTO change — `go build`/`go vet` fail.
- **Fix:** Lowered the producers to the neutral vocabulary via the new constructor helpers (`PrimitiveType`/`ArrayType`/`NamedType`/`WellKnownType`/`AnyType`/`ObjectType`/`EnumType`/`IntPrim`/`FloatPrim`/...). Go pointer fields now set both `optional` and `nullable`; free-form maps lower to `Any`. Updated the producer unit tests to assert the neutral shape (kept them passing, not just compiling).
- **Files modified:** goextract/internal/types/extract.go, goextract/internal/handlers/handlers.go, goextract/internal/types/extract_test.go, goextract/internal/handlers/handlers_test.go
- **Verification:** `go build ./...` (0), `go vet ./...` (0), `go test ./...` (all packages green).
- **Committed in:** 81b2340 (Task 3 commit)

**Note on the Rust consumers (NOT a deviation — intended by the plan):** `crates/gnr8-core/src/lower/` and `gosdk/` are deliberately left non-compiling against the new enum. This is the compile-error signal Plan 01-02 consumes; per the plan's `<verification>`, no `_ =>` arms or shims were added to make them compile. Consequence: the full `cargo build -p gnr8-core` fails by design — the asymmetry with the Go side (which the plan requires to be fully green) is intentional and explicit in the plan.

---

**Total deviations:** 1 auto-fixed (1 Rule 3 - blocking)
**Impact on plan:** The Rule-3 fix was mandatory for the Task 3 acceptance gate (`go build`/`go vet` exit 0). It stays within the contract-migration scope (no new behavior); the Go side is now fully green. No scope creep.

## Go-encoding shape (for 01-02 reference)
- `Type` -> `{"type": <variant>, "of": <payload>}`; variant tags: `primitive, well_known, array, map, named, object, enum, union, any`.
- `Prim` -> `{"prim": <variant>, "bits"?, "signed"?}`; variants: `string, bool, int, float, bytes`.
- `WellKnown` -> a `snake_case` string under `of` (`uuid, date_time, date, duration, decimal, email, uri`).
- `Map.of` -> `{"key": Type, "value": Type}`; `Array.of` -> `Type`; `Named.of` -> id string; `Object.of` -> `[]FieldFact`; `Enum.of` -> `[]string`; `Union.of` -> `[]Type`; `Any.of` -> `{}`.
- `FieldFact`: `json_name, required, optional, nullable, schema, description, example`.

## Issues Encountered
- **serde adjacently-tagged unit-variant + buffered `deny_unknown_fields` bug.** `{"type":"any"}` failed to deserialize when nested inside a `deny_unknown_fields` struct (serde buffers and demands the content key). Resolved by making `Any` an empty struct variant (`{"type":"any","of":{}}`).
- **serde adjacently-tagged struct-variant payload shape.** First-draft test JSON encoded `Prim::Int` inline; switching `Prim` to internal tagging gave the intended flat form and round-trips when buffered.

## Test Verification (this plan)
- Rust (with downstream `lower`/`gosdk` temporarily disabled to isolate, then restored): `analyze::facts` 10/10 green; `graph` 11/11 green incl. `serialization_is_byte_identical_across_two_runs`; `cargo clippy -p gnr8-core --lib -- -D warnings` clean.
- Go: `go build ./...` 0, `go vet ./...` 0, `go test ./...` all packages green (incl. `TestContractFieldNamesMatchRustDTO`, `TestAnyTypeCarriesEmptyObjectPayload`, `TestPrimWireForms`).
- Expected-failure check: full `cargo build -p gnr8-core` fails with `unresolved import crate::graph::SchemaType` / `no field kind on Schema` — the intended Plan 01-02 compile-error signal.

## Next Phase Readiness
- 01-02 ready: the closed enum will force `lower/` + `gosdk/` to update exhaustively (compile errors are the to-do list). Go-encoding shape documented above.
- 01-03 ready: the neutral graph/facts shape is the target the FastAPI/Flask/NestJS red snapshots encode.

## Self-Check: PASSED

- Files verified present: `01-01-SUMMARY.md`, `analyze/facts.rs`, `graph/mod.rs`, `goextract/internal/facts/facts.go`, `goextract/internal/facts/facts_test.go`.
- Commits verified present: `40557b1` (Task 1), `fe6ef16` (Task 2), `81b2340` (Task 3).

---
*Phase: 01-language-neutral-ir-facts-contract-fixtures*
*Completed: 2026-06-25*
