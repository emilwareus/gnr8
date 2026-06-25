# Phase 1: Language-Neutral IR + Facts Contract + Fixtures - Research

**Researched:** 2026-06-25
**Domain:** Compiler/IR design (type-system-neutral API model), Rust serialization, multi-language test fixtures + snapshot harness
**Confidence:** HIGH (codebase realities verified by direct read; design grounded in committed authoritative docs)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Grounding documents (authoritative — follow these, don't re-derive):**
- IR generalization design: `docs/extensibility.md` ("Type-enum evolution + typed Extensions side-channel"). The sanctioned shape for the neutral `Type` enum and the typed Extensions side-channel.
- Milestone brief: `docs/milestone-v2-multi-language.md` (§"IR generalization", §"Success criteria").
- Requirements: IR-01, IR-02, IR-03, IR-04 in `.planning/REQUIREMENTS.md`.

**Type vocabulary (from the brief — locked):**
- Optional and nullable are **distinct** axes (TS `?` vs `| null`; Python `Optional` vs a field that may be `None`). The IR must represent both independently, not collapse them.
- Enums generalize beyond Go string-enums to cover TS string-literal-union enums and Python `Literal`.
- Unions are first-class in the neutral vocabulary (TS `A | B`, Python `Union`).
- Arrays/lists and nested objects are neutral (`list[T]`, `T[]`, nested models).

**Invariants (CLAUDE.md — non-negotiable, carried into this phase):**
- One source of truth per fact; **no fallback / dual control-flow paths** (rule 3).
- **Zero OSS in `gnr8-core`**; prefer hand-rolled, in-repo code (rule 2). Existing `serde`/`serde_json` usage is pre-existing debt — when this phase touches `facts.rs`, prefer replacing serde usage with owned (de)serialization over extending it; at minimum do not add new OSS deps.
- No language-tool convention coupling (rule 1) — facts come from each language's own type system, never from `@nestjs/swagger` / `zod` / `class-validator` / `marshmallow` / FastAPI's runtime `/openapi.json`.
- Deterministic, sorted, byte-identical output.

**Fixture + snapshot harness (mirror v1.0's proven pattern):**
- Follow the existing snapshot test pattern under `crates/gnr8-core/tests/` (`snapshot_openapi.rs`, `snapshot_graph.rs`, etc.) and `tests/snapshots/`.
- Fixtures live alongside the existing Go fixture convention (`fixtures/goalservice/` is the Go fixture; `goextract/` is the Go sidecar host).
- "Red-by-design" means: snapshot files committed and the tests visibly failing (no extractor yet), documented as intentional so they are the acceptance contract Phases 2–5 turn green.

### Claude's Discretion
All remaining implementation choices — exact module layout for the neutral facts type, the precise Rust enum shape (guided by `docs/extensibility.md`), fixture directory naming, and snapshot file organization. Discuss was skipped: this is a pre-specified infrastructure phase.

### Deferred Ideas (OUT OF SCOPE)
None — discuss phase skipped. Out of scope per the phase boundary: ANY extraction logic (`pyextract`/`tsextract`), any new SDK target output, `.gnr8/` Source/Target built-ins for the new languages. This phase is IR generalization + fixtures + red snapshots ONLY.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| IR-01 | The IR + JSON facts contract express the cross-language type vocabulary (objects, arrays, enums, optional/nullable, unions) without Go-specific assumptions. | §Architecture Patterns (neutral `Type` enum delta), §Don't Hand-Roll, §State of the Art (optional vs nullable). The current `SchemaType { kind: String, ... }` (graph/mod.rs:191, facts.rs:125) must become a closed, expressive type model per `docs/extensibility.md` §2a. |
| IR-02 | Every language sidecar emits one shared JSON facts contract the Rust host deserializes strictly (`deny_unknown_fields`); no language terms leak into the IR. | §Standard Stack (serde debt decision), §Common Pitfalls (cross-language field-name drift), §Validation Architecture. Generalize `GoFacts` → a neutral `Facts` doc; rename Go-specific field docs/comments. |
| IR-03 | OpenAPI lowering + SDK generation consume the IR unchanged across all supported languages (no per-language branches in lowering). | §Architecture Patterns (no per-language branches — verified: `lower/mod.rs` + `gosdk/emit.rs` already branch on `schema.kind` *strings*, not on language; the delta is making them consume the new `Type` enum). §Common Pitfalls (Go-isms in lowering: `int64`/`float32` narrowing, `time.Time` mapping live in `gosdk`, which is correct — that is a *target* TypeMap, not a lowering branch). |
| IR-04 | Multi-language fixture services (FastAPI, Flask, NestJS) encode the v2.0 acceptance cases, with red-by-design snapshots in place before extraction lands. | §Architecture Patterns (fixture + snapshot harness pattern), §Code Examples (red-by-design insta snapshot), §Validation Architecture. Mirror `fixtures/goalservice/` + `tests/snapshot_*.rs` + `tests/snapshots/*.snap`. |
</phase_requirements>

## Summary

This is an infrastructure phase with no user-facing behavior: it widens the narrow waist (`ApiGraph` IR + the shared JSON facts contract) so three new type systems (Python's, TypeScript's) can flow through the *same* Rust pipeline that Go already uses, and it stands up the red-by-design acceptance harness that Phases 2–5 will turn green.

The decisive realities from the codebase: (1) the type vocabulary is **stringly-typed today** — `SchemaType { kind: String, format: Option<String>, items, ref_id, additional_properties }` appears identically in three places (`analyze/facts.rs:125`, `graph/mod.rs:191`, and as the contract the Go sidecar emits in `goextract/internal/facts/facts.go:91`), and "object/enum" lives in a separate `Schema.kind: String`. Generalizing the vocabulary means replacing these `kind: String` discriminators with the closed `Type` enum sketched in `docs/extensibility.md` §2a, threaded through all three layers plus the two consumers (`lower/`, `gosdk/`). (2) **Optional and nullable are conflated today**: `FieldFact` carries both `required` and `optional` bools, but there is no `nullable` axis, and the OpenAPI lowering expresses optionality *only* by omission from `required` (`lower/model.rs:138` even asserts, incorrectly, that 3.1 has "NO `type: [T, "null"]` array form" — see State of the Art). The brief locks optional and nullable as distinct axes; the IR must carry both. (3) **Unions and cross-language enums do not exist** in the current model and must be added as first-class `Type` variants. (4) The lowering/SDK consumers do **not** branch per-language today (they branch on `schema.kind` *strings* and on Go target type mapping inside `gosdk`) — so IR-03's "no per-language branches" is mostly already true; the work is to make those consumers read the new enum exhaustively and keep target-specific mapping (e.g. `WellKnown::DateTime → time.Time`) inside the *target* (`gosdk`), never in lowering.

The fixture/snapshot harness is a well-worn path: `fixtures/goalservice/` (a real Gin service + `expected/` reference outputs) drives `tests/snapshot_{graph,openapi,sdk,diagnostics}.rs` via `insta` against committed `.snap` files, with `tests/determinism.rs` proving byte-identical re-runs. The new work is three sibling fixture services (FastAPI, Flask, NestJS) plus committed snapshots that are intentionally RED (no extractor exists yet) — the acceptance contract.

**Primary recommendation:** Promote `SchemaType`/`Schema.kind` to a closed `Type` enum (per `docs/extensibility.md` §2a) and add explicit `optional` + `nullable` axes, threading the new model through facts.rs → graph/mod.rs → lower/ + gosdk/ in lockstep with the Go sidecar's `facts.go`; keep all language-specific type mapping inside targets. Author FastAPI/Flask/NestJS fixture services under `fixtures/`, and commit deliberately-failing `insta` snapshots wired through new `tests/snapshot_*.rs` targets, each documented as red-by-design. Do NOT remove the existing `serde`/`serde_json` debt in this phase unless the plan explicitly scopes a self-contained owned-JSON task — that is a large, risky change; prefer "do not extend serde surface, do not add new OSS" as the floor (CONTEXT permits this reading: "at minimum do not add new OSS deps").

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Neutral type vocabulary (`Type` enum) | IR / graph (`graph/mod.rs`) | Facts contract (`analyze/facts.rs`) | The IR is the narrow waist; the facts DTO is its wire mirror. Both must carry the identical closed vocabulary. |
| Shared JSON facts contract | Facts DTO (`analyze/facts.rs`) ↔ sidecar (`goextract/internal/facts/facts.go`) | — | The contract is the host↔sidecar boundary; field names must match byte-for-byte across Rust serde and Go json tags. |
| Optional vs nullable axes | Facts + IR field model | OpenAPI lowering (`lower/`), SDK targets (`gosdk/`) | The two axes are *facts* carried on fields; targets decide how to render each (omit-from-required vs `type:[T,null]` vs `*T` pointer vs `T \| null`). |
| OpenAPI 3.1 lowering | Target (`lower/to_openapi`) | — | Lowering reads the frozen IR and emits one artifact; must be language-neutral (IR-03). |
| SDK type mapping (`DateTime→time.Time`) | Target (`gosdk/`) | — | Per-language type mapping belongs in the target's TypeMap, NEVER in lowering (`docs/extensibility.md` §2a). |
| Fixture services (FastAPI/Flask/NestJS) | Test inputs (`fixtures/`) | — | Phase-1 connects them only as *future* extraction targets + committed red snapshots; no extractor runs against them yet. |
| Red-by-design snapshots | Test harness (`tests/snapshot_*.rs` + `tests/snapshots/`) | — | The acceptance contract Phases 2–5 turn green. |

## Standard Stack

> **No new packages are introduced in this phase.** CLAUDE.md rule 2 forbids OSS in `gnr8-core`; the phase adds Rust types, Go/Python/TS *fixture* code, and test snapshots — not dependencies. The "stack" below documents what already exists and how to use it; the Package Legitimacy Audit is therefore N/A for new installs.

### Core (existing, in-repo)
| Component | Location | Purpose | Why Standard |
|-----------|----------|---------|--------------|
| `ApiGraph` IR | `crates/gnr8-core/src/graph/mod.rs` | The narrow-waist API model | Single source of truth from which OpenAPI + SDKs are lowered (PROJECT.md / `docs/extensibility.md` §1). |
| Facts DTO (`GoFacts`) | `crates/gnr8-core/src/analyze/facts.rs` | serde mirror of the JSON facts contract | The host↔sidecar wire boundary; `deny_unknown_fields` already enforced (IR-02). |
| OpenAPI lowering | `crates/gnr8-core/src/lower/{mod,model,yaml}.rs` | IR → OpenAPI 3.1 YAML (hand-rolled writer) | Pure graph→typed-doc transform; deterministic. |
| Go SDK target | `crates/gnr8-core/src/gosdk/{emit,bundle,gofmt,mod}.rs` | IR → Go SDK (`format!`-based) | The twin pattern future SDK targets follow; owns Go-specific type mapping. |
| Pipeline traits | `crates/gnr8-core/src/sdk/{mod,builtins}.rs` | `Source`/`Transform`/`Target`/`PostProcess` | The composition surface; `GoGin`/`OpenApi31`/`GoSdk` built-ins. |

### Supporting (existing dev/test infra)
| Component | Version | Purpose | When to Use |
|-----------|---------|---------|-------------|
| `insta` | 1.48 (`features=["yaml"]`) | Snapshot testing | `assert_snapshot!` (plain text — OpenAPI/SDK), `assert_yaml_snapshot!` (structured — graph). Already a dev-dependency. |
| Go toolchain | go 1.26 (dev+CI) | Runs `goextract` via `go run` | Existing snapshot tests spawn it; new Python/TS fixtures will NOT run an extractor this phase. |
| `serde` / `serde_json` | 1.0 | Facts (de)serialization + graph serialization | **DEBT (rule 2).** Do not extend; do not add new OSS. See serde decision below. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Closed `Type` enum (`docs/extensibility.md` §2a) | Keep `kind: String` + add more string kinds (`"union"`, `"literal"`) | REJECTED — CONTEXT locks the enum design; strings defeat exhaustiveness checking (a new variant should fail to compile in every consumer, the whole point of the narrow-waist contract). |
| Keep serde for facts | Hand-rolled JSON (de)serializer | CONTEXT says "prefer replacing serde where reasonable" but "at minimum do not add new OSS." Replacing serde is a large, separable change (see Open Questions Q1) — recommend gating it behind its own task or deferring; the phase MUST NOT add new OSS. |
| Add nullable as a `Type::Nullable(Box<Type>)` wrapper | A `nullable: bool` field flag (parallel to `optional: bool`) | `docs/extensibility.md` §2a sketches `Optional(Box<Type>)` as a wrapper but the brief treats optional/nullable as *two distinct axes*. Plan must reconcile (see Architecture Patterns — Reconciliation Delta). |

**Installation:** None. No `npm install` / `cargo add` / `pip install` in `gnr8-core` for this phase. (Fixture services may declare their own language deps — FastAPI, Flask, `@nestjs/common` — but those live in fixture directories, are never linked into `gnr8-core`, and are never *parsed by convention* — rule 1.)

## Package Legitimacy Audit

**N/A for `gnr8-core` — this phase installs zero packages into the core (CLAUDE.md rule 2).**

Fixture services will import their frameworks (`fastapi`, `flask`, `@nestjs/common`) as ordinary application dependencies *inside the fixture directories only*. These are:
- never linked into `gnr8-core`,
- never parsed via their annotations/conventions (rule 1 — gnr8 will later derive facts from the language's own types, not from `@nestjs/swagger`/`zod`/`marshmallow`),
- not installed or run in Phase 1 (no extractor exists yet; fixtures are static source + red snapshots).

If the plan chooses to make fixtures *runnable* (e.g. a `requirements.txt` / `package.json` for a future hermetic test), the install of `fastapi`/`flask`/`@nestjs/common` should be gated behind a `checkpoint:human-verify` task and the names tagged `[ASSUMED]` until verified on PyPI/npm at plan time. They are NOT needed for static fixtures + red snapshots, so the simplest Phase-1 path declares no installs at all.

## Architecture Patterns

### System Architecture Diagram

```
  SOURCES (future)              FACTS (JSON wire)        IR (the narrow waist)        TARGETS
  ──────────────                ─────────────────        ─────────────────────       ────────────
  goextract (Go) ──┐                                                            ┌─▶ OpenApi31  → openapi.yaml
  pyextract  ──────┤  emit       ┌──────────────┐  strict   ┌──────────────┐   │      (lower/)
   (Phase 2) ──────┼──────────▶  │  Facts doc   │  deser*   │   ApiGraph   │   ├─▶ GoSdk      → sdk/*.go
  tsextract  ──────┤            │ (neutral)    │ ───────▶  │  (Type enum) │ ──┤      (gosdk/)
   (Phase 4) ──────┘            │ deny_unknown │           │  operations  │   ├─▶ PySdk  (Phase 3, later)
                                 └──────────────┘  build_   │  schemas     │   └─▶ TsSdk  (Phase 5, later)
   ▲ NONE run in Phase 1                          graph()   │  diagnostics │
   │ fixtures are STATIC source                             └──────────────┘
   │ + RED snapshots only                                        ▲
   │                                                             │ Transforms set base_path/title/security
   └── fixtures/{fastapi,flask,nestjs}-svc/   ─── (red snapshot) ┘   (rule 4 metadata, not scraped)

  * deserialize: serde + deny_unknown_fields today (rule-2 debt); owned (de)ser is the target.

  PHASE-1 SCOPE: widen [Facts doc] + [ApiGraph] vocabulary (Type enum, optional≠nullable, unions,
  cross-lang enums); make [lower/] + [gosdk/] consume the new enum with NO per-language branches;
  author the three fixtures; commit RED snapshots. NO source arrow above is wired this phase.
```

### Recommended Project Structure (additions)
```
crates/gnr8-core/src/
├── analyze/facts.rs        # generalize GoFacts → neutral Facts; Type enum mirror; rename Go-isms
├── graph/mod.rs            # promote SchemaType→Type enum; add optional+nullable axes; Union/enum variants
├── lower/mod.rs            # consume Type enum exhaustively (match all variants); no language branch
└── gosdk/emit.rs           # map Type→Go via the target's TypeMap (keep Go-isms HERE)

fixtures/
├── goalservice/            # EXISTING Go/Gin fixture (the template)
├── fastapi-<name>/         # NEW — FastAPI service encoding v2.0 acceptance cases
├── flask-<name>/           # NEW — Flask service (honest second-class envelope)
└── nestjs-<name>/          # NEW — NestJS service (@nestjs/common decorators + DTO classes)

crates/gnr8-core/tests/
├── snapshot_<lang>_graph.rs        # NEW — RED: build_graph against each fixture (no extractor yet)
├── snapshot_<lang>_openapi.rs      # NEW — RED
└── snapshots/                       # NEW committed .snap files (intentionally failing)

goextract/internal/facts/facts.go   # keep Go json tags in lockstep with the renamed Rust serde fields
```
*(Exact fixture directory names are Claude's discretion per CONTEXT.)*

### Pattern 1: Closed neutral `Type` enum replaces `kind: String`
**What:** Replace the three identical `SchemaType { kind: String, ... }` definitions and the `Schema.kind: String` / `SchemaFact.kind: String` discriminators with the closed enum from `docs/extensibility.md` §2a.
**When to use:** Everywhere a type is currently described by a string `kind`.
**Example (the sanctioned sketch — adapt, do not copy blindly):**
```rust
// Source: docs/extensibility.md §2a (authoritative grounding doc)
pub enum Type {
    Primitive(Prim),          // String | Bool | Int{bits,signed} | Float{bits} | Bytes
    WellKnown(WellKnown),     // Uuid | DateTime | Date | Duration | Decimal | Email | Uri ...
    Optional(Box<Type>),      // (see Reconciliation Delta re: optional vs nullable)
    Array(Box<Type>),
    Map { key: Box<Type>, value: Box<Type> },
    Named(SchemaId),          // $ref to a named schema
    Object(Vec<Field>),       // inline object
    Enum(Vec<EnumMember>),    // string enum / TS literal-union / Python Literal
    Union(Vec<Type>),         // oneOf / sum types
    Any,                      // free-form (map[string]any) — explicitly lossy
    Ext(ExtTypeId),           // a type only an extension understands (§2c)
}
```
**Anti-pattern avoided:** Adding `"union"` / `"literal"` as new magic strings to `kind: String` — that keeps the contract un-exhaustive and lets a new variant silently fall through to a default branch (a wrong-output bug).

### Pattern 2: Optional vs Nullable as distinct axes (Reconciliation Delta)
**What:** CONTEXT locks optional and nullable as *two distinct axes*. The §2a sketch shows only `Optional(Box<Type>)`. The plan must reconcile: the field model already carries `required` + `optional` bools (`facts.rs:107`, `graph/mod.rs:174`); add a `nullable` axis so a field can be `optional && nullable`, `optional && !nullable`, `!optional && nullable`, or neither — all four are reachable across TS (`x?: T` vs `x: T | null` vs `x?: T | null`) and Python (`x: T = None` default vs `x: Optional[T]` vs both).
**When to use:** On every object field and (for nullability) on type positions that can be `| null`.
**Recommended shape:** Keep `optional: bool` on the field (presence/requiredness — the JSON key may be absent) and represent nullability either as `nullable: bool` on the field OR as a `Type::Nullable(Box<Type>)` wrapper on the field's type. The plan picks one and applies it consistently; document the choice. (Two-bool parallelism mirrors the existing `required`/`optional` pair and is the lowest-risk extension of the current shape.)
**Why it matters downstream:**
- OpenAPI 3.1: nullable → `type: ["T", "null"]` (the canonical 3.1 form — see State of the Art); optional → omit from `required`. These are independent.
- Go SDK: nullable value type → pointer (`*T`); optional → `,omitempty` json tag. The existing `gosdk` already pointer-wraps optionals (`emit.rs:175 maybe_pointer`) — that logic conflates the two and will need to read the nullable axis.

### Pattern 3: Per-language type mapping stays in the Target, never in lowering (IR-03)
**What:** `WellKnown::DateTime → time.Time` (Go) / `→ string|Date` (TS) / `→ datetime` (Python) is a **target** concern. Lowering (`lower/`) and the IR stay neutral.
**Verified current state:** `gosdk/emit.rs:106 go_type()` already owns Go mapping (`date-time → time.Time`, `number → float32`, etc.). `lower/mod.rs` does NOT do Go mapping — it emits OpenAPI primitive names (`string`/`integer`/...) from the neutral kind. So IR-03 is *structurally already satisfied*; the delta is: (a) make both consumers `match` the new `Type` enum exhaustively, (b) ensure no `if language == ...` branch is ever introduced.
**Anti-pattern avoided:** Putting `if go { time.Time } else if ts { string }` in lowering — forbidden by IR-03 and `docs/extensibility.md` §2a.

### Pattern 4: Fixture + red-by-design snapshot harness (mirror v1.0)
**What:** Each fixture is a real service directory under `fixtures/`; a `tests/snapshot_*.rs` target drives the pipeline against it and asserts a committed `.snap` via `insta`. For Phase 1 the snapshots are RED because no extractor exists.
**The proven pattern (from `tests/snapshot_graph.rs`):**
```rust
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/<fixture>");

#[test]
fn graph_matches_expected_for_<fixture>() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("build_graph must succeed");           // <- RED in Phase 1: no py/ts extractor
    insta::assert_yaml_snapshot!("<fixture>_graph", graph);
}
```
**Red-by-design mechanics:** A snapshot test fails if (a) the `.expect()` panics (no extractor for this language yet — the most honest red), or (b) the produced output ≠ the committed `.snap`. Commit a `.snap` representing the *target* (green) output so that when Phases 2–5 land an extractor, the test flips to green with zero snapshot edits. Document each red test (doc comment + a tracking note) so reviewers know the red is intentional, mirroring how `snapshot_graph.rs`'s header documents its now-green state.

### Anti-Patterns to Avoid
- **Stringly-typed vocabulary creep:** never add another `kind: String` value — extend the enum.
- **Language terms in the IR:** no `gin`/`fastapi`/`nestjs`/`pydantic` identifiers in `graph/` or `facts.rs` field names, kinds, or doc comments. (The current `graph/mod.rs` doc comments say "Go type name", "Gin group prefix" — these MUST be neutralized when generalizing, IR-01/02.)
- **Collapsing optional into nullable** (or vice versa) — the brief forbids it.
- **CI auto-accepting snapshots:** keep `INSTA_UPDATE=no` under `CI=true` (already the convention) so a red snapshot never silently flips green.
- **`HashMap` in serialized output:** every collection stays a sorted `Vec` (GRAPH-02 determinism).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Type vocabulary discriminant | A new string-`kind` switch with a default arm | The closed `Type` enum + exhaustive `match` (no `_ =>` catch-all that hides new variants) | Exhaustiveness is the entire value of the narrow-waist contract — a new variant must fail to compile in every consumer. |
| Deterministic serialization order | Re-sorting on every read | Keep the existing "sort once in `from_facts`, store sorted `Vec`" discipline (`graph/mod.rs:263`) | Already proven byte-identical; don't reinvent. |
| Snapshot diffing/acceptance | A custom golden-file comparator | `insta` (already a dev-dependency) | `insta` is allowed as dev infra; it handles review/accept/CI-lock. |
| OpenAPI 3.1 nullable rendering | A bespoke `nullable` keyword (3.0-style) | `type: ["T","null"]` array form per JSON Schema 2020-12 | 3.1 dropped `nullable`; the array form is canonical (see State of the Art). |
| Go field naming / initialisms | Re-deriving in a new target | The existing `gosdk::emit::exported`/`lower_camel` (`emit.rs:44`) | Target-owned, already correct; new targets get their own equivalents in later phases (not this phase). |

**Key insight:** This phase is *almost entirely* a typed-model refactor plus fixtures. The hardest part is not writing code — it is keeping the three contract surfaces (`facts.rs` serde, `facts.go` json tags, `graph/mod.rs` IR) byte-aligned and neutral while making the two consumers exhaustively `match` the new enum. Lean on the compiler: a closed enum turns "did I handle the new case everywhere?" into a build error rather than a runtime wrong-output bug.

## Runtime State Inventory

> This phase **does** rename/neutralize a serialized wire contract (`GoFacts` → neutral `Facts`; Go-specific field names/comments → neutral), so the rename checklist applies even though it is not a classic "rebrand."

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | **None.** The facts JSON is a transient stdout pipe between the Go sidecar and the Rust host (`analyze/helper.rs:run_goextract`), never persisted. No database, no cache file on disk. | None — verified: facts are produced on stdout and deserialized in-process; `from_facts` consumes and discards. |
| Live service config | **None.** No external service holds the contract; the sidecar is spawned per-run via `go run`. | None — verified: `helper.rs` spawns `go run . <target>` fresh each invocation. |
| OS-registered state | **None.** No scheduled tasks, daemons, or registered processes embed the type names. | None — verified by repo scan (no launchd/systemd/Task Scheduler artifacts). |
| Secrets/env vars | **None** reference the IR type names. `INSTA_UPDATE`/`CI` env vars gate snapshot behavior but are not renamed. | None — verified. |
| Build artifacts / cross-surface contract | **The dual contract surface IS the migration risk:** `analyze/facts.rs` serde field names ↔ `goextract/internal/facts/facts.go` json tags must stay byte-identical (facts.rs:7 documents this lockstep), AND the committed `.snap` files (`tests/snapshots/*.snap`) encode the current field names/shape. Renaming any field or changing the `Type` shape will: (1) require the matching edit in `facts.go`, (2) change every existing Go-fixture snapshot. | **Code edits in BOTH languages + snapshot updates.** Any facts-contract change must update `facts.go` json tags in the same change; existing `goalservice` snapshots must be re-reviewed and re-accepted (the Go pipeline must stay green). This is the single highest-risk coupling in the phase. |

**The key question answered:** After the IR/facts vocabulary is generalized, what still carries the old shape? Only (a) the Go sidecar's `facts.go` (must be edited in lockstep) and (b) the committed Go-fixture `.snap` files (must be re-accepted). There is no persisted runtime state.

## Common Pitfalls

### Pitfall 1: Drifting the Rust↔Go facts contract
**What goes wrong:** Renaming a serde field in `facts.rs` without the matching json-tag change in `facts.go` (or vice versa) — `deny_unknown_fields` then rejects the sidecar's output and the whole Go pipeline goes red with a confusing parse error.
**Why it happens:** The contract lives in two files in two languages; only a comment ties them together (`facts.rs:7`, `facts.go:6`).
**How to avoid:** Treat any facts-contract edit as an atomic two-file change (Rust serde + Go json tags) within one task. Run the Go-fixture snapshot tests after every contract edit. Consider an explicit round-trip test (one already exists: `facts.rs` `deserializes_sample_facts_without_error`).
**Warning signs:** `CoreError::FactsParse` from `build_graph`; `deny_unknown_fields` rejection in unit tests.

### Pitfall 2: Hidden Go-isms in the "neutral" model
**What goes wrong:** The current model is documented as router-agnostic but is full of Go-specific *prose* and assumptions: `Schema.name` is "the Go type name" (`graph/mod.rs:163`), comments reference `binding:"required"`, `,omitempty`, `json:"..."` tags (`graph/mod.rs:174-187`), and `kind: String` carries Go-shaped semantics. Leaving these in fails IR-01/02 ("no language terms leak into the IR").
**Why it happens:** The IR was shaped by Go-first development (PROJECT.md: "Go must prove the model first").
**How to avoid:** When generalizing, neutralize field docs ("the type's name", not "the Go type name"), and ensure no field name or enum variant encodes a Go/Gin/FastAPI/Nest term. Grep the generalized files for `Go`, `Gin`, `gin`, `binding`, `omitempty`, `json:` in doc comments.
**Warning signs:** Any language proper noun in `graph/mod.rs` or `facts.rs` after generalization.

### Pitfall 3: A `_ =>` catch-all that swallows new `Type` variants
**What goes wrong:** A `match schema.kind { ... _ => Err(...) }` in lowering/SDK currently returns a runtime error for unknown kinds (`lower/mod.rs:383`, `gosdk/emit.rs:154`). With a closed enum, a non-exhaustive match with `_ =>` would *compile* but silently mishandle a newly-added variant.
**Why it happens:** Copying the string-match shape onto the enum.
**How to avoid:** Match every `Type` variant explicitly (no `_ =>`). Let the compiler flag any consumer that doesn't handle a new variant. For genuinely unrepresentable-in-target variants (e.g. `Union` in a target without sum types), emit a *capability diagnostic* (the §6 pattern) — but as an explicit arm, not a catch-all.
**Warning signs:** `_ =>` arms on a `Type` match; a new variant added without a compile error anywhere.

### Pitfall 4: Conflating optional and nullable in the Go SDK pointer logic
**What goes wrong:** `gosdk/emit.rs:175 maybe_pointer` pointer-wraps a field when `optional && is_value`. Once nullable becomes a distinct axis, "optional" alone should drive `,omitempty` while "nullable" should drive the pointer — conflating them produces wrong Go (a non-nullable optional value emitted as `*T`, or a nullable-required field emitted as `T`).
**Why it happens:** The current model has no nullable axis, so optional does double duty.
**How to avoid:** When adding the nullable axis, audit `maybe_pointer` and the OpenAPI `required` logic to read the correct axis. **Note:** changing this changes the existing Go-fixture snapshots — that is expected and must be reconciled (re-accept the snapshot deliberately, Pitfall 5).
**Warning signs:** Go fixture snapshot churn that doesn't match the intended optional/nullable semantics.

### Pitfall 5: Red-by-design snapshots that can't ever go green (or that go green by accident)
**What goes wrong:** (a) Committing a `.snap` whose shape can never match what a future extractor will emit (wrong field order, machine-absolute paths, Go-ism leftovers) — the test is red but *uselessly* red. (b) Committing an *empty* or trivially-matching snapshot that flips green the moment any extractor returns anything.
**Why it happens:** Authoring the target snapshot by hand without grounding it in the neutral IR shape.
**How to avoid:** Author each red snapshot to represent the *intended green* output (the acceptance contract): the neutral graph/OpenAPI the future extractor should produce for that fixture. Make the red come from the missing extractor (`.expect()` panic) or a real shape mismatch — and document it. Mirror `snapshot_graph.rs`'s relativized, sorted, neutral shape.
**Warning signs:** A snapshot test that passes in Phase 1; a snapshot containing machine-absolute paths or language proper nouns.

### Pitfall 6: Touching the serde debt mid-flight and ballooning scope
**What goes wrong:** CONTEXT invites "prefer replacing serde where reasonable." Attempting a full hand-rolled JSON (de)serializer for the newly-shaped, recursive `Type` enum *and* generalizing the vocabulary *and* authoring three fixtures in one phase risks a large, fragile change with cross-cutting failure modes.
**Why it happens:** Reading the "prefer owned" guidance as a mandate rather than a preference.
**How to avoid:** Scope the serde replacement as its own optional task (or defer it) — the phase floor is "do not add new OSS, do not extend the serde surface." If owned (de)serialization is attempted, gate it behind its own task with its own round-trip + determinism tests, after the vocabulary generalization is green. (See Open Questions Q1.)
**Warning signs:** A single task that both reshapes `Type` and replaces serde; cascading test failures spanning facts + graph + lowering.

## Code Examples

### Exhaustive `match` over the new `Type` enum (lowering consumer — IR-03)
```rust
// Pattern (adapt to the chosen enum): NO `_ =>` catch-all. Every variant explicit so a new
// variant fails to compile here until handled. Target-specific mapping (e.g. WellKnown→Go) lives
// in the gosdk target, NOT here — lowering emits neutral OpenAPI schema objects.
fn lower_type(t: &Type, refs: &BTreeMap<&str,&str>) -> Result<SchemaObject, CoreError> {
    match t {
        Type::Primitive(p)   => Ok(SchemaObject::primitive(openapi_prim(p), None)),
        Type::WellKnown(w)   => Ok(SchemaObject::primitive("string", Some(openapi_format(w)))),
        Type::Optional(inner)=> lower_type(inner, refs),            // optionality is a field axis
        Type::Array(items)   => Ok(SchemaObject::array(lower_type(items, refs)?)),
        Type::Named(id)      => Ok(SchemaObject::reference(resolve(id, refs)?)),
        Type::Enum(members)  => Ok(SchemaObject::string_enum(members)),
        Type::Union(variants)=> lower_union(variants, refs),        // 3.1 oneOf
        Type::Any            => Ok(SchemaObject::free_form_map()),  // additionalProperties: true
        Type::Map{value, ..} => Ok(SchemaObject::map(lower_type(value, refs)?)),
        Type::Object(fields) => lower_inline_object(fields, refs),
        Type::Ext(id)        => Err(CoreError::Lowering {           // explicit, not a catch-all
            message: format!("OpenAPI target cannot represent extension type {id:?}"),
        }),
    }
}
```
*(Illustrative — the exact `SchemaObject` constructors and `Type` shape are defined by the plan.)*

### Nullable → OpenAPI 3.1 (`type: [T, "null"]`)
```rust
// Source: JSON Schema 2020-12 / OpenAPI 3.1 (see State of the Art). 3.1 has NO `nullable` keyword.
// A nullable string becomes:  type: ["string", "null"]
// (NOT the 3.0-era `nullable: true`, and NOT mere omission-from-required — that is *optional*.)
```

### Red-by-design `insta` snapshot test (new fixture)
```rust
// Source: pattern lifted from crates/gnr8-core/tests/snapshot_graph.rs (proven harness).
#![allow(clippy::unwrap_used, clippy::expect_used)]
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fastapi-<name>");

#[test]
fn graph_matches_expected_for_fastapi_fixture() {
    // RED BY DESIGN (Phase 1): no pyextract yet, so build_graph cannot produce this graph.
    // The committed snapshot encodes the INTENDED neutral graph the Phase-2 extractor must produce.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — this test is intentionally red until then");
    insta::assert_yaml_snapshot!("fastapi_fixture_graph", graph);
}
```

## State of the Art

| Old Approach (in current code) | Current/Correct Approach | When Changed | Impact |
|--------------------------------|--------------------------|--------------|--------|
| `nullable` expressed only by omission from `required` (`lower/model.rs:138` claims 3.1 has "NO `type:[T,null]` array form") | OpenAPI 3.1 drops the `nullable` keyword and uses JSON Schema 2020-12 `type: ["T","null"]` for nullability; omission-from-`required` is *optionality*, a separate concept | OpenAPI 3.1.0 (2021) | The existing comment is **factually wrong** for 3.1. Adding the nullable axis must render via the array form. This is the crux of IR-01's "optional/nullable distinct." |
| `kind: String` stringly-typed vocabulary | Closed `Type` enum (`docs/extensibility.md` §2a) | This phase | Compiler-enforced exhaustiveness across all consumers. |
| Go-only enums (string newtypes) | Neutral `Enum(Vec<EnumMember>)` covering TS string-literal-unions + Python `Literal` | This phase | Cross-language enum support (IR-01). |
| No unions | First-class `Union(Vec<Type>)` | This phase | TS `A \| B`, Python `Union` (IR-01). |

**Deprecated/outdated:**
- The `lower/model.rs` `SchemaObject` doc comment asserting 3.1 has no `type:[T,null]` form — outdated; correct when implementing nullability.
- `serde`/`serde_json`/`blake3`/`thiserror` in `gnr8-core` — all on the CLAUDE.md rule-2 debt list. Do not extend; do not add more.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Replacing serde with owned (de)serialization is out of the critical path for this phase and may be deferred/gated. | Standard Stack, Pitfall 6, Open Q1 | If the user intends serde to be *removed* this phase, the plan undersizes; surface in discuss/plan. CONTEXT's "at minimum do not add new OSS" supports the conservative reading. |
| A2 | Fixtures are STATIC source + red snapshots only — no `pip install`/`npm install`, no runnable extractor in Phase 1. | Package Legitimacy Audit, Architecture Pattern 4 | If the plan wants runnable/hermetic fixtures now, framework installs (`fastapi`/`flask`/`@nestjs/common`) must be added + verified. The phase boundary ("no extraction logic") supports static-only. |
| A3 | The nullable axis is best modeled as a parallel field flag (or a `Type::Nullable` wrapper) reconciling the brief's "two distinct axes" with §2a's single `Optional(Box<Type>)`. | Architecture Pattern 2 | Picking the wrong shape forces rework in lowering+SDK. Low risk — both shapes are tractable; the plan must pick one and document it. |
| A4 | Go 1.26 + the Go toolchain remain available in dev+CI (existing snapshot tests depend on it); new Python/TS fixtures need NO toolchain in Phase 1. | Validation Architecture | If CI lacks Go, even existing tests skip (they skip gracefully). New fixtures add no toolchain need this phase. |
| A5 | OpenAPI 3.1 nullable canonical form is `type: ["T","null"]`. | State of the Art, Code Examples | LOW — verified via WebSearch against OpenAPI spec discussions + learn.openapis.org; this is well-established since 3.1.0 (2021). |

**Note:** A5 is verified (MEDIUM→HIGH via official OpenAPI migration docs). A1–A4 are scoping assumptions the planner/discuss should confirm.

## Open Questions

1. **Should serde be removed from the facts/graph path THIS phase, or deferred?**
   - What we know: CONTEXT says "prefer replacing serde where reasonable; at minimum do not add new OSS." serde/serde_json are rule-2 debt.
   - What's unclear: whether "prefer" rises to "must" for this phase.
   - Recommendation: Treat removal as a *separate, optional* task gated behind the vocabulary generalization (which must land green first). Phase floor = no new OSS, no extended serde surface. If attempted, owned (de)ser needs its own round-trip + determinism tests for the recursive `Type` enum. Surface to the user at plan time.

2. **Optional vs nullable: field-flag pair or type wrapper?**
   - What we know: brief locks two distinct axes; §2a sketches `Optional(Box<Type>)` only.
   - What's unclear: the exact Rust shape.
   - Recommendation: parallel `optional: bool` (presence) + nullable axis (flag or `Type::Nullable`), applied consistently. Document the choice in the plan; verify against the FastAPI/Flask/NestJS acceptance cases (which exercise all four combinations).

3. **How "real" must the fixture services be?**
   - What we know: they must "encode the v2.0 acceptance cases" and have red snapshots; no extractor runs against them this phase.
   - What's unclear: whether they must be runnable apps (with deps) or just representative typed source.
   - Recommendation: representative, type-rich source that exercises objects/arrays/enums/optional/nullable/unions per language — runnable-ness is a Phase-2+ concern (PYSDK-02 calls for a hermetic round-trip *later*). Keep Phase 1 static.

4. **Snapshot granularity for red fixtures: graph-only, or graph + OpenAPI + (future) SDK?**
   - What we know: the brief wants "one OpenAPI per fixture, structurally aligned" as the eventual proof.
   - Recommendation: commit at least a graph snapshot per fixture (the IR contract) and an OpenAPI snapshot per fixture (the structural-alignment proof). SDK snapshots belong to the SDK phases (3/5). Author each as the intended-green shape.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain (cargo, clippy 1.85+) | Core refactor + tests | ✓ (assumed — workspace builds today) | edition 2021, MSRV 1.85 | — |
| `insta` 1.48 | Snapshot tests | ✓ (dev-dependency, in `Cargo.toml`) | 1.48 | — |
| Go toolchain | Existing Go-fixture snapshot tests (regression guard) | ✓ (dev+CI, go 1.26 per test comments) | 1.26 | Tests skip gracefully if absent (`determinism.rs:36`) |
| Python / Node toolchains | Authoring FastAPI/Flask/NestJS *source* | Not required in Phase 1 (static fixtures, no extraction) | — | N/A — fixtures are source files; no interpreter/compiler runs this phase |

**Missing dependencies with no fallback:** None for Phase 1.
**Missing dependencies with fallback:** Go toolchain (existing tests skip if absent — no action needed).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness + `insta` 1.48 snapshots |
| Config file | none dedicated; `insta` configured via env (`INSTA_UPDATE=no` under `CI=true`) |
| Quick run command | `cargo test -p gnr8-core <test_name>` |
| Full suite command | `cargo test --workspace --locked` (then `cargo clippy --all-targets --all-features --locked -- -D warnings`) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| IR-01 | Neutral `Type` enum expresses objects/arrays/enums/optional/nullable/unions | unit | `cargo test -p gnr8-core graph::` / `lower::` / `gosdk::` | ❌ Wave 0 — new unit tests on the enum + round-trip |
| IR-02 | Facts deserialize strictly; no language terms leak | unit | `cargo test -p gnr8-core facts::` (round-trip + `deny_unknown_fields` reject) | ⚠️ extend existing `facts.rs` tests for the neutral shape |
| IR-03 | Lowering + SDK consume IR with no per-language branches | unit + snapshot | `cargo test -p gnr8-core` + existing `snapshot_openapi`/`snapshot_sdk` stay GREEN for Go | ✅ existing Go snapshots are the regression guard (must not break) |
| IR-04 | FastAPI/Flask/NestJS fixtures + red snapshots committed | snapshot | `cargo test -p gnr8-core snapshot_<lang>` (RED by design) | ❌ Wave 0 — new fixtures + new `snapshot_*.rs` + new `.snap` |
| (regression) | Go pipeline still byte-identical after generalization | integration | `cargo test -p gnr8-core --test determinism` | ✅ `tests/determinism.rs` exists |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core <touched module>` + `cargo clippy -p gnr8-core --all-targets -- -D warnings`
- **Per wave merge:** `cargo test --workspace --locked`
- **Phase gate:** Full suite — Go snapshots GREEN (regression), new fixture snapshots RED-by-design (documented), clippy clean — before `/gsd:verify-work`.

### Wave 0 Gaps
- [ ] `tests/snapshot_fastapi_graph.rs` + `tests/snapshots/…fastapi…graph.snap` — RED, covers IR-04
- [ ] `tests/snapshot_flask_graph.rs` + snapshot — RED, covers IR-04
- [ ] `tests/snapshot_nestjs_graph.rs` + snapshot — RED, covers IR-04
- [ ] (recommended) per-fixture OpenAPI red snapshots — structural-alignment proof
- [ ] New unit tests for the `Type` enum (construction, exhaustive lowering, nullable≠optional) — covers IR-01
- [ ] Extended `facts.rs` round-trip tests for the neutral contract — covers IR-02
- [ ] `fixtures/{fastapi,flask,nestjs}-<name>/` source — covers IR-04

*(No framework install needed — Rust test harness + `insta` already present.)*

## Security Domain

> `security_enforcement` config not located in this repo's `.planning/config.json` scan; this phase is a typed-model refactor + static fixtures with no new attack surface (no network, no user input parsing at runtime, no auth). The relevant invariant-adjacent controls:

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V5 Input Validation | yes (host↔sidecar boundary) | `#[serde(deny_unknown_fields)]` on the facts DTO — already enforced (`facts.rs:23`); the threat model (T-02-05) is malformed/forward-incompatible JSON from the helper. Preserve this on the generalized contract. |
| V6 Cryptography | no | — (the only hash, `blake3`, is for the ownership manifest, not in this phase's scope) |
| V2/V3/V4 (auth/session/access) | no | This phase has no runtime, no sessions, no access control. |

### Known Threat Patterns for this stack
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious/malformed facts JSON from a sidecar | Tampering | `deny_unknown_fields` strict deserialize (carry forward to the neutral contract). |
| Path traversal in generated artifact names | Tampering | Already mitigated in `sdk/builtins.rs:351` (reject `/`, `\`, `..`); not changed this phase. |
| Subprocess argument injection | Tampering | Already mitigated (`helper.rs`: target passed as discrete `Command` arg, no shell). Not changed this phase. |

*No new security-sensitive code is introduced; the floor is "do not weaken `deny_unknown_fields` when generalizing the facts contract."*

## Sources

### Primary (HIGH confidence)
- `docs/extensibility.md` §2a (Type enum), §2b (Service model), §2c (Extensions), §6 (capability diagnostics) — the authoritative IR generalization design.
- `docs/milestone-v2-multi-language.md` §"IR generalization", §"Success criteria", §"The leverage" — milestone brief.
- `crates/gnr8-core/src/analyze/facts.rs` (current `GoFacts`/`SchemaType` contract + `deny_unknown_fields` + round-trip tests).
- `crates/gnr8-core/src/graph/mod.rs` (current `ApiGraph`/`Schema`/`SchemaType` IR + `from_facts` + sorting/determinism).
- `crates/gnr8-core/src/lower/{mod,model}.rs` (OpenAPI lowering consumer; the `kind: String` match; the 3.1 nullable comment).
- `crates/gnr8-core/src/gosdk/emit.rs` (Go SDK target; Go-specific TypeMap; `maybe_pointer` optional logic).
- `crates/gnr8-core/src/sdk/{mod,builtins.rs}` (Source/Transform/Target/PostProcess traits; `GoGin`/`OpenApi31`/`GoSdk`).
- `crates/gnr8-core/tests/{snapshot_graph,snapshot_openapi,snapshot_sdk,snapshot_diagnostics,determinism}.rs` + `tests/snapshots/*.snap` (the proven fixture/snapshot harness).
- `goextract/internal/facts/facts.go` + `goextract/main.go` (the sidecar contract twin + sidecar pattern).
- `CLAUDE.md` (rules 1–4 + known debt list) and `Cargo.toml` (workspace deps + lint policy).
- `thoughts/skills/rust-best-practices/SKILL.md` (project Rust conventions — no `unwrap`/`expect`/`panic` in prod, `thiserror` for libs, exhaustive matches, `insta` for snapshots).

### Secondary (MEDIUM confidence)
- WebSearch on OpenAPI 3.1 nullable representation — corroborated by learn.openapis.org migration guide and OAI/OpenAPI-Specification issues (3.1 drops `nullable`, uses `type: ["T","null"]`).

### Tertiary (LOW confidence)
- None relied upon.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all components verified by direct read of the actual files; no new packages.
- Architecture: HIGH — neutral `Type` enum design is from the committed authoritative `docs/extensibility.md`; the consumer delta is verified against `lower/` + `gosdk/` source.
- Pitfalls: HIGH — every pitfall is grounded in a specific current code location (contract drift, Go-isms, `_ =>` catch-alls, `maybe_pointer` conflation, serde debt).
- OpenAPI 3.1 nullable fact: MEDIUM-HIGH — verified via WebSearch against official OpenAPI migration docs; corrects a factually-wrong comment in the current codebase.

**Research date:** 2026-06-25
**Valid until:** 2026-07-25 (stable — internal codebase + a 2021-stable OpenAPI fact; the only moving part is the team's own design, which is locked by CONTEXT)
