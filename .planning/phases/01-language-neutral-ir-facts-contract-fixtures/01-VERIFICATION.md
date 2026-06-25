---
phase: 01-language-neutral-ir-facts-contract-fixtures
verified: 2026-06-25T16:11:42Z
status: passed
score: 4/4 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
re_verification:
  previous_status: none
notes:
  - "REQUIREMENTS.md traceability marks IR-03 as 'Pending' (lines 20, 94) but 01-02 delivered it; the lowering/SDK 'no per-language branch' capability (success criterion 2) is VERIFIED in code. This is a stale doc-tracking status, not a goal gap — recommend flipping IR-03 to Complete in REQUIREMENTS.md."
---

# Phase 1: Language-Neutral IR + Facts Contract + Fixtures Verification Report

**Phase Goal:** Generalize the `ApiGraph` IR and the shared JSON facts contract from "router-agnostic" to fully "type-system-neutral", and stand up the multi-language fixture+snapshot harness, so that every later sidecar has a single neutral target and a red-by-design acceptance contract to turn green.
**Verified:** 2026-06-25T16:11:42Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth (Success Criterion) | Status | Evidence |
|---|---------------------------|--------|----------|
| 1 | IR + JSON facts contract express the cross-language type vocabulary (objects, arrays, enums, optional/nullable, unions) with no Go/Gin/FastAPI/Nest terms leaking into the IR | ✓ VERIFIED | `facts.rs:146-178` closed `enum Type` = Primitive/WellKnown/Array/Map/Named/Object/Enum/Union/Any. `optional` + `nullable` are distinct `FieldFact` fields (`facts.rs:125,128`) with all-four-combination round-trip tests (`facts.rs:521,528`). `graph/mod.rs:25` re-exports the SAME enum (single definition, no Go duplication). Leak grep for `gin/fastapi/nestjs/pydantic/binding:/omitempty/json:"` in IR type vocab returned ZERO hits. |
| 2 | Rust host deserializes facts strictly (`deny_unknown_fields`); OpenAPI lowering + SDK generation consume the IR with no per-language branches | ✓ VERIFIED | `#[serde(deny_unknown_fields)]` on every facts struct (`facts.rs:29,43,68,84,97,116` + Type/Prim adjacent-tag rejection); rejection asserted by tests (`facts.rs:450,476`). `lower/mod.rs:302` and `:412` match `Type` EXHAUSTIVELY (all 9 variants, no `_ =>`). `gosdk/emit.rs:114` matches `Type` exhaustively; `Union` is an explicit typed-error arm (Go has no sum types), not a catch-all. All Go-isms (time.Time/int64/float32/map[string]any) are LOCAL to gosdk. The 3 remaining `_ =>`/`other =>` arms are on HTTP-method (`lower:208`) and word/initialism string matches (`emit:57`), never on a `Type` match. |
| 3 | FastAPI, Flask, and NestJS fixture services exist and encode the v2.0 acceptance cases | ✓ VERIFIED | `fixtures/fastapi-bookstore/app/models.py`, `fixtures/flask-bookstore/app/dto.py`, `fixtures/nestjs-bookstore/src/books.dto.ts` all exist with type-rich source: objects, arrays, enums (`enum.Enum`+`Literal` / string-literal-union), unions (incl. union-of-objects), and all four optional×nullable combos explicitly documented per field. Facts come from each language's OWN types (rule 1): grep for `swagger/zod/class-validator/class-transformer/marshmallow/ApiProperty` across all fixtures returned ZERO hits. NestJS uses only `@nestjs/common` decorators (the Gin analog). |
| 4 | Red-by-design snapshots for each fixture committed and visibly failing before any extraction lands | ✓ VERIFIED | 6 test files + 6 committed `.snap` files (119–414 lines, hand-authored intended neutral shape). All 6 `#[ignore]`-marked with honest `.expect("...lands in Phase N")`. `make check` exits 0 (GREEN gate). `cargo test -p gnr8-core --no-fail-fast -- --ignored` reports exactly 6 FAILED (fastapi/flask/nestjs × graph+openapi), each via insta snapshot mismatch. Makefile `gates` target runs only the 4 Go contract tests by name, excluding the reds. `.snap.new` artifacts are untracked by git. |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/gnr8-core/src/analyze/facts.rs` | Neutral Facts DTO + closed Type enum + optional/nullable axes; strict deserialize | ✓ VERIFIED | `enum Type` present; deny_unknown_fields on all structs; 4-combination tests |
| `crates/gnr8-core/src/graph/mod.rs` | Neutral IR mirroring Type enum + axes | ✓ VERIFIED | Re-exports the single `Type` enum; `normalize_type` exhaustive; `from_facts` maps DTO→IR |
| `goextract/internal/facts/facts.go` | Go wire DTO in lockstep, stdlib `encoding/json` only | ✓ VERIFIED | json tags byte-identical to Rust serde field names (`json_name`/`operation_id`/`request_body`…) |
| `crates/gnr8-core/src/lower/mod.rs` | Exhaustive neutral Type → OpenAPI; no `_ =>`; nullable→type:[T,null] | ✓ VERIFIED | Two exhaustive Type matches; type-array rendering in yaml.rs |
| `crates/gnr8-core/src/lower/model.rs` | SchemaObject.nullable; corrected 3.1 comment | ✓ VERIFIED | `nullable: bool` field; comment corrected to "3.1 type array form" |
| `crates/gnr8-core/src/gosdk/emit.rs` | Exhaustive Type → Go; maybe_pointer reads nullable, json_tag reads optional | ✓ VERIFIED | go_type exhaustive; conflation fix documented + implemented |
| `fixtures/fastapi-bookstore/` | Type-rich FastAPI source | ✓ VERIFIED | models.py: full acceptance vocabulary |
| `fixtures/flask-bookstore/` | Typed-envelope Flask source | ✓ VERIFIED | dto.py + routes.py: opt-in typed DTOs, untyped spots flagged for diagnostics |
| `fixtures/nestjs-bookstore/` | `@nestjs/common` + DTO classes | ✓ VERIFIED | books.dto.ts + books.controller.ts: ordinary TS types only |
| `crates/gnr8-core/tests/snapshot_fastapi_graph.rs` (+5) | RED-by-design snapshot tests | ✓ VERIFIED | All 6 present, `#[ignore]`, honest `.expect()` |

### Key Link Verification

| From | To | Via | Status |
|------|----|----|--------|
| `facts.rs` (serde fields) | `facts.go` (json tags) | byte-identical names | ✓ WIRED |
| `facts.rs` Facts DTO | `graph/mod.rs` IR | `ApiGraph::from_facts` (graph:229, analyze:32) | ✓ WIRED |
| `lower/mod.rs` | `Type` enum | exhaustive match (no per-language branch) | ✓ WIRED |
| `gosdk/emit.rs` | `Type` enum | exhaustive match, Go-isms local (time.Time) | ✓ WIRED |
| snapshot tests | fixture dirs | `FIXTURE_DIR` concat → `build_graph` | ✓ WIRED (red-by-design) |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Green gate passes | `make check` | exit 0 (full Rust + Go fixture/goextract build/vet/test) | ✓ PASS |
| Red-by-design count | `cargo test -p gnr8-core --no-fail-fast -- --ignored` | exactly 6 FAILED (insta snapshot mismatch panics) | ✓ PASS |
| No forbidden tool coupling | grep swagger/zod/class-validator/marshmallow in fixtures + crates/goextract src | 0 hits | ✓ PASS |
| No new OSS deps in gnr8-core | inspect Cargo.toml `[dependencies]` | serde/serde_json/thiserror/blake3 = pre-existing debt (CLAUDE.md known-debt); none added (01-02 `added: []`) | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| IR-01 | 01-01 | Cross-language type vocabulary without Go assumptions | ✓ SATISFIED | Truth 1 — closed neutral Type enum |
| IR-02 | 01-01 | One shared JSON contract, strict deserialize, no language leak | ✓ SATISFIED | Truth 2 — deny_unknown_fields + lockstep Go DTO + leak grep clean |
| IR-03 | 01-02 | OpenAPI lowering + SDK consume IR unchanged, no per-language branches | ✓ SATISFIED | Truth 2 — exhaustive Type matches in lower/ + gosdk/, Go-isms local. NOTE: REQUIREMENTS.md traceability still marks this "Pending" (stale doc status; 01-02-SUMMARY marks it complete with verifiable code). |
| IR-04 | 01-03 | Multi-language fixtures + red-by-design snapshots before extraction | ✓ SATISFIED | Truths 3 & 4 |

All 4 phase requirement IDs (IR-01..IR-04) are declared in plan frontmatter and accounted for. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| — | — | No TBD/FIXME/XXX in phase-modified src | — | None |
| — | — | No TODO/HACK/PLACEHOLDER/"not yet implemented" in src or fixtures | — | None |
| — | — | No forbidden tool-convention parsing (rule 1) | — | None |

The `.expect("...lands in Phase 2/N")` panics in the 6 red snapshot tests are INTENTIONAL red-by-design honesty, not stub anti-patterns — they are `#[ignore]`-gated and excluded from the green gate.

### Human Verification Required

None. All four success criteria are programmatically verifiable and were verified against the codebase (closed-enum exhaustiveness, deny_unknown_fields rejection, fixture source content, `make check` green + exactly 6 red `--ignored` failures). No visual/real-time/external-service behavior is in scope for this phase.

### Gaps Summary

No gaps. All four roadmap success criteria are VERIFIED with codebase evidence:
- The neutral `Type` enum + independent optional/nullable axes are the single IR vocabulary, with zero language proper nouns leaking in.
- `deny_unknown_fields` is enforced and tested; both lowering and the Go SDK consume the enum via exhaustive matches with all Go-isms localized — no per-language branches.
- All three fixtures (FastAPI, Flask, NestJS) exist, are type-rich, and derive facts from each language's own type system (no swagger/zod/class-validator/marshmallow coupling).
- The 6 red-by-design snapshots are committed, substantive, honestly failing under `--ignored`, and excluded from the green `make check` gate (which exits 0).

One non-blocking documentation note: REQUIREMENTS.md still lists IR-03 as "Pending" in its traceability table, while 01-02 delivered it and the underlying capability is verified in code. Recommend updating IR-03 to "Complete". This does not affect the phase goal or status.

---

_Verified: 2026-06-25T16:11:42Z_
_Verifier: Claude (gsd-verifier)_
