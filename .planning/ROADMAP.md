### Phase 4: TypeScript Source — `tsextract`

**Goal**: A developer can turn a real NestJS service into the neutral IR via a `tsextract` sidecar built on the `typescript` Compiler API, deriving every schema fact from the source's own TS types and never from third-party annotation/validation tools, so the reused Rust pipeline produces OpenAPI 3.1 for NestJS services.
**Depends on**: Phase 1
**Requirements**: TSSRC-01, TSSRC-02, TSSRC-03, TSSRC-04
**Success Criteria** (what must be TRUE):

  1. A developer can extract routes, params, and request/response DTOs from a NestJS service (`@nestjs/common` decorators + DTO classes) into the IR.
  2. The sidecar derives every schema fact from the source's own TS types via the `typescript` Compiler API — never from `@nestjs/swagger`, `zod`, or `class-validator` (bright-line exclusion enforced).
  3. Unsupported or untyped surfaces produce diagnostics, never guessed facts — there is no fallback path; extraction is static only (the target TS is never executed).
  4. A developer enables NestJS extraction from `.gnr8/` code via a `NestJs` `Source` built-in, and the NestJS fixture's red snapshot turns green through the reused Rust pipeline.

**Plans**: 3 plans

Plans:
**Wave 1**

- [x] 04-01-PLAN.md — Rust host seam: Lang::TypeScript + 3-way detect_language + run_tsextract driver + 3-arm dispatch (build_graph + diagnostics::collect) + TypeScriptToolchainMissing + NestJs Source built-in; tsextract/ package skeleton (sole dep typescript) + vendoring decision [TSSRC-04]

**Wave 2** *(blocked on Wave 1)*

- [x] 04-02-PLAN.md — tsextract extractor core: load.js (ts.Program + TypeChecker, static-only, synthesized strict options) + types.js (strip-axes, number→float64, named-vs-inline predicate) + schemas.js (transitive DTO collection) + facts.js marshal; golden + type tests [TSSRC-02, TSSRC-03]

**Wave 3** *(blocked on Waves 1-2)*

- [ ] 04-03-PLAN.md — NestJS routes.js recognition (decorators, group-relative paths, method-derived status); reconcile fixture span lines; flip both nestjs snapshots GREEN (zero snapshot edits) + skip-guard + determinism twins + make check gate [TSSRC-01]

**UI hint**: yes

### Phase 5: TypeScript Target — `TsSdk`
