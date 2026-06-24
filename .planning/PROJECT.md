# gnr8

## What This Is

`gnr8` is a Rust-based code generation tool for code-first API projects. The first milestone is a Go-to-Go proof of concept: analyze Go service code, build an internal API graph, emit OpenAPI, and generate a usable Go SDK.

The product is for developers who are frustrated by fragmented Swagger/OpenAPI and SDK toolchains that are slow, annotation-heavy, hard to customize, and poor at save-time incremental workflows.

## Core Value

Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Build a narrow Go source to Go SDK proof of concept before adding other languages.
- [ ] Own the native extraction, graph, OpenAPI lowering, and SDK generation pipeline instead of wrapping existing generators.
- [ ] Infer API facts primarily from Go code structure and types, using comments only as escape hatches.
- [ ] Keep OpenAPI as an output artifact rather than the internal model.
- [ ] Provide a `.gnr8/` project-local workspace where code is configuration.
- [ ] Keep the first implementation simple: no dynamic plugin runtime, macro-heavy API, graph database, or multi-language implementation.
- [ ] Validate the PoC against realistic Go fixtures, not only toy examples.
- [ ] Follow Rust implementation guardrails from the vendored `rust-best-practices` skill.

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
| Own the native extraction and generation pipeline | The product promise is not to wrap fragmented infrastructure. | — Pending |
| Code is configuration | YAML/TOML/JSON is not expressive enough for framework-specific generation and SDK customization. | — Pending |
| Use comments only as escape hatches | Comment-driven API descriptions drift from source behavior. | — Pending |
| `.gnr8/` is the likely project workspace | Mirrors the desired project-local code and lifecycle model. | — Pending |
| OpenAPI is an artifact, not the internal model | SDK generation and diagnostics need source-level facts OpenAPI cannot preserve cleanly. | — Pending |
| Simpler is better until proven otherwise | Avoid overengineering before the product loop works. | ✓ Good |
| Design for more source and target languages, but do not start multi-language | The core should not bake in Go-only assumptions, but Go must prove the model first. | — Pending |
| Use Rust best-practice guardrails | Keeps the future implementation maintainable and measurable. | — Pending |

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
*Last updated: 2026-06-24 after initialization*
