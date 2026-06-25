# Roadmap: gnr8

## Milestones

- ✅ **v1.0 — PoC: Go → OpenAPI → Go SDK** — Phases 1-5 (shipped 2026-06-24)
- 🚧 **v2.0 — Multi-language: TypeScript & Python (parse + generate)** — Phases 1-6 (in progress)

> **Phase numbering note:** v2.0 was initialized with `--reset-phase-numbers`, so this milestone's
> phases restart at **Phase 1** (directories `01-*` … `06-*` under `.planning/phases/`). The completed
> v1.0 phases (also 1-5) are archived under `.planning/milestones/v1.0-*` and are unaffected.

## Phases

**Phase Numbering:**

- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

<details>
<summary>✅ v1.0 PoC: Go → OpenAPI → Go SDK (Phases 1-5) — SHIPPED 2026-06-24</summary>

- [x] Phase 1: Foundation And Fixtures (3/3 plans) — completed 2026-06-24
- [x] Phase 2: Go Analysis And API Graph (3/3 plans) — completed 2026-06-24
- [x] Phase 3: OpenAPI And Go SDK Generation (3/3 plans) — completed 2026-06-24
- [x] Phase 4: `.gnr8` Lifecycle And Watch Mode (3/3 plans) — completed 2026-06-24
- [x] Phase 5: PoC Hardening And Demo (2/2 plans) — completed 2026-06-24

Full detail archived in `.planning/milestones/v1.0-ROADMAP.md`.
Requirements: 38/38 satisfied (`.planning/milestones/v1.0-REQUIREMENTS.md`).

</details>

### 🚧 v2.0 Multi-language: TypeScript & Python (In Progress)

**Milestone Goal:** Code-first parsing **and** dependency-free SDK generation for **Python (FastAPI +
Flask)** and **TypeScript (NestJS)**, proving the `ApiGraph` IR is a true **language-neutral narrow
waist** — not just router-agnostic. Each new language ships as a stdlib-only sidecar emitting the
**same JSON facts contract** plus one new SDK `Target`; the Rust lowering → OpenAPI pipeline is reused,
never forked. Every v1 invariant holds (one source per fact, zero OSS deps in `gnr8-core`, no fallback
paths, static-only extraction, deterministic byte-identical output).

- [x] **Phase 1: Language-Neutral IR + Facts Contract + Fixtures** - Generalize the IR/`Type` model and the shared JSON facts contract to be type-system-neutral; stand up FastAPI/Flask/NestJS fixtures with red-by-design snapshots. No extraction yet. (completed 2026-06-25)
- [x] **Phase 2: Python Source — `pyextract`** - Static stdlib-`ast` sidecar (FastAPI full, Flask typed-envelope) with an owned cross-module symbol table; `FastApi`/`Flask` Source built-ins; reuse the Rust lowering → OpenAPI. (completed 2026-06-25)
- [ ] **Phase 3: Python Target — `PySdk`** - Dependency-free `urllib` + `@dataclass` SDK with a typed `ApiError`; hermetic generate-and-run test against the FastAPI fixture; `PySdk` Target built-in.
- [ ] **Phase 4: TypeScript Source — `tsextract`** - NestJS recognizer on the `typescript` Compiler API with bright-line third-party-schema exclusions; `NestJs` Source built-in (the documented rule-2 carve-out).
- [ ] **Phase 5: TypeScript Target — `TsSdk`** - Dependency-free `fetch`-based typed client; hermetic `tsc --noEmit` typecheck; `TsSdk` Target built-in.
- [ ] **Phase 6: Cross-Language Hardening + Examples + Docs** - FastAPI + NestJS `.gnr8/` example lifecycles with real committed output; per-language envelope tables in `docs/USAGE.md`; doctor/check/watch parity; cross-language determinism; record the `typescript` carve-out.

## Phase Details

### Phase 1: Language-Neutral IR + Facts Contract + Fixtures

**Goal**: Generalize the `ApiGraph` IR and the shared JSON facts contract from "router-agnostic" to fully "type-system-neutral", and stand up the multi-language fixture+snapshot harness, so that every later sidecar has a single neutral target and a red-by-design acceptance contract to turn green.
**Depends on**: Nothing (first phase)
**Requirements**: IR-01, IR-02, IR-03, IR-04
**Success Criteria** (what must be TRUE):

  1. The IR + JSON facts contract express the cross-language type vocabulary (objects, arrays, enums, optional/nullable, unions) with no Go/Gin/FastAPI/Nest terms leaking into the IR.
  2. The Rust host deserializes the shared facts contract strictly (`#[serde(deny_unknown_fields)]`); OpenAPI lowering + SDK generation consume the IR with no per-language branches.
  3. FastAPI, Flask, and NestJS fixture services exist and encode the v2.0 acceptance cases.
  4. Red-by-design snapshots for each fixture are committed and visibly failing before any extraction lands.

**Plans**: 3 plans

Plans:
**Wave 1**

- [x] 01-01-PLAN.md — Neutral Type enum + facts contract (facts.rs serde + facts.go json tags + graph IR) in lockstep; optional/nullable axes [IR-01, IR-02]

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 01-02-PLAN.md — Consumers: lower/ + gosdk/ exhaustive Type match (no per-language branch), fix optional/nullable + 3.1 nullable rendering, re-accept Go snapshots green [IR-03]

**Wave 3** *(blocked on Wave 2 completion)*

- [x] 01-03-PLAN.md — FastAPI/Flask/NestJS fixtures + red-by-design graph/OpenAPI snapshots, kept visible but out of the green gate [IR-04]

### Phase 2: Python Source — `pyextract`

**Goal**: A developer can turn a real FastAPI service (full) and a Flask service (typed envelope) into the neutral IR via a static `pyextract` sidecar that never imports or executes the target code, so the existing Rust lowering produces OpenAPI 3.1 for Python services unchanged.
**Depends on**: Phase 1
**Requirements**: PYSRC-01, PYSRC-02, PYSRC-03, PYSRC-04, PYSRC-05
**Success Criteria** (what must be TRUE):

  1. A developer can extract routes, path/query params, request bodies (Pydantic/`@dataclass`), response models, and status codes from a FastAPI service into the IR, and routes + blueprint/`APIRouter` prefixes + opt-in typed DTOs from a Flask service.
  2. The sidecar resolves types statically using stdlib `ast` + an owned cross-module symbol table, and never imports or executes the target code.
  3. Unresolvable or untyped surfaces (untyped `request.json`, dynamic prefixes, foreign types) produce diagnostics, never guessed facts — there is no fallback path.
  4. A developer enables Python extraction from `.gnr8/` code via `FastApi` / `Flask` `Source` built-ins, and the FastAPI fixture's red snapshot turns green through the reused Rust pipeline.

**Plans**: 4 plans

Plans:
**Wave 1**

- [x] 02-01-PLAN.md — Rust host seam: PythonToolchainMissing + run_pyextract driver + single deterministic build_graph/collect language dispatch + FastApi/Flask Source built-ins [PYSRC-05]

**Wave 2** *(blocked on Wave 1)*

- [x] 02-02-PLAN.md — pyextract sidecar core: stdlib-ast loader, OWNED cross-module symbol table, annotation→neutral-Type mapper (four-axis fields), deterministic facts marshal; static-only, unittest golden harness [PYSRC-03]

**Wave 3** *(blocked on Wave 2)*

- [x] 02-03-PLAN.md — FastAPI recognition (routes/params/bodies/response_model/status_code, separate prefix); reconcile fixture span lines; flip both FastAPI snapshots GREEN (zero snapshot edits) + determinism [PYSRC-01]

**Wave 4** *(blocked on Wave 3)*

- [x] 02-04-PLAN.md — Flask typed envelope (blueprint prefix, <int:> converter, method-derived status POST→201, typed DTOs, untyped→diagnostics); reconcile diagnostic/span lines (42/69/78); flip both Flask snapshots GREEN + determinism [PYSRC-02, PYSRC-04]

### Phase 3: Python Target — `PySdk`

**Goal**: A developer can generate a dependency-free Python SDK from the neutral IR and prove it works against the live FastAPI fixture, establishing the second SDK target as a pure IR→string twin of the Go SDK.
**Depends on**: Phase 1 (IR/Target seam); Phase 2 (FastAPI fixture facts for the end-to-end test)
**Requirements**: PYSDK-01, PYSDK-02, PYSDK-03
**Success Criteria** (what must be TRUE):

  1. A developer can generate a dependency-free Python SDK from the IR (stdlib `urllib`, `@dataclass` models, a typed `ApiError`, an injectable `OpenerDirector`).
  2. The generated Python SDK imports and type-checks, and round-trips against the FastAPI fixture in a hermetic test (no third-party HTTP deps).
  3. A developer adds the Python SDK to a `.gnr8/` Pipeline via a `PySdk` `Target` built-in, and the output is byte-identical across repeated runs.

**Plans**: TBD

### Phase 4: TypeScript Source — `tsextract`

**Goal**: A developer can turn a real NestJS service into the neutral IR via a `tsextract` sidecar built on the `typescript` Compiler API, deriving every schema fact from the source's own TS types and never from third-party annotation/validation tools, so the reused Rust pipeline produces OpenAPI 3.1 for NestJS services.
**Depends on**: Phase 1
**Requirements**: TSSRC-01, TSSRC-02, TSSRC-03, TSSRC-04
**Success Criteria** (what must be TRUE):

  1. A developer can extract routes, params, and request/response DTOs from a NestJS service (`@nestjs/common` decorators + DTO classes) into the IR.
  2. The sidecar derives every schema fact from the source's own TS types via the `typescript` Compiler API — never from `@nestjs/swagger`, `zod`, or `class-validator` (bright-line exclusion enforced).
  3. Unsupported or untyped surfaces produce diagnostics, never guessed facts — there is no fallback path; extraction is static only (the target TS is never executed).
  4. A developer enables NestJS extraction from `.gnr8/` code via a `NestJs` `Source` built-in, and the NestJS fixture's red snapshot turns green through the reused Rust pipeline.

**Plans**: TBD
**UI hint**: yes

### Phase 5: TypeScript Target — `TsSdk`

**Goal**: A developer can generate a dependency-free TypeScript SDK from the neutral IR and prove it type-checks, completing the fourth language path (NestJS source → TS SDK) as a pure IR→string twin.
**Depends on**: Phase 1 (IR/Target seam); Phase 4 (NestJS fixture facts for the end-to-end test)
**Requirements**: TSSDK-01, TSSDK-02, TSSDK-03
**Success Criteria** (what must be TRUE):

  1. A developer can generate a dependency-free TypeScript SDK from the IR (built-in `fetch`, typed `interface` models + string-literal-union enums, a typed `ApiError`, a configurable `Client`).
  2. The generated TS SDK type-checks under `tsc --noEmit` in a hermetic test, with zero runtime dependencies (no axios).
  3. A developer adds the TS SDK to a `.gnr8/` Pipeline via a `TsSdk` `Target` built-in, and the output is byte-identical across repeated runs.

**Plans**: TBD
**UI hint**: yes

### Phase 6: Cross-Language Hardening + Examples + Docs

**Goal**: A developer can drive complete FastAPI and NestJS projects end-to-end from a `.gnr8/` lifecycle with real committed output, trust an honest per-language supported-envelope, and rely on `doctor`/`check`/`watch` parity and cross-language determinism — with the `typescript` rule-2 carve-out recorded.
**Depends on**: Phases 2, 3, 4, 5
**Requirements**: XLANG-01, XLANG-02, XLANG-03, XLANG-04, XLANG-05
**Success Criteria** (what must be TRUE):

  1. A developer drives a FastAPI service end-to-end (`gnr8 generate` from a `.gnr8/` lifecycle) → OpenAPI 3.1 + a Python SDK, and a NestJS service end-to-end → OpenAPI 3.1 + a TS SDK, each with real committed example output.
  2. `docs/USAGE.md` documents the honest per-language supported envelope (FastAPI full; Flask typed-only; NestJS class DTOs) with limits stated.
  3. `gnr8 doctor` / `check` / `watch` work across all supported language sidecars (toolchain detection, drift, loop-safety).
  4. Every sidecar is stdlib-only in its language (Python `ast`; TS = `typescript` only); `gnr8-core` takes zero OSS deps; all output is deterministic and byte-identical — and the `typescript` carve-out is recorded in PROJECT.md/CLAUDE.md.

**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6

(Phases 3 and 5 are SDK targets; each depends on Phase 1 for the IR/Target seam and on its source phase —
2 and 4 respectively — only for the end-to-end fixture facts. Sequential 1→6 execution satisfies all
dependencies.)

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Language-Neutral IR + Facts Contract + Fixtures | v2.0 | 3/3 | Complete   | 2026-06-25 |
| 2. Python Source — `pyextract` | v2.0 | 4/4 | Complete   | 2026-06-25 |
| 3. Python Target — `PySdk` | v2.0 | 0/TBD | Not started | - |
| 4. TypeScript Source — `tsextract` | v2.0 | 0/TBD | Not started | - |
| 5. TypeScript Target — `TsSdk` | v2.0 | 0/TBD | Not started | - |
| 6. Cross-Language Hardening + Examples + Docs | v2.0 | 0/TBD | Not started | - |
