# gnr8

## What This Is

`gnr8` is a Rust-based code generation tool for code-first API projects. It analyzes real server source,
builds an internal API graph, emits OpenAPI 3.1, and generates installable SDKs without making OpenAPI
the internal source of truth.

The product is for developers who are frustrated by fragmented Swagger/OpenAPI and SDK toolchains that are slow, annotation-heavy, hard to customize, and poor at save-time incremental workflows.

## Core Value

Generate accurate OpenAPI and SDK outputs from real source code quickly, with code-based customization and minimal duplicated API descriptions.

## Shipped Milestone: v2.0 Multi-language — TypeScript & Python (parse + generate) ✅ 2026-06-26

**Goal (achieved):** Code-first parsing **and** dependency-free SDK generation for **Python (FastAPI + Flask)** and
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

## Shipped Milestone: v3.0 Production-ready SDK adoption ✅ 2026-07-09

**Goal:** Make `gnr8` a production-ready SDK publishing pipeline where server source drives OpenAPI 3.1
and installable, operationally credible SDKs.

**Target features:**
- A shared SDK semantic model/runtime contract that prevents Go/Python/TypeScript emitter drift.
- Graph-driven auth, typed error parity, stable operation naming, and resource grouping.
- `doctor` SDK readiness checks and package metadata/local package validation.
- SDK runtime ergonomics: pagination helpers, conservative retries/timeouts/idempotency, and transport hooks.
- Public API product metadata: operation summaries/descriptions/examples/error responses and common content types.

Design brief: `thoughts/research/adoption-support.md`.

## Requirements

### Validated Through v2.0

- ✓ Build a narrow Go source to Go SDK proof of concept before adding other languages. — v1.0
- ✓ Own the native extraction, graph, OpenAPI lowering, and SDK generation pipeline instead of wrapping existing generators. — v1.0 (goextract → ApiGraph → lower/sdk, no wrapped generators)
- ✓ Infer API facts from Go code structure and types; what the typed source can't express comes from `.gnr8/` config, never comments. — v1.0 (go/types code-first; no annotation parsing — config is the only escape hatch)
- ✓ Keep OpenAPI as an output artifact rather than the internal model. — v1.0 (graph is source of truth; OpenAPI 3.1 serialized from it)
- ✓ Provide a `.gnr8/` project-local workspace where code is configuration. — v1.0 (the `.gnr8/` Rust crate IS the config: a `Pipeline` of `Source`/`Transform`/`Target`/`PostProcess`; no data-file config)
- ✓ Keep the first implementation simple: no dynamic plugin runtime, macro-heavy API, graph database, or multi-language implementation. — v1.0
- ✓ Validate the PoC against realistic Go fixtures, not only toy examples. — v1.0 (Gin goalservice fixture derived from a real production service shape)
- ✓ Follow Rust implementation guardrails from the vendored `rust-best-practices` skill. — v1.0 (thiserror/anyhow boundaries, no prod unwrap, clippy -D warnings)
- ✓ Language-neutral IR + facts contract proven across Go / Python / TypeScript (the narrow waist). — v2.0 (closed neutral `Type` enum; same `build_graph`→`lower`→SDK with no per-language branch)
- ✓ Python source extraction — FastAPI (full) + Flask (typed envelope), static stdlib-`ast` sidecar. — v2.0 (`pyextract`; owned cross-module symbol table; never imports/executes target)
- ✓ Python SDK target — dependency-free (`urllib` + `@dataclass` + typed error). — v2.0 (`PySdk`; hermetic generate+run against the FastAPI fixture)
- ✓ TypeScript source extraction — NestJS, `typescript` Compiler API sidecar. — v2.0 (`tsextract`; bright-line excludes @nestjs/swagger/zod/class-validator; static-only)
- ✓ TypeScript SDK target — dependency-free (built-in `fetch` + typed interfaces). — v2.0 (`TsSdk`; hermetic `tsc --noEmit` typecheck)
- ✓ FastAPI + NestJS examples with `.gnr8/` lifecycles and real committed output. — v2.0 (`examples/{fastapi,nestjs}-bookstore`; `make examples-check` cross-language determinism gate)

### Validated In v3.0

- ✓ Deliver a shared SDK planning layer so package, service, operation, schema, auth, errors, runtime policy,
  docs metadata, and file plans are derived once before target-language rendering. — v3.0
- ✓ Make auth graph-driven across OpenAPI and every generated SDK target. — v3.0
- ✓ Make typed non-2xx error handling consistent across generated SDK targets and OpenAPI artifact input. — v3.0
- ✓ Stabilize operation IDs, SDK names, and resource grouping so SDK public surfaces do not drift accidentally. — v3.0
- ✓ Expose generated SDK readiness through `gnr8 doctor` for user projects. — v3.0
- ✓ Generate installable package metadata and local package validation for supported SDK targets. — v3.0
- ✓ Add explicit pagination policies and SDK helpers for common list/search operations. — v3.0
- ✓ Add conservative SDK runtime policies for timeouts, retries, idempotency, and transport hooks. — v3.0
- ✓ Support operation metadata, examples, documented error responses, and common content types in OpenAPI and SDK docs. — v3.0

## Current State

**Shipped v3.0** (2026-07-09): production-ready SDK adoption across the existing Go, Python, and
TypeScript target languages. The shared SDK model now carries docs metadata, auth, errors, runtime
policy, pagination, package metadata, and file-plan facts into all emitters; SDKs support graph-driven
auth, typed errors, stable grouped surfaces, readiness checks, local package metadata, publishing recipes,
pagination helpers, conservative retry/timeout/idempotency policies, hooks, operation docs/examples, and
common request media types.

- **SDK semantics:** one shared SDK planning layer feeds Go/Python/TypeScript renderers and keeps naming,
  grouping, docs metadata, auth, error, runtime, pagination, and package facts deterministic.
- **Runtime behavior:** generated SDKs expose configured auth, typed API errors, client/per-request
  timeouts and retries, idempotency-key preservation, and request/response/error hooks.
- **Adoption readiness:** `gnr8 doctor --json` reports OpenAPI and SDK readiness; generated packages include
  Go module, Python project, TypeScript package metadata, local validation hooks, and publishing recipes.
- **API surface coverage:** operation documentation transforms propagate summaries, descriptions, tags,
  deprecation, examples, response docs, and documented JSON errors into OpenAPI and SDK docs; SDK request
  bodies cover JSON, text, form-urlencoded, multipart, and binary uploads.
- **Quality:** full `cargo test -p gnr8`, `cargo clippy -p gnr8 --all-targets -- -D warnings`,
  `cargo clippy -p gnr8-cli --all-targets -- -D warnings`, `cargo fmt --all --check`, and `git diff --check`
  passed on 2026-07-09.

**Shipped v2.0** (2026-06-26): the `ApiGraph` IR proven a true language-neutral narrow waist —
code-first parsing **and** dependency-free SDK generation for **Go (Gin)**, **Python (FastAPI + Flask)**,
and **TypeScript (NestJS)**. 6 phases / 19 plans / 43 tasks.

- **Pipeline (4 language paths):** a per-language static sidecar — `goextract` (`go/types`), `pyextract`
  (stdlib `ast` + owned cross-module symbol table), `tsextract` (the user's own `typescript` Compiler API)
  — each emits the SAME neutral JSON facts → `build_graph` (single deterministic `detect_language` dispatch)
  → reused `lower::to_openapi` (OpenAPI 3.1, no per-language branch) + SDK targets (`GoSdk`/`PySdk`/`TsSdk`,
  all dependency-free) → `.gnr8/` lifecycle (blake3 ownership, loop-safe watch) → `doctor`/`check`/`watch`
  generalized to the source language.
- **`.gnr8/` built-ins:** Sources `GoGin`/`FastApi`/`Flask`/`NestJs`; Targets `OpenApi31`/`GoSdk`/`PySdk`/`TsSdk`.
- **Examples:** `examples/{bookstore(Go),fastapi-bookstore,nestjs-bookstore}` — real committed output,
  byte-identical regen proven by the `make examples-check` cross-language determinism gate.
- **Invariants held:** gnr8 owns the source-to-SDK pipeline and uses focused dependencies for commodity
  concerns; generated SDKs remain standard-library-only. `typescript` is a **required user toolchain**
  (resolved from the target project, never shipped). One source of truth per fact and deterministic,
  byte-identical output remain requirements.
- **Quality:** `make check` green (fmt, clippy `-D warnings`, all Rust tests, Python `unittest`, TS `tsc`
  typecheck, 6 multi-language acceptance snapshots, hermetic SDK round-trips, cross-language determinism).
- **Known tech debt (carried/deferred):** gnr8-core OSS known-debt (serde/serde_json/blake3/thiserror —
  documented in CLAUDE.md, deferred by design); `goextract` `golang.org/x/tools` + compile-time path;
  ROADMAP backlog 999.1/999.2 (TsSdk WR-02/WR-04 SDK hardening).

**Shipped v1.0** (2026-06-24): the Go → OpenAPI 3.1 → compiling Go SDK PoC (~10.2K LOC Rust + ~3.5K LOC Go,
14 plans / 5 phases). Full detail: `.planning/milestones/v1.0-*`.

## Next Milestone Goals

The next milestone has not been selected. v3.0 intentionally did not add server stubs, older OpenAPI
output profiles, generic template overrides, registry publishing automation, docs-site generation,
Terraform/CLI/React helpers, or vendor-extension parity.

### Out of Scope

- Server stub generation.
- Older OpenAPI output profiles and broad OpenAPI version compatibility.
- Generic template override/custom generator authoring.
- Registry publishing automation and credential management.
- Docs-site generation.
- Terraform, CLI, React hooks, and broad webhook helper generation.
- Vendor-extension parity with OpenAPI Generator, Speakeasy, Stainless, Fern, or Kiota.
- OAuth/OIDC token acquisition flows, mTLS, AWS SigV4, HMAC signing, CSRF/session-cookie workflows.
- XML, protobuf, arbitrary custom media types, and every OpenAPI encoding edge case.

## Context

The current repository is intentionally discovery-first. Product thinking, research, architecture, roadmap, decisions, features, and a vendored Rust best-practices skill live under `thoughts/`.

Key source documents:

- `thoughts/ARCHITECTURE.md` — target architecture and testing strategy.
- `thoughts/ROADMAP.md` — rough Go-to-Go PoC roadmap.
- `thoughts/DECISION.md` — accepted and proposed product decisions.
- `thoughts/FEATURE.md` — feature ledger.
- `thoughts/research/` — research notes for native Go extraction, SDK structure, code-as-config UX, lifecycle, incrementality, OpenAPI, and multi-language direction.
- `thoughts/research/adoption-support.md` — validated v3.0 adoption support research and scope.
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
| v2.0: add TypeScript (NestJS) + Python (FastAPI/Flask) — parse + generate | The router-agnostic IR is the narrow waist; each new language is a sidecar emitting the same JSON facts + one new SDK `Target`. The whole Rust lowering/OpenAPI pipeline is reused. | ✓ Good — v2.0 (4 language paths green; one neutral facts contract, reused lowering, no per-language branch) |
| Python sidecar uses stdlib `ast`; resolve types via an owned cross-module symbol table, never importing user code | `ast` is Python's stdlib (the `go/types` analog); importing user code = executing it (a security boundary). Static-only; unresolved types → diagnostic, never a fallback (rule 3). | ✓ Good — v2.0 (`pyextract` static-only; owned symbol table; unresolved→diagnostic, no fallback) |
| TypeScript sidecar uses the `typescript` Compiler API in an isolated Node sidecar | TS has no stdlib type-checker; `typescript` is the language's own reference compiler (the `go/types` analog). It is a **required user toolchain, not a bundled dependency**: `tsextract` resolves the user's own `typescript` from the target project (`tsextract/ts.js`) — exactly as `goextract` uses the user's `go` and `pyextract` uses `python3`. gnr8 itself uses focused commodity dependencies while owning the API pipeline end to end; generated SDKs stay standard-library-only. | ✓ Good (required user toolchain; generated SDK remains dependency-free) |
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
*Last updated: 2026-07-09 after completing milestone v3.0*
