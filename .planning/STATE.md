---
gsd_state_version: 1.0
milestone: v3.0
milestone_name: "Production-ready SDK adoption"
status: in_progress
stopped_at: Phase 2 Plan 1 complete; bearer/basic auth and richer typed errors remain
last_updated: "2026-07-09T12:00:00.000Z"
last_activity: 2026-07-09 — Completed Phase 2 Plan 1 query API-key auth
progress:
  total_phases: 5
  completed_phases: 1
  total_plans: 2
  completed_plans: 2
  percent: 20
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-09)

**Core value:** Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.
**Current focus:** Milestone v3.0 — Production-ready SDK adoption

## Current Position

Phase: Phase 2 — Auth And Typed Error Runtime
Plan: Plan 2 pending
Status: Phase 2 in progress
Last activity: 2026-07-09 — Query API-key auth implemented across OpenAPI and SDK targets

## Performance Metrics

**Velocity:**

- Total plans completed: 2 (this milestone)
- Average duration: N/A
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 3 | - | - |
| 02 | 4 | - | - |
| 03 | 3 | - | - |
| 04 | 3 | - | - |
| 05 | 3 | - | - |
| 06 | 3 | - | - |

**Recent Trend:**

- Last 5 plans: N/A
- Trend: N/A

*Updated after each plan completion. v1.0 velocity (14 plans, ~10min avg) archived in .planning/milestones/v1.0-*.*
| Phase 01 P01 | 23 | 3 tasks | 8 files |
| Phase 01 P03 | 38 | 3 tasks | 23 files |
| Phase 02 P01 | 18 | 3 tasks | 7 files |
| Phase 02 P02 | 22 | 3 tasks | 13 files |
| Phase 02 P03 | 40 | 3 tasks | 14 files |
| Phase 02 P04 | 35 | 3 tasks | 10 files |
| Phase 03 P01 | 20m | 3 tasks | 4 files |
| Phase 03 P02 | 10m | 2 tasks | 3 files |
| Phase 03 P03 | 35m | 2 tasks | 1 files |
| Phase 04 P01 | 6m | 3 tasks | 10 files |
| Phase 04 P02 | 8min | 2 tasks | 9 files |
| Phase 04 P03 | 14min | 3 tasks | 14 files |
| Phase 05 P01 | 18min | 3 tasks | 4 files |
| Phase 05 P02 | 9min | 2 tasks | 3 files |
| Phase 05 P03 | 11min | 2 tasks | 3 files |
| Phase 06 P01 | 7min | 3 tasks | 4 files |
| Phase 06 P02 | 18min | 3 tasks | 26 files |
| Phase 06 P03 | 9min | 4 tasks | 4 files |

## Accumulated Context

### Decisions

Decisions are logged in .planning/PROJECT.md (Key Decisions) and thoughts/DECISION.md.
Recent decisions affecting current work:

- [v2.0]: Each new language = a sidecar emitting the SAME JSON facts contract (`crates/gnr8-core/src/analyze/facts.rs`, `deny_unknown_fields`) + one new SDK `Target`; the Rust lowering → OpenAPI pipeline is reused, never forked. The router-agnostic IR is the narrow waist.
- [v2.0]: Python sidecar uses stdlib `ast`; resolve types via an owned cross-module symbol table, never importing/executing user code (importing = executing = a security boundary). Static-only; unresolved → diagnostic, never a fallback (rule 3).
- [v2.0]: TypeScript sidecar uses the `typescript` Compiler API in an isolated Node sidecar — the single documented rule-2 carve-out (the language's own reference compiler, zero-dependency, behind the JSON-facts boundary). Bright line: never read `@nestjs/swagger` / `zod` / `class-validator` (rule 1).
- [Phase 06]: [06-03] CORRECTED FRAMING — `typescript` is recorded as a REQUIRED USER TOOLCHAIN (tsextract borrows the user's own via `tsextract/ts.js`, exactly as goextract uses `go` / pyextract uses `python3`), NOT a shipped/bundled/vendored dep. gnr8 ships ZERO OSS, so rule 2 holds LITERALLY (not a carve-out/loosening). CLAUDE.md `### TypeScript toolchain (required, not shipped)` + PROJECT.md reworded to agree; bright line still excludes @nestjs/swagger/zod/class-validator. XLANG-05 "zero OSS" = add no NEW dep (the four debt crates unchanged, NOT retired); WR-02/WR-04 backlogged 999.x; `make check` incl. `examples-check` green end-to-end. Phase 06 + v2.0 ready for verification.
- [v2.0]: Generated SDKs stay dependency-free (`PySdk` = stdlib `urllib` + `@dataclass`; `TsSdk` = built-in `fetch` + typed interfaces), like the v1.0 Go SDK's `net/http`.
- [v1.0]: OpenAPI is an artifact, not the internal model; the graph is the source of truth; deterministic byte-identical output (carried forward, still in force).
- [Phase ?]: [01-01] Neutral Type enum: adjacently-tagged ({type,of}); Prim internally-tagged; Type::Any = empty struct variant for buffered deny_unknown_fields safety. IR re-exports the facts vocabulary (one definition, zero drift).
- [Phase ?]: [01-01] optional and nullable are independent parallel bool flags; all four combinations representable. Type::Ext omitted (no extension runtime this phase).
- [Phase ?]: [01-01] lower/ and gosdk/ left intentionally non-compiling against the new enum (Plan 01-02 compile-error signal; no _ => shims); the Go side is fully green.
- [Phase ?]: 01-03: red-by-design multi-language fixtures gated via #[ignore]; six intended-green graph+OpenAPI snapshots flip green in Phases 2/4 with zero edits
- [Phase ?]: [02-02] pyextract sidecar: static-only (ast.parse of file TEXT, never import/exec the target); an owned cross-module symbol table replaces go/types (rule 2 hand-rolled); unresolvable name -> UNRESOLVABLE sentinel -> diagnostic + omit (rule 3, never a guessed default).
- [Phase ?]: [02-02] Literal[...] alias is inlined-only and never a standalone schema; only a Union alias (referenced by ref_id) becomes a schema. Python int->int64-signed, float->float64. Optional/|None is a FIELD nullable axis. Snapshot is authoritative.
- [Phase ?]: [02-03] FastAPI route recognition: decorator + typed signature is the one source of every route fact; APIRouter(prefix=) recorded separately (never folded, rule 1); a Union alias is a valid named oneOf component. Both FastAPI snapshots green through real extraction, zero snapshot edits.
- [Phase ?]: [02-04] Flask typed-envelope: @bp.route/methods= one route per method; Blueprint(url_prefix=) recorded separately (rule 1); <int:> converter -> int64 path param; status method-derived (typed POST->201, else 200) NEVER from a docstring (Q1); untyped request.json/args/return -> diagnostic + omit (rule 3, no fallback). FastAPI/Flask are parallel deterministic recognizers. Both Flask snapshots green via real extraction, zero edits; diagnostics at 42/69/78. Phase 02 complete.
- [Phase ?]: [03-01] pysdk emit: exhaustive py_type (NO _=>) maps Union->Union[...], inline Enum->Literal[...], named non-object body->type alias (BookOrError=Union[Book,OutOfStock]) — cases the Go target rejects; inline Object stays a typed SdkGen error (parity). @dataclass fields partitioned required-first (3.9-safe; kw_only is 3.10+). Dependency-free urllib Client + injectable OpenerDirector; from __future__ import annotations + fixed typing header. Zero new crates; emitted SDK py_compiles+imports on 3.9.25.
- [Phase 03]: PySdk Target is a verbatim structural clone of GoSdk reusing sdk_package + ir.base_path as the single source of truth; no new CoreError variant, no second name derivation
- [Phase 03]: pysdk templated-path f-string emits safe='' (single quotes) — escaped double quotes are a Python 3.9-3.11 SyntaxError — Caught by the new py_compile gate; string-only unit tests missed it
- [Phase 03]: pysdk named type alias emits a PEP-484 string forward reference (Name = "Union[...]") — Eager module-level alias assignment NameErrored on a later-defined class at import; caught by the import gate
- [Phase ?]: [04-01] Lang::TypeScript first-class: detect_language is a SINGLE count-based classification over {go,python,ts} markers (TS marker = tsconfig.json OR *.ts — *.ts REQUIRED, the nestjs fixture has no tsconfig); 0/>1 -> typed Config error (rule 3, no fallback). Both build_graph AND diagnostics::collect dispatch 3-arm with no _ =>. run_tsextract drives node index.js <target> with discrete args (T-04-01); TypeScriptToolchainMissing typed error. NestJs Source clones FastApi calling the SAME build_graph (lang from target, rule 3/4).
- [Phase ?]: [04-01] tsextract sole dep = typescript pinned EXACT 5.9.3 (rule-2 carve-out; gnr8-core ZERO crates). VENDORING Option A (hermetic): committed tsextract/node_modules/typescript via .gitignore negation + committed package-lock.json (offline tests, no npm ci). index.js stub emits {module,routes,schemas,diagnostics} empty; real Compiler-API extractor lands 04-02/04-03, the 2 nestjs snapshots stay #[ignore] red until then.
- [Phase 04]: [04-02] Open Q1 resolved empirically: the named-vs-inline enum discriminator is aliasSymbol on the FULL type BEFORE stripping null/undefined (NOT the residual). format keeps aliasSymbol -> named ref; sort?: SortOrder|null becomes a synthetic union whose aliasSymbol TS drops -> inline enum. One discriminator, one path (rule 3).
- [Phase 04]: [04-02] tsextract type core: number->float64 (never int); strip undefined/null arms FIRST as the optional/nullable axes (never union members); collapse TS synthetic boolean true|false to a single bool; class->named ref; unresolvable->diagnostic+omit, never an any guess.
- [Phase 04]: [04-02] schema collection: fixpoint Registry follows named refs through fields AND union arms (OutOfStockDto via the BookOrError union arm only); direct-root seeding excludes a class with a class-decorator or methods (the routing controller); a string-literal-union alias is a schema only when referenced (BookFormat via format; SortOrder not).
- [Phase ?]: [04-03] NestJS route recognition: routes.js reads @nestjs/common routing decorators ONLY (rule 1); @Controller prefix recorded as provenance, NEVER folded into op paths; status method-derived (POST->201/else 200) with @HttpCode override (single rule, rule 3); routes seed the transitive schema collection. Both nestjs snapshots green via real extraction, zero edits. Phase 04 complete.
- [Phase ?]: [04-03] named-vs-inline discriminator HARDENED to the syntactic annotation node (bare TypeReference->named ref; UnionType->inline); the 04-02 aliasSymbol-on-full-type rule failed fmt?: BookFormat (TS drops aliasSymbol once |undefined mixed in). One source (the author's annotation), one path, uniform for fields AND optional params.
- [Phase 05]: [05-03] tssdk hermetic typecheck gate (tests/tssdk_compile.rs): generate -> write to a unique stdlib temp dir -> run the VENDORED tsc --noEmit --strict --lib es2022,dom (,dom is load-bearing: lib.dom.d.ts declares fetch) over the generated .ts (exit 0); discrete Command args + current_dir; banned-import grep; skip-if-toolchain-absent; two-run determinism. Wired into the explicit gates --test list.
- [Phase 05]: [05-03] Rule-1 fix the gate caught: ts_type gained an ns namespace-prefix arg (one path, rule 3) — models.ts passes empty (bare), client.ts passes 'models.' so a named-enum/model PARAM resolves through the client's models import (fixed client.ts TS2304 Cannot find name BookFormat).
- [Phase ?]: [06-01] doctor/watch follow the SOURCE language via one pub source_toolchain decision delegating to detect_language (rule 3, no fallback); LifecycleHealth.go_toolchain renamed to source_toolchain + a language field added; pinned --json field set updated in lockstep (T-06-03).
- [Phase 06]: [06-02] Cross-language examples = copied static source + a .gnr8/ Pipeline crate (config is code, rule 4) + REAL committed gnr8 generate output. FastApi uses inputs([.]) not inputs([app]) so the source's absolute app.models imports resolve (.gnr8/ already excluded from detection); make examples-check reuses gnr8 check (exits 1 on drift) as the regen-and-diff (rule 2), wired into make check across Go/Python/TS.

### Pending Todos

None yet.

### Blockers/Concerns

- [v2.0] Carried tech debt from v1.0 (not blocking, retire when touched): `goextract` path baked at compile time (relocatable install); `diagnostics::collect` is a redundant test-only seam.

## Deferred Items

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Source frontends | Hono (TS), typed-Express/Fastify, Rust source | Deferred to v2+ (FUT-01..03) | v2.0 scoping |
| Toolchain | stdlib-pure TypeScript extraction path (retire the `typescript` carve-out) | Deferred (FUT-04) | v2.0 scoping |
| SDK targets | Rust SDK target; SDKs with third-party HTTP deps | Out of scope | v2.0 scoping |
| Extension runtime | Dynamic plugins and macro-heavy APIs | Deferred until repeated pressure | Initialization |

## Session Continuity

Last session: 2026-06-26T05:59:30.054Z
Stopped at: Completed 01-03-PLAN.md (phase 01 complete)
Resume file: None

## Operator Next Steps

- Continue Phase 2 with bearer/basic auth and richer typed error runtime behavior
