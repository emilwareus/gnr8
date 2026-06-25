---
phase: 01-language-neutral-ir-facts-contract-fixtures
plan: 02
subsystem: api
tags: [ir, openapi-3.1, go-sdk, exhaustive-match, optional-nullable, narrow-waist, snapshots]

# Dependency graph
requires:
  - phase: 01-01
    provides: Closed neutral Type enum (Primitive/WellKnown/Array/Map/Named/Object/Enum/Union/Any) + independent optional/nullable axes in facts.rs and the IR; goextract already emits nullable from pointer detection
provides:
  - OpenAPI lowering (lower/) consumes the neutral Type enum with an EXHAUSTIVE match (no _ => / other => catch-all, no per-language branch)
  - Go SDK target (gosdk/) consumes the neutral Type enum exhaustively; all Go-isms (time.Time, int64, float32, map[string]any, []byte) stay LOCAL to the target
  - nullable drives the Go pointer (*T) + OpenAPI type:[T,null] / oneOf-with-null; optional drives ,omitempty + required-omission — the conflation is fixed end-to-end
  - OpenAPI 3.1 nullability rendered via type:["T","null"] (and oneOf:[{$ref},{type:null}] for nullable refs); the factually-wrong 3.1 comment corrected
  - goalservice graph/openapi/sdk snapshots GREEN at the neutral shape, byte-identical across runs
affects: [01-03 (red fixtures encode this neutral OpenAPI/graph shape), Phase 2-5 (pyextract/tsextract feed these now-neutral consumers unchanged)]

# Tech tracking
tech-stack:
  added: []  # No new deps (CLAUDE.md rule 2). Rust + Go toolchains used to verify.
  patterns:
    - "Exhaustive match on the closed Type enum in every consumer; capability gaps (e.g. Union in Go) are EXPLICIT typed-error arms, never _ => catch-alls"
    - "Per-target type mapping stays in the target: lowering emits neutral OpenAPI primitive/format names; gosdk owns DateTime->time.Time, Int->int64, Float->float32"
    - "nullable (value axis) and optional (presence axis) are read by distinct mechanisms: maybe_pointer reads nullable, json_tag reads optional"
    - "3.1 nullability: type:[T,null] array form for typed schemas; oneOf:[{$ref},{type:null}] for a bare $ref (which cannot carry a sibling type)"

key-files:
  created:
    - .planning/phases/01-language-neutral-ir-facts-contract-fixtures/01-02-SUMMARY.md
  modified:
    - crates/gnr8-core/src/lower/mod.rs
    - crates/gnr8-core/src/lower/model.rs
    - crates/gnr8-core/src/lower/yaml.rs
    - crates/gnr8-core/src/gosdk/emit.rs
    - crates/gnr8-core/src/gosdk/mod.rs
    - crates/gnr8-core/src/lifecycle/mod.rs
    - crates/gnr8-core/src/error.rs
    - crates/gnr8/src/render.rs
    - crates/gnr8-core/tests/snapshots/snapshot_graph__goalservice_graph.snap
    - crates/gnr8-core/tests/snapshots/snapshot_openapi__goalservice_openapi.snap
    - crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap

key-decisions:
  - "SchemaObject gained `nullable: bool` (renders 3.1 type:[T,null]), `one_of: Vec<SchemaObject>` (Union + nullable-$ref oneOf), and `additional_properties_schema: Option<Box<SchemaObject>>` (typed Map value schema)."
  - "Nullable $ref renders oneOf:[{$ref},{type:null}] (JSON-Schema-2020-12 correct: a $ref ignores sibling keys, so the null member must be a oneOf branch)."
  - "Neutral Prim::Int{bits}/Float{bits} lower to bare OpenAPI type:integer/number WITHOUT a `format` (width is a target concern, not a neutral OpenAPI format) — the SDK still narrows Int->int64/Float->float32 locally."
  - "maybe_pointer's `optional` parameter renamed to `nullable`; the pointer condition wraps *T iff nullable && is_value. json_tag keeps reading optional for ,omitempty."
  - "Union is unrepresentable in Go (no sum types): go_type emits an EXPLICIT CoreError::SdkGen capability-gap arm, not a catch-all (the Go fixture exercises no unions)."

patterns-established:
  - "Every consumer of the closed Type enum matches all variants explicitly; a new variant is a compile error, never a silent wrong-output."
  - "Snapshot re-acceptance is deliberate + reviewed: the SDK churn is a single semantically-correct field flip, not drift."

requirements-completed: [IR-03]

# Metrics
duration: 15min
completed: 2026-06-25
---

# Phase 1 Plan 02: IR Consumers Adopt the Neutral Type Enum Summary

**Converted OpenAPI lowering and the Go SDK target to consume the closed neutral `Type` enum with exhaustive matches (no catch-alls, no per-language branches), fixed the optional-vs-nullable conflation end-to-end (nullable -> `*T` + `type:[T,null]`; optional -> `,omitempty` + required-omission), corrected the wrong OpenAPI-3.1 nullable comment, and deliberately re-accepted the goalservice snapshots to the neutral shape.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-06-25T14:47:58Z
- **Completed:** 2026-06-25
- **Tasks:** 3
- **Files modified:** 11 (+1 summary created)

## Accomplishments
- `lower/` lowers every neutral `Type` variant with an exhaustive match (`Primitive`/`WellKnown`/`Array`/`Map`/`Named`/`Object`/`Enum`/`Union`/`Any`) — both `lower_named_schema` and `lower_schema_type` lost their `other => Err(...)` catch-alls; a future variant fails to compile here.
- `gosdk/` maps every variant exhaustively with ALL Go-isms LOCAL to the target; `Union` is an explicit capability-gap error arm (Go has no sum types).
- The optional/nullable conflation (RESEARCH Pitfall 4) is fixed: `maybe_pointer` wraps `*T` on the NULLABLE axis; `json_tag` adds `,omitempty` on the OPTIONAL axis. Proven on the real fixture — `IncludePast` (an optional-not-nullable Go `bool`) is now emitted as `bool` not `*bool`.
- OpenAPI 3.1 nullability renders correctly: `type:["T","null"]` for typed schemas, `oneOf:[{$ref},{type:null}]` for nullable refs; the factually-wrong `model.rs` 3.1 comment is rewritten.
- The goalservice graph/openapi/sdk snapshots are GREEN at the neutral shape and byte-identical across runs; the Go sidecar pipeline (`go build`/`vet`/`test`) stays green.

## Task Commits

Each task was committed atomically:

1. **Task 1: Exhaustive neutral Type -> OpenAPI 3.1 lowering + 3.1 nullability + corrected comment** - `a7e3c94` (feat)
2. **Task 2: Go SDK target consumes the neutral Type enum; fix maybe_pointer optional/nullable conflation** - `58baa56` (feat)
3. **Task 3: Deliberately re-accept goalservice snapshots to the neutral-enum + nullable/optional shape** - `fa69262` (test)

## SchemaObject changes (Task 1)
Added three fields to `lower/model.rs::SchemaObject`:
- `nullable: bool` — the writer renders `type` as the 3.1 array form `["<type>", "null"]` instead of the scalar form when set.
- `one_of: Vec<SchemaObject>` — emitted as a `oneOf:` block sequence; carries a `Type::Union`'s variants OR the nullable-`$ref` form `[{$ref}, {type: null}]`.
- `additional_properties_schema: Option<Box<SchemaObject>>` — a typed `Map { value, .. }`'s value schema (rendered under `additionalProperties:`); takes precedence over the `additional_properties: Some(true)` free-form-map flag.

The yaml writer (`lower/yaml.rs`) learned: the `[T,"null"]` type-array rendering, a `oneOf` block-sequence emitter (`write_schema_seq_item`), and the typed-`additionalProperties` rendering.

## maybe_pointer signature change (Task 2)
`fn maybe_pointer(base, optional, is_value)` -> `fn maybe_pointer(base, nullable, is_value)`; the body wraps `*T` iff `nullable && is_value`. `go_type`'s second parameter was likewise renamed `optional` -> `nullable`. The field-emission call site passes `field.nullable` to `go_type`/`maybe_pointer` and `field.optional` to `json_tag`. Param call sites pass `false` for the pointer axis (params are not nullable) and continue to pointer-wrap optional query params in their own params-struct logic (unchanged).

## goalservice snapshot churn (Task 3) — exactly which fields changed and why each is correct

**SDK (`sdk/models.go`): one field changed.**
- `IncludePast *bool ... omitempty` -> `IncludePast bool ... omitempty`. The fixture declares `IncludePast bool json:"includePast,omitempty"` — a non-pointer bool with omitempty, i.e. optional-but-NOT-nullable. The old code pointer-wrapped on `optional`; the fix wraps on `nullable=false`, so the SDK now matches the field's real Go declaration. This is the conflation fix proven on the real fixture. No other SDK field changed (all other value types that became `*T` are genuinely nullable pointer fields in the fixture).

**OpenAPI (`openapi.yaml`):**
- Nullable scalars (`targetValue` `*float64`, `nextCursor` `*uuid`) now render `type: [number, null]` / `type: [string, null]` (the 3.1 array form) instead of a bare scalar — the nullable axis is now expressed.
- Nullable `$ref`s (`targetDirection`, `analyticsQuery` — both `*` enum/struct fields) now render `oneOf: [{$ref}, {type: null}]` instead of a bare `$ref` — a $ref cannot carry a sibling type, so the null member is a oneOf branch.
- `format: int64` dropped from integer fields (`windowDays`, `pageSize`, `total`). The neutral `Prim::Int{bits:64}` lowers to bare `type: integer` — the 64-bit width is a TARGET concern (the Go SDK still narrows to `int64`), not a neutral OpenAPI format. Intended IR-03 consequence; the SDK is unaffected (its int64 mapping is local to gosdk).

**Graph (`graph.yaml`):** the schema/facts now express types via the neutral enum shape (`type: primitive|well_known|object|enum|array|named|any`, `prim:` width tags) instead of `kind: String`, and every field carries the `nullable` axis. No machine-absolute path and no `kind: string` artifact remain.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated two additional in-crate consumers of the closed enum**
- **Found during:** Task 1 (build) and Task 3 (`cargo test --workspace`).
- **Issue:** The plan's `files_modified` listed `lower/` + `gosdk/`, but the closed-enum migration broke two OTHER consumers that read the removed `Schema.kind`/`.fields`/`.enum_values`: `crates/gnr8-core/src/lifecycle/mod.rs` (the rename rewriter that walks schema bodies + param schemas) and `crates/gnr8/src/render.rs` (the `inspect schemas|graph` CLI renderer). The lib (Task 1/2) and the `gnr8` binary (`cargo test --workspace`) do not compile without these.
- **Fix:** Converted `lifecycle::rewrite_schema_type_ref` to an exhaustive walk over the neutral `Type` (rewriting `Type::Named` ids, recursing through Array/Map/Object/Union); updated the apply loop to walk `schema.body`. Added neutral `schema_kind`/`field_count`/`enum_members` helpers to `render.rs` deriving the CLI's KIND/FIELDS/ENUM columns from the neutral body (exhaustive matches, no catch-all). Also corrected two stale doc comments referencing `SchemaType` (`error.rs`, `lifecycle/mod.rs`).
- **Files modified:** crates/gnr8-core/src/lifecycle/mod.rs, crates/gnr8/src/render.rs, crates/gnr8-core/src/error.rs.
- **Committed in:** `a7e3c94` (lifecycle/error with Task 1), `fa69262` (render.rs with Task 3).

**Note (NOT a deviation):** No goextract change was needed in Task 3. Plan 01-01 Task 3 already populated the Go sidecar's `nullable` from pointer detection (`isPointer(f.Type())`), so the snapshot reflects the real nullable axis without touching `facts.go`/`extract.go`. The plan's Task 3 conditional ("IF the goextract sidecar must emit nullable... make the minimal change") did not apply.

---

**Total deviations:** 1 auto-fixed (Rule 3 - blocking, two extra consumers).
**Impact on plan:** The Rule-3 fixes were mandatory for the workspace to compile and for `cargo test --workspace --locked` to pass (a plan verification gate). They stay within the closed-enum migration scope (no new behavior), keep every consumer's match exhaustive, and introduce no Go-ism into neutral code.

## Verification (this plan)
- `cargo test -p gnr8-core --lib lower` / `--lib gosdk`: green (incl. nullable-not-optional [T,null]+required, optional-not-nullable scalar+omitted, nullable-$ref oneOf, union oneOf, and the three SDK optional/nullable pointer/omitempty unit tests).
- `cargo test -p gnr8-core --test snapshot_graph --test snapshot_openapi --test snapshot_sdk --test determinism`: all GREEN against the re-accepted `.snap` files; byte-identical re-runs.
- `cargo test --workspace --locked`: all green (gnr8-core lib 119, integration suites, gnr8 bin).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- Go sidecar: `go build ./...` (0), `go vet ./...` (0), `go test ./...` (all packages green) — the strict-deserialize boundary (T-01-05) re-exercised via the real sidecar through `build_graph`.
- IR-03 grep gates: no Go-ism (`time.Time`/`int64`/`float32`/`go_type`) and no per-language branch in `lower/mod.rs`; `nullable` present in `model.rs`; the wrong 3.1 comment is gone; `maybe_pointer` reads `nullable`. (The few `_ =>`/`other =>` arms remaining in both files are on HTTP-method / Go-type-string / word-split matches — never on a `Type` match.)

## Threat surface
No new runtime input surface introduced. T-01-05 (facts deserialize) re-validated by running the real goextract sidecar through `build_graph` under `deny_unknown_fields` in the snapshot suite. T-01-03 (no catch-all swallowing a future variant) enforced: every `Type` match in `lower/`, `gosdk/`, `lifecycle/`, and the CLI renderer is exhaustive. T-01-04 (no absolute paths in snapshots) confirmed by grep.

## Next Phase Readiness
- 01-03 ready: the neutral OpenAPI + graph shape the FastAPI/Flask/NestJS red snapshots must encode is now the real, green output shape for the Go fixture (the structural template). nullable -> type:[T,null]/oneOf and optional -> omitempty/required-omission are the cross-language semantics the new fixtures exercise.

## Self-Check: PASSED
