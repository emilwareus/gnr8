# Roadmap: gnr8

## Milestones

- ✅ **v1.0 — PoC: Go → OpenAPI → Go SDK** — Phases 1-5 (shipped 2026-06-24)
- ✅ **v2.0 — Multi-language: TypeScript & Python (parse + generate)** — Phases 1-6 (shipped 2026-06-26)
- 🚧 **v3.0 — Production-ready SDK adoption** — Phases 1-5 (planning)

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

<details open>
<summary>🚧 v3.0 Production-ready SDK adoption (Phases 1-5) — PLANNING</summary>

Full requirements: `.planning/REQUIREMENTS.md` · Research: `thoughts/research/adoption-support.md`

- [x] Phase 1: SDK Semantic Model Foundation — introduce the shared SDK planning layer used by all SDK targets.
- [x] Phase 2: Auth And Typed Error Runtime — auth and typed error behavior implemented and verified
      across available SDK runtime toolchains.
- [ ] Phase 3: Stable SDK Surface And Readiness — stabilize naming/grouping, expose SDK readiness in `doctor`, and normalize package metadata.
- [ ] Phase 4: SDK Runtime Ergonomics — add pagination helpers, conservative retries/timeouts/idempotency, and transport hooks.
- [ ] Phase 5: API Metadata And Common Content Types — propagate operation metadata/examples/error docs and round out common content types.

### Phase 1: SDK Semantic Model Foundation

**Goal:** Introduce a shared SDK planning layer so SDK semantic decisions are derived once before
Go/Python/TypeScript rendering.

**Requirements:** SDKM-01, SDKM-02, SDKM-03

**Success criteria:**
1. `ApiGraph` lowers into a deterministic SDK planning model carrying package, service/group, operation,
   schema, auth, error, runtime-policy, docs-metadata, and file-plan facts.
2. Existing SDK targets render from the shared SDK planning facts for the semantic surfaces in scope.
3. Snapshot/compile tests prove current minimal outputs are preserved except for explicitly accepted v3.0
   changes.
4. The SDK model boundary is documented enough for later phases to add auth, errors, grouping, package
   metadata, and runtime policies without target-specific semantic drift.

### Phase 2: Auth And Typed Error Runtime

**Goal:** Make auth and non-2xx error behavior graph-driven and consistent across OpenAPI plus generated
Go/Python/TypeScript SDKs.

**Requirements:** AUTH-01..05, ERR-01..05

**Success criteria:**
1. `.gnr8` pipelines can declare global and per-operation auth, and OpenAPI plus all SDK targets agree on
   which operations are protected or public.
2. Generated SDK consumers can configure API key header/query auth, bearer token auth, and basic auth at
   client construction.
3. Runtime smoke tests verify outgoing auth headers/query params across Go, Python, and TypeScript.
4. Every non-2xx response returns/raises a typed SDK error exposing status, headers/request ID, raw body,
   parsed JSON body, and decoded declared error schema when present.
5. Explicit status, range/default, and undeclared error behavior is covered for both code-first and
   OpenAPI artifact sources.

### Phase 3: Stable SDK Surface And Readiness

**Goal:** Make generated SDK public surfaces predictable and make `doctor` report whether generated SDKs
are ready to consume or publish locally.

**Requirements:** SURF-01..04, READY-01..05, PKG-01..06

**Success criteria:**
1. Pipelines can configure stable operation IDs and operation grouping by tags, path prefix, source
   module/package, or explicit selectors, with collision diagnostics.
2. Group/name facts drive OpenAPI output, SDK methods, generated docs, and `gnr8 compat`.
3. `gnr8 doctor --json` includes `sdk_readiness` for each configured SDK target with language, output
   path, required toolchain, status, and failure reason.
4. `doctor` can surface Go compile/vet, Python `py_compile`/import, TypeScript typecheck, OpenAPI parse,
   local ref, operation ID, and schema-name readiness failures.
5. Go, TypeScript, and Python SDK targets emit coherent package metadata and can run local package
   validation checks without `gnr8` handling registry credentials or uploads.

### Phase 4: SDK Runtime Ergonomics

**Goal:** Add common production SDK ergonomics without unsafe retry behavior or generated-file edits.

**Requirements:** PAGE-01..04, RUN-01..07

**Success criteria:**
1. Pipelines can explicitly configure cursor and page/offset pagination policies without broad inference
   or vendor-extension dependence.
2. SDK consumers can use idiomatic page/iterator helpers while raw operation methods remain available.
3. SDK consumers can configure client-level and per-request timeouts and max retries.
4. Generated SDKs retry only network errors, `408`, `429`, and `5xx` by default, respect `Retry-After`
   where straightforward, and do not retry unsafe mutations unless explicitly marked idempotent.
5. SDK consumers can install deterministic request, response, and error hooks with operation-context
   metadata.

### Phase 5: API Metadata And Common Content Types

**Goal:** Improve generated OpenAPI and SDK docs as public API artifacts and support common non-JSON
endpoint surfaces.

**Requirements:** META-01..03, MEDIA-01

**Success criteria:**
1. Pipelines can configure operation summaries, descriptions, deprecation, tags, response descriptions,
   documented error responses, and named examples through selector-based transforms.
2. Generated OpenAPI and SDK docs/README/reference files propagate operation metadata, examples,
   deprecation notes, tags, and documented error responses.
3. Common content types work across OpenAPI and SDKs: JSON, `text/plain`, form-urlencoded, multipart,
   and binary upload/download.
4. Tests prove unsupported media/type cases emit clear diagnostics rather than guessed behavior.

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
| 1. SDK Semantic Model Foundation | v3.0 | 1/1 | Complete | 2026-07-09 |
| 2. Auth And Typed Error Runtime | v3.0 | 4/4 | Complete | 2026-07-09 |
| 3. Stable SDK Surface And Readiness | v3.0 | 0/? | Planned | — |
| 4. SDK Runtime Ergonomics | v3.0 | 0/? | Planned | — |
| 5. API Metadata And Common Content Types | v3.0 | 0/? | Planned | — |

## Backlog (deferred)

Deferred hardening items, recorded so they are tracked without disturbing the green snapshots. These are
**not** folded into the current acceptance output: folding them changes byte-committed TsSdk snapshots +
the NestJS example output, which the Phase-5 REVIEW-FIX deliberately skipped for that reason.

- **Backlog 999.1 — WR-02: TsSdk non-scalar query param `String()` wire-encoding rule** (deferred
  Phase-5 finding; folding it changes the green TsSdk snapshots).
- **Backlog 999.2 — WR-04: TsSdk asymmetric success/error JSON decode** (same reason).
