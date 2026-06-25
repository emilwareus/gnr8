---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: "Multi-language: TypeScript & Python (parse + generate)"
status: executing
stopped_at: Completed 01-03-PLAN.md (phase 01 complete)
last_updated: "2026-06-25T15:47:50.567Z"
last_activity: 2026-06-25 -- Phase 01 execution started
progress:
  total_phases: 6
  completed_phases: 1
  total_plans: 3
  completed_plans: 3
  percent: 17
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-06-25)

**Core value:** Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.
**Current focus:** Phase 01 — Language-Neutral IR + Facts Contract + Fixtures

## Current Position

Phase: 01 (Language-Neutral IR + Facts Contract + Fixtures) — EXECUTING
Plan: 3 of 3
Status: Executing Phase 01
Last activity: 2026-06-25 -- Phase 01 execution started

Progress: [██████████] 100%

## Performance Metrics

**Velocity:**

- Total plans completed: 0 (this milestone)
- Average duration: N/A
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**

- Last 5 plans: N/A
- Trend: N/A

*Updated after each plan completion. v1.0 velocity (14 plans, ~10min avg) archived in .planning/milestones/v1.0-*.*
| Phase 01 P01 | 23 | 3 tasks | 8 files |
| Phase 01 P03 | 38 | 3 tasks | 23 files |

## Accumulated Context

### Decisions

Decisions are logged in .planning/PROJECT.md (Key Decisions) and thoughts/DECISION.md.
Recent decisions affecting current work:

- [v2.0]: Each new language = a sidecar emitting the SAME JSON facts contract (`crates/gnr8-core/src/analyze/facts.rs`, `deny_unknown_fields`) + one new SDK `Target`; the Rust lowering → OpenAPI pipeline is reused, never forked. The router-agnostic IR is the narrow waist.
- [v2.0]: Python sidecar uses stdlib `ast`; resolve types via an owned cross-module symbol table, never importing/executing user code (importing = executing = a security boundary). Static-only; unresolved → diagnostic, never a fallback (rule 3).
- [v2.0]: TypeScript sidecar uses the `typescript` Compiler API in an isolated Node sidecar — the single documented rule-2 carve-out (the language's own reference compiler, zero-dependency, behind the JSON-facts boundary). Bright line: never read `@nestjs/swagger` / `zod` / `class-validator` (rule 1).
- [v2.0]: Generated SDKs stay dependency-free (`PySdk` = stdlib `urllib` + `@dataclass`; `TsSdk` = built-in `fetch` + typed interfaces), like the v1.0 Go SDK's `net/http`.
- [v1.0]: OpenAPI is an artifact, not the internal model; the graph is the source of truth; deterministic byte-identical output (carried forward, still in force).
- [Phase ?]: [01-01] Neutral Type enum: adjacently-tagged ({type,of}); Prim internally-tagged; Type::Any = empty struct variant for buffered deny_unknown_fields safety. IR re-exports the facts vocabulary (one definition, zero drift).
- [Phase ?]: [01-01] optional and nullable are independent parallel bool flags; all four combinations representable. Type::Ext omitted (no extension runtime this phase).
- [Phase ?]: [01-01] lower/ and gosdk/ left intentionally non-compiling against the new enum (Plan 01-02 compile-error signal; no _ => shims); the Go side is fully green.
- [Phase ?]: 01-03: red-by-design multi-language fixtures gated via #[ignore]; six intended-green graph+OpenAPI snapshots flip green in Phases 2/4 with zero edits

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

Last session: 2026-06-25T15:47:50.557Z
Stopped at: Completed 01-03-PLAN.md (phase 01 complete)
Resume file: None

## Operator Next Steps

- Plan the first phase with `/gsd:plan-phase 1` (Language-Neutral IR + Facts Contract + Fixtures).
