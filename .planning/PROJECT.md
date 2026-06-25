# gnr8

## What This Is

`gnr8` is a Rust-based code generation tool for code-first API projects. The first milestone is a Go-to-Go proof of concept: analyze Go service code, build an internal API graph, emit OpenAPI, and generate a usable Go SDK.

The product is for developers who are frustrated by fragmented Swagger/OpenAPI and SDK toolchains that are slow, annotation-heavy, hard to customize, and poor at save-time incremental workflows.

## Core Value

Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.

## Current Milestone: v2.0 Multi-language — TypeScript & Python (parse + generate)

**Goal:** Code-first parsing **and** dependency-free SDK generation for **Python (FastAPI + Flask)** and
**TypeScript (NestJS)**, proving the `ApiGraph` IR is a true language-neutral narrow waist — not just
router-agnostic. New sources/targets ship as `.gnr8/` code-as-config built-ins. Every v1 invariant holds.

**Target features:**
- A language-neutral IR + JSON facts contract that lowers identically across Go / FastAPI / Flask / NestJS.
- Python source: a stdlib-`ast` `pyextract` sidecar (FastAPI full; Flask typed-envelope), static-only.
- Python SDK target: dependency-free (`urllib` + `@dataclass` + typed error).
- TypeScript source: a `tsextract` sidecar on the `typescript` Compiler API (NestJS decorators + DTO classes).
- TypeScript SDK target: dependency-free (built-in `fetch` + typed interfaces + typed error).
- FastAPI + NestJS examples with `.gnr8/` lifecycles and real committed output.

Design brief: `docs/milestone-v2-multi-language.md`.

## Requirements

### Validated

- ✓ Build a narrow Go source to Go SDK proof of concept before adding other languages. — v1.0
- ✓ Own the native extraction, graph, OpenAPI lowering, and SDK generation pipeline instead of wrapping existing generators. — v1.0 (goextract → ApiGraph → lower/sdk, no wrapped generators)
- ✓ Infer API facts from Go code structure and types; what the typed source can't express comes from `.gnr8/` config, never comments. — v1.0 (go/types code-first; no annotation parsing — config is the only escape hatch)
- ✓ Keep OpenAPI as an output artifact rather than the internal model. — v1.0 (graph is source of truth; OpenAPI 3.1 serialized from it)
- ✓ Provide a `.gnr8/` project-local workspace where code is configuration. — v1.0 (the `.gnr8/` Rust crate IS the config: a `Pipeline` of `Source`/`Transform`/`Target`/`PostProcess`; no data-file config)
- ✓ Keep the first implementation simple: no dynamic plugin runtime, macro-heavy API, graph database, or multi-language implementation. — v1.0
- ✓ Validate the PoC against realistic Go fixtures, not only toy examples. — v1.0 (Gin goalservice fixture derived from a real production service shape)
- ✓ Follow Rust implementation guardrails from the vendored `rust-best-practices` skill. — v1.0 (thiserror/anyhow boundaries, no prod unwrap, clippy -D warnings)

### Active

**v2.0 — Multi-language: TypeScript & Python (parse + generate).** Full, scoped requirements in
`.planning/REQUIREMENTS.md`.

- [ ] Language-neutral IR + facts contract proven across Go / Python / TypeScript (the narrow waist)
- [ ] Python source extraction — FastAPI (full) + Flask (typed envelope), static stdlib-`ast` sidecar
- [ ] Python SDK target — dependency-free (`urllib` + `@dataclass` + typed error)
- [ ] TypeScript source extraction — NestJS, `typescript` Compiler API sidecar
- [ ] TypeScript SDK target — dependency-free (built-in `fetch` + typed interfaces)
- [ ] FastAPI + NestJS examples with `.gnr8/` lifecycles and real committed output

## Current State

**Shipped v1.0** (2026-06-24): a working Go → OpenAPI 3.1 → compiling Go SDK pipeline.
~10.2K LOC Rust (`gnr8-core` lib + `gnr8` CLI) + ~3.5K LOC Go (`goextract` sidecar + fixtures), 14 plans / 38 requirements across 5 phases.

- **Pipeline:** `goextract` (go/packages + go/types) → router-agnostic `ApiGraph` (stable IDs, deterministic) → `lower::to_openapi` + `sdk::generate` → `.gnr8/` lifecycle (blake3 ownership manifest, no-op detection, loop-safe watch) → `gnr8 doctor`.
- **CLI:** `init`, `generate` (`--force`), `check`, `inspect routes|schemas|graph`, `watch`, `doctor` — all wired to the real pipeline; human tables + `--json`.
- **Quality:** `make check` green (fmt, clippy `-D warnings`, all tests, 4 contract snapshots, hermetic SDK `go build` + httptest smoke, determinism, lifecycle/watch). Demo (`docs/demo.md`) and evidence (`docs/evidence.md`) reproducible.
- **Known tech debt (v1.1+):** `goextract` path baked at compile time (relocatable install); `diagnostics::collect` is a redundant test-only seam.

## Next Milestone Goals

Candidate directions (not yet scoped): additional source frontends (TS/Python/Rust) + SDK targets; packaged/relocatable `goextract`; richer programmatic customization; deeper incremental graph invalidation if benchmarks justify it.

### Out of Scope

- Multi-language source support in the first milestone — Go must prove the model first.
- Multi-language SDK targets in the first milestone — Go SDK quality comes first.
- Dynamic plugin loading — defer until repeated extension pressure proves the need.
- Macro-heavy configuration APIs — plain Rust code should come first.
- Full Go framework coverage — support one or two route styles for the PoC.
- Full OpenAPI 3.2 feature coverage — emit a useful modern OpenAPI artifact first.
- Arbitrary handler body interpretation — start with typed handlers and simple patterns.
- Wrapping Swaggo, oapi-codegen, OpenAPI Generator, or similar tools as the core engine — existing tools are comparison targets only.

## Context

The current repository is intentionally discovery-first. Product thinking, research, architecture, roadmap, decisions, features, and a vendored Rust best-practices skill live under `thoughts/`.

Key source documents:

- `thoughts/ARCHITECTURE.md` — target architecture and testing strategy.
- `thoughts/ROADMAP.md` — rough Go-to-Go PoC roadmap.
- `thoughts/DECISION.md` — accepted and proposed product decisions.
- `thoughts/FEATURE.md` — feature ledger.
- `thoughts/research/` — research notes for native Go extraction, SDK structure, code-as-config UX, lifecycle, incrementality, OpenAPI, and multi-language direction.
- `thoughts/skills/rust-best-practices/` — vendored implementation guidance.

The core product bet is that a small Rust engine can own orchestration, graph management, OpenAPI lowering, SDK generation, diagnostics, and watch-mode lifecycle while using official language tooling where it provides semantic truth.

## Constraints

- **Implementation language**: Rust — chosen for CLI performance, long-running watch mode, typed internal models, and generator reliability.
- **First vertical slice**: Go source to OpenAPI to Go SDK — prevents premature platform design.
- **Configuration model**: code-as-config under `.gnr8/` — YAML/TOML/JSON must not be the main customization surface.
- **Source of truth**: internal API graph — OpenAPI is generated from the graph, not used as the core data model.
- **Extraction philosophy**: code-first, not comment-first — comments are only escape hatches.
- **Testing**: realistic fixtures drive implementation — fixture tests must cover route extraction, schema extraction, OpenAPI output, SDK output, diagnostics, and CLI behavior.
- **Quality gate**: Rust best practices — typed library errors, `anyhow` only at binary boundaries, no production `unwrap`/`expect`, clippy with warnings denied, and benchmark-before-optimizing.
- **Scope control**: simpler is better — no dynamic plugins, graph database, macro-heavy configuration API, or multi-language runtime machinery until the PoC proves real pressure.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Stay in research before implementation | Premature scaffolding would freeze weak assumptions. | ✓ Good |
| Own the native extraction and generation pipeline | The product promise is not to wrap fragmented infrastructure. | ✓ Good |
| Code is configuration | YAML/TOML/JSON is not expressive enough for framework-specific generation and SDK customization. | ✓ Good (static knobs shipped; programmatic = v2) |
| Use comments only as escape hatches | Comment-driven API descriptions drift from source behavior. | ✓ Good |
| `.gnr8/` is the likely project workspace | Mirrors the desired project-local code and lifecycle model. | ✓ Good |
| OpenAPI is an artifact, not the internal model | SDK generation and diagnostics need source-level facts OpenAPI cannot preserve cleanly. | ✓ Good |
| Simpler is better until proven otherwise | Avoid overengineering before the product loop works. | ✓ Good |
| Design for more source and target languages, but do not start multi-language | The core should not bake in Go-only assumptions, but Go must prove the model first. | ✓ Good (v1.0 proved the model; v2.0 now adds TS + Python) |
| v2.0: add TypeScript (NestJS) + Python (FastAPI/Flask) — parse + generate | The router-agnostic IR is the narrow waist; each new language is a sidecar emitting the same JSON facts + one new SDK `Target`. The whole Rust lowering/OpenAPI pipeline is reused. | — Pending |
| Python sidecar uses stdlib `ast`; resolve types via an owned cross-module symbol table, never importing user code | `ast` is Python's stdlib (the `go/types` analog); importing user code = executing it (a security boundary). Static-only; unresolved types → diagnostic, never a fallback (rule 3). | — Pending |
| TypeScript sidecar uses the `typescript` Compiler API in an isolated Node sidecar | TS has no stdlib type-checker; `typescript` is the language's own reference compiler (the `go/types` analog), zero-dependency, run behind the JSON facts boundary; generated SDKs stay dependency-free. Documented carve-out to rule 2. | ⚠️ Revisit (the single OSS-in-toolchain exception — revisit if a stdlib-pure TS path ever appears) |
| Use Rust best-practice guardrails | Keeps the future implementation maintainable and measurable. | ✓ Good |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `$gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `$gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-06-25 after starting milestone v2.0*
