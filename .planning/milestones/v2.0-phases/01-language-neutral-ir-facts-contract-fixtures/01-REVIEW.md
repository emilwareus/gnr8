---
phase: 01-language-neutral-ir-facts-contract-fixtures
reviewed: 2026-06-25T00:00:00Z
depth: standard
files_reviewed: 24
files_reviewed_list:
  - crates/gnr8-core/src/analyze/facts.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/gosdk/emit.rs
  - crates/gnr8-core/src/gosdk/mod.rs
  - crates/gnr8-core/src/graph/mod.rs
  - crates/gnr8-core/src/lower/mod.rs
  - crates/gnr8-core/src/lower/model.rs
  - crates/gnr8-core/src/lower/yaml.rs
  - crates/gnr8-core/src/render.rs
  - crates/gnr8-core/tests/snapshot_fastapi_graph.rs
  - crates/gnr8-core/tests/snapshot_fastapi_openapi.rs
  - crates/gnr8-core/tests/snapshot_flask_graph.rs
  - crates/gnr8-core/tests/snapshot_flask_openapi.rs
  - crates/gnr8-core/tests/snapshot_nestjs_graph.rs
  - crates/gnr8-core/tests/snapshot_nestjs_openapi.rs
  - crates/gnr8-core/tests/snapshots/snapshot_fastapi_graph__fastapi_graph.snap
  - crates/gnr8-core/tests/snapshots/snapshot_fastapi_openapi__fastapi_openapi.snap
  - crates/gnr8-core/tests/snapshots/snapshot_flask_graph__flask_graph.snap
  - crates/gnr8-core/tests/snapshots/snapshot_flask_openapi__flask_openapi.snap
  - crates/gnr8-core/tests/snapshots/snapshot_nestjs_openapi__nestjs_openapi.snap
  - goextract/internal/facts/facts.go
  - goextract/internal/facts/facts_test.go
  - goextract/internal/handlers/handlers.go
  - goextract/internal/types/extract.go
findings:
  critical: 4
  warning: 4
  info: 3
  total: 11
status: issues_found
---

# Phase 1: Code Review Report

**Reviewed:** 2026-06-25
**Depth:** standard
**Files Reviewed:** 24
**Status:** issues_found

## Summary

This phase ships the language-neutral IR / facts contract (`analyze::facts`, `graph`),
the OpenAPI lowering (`lower/`), the Go-SDK emitter (`gosdk/emit.rs`), the Go sidecar
facts contract (`goextract/internal/facts`), three new fixture services
(FastAPI / Flask / NestJS), and red-by-design acceptance snapshots for those fixtures.

The production Rust code is in good shape on the project invariants the priorities call
out: every neutral `Type` consumer in `lower/` and `gosdk/` uses an **exhaustive match
with no `_ =>` catch-all** (rule 3 ✓); `gosdk/emit.rs` treats nullable→`*T` and
optional→`,omitempty` as **independent axes** (✓); `goextract/internal/facts/facts.go`
is **stdlib-only** (✓); no **new OSS deps** are added to `gnr8-core` (✓); the fixtures
derive schema facts from **each language's own type system** and avoid every banned
schema-tool annotation (rule 1 ✓).

The serious problems are in the **hand-authored red-by-design acceptance snapshots**,
which are themselves the deliverable of this phase (they are the contract Phases 2/4 must
satisfy). Four of them encode shapes the existing `lower::to_openapi` + `lower::yaml`
writer **provably cannot produce**, and one encodes diagnostics in an order the graph
**provably re-sorts away**. Because those snapshots are the acceptance contract, a *correct*
future extractor would still make these tests fail — the contract is self-inconsistent
with the very lowering code that will render it. These are tracked as BLOCKER findings
below, each with the exact lowering path that contradicts the snapshot.

## Critical Issues

### CR-01: Nullable-union acceptance snapshots assert a flat 3-member `oneOf` the lowering cannot emit (it produces a nested `oneOf`)

**File:** `crates/gnr8-core/tests/snapshots/snapshot_fastapi_openapi__fastapi_openapi.snap:120-124` (also `snapshot_flask_openapi__flask_openapi.snap:95-99`)
**Issue:**
The graph snapshots encode `Book.rating` (FastAPI) and `OrderInput.discount` (Flask) as a
field with `nullable: true` whose `schema` is a `Type::Union([int, float])`
(`snapshot_fastapi_graph__...snap:199-216`, `snapshot_flask_graph__...snap:175-192`). The
OpenAPI snapshots assert this lowers to a **flat** three-member `oneOf`:

```yaml
rating:
  oneOf:
  - type: integer
  - type: number
  - type: null
```

But `lower::lower_field_schema` (`crates/gnr8-core/src/lower/mod.rs:371-393`) lowers a
nullable union by first calling `lower_schema_type`, which for a `Union` returns a
`SchemaObject` whose `one_of` is already non-empty (`mod.rs:445-454`). The nullable wrap
then hits the `!lowered.one_of.is_empty()` branch (`mod.rs:382-387`) and produces
`one_of: vec![lowered, null_schema()]` — i.e. a **nested** `oneOf`. The writer
(`lower/yaml.rs:201-209` + `write_schema_seq_item:257-271`) renders that as:

```yaml
rating:
  oneOf:
  - oneOf:
    - type: integer
    - type: number
  - type: null
```

The existing GREEN goalservice snapshot confirms the real behavior: it has no nullable
union, only nullable `$ref` (2-member `oneOf`) and nullable scalar (`type: [T, null]`).
So the flat 3-member form in the new snapshots is unreachable, and the red-by-design test
will fail even against a correct extractor.

**Fix:** Pick one source of truth and make the other match it.
- If the *flat* `oneOf` is the intended contract, teach `lower_field_schema` to flatten a
  nullable union into its variants plus `null_schema()`:

```rust
// in lower_field_schema, before the generic one_of branch:
if !lowered.one_of.is_empty() && lowered.schema_ref.is_none() {
    // a nullable union: append the null member to the existing variants (flatten)
    let mut variants = lowered.one_of;
    variants.push(null_schema());
    return Ok(SchemaObject { one_of: variants, ..SchemaObject::default() });
}
```
- Otherwise, rewrite both `.snap` files to the nested-`oneOf` shape the current lowering
  actually emits. Either way, add a unit test in `lower/mod.rs::tests` for a nullable
  union so the chosen shape is locked (today no test exercises it).

### CR-02: Nullable-enum acceptance snapshots assert a `oneOf` wrapper the lowering cannot emit (it produces `type: [string, null]` + `enum`)

**File:** `crates/gnr8-core/tests/snapshots/snapshot_fastapi_openapi__fastapi_openapi.snap:141-145` (also `snapshot_nestjs_openapi__nestjs_openapi.snap:138-142`)
**Issue:**
`BookFilters.sort` is encoded in the graph as `nullable: true` with a `Type::Enum`
(`snapshot_fastapi_graph__...snap:280-290`; nestjs `...graph.snap:269-275`). The OpenAPI
snapshots assert it lowers to:

```yaml
sort:
  oneOf:
  - type: string
    enum: [asc, desc]
  - type: null
```

But a `Type::Enum` lowers to a `SchemaObject` with `type_name: Some("string")` and
populated `enum_values` (`lower/mod.rs:435-443`) — it is neither a `$ref` nor a `one_of`.
In `lower_field_schema` (`mod.rs:371-393`) the `nullable` wrap therefore falls through to
the final branch `SchemaObject { nullable: true, ..lowered }` (`mod.rs:389-392`), and the
writer renders the **type-array** form (`yaml.rs:210-221, 228-230`):

```yaml
sort:
  type: [string, null]
  enum: [asc, desc]
```

The `oneOf` wrapper in the snapshots is unreachable from the current lowering, so the
red-by-design test cannot flip green.

**Fix:** Decide the canonical nullable-enum rendering and reconcile.
- If `type: [string, null]` + `enum` is acceptable (it is valid JSON-Schema-2020-12),
  rewrite the two `.snap` blocks to that shape.
- If the `oneOf` form is required, special-case a nullable enum in `lower_field_schema`
  (an enum carries sibling `enum` keys that are *not* ignored beside a type, so wrapping
  in `oneOf` is only needed if you deliberately want the composed form). Add a unit test
  for nullable-enum either way — none exists today.

### CR-03: Flask OpenAPI snapshot asserts `responses: {}` for a response-less operation, but the writer emits a bare `responses:` (null) — and the OpenAPI spec requires a non-empty `responses`

**File:** `crates/gnr8-core/tests/snapshots/snapshot_flask_openapi__flask_openapi.snap:43-46`
**Issue:**
`create_order_raw` has no typed response, so the graph carries `responses: []`
(`snapshot_flask_graph__...snap:53`). The OpenAPI snapshot asserts:

```yaml
'/orders/raw':
  post:
    operationId: create_order_raw
    responses: {}
```

`write_responses` (`lower/yaml.rs:135-157`) unconditionally writes `"{pad}responses:"`
then iterates the (empty) list, producing a bare `responses:` line (a YAML null), **never**
`responses: {}`. So the snapshot does not match the writer's output. Independently, the
bare-`responses:` output is **invalid OpenAPI**: the `responses` object is REQUIRED and
must contain at least one entry, and a YAML-null is not the empty-map `{}` the snapshot
shows. Both the writer and the contract are wrong, in different directions.

**Fix:** Make the writer emit a valid, deterministic empty-responses form and align the
snapshot to it:

```rust
fn write_responses(out: &mut String, responses: &[(String, ResponseObj)], depth: usize) {
    let pad = INDENT.repeat(depth);
    if responses.is_empty() {
        let _ = writeln!(out, "{pad}responses: {{}}");
        return;
    }
    let _ = writeln!(out, "{pad}responses:");
    // ... existing loop ...
}
```
Add a `lower/yaml.rs` unit test asserting the empty-responses operation renders
`responses: {}` so this stays locked. (Consider whether a response-less operation should
instead carry a synthesized default response — but at minimum the writer and snapshot must
agree.)

### CR-04: Flask graph acceptance snapshot lists diagnostics in an order the graph provably re-sorts, so the contract can never match the produced graph

**File:** `crates/gnr8-core/tests/snapshots/snapshot_flask_graph__flask_graph.snap:271-283`
**Issue:**
The snapshot lists three diagnostics, all in file `app/routes.py`, in line order
**78, 42, 69**:

```yaml
diagnostics:
  - ... file: app/routes.py  line: 78   # untyped request body on POST /raw
  - ... file: app/routes.py  line: 42   # untyped query param 'q' on GET /
  - ... file: app/routes.py  line: 69   # untyped response on POST /raw
```

But `ApiGraph::from_facts` sorts diagnostics by `(file, line, message)`
(`crates/gnr8-core/src/graph/mod.rs:256-261`). Since all three share a file, the produced
graph orders them by line ascending: **42, 69, 78**. The hand-authored order (78, 42, 69)
is therefore unreachable; the `assert_yaml_snapshot!` will never match a real graph even
once `pyextract` lands and emits exactly these three diagnostics.

**Fix:** Reorder the three diagnostic entries in the snapshot to line ascending (42, then
69, then 78) to match the graph's deterministic `(file, line, message)` sort.

## Warnings

### WR-01: No unit test covers nullable-union or nullable-enum lowering — the two shapes that broke (CR-01/CR-02)

**File:** `crates/gnr8-core/src/lower/mod.rs:522-980` (tests module)
**Issue:**
The lowering test module covers nullable scalar (`type: [string, null]`), nullable `$ref`
(`oneOf` of 2), and plain union — but never a **nullable union** or a **nullable enum**.
Those are precisely the two combinations the new fixture snapshots exercise and that
CR-01/CR-02 show are mis-specified. The gap let an unreachable contract ship.
**Fix:** Add `nullable_union_field_lowers_to_<chosen>` and
`nullable_enum_field_lowers_to_<chosen>` tests asserting the byte form once CR-01/CR-02
resolve the canonical shape, so the green gate (not just the ignored red snapshots)
protects these axes.

### WR-02: Go extractor maps every integer kind — including all unsigned widths — to a signed 64-bit `IntPrim(64, true)`

**File:** `goextract/internal/types/extract.go:219-221`
**Issue:**
`mapBasic` lowers `Uint, Uint8..Uint64` to `facts.IntPrim(64, true)` (signed). The neutral
`Prim::Int { bits, signed }` carries a `signed` axis precisely so a target can distinguish
`uint64` from `int64`, but the extractor discards it for unsigned source types, silently
mis-encoding an unsigned field as signed. The bookstore fixtures only use `int`/`number`
so this is latent, but it is a fact-loss the IR was designed to avoid (and it is the kind
of conflation rule 3 forbids — one source of truth per fact, faithfully carried).
**Fix:** Branch the unsigned kinds to `facts.IntPrim(64, false)`:

```go
case gotypes.Int, gotypes.Int8, gotypes.Int16, gotypes.Int32, gotypes.Int64:
    return facts.PrimitiveType(facts.IntPrim(64, true))
case gotypes.Uint, gotypes.Uint8, gotypes.Uint16, gotypes.Uint32, gotypes.Uint64:
    return facts.PrimitiveType(facts.IntPrim(64, false))
```

### WR-03: `mapBasic` default arm silently coerces unknown basic kinds to `string` (a quiet wrong fact, not a diagnostic)

**File:** `goextract/internal/types/extract.go:227-228`
**Issue:**
The `default:` arm of `mapBasic` returns `PrimitiveType(StringPrim())` for any unhandled
`*types.Basic` kind (complex64/128, uintptr, untyped constants, etc.). That manufactures a
`string` fact with no evidence — the opposite of the project's "emit a diagnostic, never
guess" discipline (CLAUDE.md rule 3 / GO-06, which the handlers package follows for untyped
query params). A wrong-but-confident `string` will flow into both OpenAPI and the SDK.
**Fix:** Emit a diagnostic for the unsupported kind (and choose a deliberate fallback such
as `AnyType()` rather than `string`), mirroring `ctx.diags.FreeFormMap`/`Floatf`:

```go
default:
    ctx.diags.UnsupportedType(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
    return facts.AnyType()
```

### WR-04: `typeString` wraps `gotypes.TypeString` in a no-op `fmt.Sprintf("%s", ...)`

**File:** `goextract/internal/types/extract.go:394-398`
**Issue:**
`fmt.Sprintf("%s", gotypes.TypeString(...))` formats an already-`string` value through
`fmt`, which `go vet`'s `S1025`/simplify flags and which adds an allocation + reflection
path for zero benefit. The doc comment claims it normalizes `interface{}`→`any`, but that
normalization is done by `TypeString` itself, not by the `Sprintf`.
**Fix:** Return the value directly:

```go
func typeString(t gotypes.Type) string {
    return gotypes.TypeString(t, func(p *gotypes.Package) string { return p.Name() })
}
```

## Info

### IN-01: `analyze::facts` carries `description`/`example` field facts the param/route surface deliberately dropped, risking the rule-1 line being re-crossed

**File:** `crates/gnr8-core/src/analyze/facts.rs:131-134`
**Issue:**
`FieldFact` keeps `description` and `example` while the contract docs repeatedly state
"description was an annotation fact and has been removed" for params/operations. For struct
fields these can legitimately come from the source's own type system (e.g. a Go doc comment
on a field), so retaining them is defensible — but the asymmetry is easy to misread, and a
future contributor could wire them from a banned schema-annotation tool. Worth a one-line
note on `FieldFact` clarifying these must originate from the language's own constructs only
(rule 1), never a schema-tool annotation.

### IN-02: `.snap.new` pending insta files are present in the working tree

**File:** `crates/gnr8-core/tests/snapshots/*.snap.new`
**Issue:**
Five `*.snap.new` pending-snapshot files exist in the tree (from running the ignored
red-by-design tests). They are correctly gitignored and not committed, so this is not a
contract risk — but they are stale artifacts that can confuse `cargo insta` review and
hint that someone ran the red tests and left output behind. Worth a `cargo insta reject`
(or `rm`) to keep the snapshot dir clean.

### IN-03: `default:` arm in `mapType` returns `AnyType()` without a diagnostic

**File:** `goextract/internal/types/extract.go:190-192`
**Issue:**
Sibling to WR-03 on the composite side: the `mapType` `default:` arm returns `AnyType()`
for any unhandled `types.Type` (channels, funcs, tuples, etc.). `Any` is at least an honest
"free-form / unknown" rather than a fabricated `string`, so this is lower severity — but it
is still a silent lossy lowering with no diagnostic, unlike the `*types.Map` arm right above
it which does warn. Consider emitting a diagnostic here too for parity.

---

_Reviewed: 2026-06-25_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
