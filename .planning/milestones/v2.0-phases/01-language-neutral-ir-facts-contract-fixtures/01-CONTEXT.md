# Phase 1: Language-Neutral IR + Facts Contract + Fixtures - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Infrastructure phase (no user-facing behavior — IR generalization + fixture/snapshot harness)

<domain>
## Phase Boundary

Generalize the existing `ApiGraph` IR and the shared JSON facts contract from "router-agnostic"
(Go/Gin-shaped) to fully **type-system-neutral**, and stand up the multi-language fixture +
red-by-design snapshot harness — so every later language sidecar (Phases 2–5) has a single neutral
target and a failing acceptance contract to turn green.

**In scope:**
- Evolve the IR `Type`/`Schema` vocabulary to neutrally express: objects, arrays, enums,
  optional vs nullable (distinct), and unions — with no Go/Gin/FastAPI/Nest terms leaking in.
- Generalize the JSON facts contract (currently `GoFacts` in `crates/gnr8-core/src/analyze/facts.rs`)
  into a language-neutral facts document, deserialized strictly (`#[serde(deny_unknown_fields)]`
  today — note this is on the rule-2 debt list; prefer owned (de)serialization where reasonable).
- Ensure OpenAPI lowering (`lower/`) + SDK generation (`sdk/`) consume the IR with **no per-language
  branches** (IR-03).
- Author FastAPI, Flask, and NestJS fixture services encoding the v2.0 acceptance cases.
- Commit **red-by-design** snapshots for each fixture that visibly fail before any extraction exists.

**Out of scope (later phases):** any extraction logic (`pyextract`/`tsextract`), any new SDK target
output, `.gnr8/` Source/Target built-ins for the new languages.

</domain>

<decisions>
## Implementation Decisions

### Grounding documents (authoritative — follow these, don't re-derive)
- IR generalization design: `docs/extensibility.md` ("Type-enum evolution + typed Extensions
  side-channel"). This is the sanctioned shape for the neutral `Type` enum and the typed Extensions
  side-channel — plan-phase research must read it and follow it.
- Milestone brief: `docs/milestone-v2-multi-language.md` (§"IR generalization", §"Success criteria").
- Requirements: IR-01, IR-02, IR-03, IR-04 in `.planning/REQUIREMENTS.md`.

### Type vocabulary (from the brief — locked)
- Optional and nullable are **distinct** axes (TS `?` vs `| null`; Python `Optional` vs a field that
  may be `None`). The IR must represent both independently, not collapse them.
- Enums generalize beyond Go string-enums to cover TS string-literal-union enums and Python `Literal`.
- Unions are first-class in the neutral vocabulary (TS `A | B`, Python `Union`).
- Arrays/lists and nested objects are neutral (`list[T]`, `T[]`, nested models).

### Invariants (CLAUDE.md — non-negotiable, carried into this phase)
- One source of truth per fact; **no fallback / dual control-flow paths** (rule 3).
- **Zero OSS in `gnr8-core`**; prefer hand-rolled, in-repo code (rule 2). Existing `serde`/`serde_json`
  usage is pre-existing debt — when this phase touches `facts.rs`, prefer replacing serde usage with
  owned (de)serialization over extending it; at minimum do not add new OSS deps.
- No language-tool convention coupling (rule 1) — facts come from each language's own type system,
  never from `@nestjs/swagger` / `zod` / `class-validator` / `marshmallow` / FastAPI's runtime
  `/openapi.json`.
- Deterministic, sorted, byte-identical output.

### Fixture + snapshot harness (mirror v1.0's proven pattern)
- Follow the existing snapshot test pattern under `crates/gnr8-core/tests/` (`snapshot_openapi.rs`,
  `snapshot_graph.rs`, etc.) and `tests/snapshots/`.
- Fixtures live alongside the existing Go fixture convention (`goextract/` is the Go fixture host);
  add FastAPI / Flask / NestJS fixture services in the analogous per-language layout chosen at plan time.
- "Red-by-design" means: snapshot files committed and the tests visibly failing (no extractor yet),
  documented as intentional so they are the acceptance contract Phases 2–5 turn green.

### Claude's Discretion
All remaining implementation choices — exact module layout for the neutral facts type, the precise
Rust enum shape (guided by `docs/extensibility.md`), fixture directory naming, and snapshot file
organization — are at Claude's discretion, guided by the grounding documents above and existing
v1.0 codebase conventions. Discuss was skipped: this is a pre-specified infrastructure phase.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/gnr8-core/src/analyze/facts.rs` — current `GoFacts` facts contract (the type to generalize).
- `crates/gnr8-core/src/graph/` — the `ApiGraph` IR (router-agnostic today; generalize to neutral).
- `crates/gnr8-core/src/lower/` — OpenAPI 3.1 lowering (must consume IR with no per-language branches).
- `crates/gnr8-core/src/sdk/` + `gosdk/` — SDK generation (twin pattern future targets will follow).
- `crates/gnr8-core/tests/snapshot_*.rs` + `tests/snapshots/` — the snapshot harness to extend.

### Established Patterns
- Facts flow: language sidecar → JSON facts → strict deserialize → `build_graph` → `ApiGraph` →
  lower/sdk. This phase widens the JSON facts + IR without forking the downstream pipeline.
- Snapshot-driven acceptance: contract snapshots gate behavior; determinism test enforces
  byte-identical output.

### Integration Points
- New fixture services connect only as test inputs + committed snapshots this phase (no extraction).
- The generalized IR is the single seam every later phase plugs into.

</code_context>

<specifics>
## Specific Ideas

Follow `docs/extensibility.md`'s "Type-enum evolution + typed Extensions side-channel" sketch as the
IR generalization design — it is the sanctioned approach, not a starting point to debate.

</specifics>

<deferred>
## Deferred Ideas

None — discuss phase skipped (infrastructure phase, scope is fully specified by the brief +
REQUIREMENTS IR-01..04).

</deferred>
