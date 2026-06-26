---
phase: 04-typescript-source-tsextract
verified: 2026-06-25T23:45:00Z
status: passed
score: 4/4 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
re_verification:
  previous_status: null
  previous_score: null
---

# Phase 4: TypeScript Source — `tsextract` Verification Report

**Phase Goal:** A developer can turn a real NestJS service into the neutral IR via a `tsextract` sidecar built on the `typescript` Compiler API, deriving every schema fact from the source's own TS types and never from third-party annotation/validation tools, so the reused Rust pipeline produces OpenAPI 3.1 for NestJS services.
**Verified:** 2026-06-25T23:45:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria = the contract)

| # | Truth (Success Criterion) | Status | Evidence |
|---|---------------------------|--------|----------|
| 1 | A developer can extract routes, params, and request/response DTOs from a NestJS service into the IR | ✓ VERIFIED | `node tsextract/index.js fixtures/nestjs-bookstore` → 4 routes (GET / listBooks 200, POST / createBook 201, GET /{bookId} getBook 200, PUT /{bookId} updateBook 200) + 8 schemas + 0 diagnostics. `snapshot_nestjs_graph` GREEN. routes.js uses `ts.getDecorators` for @Controller/@Get/@Post/@Put/@Patch/@Delete/@Param/@Query/@Body; group-relative paths (no `/books` prefix folded). |
| 2 | The sidecar derives every schema fact from the source's OWN TS types via the `typescript` Compiler API — never from `@nestjs/swagger`, `zod`, or `class-validator` (bright-line) | ✓ VERIFIED | load.js: `ts.createProgram` + `program.getTypeChecker()`, synthesized `strictNullChecks`+`experimentalDecorators`, NO tsconfig read. `grep -niE "swagger|zod|class-validator" tsextract/*.js` → CLEAN. package.json sole dependency `typescript@5.9.3`. `git diff Cargo.toml/Cargo.lock` across phase EMPTY (zero new Rust crates). |
| 3 | Unsupported/untyped surfaces produce diagnostics, never guessed facts; no fallback; static-only (target TS never executed) | ✓ VERIFIED | Static-only grep gate (`eval/vm/child_process/transpileModule/require(target)`) → CLEAN; all requires are `typescript`/`fs`/`path`/relative only. types.js: unresolvable → `(null, diagnostic)`, never an `any` fact (rule 3). CR-01..04 + WR-01..05 rule-3 hardening present (`isSchemaBearingAlias`, `load.underTarget`, `Record`→map, `_propertyKey`) with edge-cases + route-edges regression suites passing. |
| 4 | A developer enables NestJS extraction from `.gnr8/` via a `NestJs` `Source` built-in; the NestJS red snapshot turns green through the reused Rust pipeline | ✓ VERIFIED | `pub struct NestJs` in sdk/builtins.rs calls the SAME `crate::analyze::build_graph` (language detected from target, not Source); exported in `prelude`. `detect_language` is a single 3-way classifier; both build_graph + diagnostics::collect dispatch are exhaustive 3-arm (no `_ =>`). Both `snapshot_nestjs_graph` + `snapshot_nestjs_openapi` GREEN (no `#[ignore]`), OpenAPI 3.1.0 emitted. |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `tsextract/load.js` | createProgram + getTypeChecker, static-only, synthesized strict opts | ✓ VERIFIED | createProgram@71, getTypeChecker@78, strictNullChecks@74, experimentalDecorators@73, no tsconfig read |
| `tsextract/types.js` | TS Type → neutral Type, axis stripping, number→float64, named-vs-inline | ✓ VERIFIED | TypeFlags.Undefined/Null stripping, float64 mapping, single aliasSymbol discriminator (rule 3) |
| `tsextract/schemas.js` | DTO → SchemaFact, transitive collection, schema-id | ✓ VERIFIED | 8 schemas transitively collected; underTarget gate; isSchemaBearingAlias |
| `tsextract/routes.js` | NestJS decorator recognition → RouteFacts | ✓ VERIFIED | getDecorators@96, Controller@105, method-derived status + @HttpCode override |
| `tsextract/facts.js` | deterministic sorted JSON marshal | ✓ VERIFIED | byte-identical across 2 runs |
| `tsextract/package.json` | sole dep typescript pinned | ✓ VERIFIED | exactly one dependency `typescript: 5.9.3` |
| `crates/.../analyze/helper.rs` | run_tsextract + tsextract_dir subprocess driver | ✓ VERIFIED | discrete args `["index.js", target_dir]`, no shell; typed errors; no prod panic |
| `crates/.../sdk/builtins.rs` | NestJs Source built-in | ✓ VERIFIED | `pub struct NestJs`, calls build_graph, exported in prelude |
| `crates/.../tests/snapshot_nestjs_graph.rs` | GREEN, no #[ignore], skip-guard | ✓ VERIFIED | `#[ignore]` count 0, skip-guard present, test ok |
| `crates/.../tests/snapshot_nestjs_openapi.rs` | GREEN, no #[ignore], skip-guard | ✓ VERIFIED | `#[ignore]` count 0, OpenAPI 3.1.0, test ok |

### Key Link Verification

| From | To | Via | Status |
|------|----|----|--------|
| analyze/mod.rs build_graph | helper::run_tsextract | Lang::TypeScript arm (exhaustive, no `_=>`) | ✓ WIRED |
| diagnostics/mod.rs collect | helper::run_tsextract | Lang::TypeScript arm (exhaustive) | ✓ WIRED |
| helper.rs run_tsextract | node subprocess | Command::new("node").args(["index.js", target]) discrete args | ✓ WIRED |
| NestJs Source | crate::analyze::build_graph | same shared seam (lang from target) | ✓ WIRED |
| types.js | TypeChecker / aliasSymbol | single discriminator, one path | ✓ WIRED |
| routes.js | ts.getDecorators | @Controller + verb/param decorator walk | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Produces Real Data | Status |
|----------|------|--------|--------------------|--------|
| snapshot_nestjs_graph/openapi | graph/openapi | real `build_graph` → tsextract → ts.createProgram over fixture *.ts | Yes — 4 real routes + 8 real schemas + OpenAPI 3.1.0 | ✓ FLOWING |
| tsextract facts doc | routes/schemas | TypeChecker resolution of fixture DTOs | Yes — non-empty, snapshot-matching | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Determinism (2 runs byte-identical) | `node tsextract/index.js fixtures/nestjs-bookstore` ×2 + Buffer.equals | BYTE-IDENTICAL | ✓ PASS |
| Counts | facts doc | 8 schemas, 4 routes, 0 diagnostics | ✓ PASS |
| Schema ids | facts doc | exactly the 8 (SortOrder absent, OutOfStockDto present) | ✓ PASS |
| Group-relative paths | facts doc | /, /, /{bookId}, /{bookId} (no /books fold) | ✓ PASS |
| Method-derived status | facts doc | POST=201, others=200 | ✓ PASS |
| tsextract test suite | `node tsextract/tests/*.test.js` (7 files) | all OK incl. edge-cases + route-edges regression | ✓ PASS |
| nestjs snapshots | `cargo test --test snapshot_nestjs_graph --test snapshot_nestjs_openapi` | 2 passed, 0 ignored | ✓ PASS |
| lib unit tests (seam) | `cargo test -p gnr8-core --lib` | 174 passed; detect_language 3-way, NestJs Source error, run_tsextract toolchain-missing, TS Display all ok | ✓ PASS |
| red-by-design contract | `cargo test -p gnr8-core -- --ignored` | 0 ignored tests anywhere (no `#[ignore]` in codebase) | ✓ PASS |
| Green gate | `make check` | exit 0 (all snapshots + go build/vet/test green) | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| TSSRC-01 | 04-03 | Extract NestJS routes/params/request+response DTOs into IR | ✓ SATISFIED | routes.js + green nestjs graph snapshot + 4-route facts doc |
| TSSRC-02 | 04-02 | typescript Compiler API; facts from source's own types; bright-line | ✓ SATISFIED | createProgram/getTypeChecker; swagger/zod/class-validator grep clean; sole dep typescript |
| TSSRC-03 | 04-02 | Unsupported/untyped → diagnostics, no fallback; static-only | ✓ SATISFIED | static-only grep clean; unresolvable→diagnostic+omit; CR/WR rule-3 hardening + regression tests |
| TSSRC-04 | 04-01 | NestJs Source built-in for .gnr8/ | ✓ SATISFIED | `pub struct NestJs` in builtins.rs, prelude-exported, single 3-way dispatch, snapshot green |

All 4 declared requirement IDs cross-referenced against REQUIREMENTS.md (lines 39-42, 104-107). No orphaned requirements — REQUIREMENTS.md maps exactly TSSRC-01..04 to Phase 4, all claimed by plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| crates/gnr8-core/src/error.rs | 19 | "not yet implemented" string | ℹ️ Info | Pre-existing `Unimplemented` typed-error message (verified present at phase base `ee12ac8`), NOT a phase-04 stub. No impact. |

No debt markers (TBD/FIXME/XXX) in phase-04-modified source. No production unwrap/expect/panic in the Rust seam (the two `.expect` in helper.rs are inside `#[cfg(test)]` at line 194+).

### CLAUDE.md Invariant Compliance

- Rule 1 (no coupling to other tools' conventions): bright-line grep for swagger/zod/class-validator CLEAN; @Controller prefix recorded as provenance only, never folded; fixture reconciliation added only blank lines/non-fact comments (no forbidden import/annotation in fixture diff).
- Rule 2 (no OSS deps): tsextract sole dep `typescript` (documented carve-out); gnr8-core added ZERO crates (`git diff Cargo.toml/Cargo.lock` across phase EMPTY).
- Rule 3 (no fallback/dual paths): detect_language single 3-way classifier; both dispatch matches exhaustive 3-arm; single named-vs-inline discriminator; unresolvable → diagnostic+omit (no guessed `any`).
- Rule 4 (config-as-code): NestJs Source is a Rust built-in; title/base_path/security test-supplied via to_openapi.
- Deterministic byte-identical output: confirmed (2-run Buffer.equals + determinism twins + zero snapshot edits across phase).

### Human Verification Required

None. All success criteria are programmatically verifiable and were verified by running the toolchain (node v24.14.1 + vendored typescript 5.9.3 + Go 1.26 + cargo 1.96). The 04-REVIEW-FIX flagged CR-01/WR-01 as "requires human verification (logic)", but those are now locked by passing regression suites (edge-cases.test.js, route-edges.test.js) and the byte-identical green snapshots, which provide objective evidence — no manual judgment needed.

### Gaps Summary

No gaps. All 4 ROADMAP success criteria are observably true in the codebase. The full NestJS → neutral IR → OpenAPI 3.1 path works through real Compiler-API extraction: `make check` is green, both nestjs snapshots flipped green with ZERO snapshot edits and ZERO `#[ignore]` remaining, the sidecar is static-only / bright-line-clean / single-dep / deterministic, and all 4 requirement IDs are satisfied. The 9 code-review findings (CR-01..04, WR-01..05) were fixed with regression coverage and the green gate confirms no regressions.

---

_Verified: 2026-06-25T23:45:00Z_
_Verifier: Claude (gsd-verifier)_
