---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: "Multi-language: TypeScript & Python (parse + generate)"
status: planning
last_updated: "2026-06-25T15:00:00.000Z"
last_activity: 2026-06-25
progress:
  total_phases: 6
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-06-25)

**Core value:** Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.
**Current focus:** Phase 1 — Language-Neutral IR + Facts Contract + Fixtures

## Current Position

Phase: 1 of 6 (Language-Neutral IR + Facts Contract + Fixtures)
Plan: — (roadmap created; phase not yet planned)
Status: Ready to plan
Last activity: 2026-06-25 — v2.0 roadmap created (6 phases, 24/24 requirements mapped); phase numbers reset to start at 1

Progress: [░░░░░░░░░░] 0%

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

## Accumulated Context

### Decisions

Decisions are logged in .planning/PROJECT.md (Key Decisions) and thoughts/DECISION.md.
Recent decisions affecting current work:

- [v2.0]: Each new language = a sidecar emitting the SAME JSON facts contract (`crates/gnr8-core/src/analyze/facts.rs`, `deny_unknown_fields`) + one new SDK `Target`; the Rust lowering → OpenAPI pipeline is reused, never forked. The router-agnostic IR is the narrow waist.
- [v2.0]: Python sidecar uses stdlib `ast`; resolve types via an owned cross-module symbol table, never importing/executing user code (importing = executing = a security boundary). Static-only; unresolved → diagnostic, never a fallback (rule 3).
- [v2.0]: TypeScript sidecar uses the `typescript` Compiler API in an isolated Node sidecar — the single documented rule-2 carve-out (the language's own reference compiler, zero-dependency, behind the JSON-facts boundary). Bright line: never read `@nestjs/swagger` / `zod` / `class-validator` (rule 1).
- [v2.0]: Generated SDKs stay dependency-free (`PySdk` = stdlib `urllib` + `@dataclass`; `TsSdk` = built-in `fetch` + typed interfaces), like the v1.0 Go SDK's `net/http`.
- [v1.0]: OpenAPI is an artifact, not the internal model; the graph is the source of truth; deterministic byte-identical output (carried forward, still in force).

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

Last session: 2026-06-25 15:00
Stopped at: Created v2.0 ROADMAP.md (6 phases) + populated REQUIREMENTS.md traceability (24/24)
Resume file: None

## Operator Next Steps

- Plan the first phase with `/gsd:plan-phase 1` (Language-Neutral IR + Facts Contract + Fixtures).
