# Phase 1: Language-Neutral IR + Facts Contract + Fixtures - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 12 (8 modify, 4+ create)
**Analogs found:** 11 / 12 (one create has no analog — see No Analog Found)

> **Read order for the planner:** this map is *vocabulary refactor + dual-contract lockstep + red snapshot harness*. The hardest invariant is keeping three contract surfaces byte-aligned: `analyze/facts.rs` (serde) ↔ `goextract/internal/facts/facts.go` (json tags) ↔ `graph/mod.rs` (IR). Every modify-file below has a concrete analog *inside its own file* (the existing `kind: String` shape it replaces) — the planner should treat the "before" excerpts here as the exact text the new closed `Type` enum supersedes.

## File Classification

| New/Modified File | Op | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|-----|------|-----------|----------------|---------------|
| `crates/gnr8-core/src/analyze/facts.rs` | modify | model (wire DTO) | transform (deserialize) | itself — current `GoFacts`/`SchemaType` | exact (self) |
| `crates/gnr8-core/src/graph/mod.rs` | modify | model (IR) | transform (`from_facts`) | itself — current `Schema`/`SchemaType` | exact (self) |
| `goextract/internal/facts/facts.go` | modify | model (wire DTO, Go) | transform (marshal) | itself — current Go `SchemaType`/`SchemaFact` | exact (self) |
| `crates/gnr8-core/src/lower/mod.rs` | modify | target (consumer) | transform (IR→OpenAPI) | itself — `lower_schema_type` `kind` match | exact (self) |
| `crates/gnr8-core/src/lower/model.rs` | modify | model (OpenAPI doc) | transform | itself — `SchemaObject` nullable comment | exact (self) |
| `crates/gnr8-core/src/gosdk/emit.rs` | modify | target (consumer) | transform (IR→Go) | itself — `go_type`/`maybe_pointer` `kind` match | exact (self) |
| `crates/gnr8-core/tests/snapshot_<lang>_graph.rs` (×3) | create | test | request-response (build_graph) | `tests/snapshot_graph.rs` | exact |
| `crates/gnr8-core/tests/snapshot_<lang>_openapi.rs` (×3) | create | test | request-response (lower) | `tests/snapshot_openapi.rs` | exact |
| `crates/gnr8-core/tests/snapshots/*.snap` (RED) | create | test fixture | golden file | `tests/snapshots/snapshot_graph__goalservice_graph.snap` | exact |
| `fixtures/{fastapi,flask,nestjs}-<name>/` source | create | test fixture (service) | n/a (static input) | `fixtures/goalservice/` | role-match (cross-language) |
| `crates/gnr8-core/tests/determinism.rs` | modify (optional) | test | integration | itself | exact (self) |
| facts round-trip unit tests (in `facts.rs` `#[cfg(test)]`) | modify | test | unit | `facts.rs::tests::go_facts` | exact (self) |

---

## Pattern Assignments

### `crates/gnr8-core/src/analyze/facts.rs` (model, deserialize) + `crates/gnr8-core/src/graph/mod.rs` (model, IR)

**Analog:** themselves — the stringly-typed `SchemaType`/`Schema.kind` to replace. These two files carry **byte-identical** struct definitions today (the IR mirrors the DTO). The closed `Type` enum from `docs/extensibility.md` §2a replaces both `SchemaType { kind: String, ... }` and the `Schema.kind: String` / `SchemaFact.kind: String` discriminators.

**The exact "before" shape to supersede** — `facts.rs:122-136` (identical at `graph/mod.rs:189-202`):
```rust
/// A router-/OpenAPI-agnostic description of a Go type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaType {
    /// One of `string|integer|number|boolean|array|object|ref`.
    pub(crate) kind: String,
    pub(crate) format: Option<String>,
    pub(crate) items: Option<Box<SchemaType>>,
    pub(crate) ref_id: Option<String>,
    pub(crate) additional_properties: Option<bool>,
}
```

**Named-schema discriminator to supersede** — `SchemaFact.kind` at `facts.rs:94-95` and `Schema.kind` at `graph/mod.rs:162-163`:
```rust
    /// `"object"` for structs, `"enum"` for string-enum newtypes.
    pub(crate) kind: String,
```

**Field model — the optional/nullable axis lives HERE** (`facts.rs:104-120`, identical at `graph/mod.rs:172-187`):
```rust
pub(crate) struct FieldFact {
    pub(crate) json_name: String,
    /// Whether the field is required (`binding:"required"`).
    pub(crate) required: bool,
    /// Whether the field is optional (pointer or `,omitempty`).
    pub(crate) optional: bool,
    pub(crate) schema: SchemaType,
    pub(crate) description: Option<String>,
    pub(crate) example: Option<String>,
}
```
> Reconciliation (RESEARCH Pattern 2): keep `optional: bool` (presence/requiredness) and ADD a `nullable` axis as a *parallel* `bool` flag on the field (lowest-risk, mirrors the existing `required`/`optional` pair) OR a `Type::Nullable(Box<Type>)` wrapper. Pick one, document it. All four combinations (`optional`×`nullable`) must be reachable. **Neutralize the Go-ism doc comments** while here — `binding:"required"`, `pointer or ,omitempty`, "the Go type name" must become language-neutral prose (RESEARCH Pitfall 2).

**Determinism discipline to PRESERVE** (`graph/mod.rs:255-270`) — sort once in `from_facts`, store sorted `Vec`, never `HashMap`:
```rust
    pub(crate) fn from_facts(facts: GoFacts, module_root: &str) -> Self {
        let root = normalize_root(module_root);
        let mut schemas: Vec<Schema> = facts.schemas.into_iter()
            .map(|schema| Schema::from_fact(schema, &root)).collect();
        schemas.sort_by(|a, b| a.id.cmp(&b.id));
        // ... operations + diagnostics likewise sorted before storage
    }
```

**Strict-deserialize invariant to PRESERVE on every struct** (`facts.rs:22-23`) — do not weaken (Security V5):
```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
```

**serde-debt note (CLAUDE.md rule 2):** `use serde::{Deserialize, Serialize};` (`facts.rs:19`) and the `serde::Serialize` derives on the graph (`graph/mod.rs:156, 173, 190`) are KNOWN DEBT. **Floor: do not extend the serde surface, do not add new OSS.** Replacing serde with owned (de)serialization for the recursive `Type` enum is a separate, optional, gated task (RESEARCH Pitfall 6 / Open Q1) — not in the critical path.

---

### `goextract/internal/facts/facts.go` (model, marshal — the Go contract twin)

**Analog:** itself — the json-tagged twin that MUST move in lockstep with `facts.rs`. The header comment makes the coupling explicit (`facts.go:5-9`): *"The json tags here MUST match the serde field names in crates/gnr8-core/src/analyze/facts.rs exactly."*

**The Go struct to supersede in lockstep** (`facts.go:90-97`):
```go
// SchemaType is a router-/OpenAPI-agnostic primitive description of a Go type.
type SchemaType struct {
	Kind                 string      `json:"kind"` // string|integer|number|boolean|array|object|ref
	Format               *string     `json:"format"`
	Items                *SchemaType `json:"items"`
	RefID                *string     `json:"ref_id"`
	AdditionalProperties *bool       `json:"additional_properties"`
}
```
and the named-type discriminator (`facts.go:71-78`):
```go
type SchemaFact struct {
	ID         string      `json:"id"`
	Name       string      `json:"name"`
	Kind       string      `json:"kind"` // "object" | "enum"
	Fields     []FieldFact `json:"fields"`
	EnumValues []string    `json:"enum_values"`
	Span       SourceSpan  `json:"span"`
}
```

**Deterministic marshal pattern to PRESERVE** (`facts.go:129-149`) — sort every slice by a stable key before encoding; `SetEscapeHTML(false)`; never range a Go map:
```go
func Marshal(doc GoFacts, w io.Writer) error {
	sortDoc(&doc)
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	enc.SetEscapeHTML(false)
	return enc.Encode(doc)
}
```
> **Atomic two-file rule (RESEARCH Pitfall 1):** any field rename or `Type`-shape change is a single change touching BOTH `facts.rs` (serde) AND `facts.go` (json tags), or `deny_unknown_fields` rejects the sidecar output and the whole Go pipeline goes red. Run the Go-fixture snapshot tests after every contract edit. **CLAUDE.md rule 2 for Go: stdlib only** — `facts.go` already uses only `encoding/json`/`io`/`sort`; keep it that way.

---

### `crates/gnr8-core/src/lower/mod.rs` (target consumer — IR-03, exhaustive match)

**Analog:** itself — the two `match schema.kind.as_str()` blocks that become exhaustive `match` over the closed `Type` enum (NO `_ =>` / `other =>` catch-all — RESEARCH Pitfall 3).

**Named-schema consumer to convert** (`lower/mod.rs:297-341`) — note the current `other => Err(...)` catch-all that MUST be removed when the enum is closed:
```rust
fn lower_named_schema(schema: &Schema, ref_to_name: &BTreeMap<&str, &str>)
    -> Result<SchemaObject, crate::CoreError> {
    match schema.kind.as_str() {
        "enum"   => { /* sorted enum_values → string enum */ }
        "object" => { /* required-from-fields + properties */ }
        other => Err(crate::CoreError::Lowering {
            message: format!("unknown schema kind '{other}' for schema '{}'", schema.id),
        }),  // <- becomes UNREACHABLE / deleted under a closed enum
    }
}
```

**Type consumer to convert** (`lower/mod.rs:346-387`) — the string-kind switch that becomes the exhaustive `Type` match (see RESEARCH "Code Examples" for the target shape):
```rust
fn lower_schema_type(schema: &SchemaType, ref_to_name: &BTreeMap<&str, &str>)
    -> Result<SchemaObject, crate::CoreError> {
    match schema.kind.as_str() {
        "ref"     => Ok(SchemaObject::reference(resolve_ref(ref_id, ref_to_name)?)),
        "array"   => Ok(SchemaObject { type_name: Some("array".into()), items: ..., .. }),
        "object"  => Ok(SchemaObject { type_name: Some("object".into()),
                          additional_properties: schema.additional_properties, .. }),
        "string" | "integer" | "number" | "boolean" =>
            Ok(SchemaObject::primitive(&schema.kind, schema.format.clone())),
        other => Err(crate::CoreError::Lowering {
            message: format!("unknown SchemaType kind '{other}'"),
        }),  // <- DELETE the catch-all; add explicit Enum/Union/Map/WellKnown/Ext arms
    }
}
```
> **IR-03 invariant:** lowering emits *neutral* OpenAPI primitive names from the neutral type — it must NOT gain any `if language == ...` branch. New arms needed: `Enum` (string enum), `Union` (3.1 `oneOf`), `Map`, `WellKnown` (→ `string` + format), `Ext` (explicit typed error, not a catch-all). Nullable renders as `type: ["T","null"]` (3.1 array form) — independent of `optional` (which is omission-from-`required`).

---

### `crates/gnr8-core/src/lower/model.rs` (OpenAPI doc model — fix the factually-wrong 3.1 comment)

**Analog:** itself — the `SchemaObject` doc comment that is WRONG for OpenAPI 3.1.

**The comment to correct** (`lower/model.rs:136-139`):
```rust
/// A `JSON-Schema-2020-12` schema object (the `OpenAPI` 3.1 schema subset this `PoC` emits).
/// Optionality is expressed purely by omission from [`Self::required`] — there is NO `nullable` key
/// and NO `type: [T, "null"]` array form in 3.1 (RESEARCH Pattern 2).
```
> 3.1 DOES use `type: ["T","null"]` (JSON Schema 2020-12). The nullable axis added in `facts.rs`/`graph/mod.rs` must render here via the array form. The `SchemaObject` likely needs a way to carry the `"null"` member alongside the base type (e.g. a `nullable: bool` that the yaml writer renders as the type array). `required: Vec<String>` (`model.rs:153`) stays as the *optional* (omission) axis — keep them independent.

---

### `crates/gnr8-core/src/gosdk/emit.rs` (target consumer — keep Go-isms HERE, audit pointer logic)

**Analog:** itself — `go_type` (`emit.rs:106-163`) owns ALL Go-specific type mapping; this is the *correct* place for per-target mapping (IR-03). Convert its `match schema.kind.as_str()` to the exhaustive `Type` match.

**The target TypeMap to convert** (`emit.rs:106-162`) — Go-isms (`date-time → time.Time`, `number → float32`, `integer → int64`) STAY here:
```rust
fn go_type(schema: &SchemaType, optional: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema.kind.as_str() {
        "string" => match schema.format.as_deref() {
            Some("date-time") => "time.Time".to_string(),
            _ => "string".to_string(),
        },
        "boolean" => "bool".to_string(),
        "integer" => "int64".to_string(),
        "number"  => "float32".to_string(),       // TARGET narrowing — target concern, OK here
        "array"   => return Ok(format!("[]{}", go_type(items, false, graph)?)),
        "object"  => /* additionalProperties → map[string]any */,
        "ref"     => return Ok(maybe_pointer(target.name.clone(), optional, is_value_ref(target))),
        other => return Err(CoreError::SdkGen { message: format!("unknown SchemaType kind '{other}'") }),
    };
    let is_value = matches!(base.as_str(), "bool" | "int64" | "float32" | "time.Time");
    Ok(maybe_pointer(base, optional, is_value))
}
```

**The conflation to FIX** (`emit.rs:174-181`) — `maybe_pointer` currently pointer-wraps on `optional`; once nullable is a distinct axis, **nullable** drives the pointer (`*T`) and **optional** drives `,omitempty` (RESEARCH Pitfall 4):
```rust
fn maybe_pointer(base: String, optional: bool, is_value: bool) -> String {
    if optional && is_value { format!("*{base}") } else { base }   // <- read `nullable`, not `optional`
}
```
and the json-tag side (`emit.rs:183-185`, `json_tag`) keeps reading `optional` for `,omitempty`.
> **Expected snapshot churn:** fixing `maybe_pointer` changes the existing `goalservice` SDK/graph snapshots. That churn is intended — re-accept deliberately (RESEARCH Pitfall 5), and verify it matches the new optional/nullable semantics, not accidental drift.

---

### `crates/gnr8-core/tests/snapshot_<lang>_graph.rs` (×3) — RED-by-design (IR-04)

**Analog:** `crates/gnr8-core/tests/snapshot_graph.rs` (the proven, now-green Go harness).

**Copy this whole-file shape** (`snapshot_graph.rs:13-27`), retargeting `FIXTURE_DIR` and the snapshot name, and rewriting the header to document the RED intent:
```rust
// Tests legitimately use unwrap/expect; scope the allow to this test target so the
// workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// Resolved relative to this crate's manifest dir (mirrors the Go fixture convention).
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fastapi-<name>");

#[test]
fn graph_matches_expected_for_fastapi() {
    // RED BY DESIGN (Phase 1): no pyextract yet, so build_graph cannot produce this graph.
    // The committed .snap encodes the INTENDED neutral graph the Phase-2 extractor must produce.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — intentionally red until then");
    insta::assert_yaml_snapshot!("fastapi_graph", graph);
}
```
> `assert_yaml_snapshot!` for graphs (structured), `assert_snapshot!` for OpenAPI/SDK (plain text) — see Shared Patterns. The red comes honestly from the `.expect()` panic (no extractor). Author the committed `.snap` as the *intended green* neutral shape (RESEARCH Pitfall 5), mirroring the relativized/sorted/neutral shape of the Go graph snapshot.

---

### `crates/gnr8-core/tests/snapshot_<lang>_openapi.rs` (×3) — RED-by-design (IR-04)

**Analog:** `crates/gnr8-core/tests/snapshot_openapi.rs` (`snapshot_openapi.rs:24-40`).

**Copy the build_graph → to_openapi → snapshot shape**, including the code-as-config security helper (CLAUDE.md rule 4 — security is supplied, never scraped):
```rust
fn fixture_security() -> Vec<gnr8_core::graph::SecurityScheme> {
    vec![gnr8_core::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(), kind: "apiKey".to_string(),
        location: "header".to_string(), name: "X-API-Key".to_string(),
    }]
}

#[test]
fn openapi_matches_expected_for_fastapi() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — intentionally red until then");
    let openapi = gnr8_core::lower::to_openapi(&graph, "<name>", "/<base>", &fixture_security())
        .expect("lower::to_openapi must succeed");
    insta::assert_snapshot!("fastapi_openapi", openapi);   // plain text
}
```

---

### `fixtures/{fastapi,flask,nestjs}-<name>/` (test fixture services — IR-04)

**Analog:** `fixtures/goalservice/` — the proven per-language fixture layout: a real service tree plus an `expected/` reference-output directory.

**Layout to mirror** (from `fixtures/goalservice/`):
```
fixtures/goalservice/
├── go.mod, go.sum                         # language manifest (per-fixture; never linked into gnr8-core)
├── internal/.../{dto,handlers,http}.go    # type-rich source (objects, enum, request/response)
└── expected/
    ├── openapi.yaml                        # reference artifact (semantic anchor for the red snapshot)
    ├── diagnostics.txt
    └── sdk/{client,models,errors,operations}.go
```
> Each new fixture must exercise the v2.0 acceptance vocabulary in its OWN type system: objects, arrays/lists, cross-language enums (TS string-literal-union, Python `Literal`), **unions** (TS `A | B`, Python `Union`), and all four optional×nullable combinations (TS `x?: T` / `x: T | null` / `x?: T | null`; Python `x: T = None` / `Optional[T]` / both). **Rule 1 (non-negotiable):** the fixtures carry framework imports (`fastapi`/`flask`/`@nestjs/common`) as ordinary app deps, but gnr8 will later derive facts from the language's own types — NEVER from `@nestjs/swagger`/`zod`/`class-validator`/`marshmallow`/FastAPI's `/openapi.json`. Do not encode API facts in a way that assumes those tools. **Phase 1 = static source + red snapshots only** (no `pip install`/`npm install`, no extractor runs — RESEARCH A2). Fixture directory names are Claude's discretion.

---

## Shared Patterns

### Strict deserialize (Security V5 — do not weaken)
**Source:** `crates/gnr8-core/src/analyze/facts.rs:22-23` (on every DTO struct)
**Apply to:** Every struct in the generalized neutral `Facts` contract.
```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
```

### Dual-contract lockstep (the #1 risk)
**Source:** `facts.rs:5-7` ↔ `facts.go:5-9` (the only thing tying the two files is a comment).
**Apply to:** Any field/shape change to the facts contract — atomic two-file edit + run Go snapshots.

### Determinism: sort-once, store sorted `Vec`, never `HashMap`
**Source (Rust):** `graph/mod.rs:255-270` (`from_facts` sorts each collection before storage).
**Source (Go):** `facts.go:129-149` (`Marshal`/`sortDoc` sort before encode; `SetEscapeHTML(false)`).
**Apply to:** All IR collections, all sidecar output, all snapshot-bearing output (GRAPH-02 byte-identical).

### Exhaustive `match` (no catch-all) over the closed `Type` enum
**Source:** the `match ...kind.as_str() { ... other => Err }` blocks at `lower/mod.rs:297, 350` and `gosdk/emit.rs:107`.
**Apply to:** Every consumer of the new enum — DELETE the `other =>` / `_ =>` arm so a new variant fails to compile (RESEARCH Pitfall 3). `Ext` / unrepresentable-in-target variants get an *explicit* error arm, never a catch-all.

### Per-target type mapping stays in the target (IR-03)
**Source:** `gosdk/emit.rs:106-162` (`go_type` owns `date-time→time.Time`, `number→float32`).
**Apply to:** All target-specific mapping. Lowering (`lower/`) and the IR (`graph/`) stay neutral — no `if language == ...` ever.

### Test target conventions
**Source:** `tests/snapshot_graph.rs:13-18`, `tests/determinism.rs:14-19`.
**Apply to:** Every new `snapshot_*.rs`:
- `#![allow(clippy::unwrap_used, clippy::expect_used)]` scoped to the test target (keeps prod RUST-04 deny intact).
- `FIXTURE_DIR` via `concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/<name>")`.
- `assert_yaml_snapshot!` for graphs; `assert_snapshot!` for OpenAPI/SDK text.
- `INSTA_UPDATE=no` under `CI=true` (existing convention) — a red snapshot never auto-flips green.
- Skip-gracefully pattern (`let Ok(..) = build_graph(..) else { return }`, `determinism.rs:36`) if a toolchain is genuinely optional; for RED-by-design tests prefer the honest `.expect()` panic instead.

### Neutralize Go-isms in IR doc/prose (IR-01/02)
**Source of the violations to fix:** `graph/mod.rs:161-187` ("the Go type name", `binding:"required"`, `pointer or ,omitempty`); `facts.rs:93-113` (same).
**Apply to:** All generalized field docs — grep the touched files for `Go`, `Gin`, `gin`, `binding`, `omitempty`, `json:` in comments after generalization (RESEARCH Pitfall 2).

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `fixtures/{flask,nestjs}-<name>/expected/openapi.yaml` (the *intended-green* reference artifacts for non-Go fixtures) | reference artifact | golden | `fixtures/goalservice/expected/openapi.yaml` is the structural template, but no Python/TS extractor exists to *generate* one — the planner must hand-author each to the neutral IR shape (grounded, not from a tool), then the red `.snap` is reconciled to it for semantic equivalence (mirroring how the Go OpenAPI snapshot was reconciled to `expected/openapi.yaml`, `snapshot_openapi.rs:5-10`). This is a *create from the IR contract*, not a copy. |

> Everything else has a concrete analog. The closed `Type` enum itself has an authoritative *design* source (`docs/extensibility.md` §2a, quoted in RESEARCH) rather than a code analog — adapt that sketch; do not copy it blindly (the optional/nullable reconciliation in RESEARCH Pattern 2 is the required deviation).

## Metadata

**Analog search scope:** `crates/gnr8-core/src/{analyze,graph,lower,gosdk}/`, `crates/gnr8-core/tests/`, `crates/gnr8-core/tests/snapshots/`, `goextract/internal/facts/`, `fixtures/`.
**Files scanned (read):** `analyze/facts.rs`, `graph/mod.rs` (schema region + `from_facts`), `goextract/internal/facts/facts.go`, `lower/mod.rs` (schema lowering), `lower/model.rs` (nullable comment), `gosdk/emit.rs` (`go_type`/`maybe_pointer`), `tests/snapshot_graph.rs`, `tests/snapshot_openapi.rs`, `tests/determinism.rs`, `fixtures/goalservice/` tree.
**No project skills found:** `.claude/skills/` and `.agents/skills/` absent; conventions taken from `CLAUDE.md` (rules 1–4) + RESEARCH.md's cited `thoughts/skills/rust-best-practices/SKILL.md`.
**Pattern extraction date:** 2026-06-25
