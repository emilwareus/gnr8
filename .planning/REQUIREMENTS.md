# Requirements: gnr8 — Milestone v2.0 (Multi-language: TypeScript & Python)

**Defined:** 2026-06-25
**Core Value:** Generate accurate OpenAPI and SDK outputs from real source code quickly, with
code-based customization and minimal duplicated API descriptions.

Milestone goal: code-first **parsing + dependency-free SDK generation** for **Python (FastAPI +
Flask)** and **TypeScript (NestJS)**, proving the `ApiGraph` IR is a true language-neutral narrow
waist. Design brief: `docs/milestone-v2-multi-language.md`. Invariants unchanged (CLAUDE.md): one
source per fact, no OSS deps in `gnr8-core`, no fallback paths, deterministic byte-identical output.

## v1 Requirements

Requirements for this milestone. Each maps to exactly one roadmap phase.

### Language-neutral core (IR)

- [x] **IR-01**: The IR + JSON facts contract express the cross-language type vocabulary (objects, arrays, enums, optional/nullable, unions) without Go-specific assumptions.
- [x] **IR-02**: Every language sidecar emits one shared JSON facts contract the Rust host deserializes strictly (`deny_unknown_fields`); no language terms leak into the IR.
- [ ] **IR-03**: OpenAPI lowering + SDK generation consume the IR unchanged across all supported languages (no per-language branches in lowering).
- [x] **IR-04**: Multi-language fixture services (FastAPI, Flask, NestJS) encode the v2.0 acceptance cases, with red-by-design snapshots in place before extraction lands.

### Python source extraction (PYSRC)

- [x] **PYSRC-01**: A developer can extract routes, path/query params, request bodies (Pydantic/`@dataclass`), response models, and status codes from a **FastAPI** service into the IR.
- [x] **PYSRC-02**: A developer can extract routes (including blueprint/`APIRouter` prefixes) and opt-in typed DTOs/returns from a **Flask** service into the IR.
- [x] **PYSRC-03**: The Python sidecar resolves types **statically** via stdlib `ast` + an owned cross-module symbol table, and never imports/executes the target code.
- [x] **PYSRC-04**: Unresolvable or untyped surfaces (untyped `request.json`, dynamic prefixes, foreign types) produce **diagnostics**, never guessed facts (no fallback).
- [x] **PYSRC-05**: A developer enables Python extraction from `.gnr8/` code via `FastApi` / `Flask` `Source` built-ins.

### Python SDK target (PYSDK)

- [x] **PYSDK-01**: A developer can generate a **dependency-free** Python SDK from the IR (stdlib `urllib`, `@dataclass` models, a typed `ApiError`, an injectable opener).
- [x] **PYSDK-02**: The generated Python SDK imports/type-checks and round-trips against the FastAPI fixture in a hermetic test.
- [x] **PYSDK-03**: A developer adds the Python SDK to a `.gnr8/` Pipeline via a `PySdk` `Target` built-in; output is deterministic (byte-identical across runs).

### TypeScript source extraction (TSSRC)

- [ ] **TSSRC-01**: A developer can extract routes, params, and request/response DTOs from a **NestJS** service (`@nestjs/common` decorators + DTO classes) into the IR.
- [x] **TSSRC-02**: The TS sidecar uses the `typescript` Compiler API and derives every schema fact from the source's own types — never from `@nestjs/swagger`, `zod`, or `class-validator`.
- [x] **TSSRC-03**: Unsupported or untyped surfaces produce **diagnostics**, never guessed facts (no fallback).
- [x] **TSSRC-04**: A developer enables NestJS extraction from `.gnr8/` code via a `NestJs` `Source` built-in.

### TypeScript SDK target (TSSDK)

- [ ] **TSSDK-01**: A developer can generate a **dependency-free** TypeScript SDK from the IR (built-in `fetch`, typed `interface` models + string-literal-union enums, a typed `ApiError`, a configurable `Client`).
- [ ] **TSSDK-02**: The generated TS SDK type-checks (`tsc --noEmit`) in a hermetic test.
- [ ] **TSSDK-03**: A developer adds the TS SDK to a `.gnr8/` Pipeline via a `TsSdk` `Target` built-in; output is deterministic.

### Cross-language hardening & examples (XLANG)

- [ ] **XLANG-01**: A developer drives a **FastAPI** service end-to-end (`gnr8 generate`) → OpenAPI 3.1 + a Python SDK from a `.gnr8/` lifecycle, with real committed example output.
- [ ] **XLANG-02**: A developer drives a **NestJS** service end-to-end → OpenAPI 3.1 + a TS SDK from a `.gnr8/` lifecycle, with real committed example output.
- [ ] **XLANG-03**: `docs/USAGE.md` documents the honest per-language supported envelope (FastAPI full; Flask typed-only; NestJS class DTOs), with limits stated.
- [ ] **XLANG-04**: `gnr8 doctor` / `check` / `watch` work across all supported language sidecars (toolchain detection, drift, loop-safety).
- [ ] **XLANG-05**: Every sidecar is stdlib-only in its language (Python `ast`; TS = `typescript` only); `gnr8-core` takes zero OSS deps; all output is deterministic.

## v2 Requirements

Deferred to a future release; tracked, not in this roadmap.

### Source frontends

- **FUT-01**: Hono (TypeScript) source extraction (chained RPC + inferred response types).
- **FUT-02**: Typed-Express / Fastify-with-typed-schema source extraction, where a typed surface exists.
- **FUT-03**: Rust source frontend + Rust SDK target.

### Toolchain

- **FUT-04**: A stdlib-pure TypeScript extraction path (would retire the `typescript` carve-out, if one ever becomes feasible).

## Out of Scope

Explicitly excluded for v2.0. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Express / Fastify / tRPC / ts-rest source | No typed request/response surface (Express/Fastify) or third-party-schema coupling — `zod` in tRPC/ts-rest is forbidden by rule 1. |
| Reading `@nestjs/swagger`, `zod`, `class-validator`, `marshmallow` | Third-party annotation/validation tools — gnr8 derives facts from the language's own types (rule 1). |
| Consuming FastAPI's runtime `/openapi.json` | Another tool's output (rule 1), and it requires running the app. gnr8 derives from source. |
| Importing/executing the target Python/TS at analysis time | Security boundary — extraction is static only. |
| Rust source frontend; non-(Go/Py/TS) SDK targets | Out of this milestone's scope (see Future). |
| Hand-rolled Rust TS parser | Deferred unless the `typescript` carve-out is reversed (multi-month; see FUT-04). |
| SDKs with third-party HTTP deps (axios / requests / httpx) | Generated SDKs stay dependency-free, like the Go SDK's stdlib `net/http`. |

## Traceability

Each requirement maps to exactly one phase (v2.0 phases restart at 1 — `--reset-phase-numbers`).

| Requirement | Phase | Status |
|-------------|-------|--------|
| IR-01 | Phase 1 | Complete |
| IR-02 | Phase 1 | Complete |
| IR-03 | Phase 1 | Complete |
| IR-04 | Phase 1 | Complete |
| PYSRC-01 | Phase 2 | Complete |
| PYSRC-02 | Phase 2 | Complete |
| PYSRC-03 | Phase 2 | Complete |
| PYSRC-04 | Phase 2 | Complete |
| PYSRC-05 | Phase 2 | Complete |
| PYSDK-01 | Phase 3 | Complete |
| PYSDK-02 | Phase 3 | Complete |
| PYSDK-03 | Phase 3 | Complete |
| TSSRC-01 | Phase 4 | Pending |
| TSSRC-02 | Phase 4 | Complete |
| TSSRC-03 | Phase 4 | Complete |
| TSSRC-04 | Phase 4 | Complete |
| TSSDK-01 | Phase 5 | Pending |
| TSSDK-02 | Phase 5 | Pending |
| TSSDK-03 | Phase 5 | Pending |
| XLANG-01 | Phase 6 | Pending |
| XLANG-02 | Phase 6 | Pending |
| XLANG-03 | Phase 6 | Pending |
| XLANG-04 | Phase 6 | Pending |
| XLANG-05 | Phase 6 | Pending |

**Coverage:**
- v1 (v2.0 milestone) requirements: 24 total
- Mapped to phases: 24 ✓
- Unmapped: 0 ✓

**Per-phase counts:** Phase 1 = 4 (IR), Phase 2 = 5 (PYSRC), Phase 3 = 3 (PYSDK),
Phase 4 = 4 (TSSRC), Phase 5 = 3 (TSSDK), Phase 6 = 5 (XLANG). 4+5+3+4+3+5 = 24.

---
*Requirements defined: 2026-06-25*
*Last updated: 2026-06-25 after roadmap creation (traceability populated)*
