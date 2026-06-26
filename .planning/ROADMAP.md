# Roadmap: gnr8

## Milestones

- ✅ **v1.0 — PoC: Go → OpenAPI → Go SDK** — Phases 1-5 (shipped 2026-06-24)
- ✅ **v2.0 — Multi-language: TypeScript & Python (parse + generate)** — Phases 1-6 (shipped 2026-06-26)

> **Phase numbering note:** each milestone's phases are numbered from 1. The v1.0 phases are archived
> under `.planning/milestones/v1.0-*`; the v2.0 phases under `.planning/milestones/v2.0-*`.

## Phases

<details>
<summary>✅ v1.0 PoC: Go → OpenAPI → Go SDK (Phases 1-5) — SHIPPED 2026-06-24</summary>

Full detail: `.planning/milestones/v1.0-ROADMAP.md`

- [x] Phase 1: Foundation And Fixtures (3/3 plans) — completed 2026-06-24
- [x] Phase 2: Go Analysis And API Graph (3/3 plans) — completed 2026-06-24
- [x] Phase 3: OpenAPI And Go SDK Generation (3/3 plans) — completed 2026-06-24
- [x] Phase 4: `.gnr8` Lifecycle And Watch Mode (3/3 plans) — completed 2026-06-24
- [x] Phase 5: PoC Hardening And Demo (2/2 plans) — completed 2026-06-24

</details>

<details>
<summary>✅ v2.0 Multi-language: TypeScript & Python (Phases 1-6) — SHIPPED 2026-06-26</summary>

Full detail: `.planning/milestones/v2.0-ROADMAP.md` · Audit: `.planning/milestones/v2.0-MILESTONE-AUDIT.md`

- [x] Phase 1: Language-Neutral IR + Facts Contract + Fixtures (3/3 plans) — completed 2026-06-25
- [x] Phase 2: Python Source — `pyextract` (4/4 plans) — completed 2026-06-25
- [x] Phase 3: Python Target — `PySdk` (3/3 plans) — completed 2026-06-25
- [x] Phase 4: TypeScript Source — `tsextract` (3/3 plans) — completed 2026-06-25
- [x] Phase 5: TypeScript Target — `TsSdk` (3/3 plans) — completed 2026-06-26
- [x] Phase 6: Cross-Language Hardening + Examples + Docs (3/3 plans) — completed 2026-06-26

</details>

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-5 (Go PoC) | v1.0 | 14/14 | Complete | 2026-06-24 |
| 1. Language-Neutral IR + Facts Contract + Fixtures | v2.0 | 3/3 | Complete | 2026-06-25 |
| 2. Python Source — `pyextract` | v2.0 | 4/4 | Complete | 2026-06-25 |
| 3. Python Target — `PySdk` | v2.0 | 3/3 | Complete | 2026-06-25 |
| 4. TypeScript Source — `tsextract` | v2.0 | 3/3 | Complete | 2026-06-25 |
| 5. TypeScript Target — `TsSdk` | v2.0 | 3/3 | Complete | 2026-06-26 |
| 6. Cross-Language Hardening + Examples + Docs | v2.0 | 3/3 | Complete | 2026-06-26 |

## Backlog (deferred)

Deferred hardening items, recorded so they are tracked without disturbing the green snapshots. These are
**not** folded into the current acceptance output: folding them changes byte-committed TsSdk snapshots +
the NestJS example output, which the Phase-5 REVIEW-FIX deliberately skipped for that reason.

- **Backlog 999.1 — WR-02: TsSdk non-scalar query param `String()` wire-encoding rule** (deferred
  Phase-5 finding; folding it changes the green TsSdk snapshots).
- **Backlog 999.2 — WR-04: TsSdk asymmetric success/error JSON decode** (same reason).
