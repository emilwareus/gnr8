# Phase 5: TypeScript Target — `TsSdk` - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous) — decisions grounded in locked PROJECT/REQUIREMENTS; recommended defaults auto-accepted

<domain>
## Phase Boundary

Generate a **dependency-free TypeScript SDK** from the neutral `ApiGraph` IR — a pure IR→string TWIN of
the Go SDK (`gosdk/`) and Python SDK (`pysdk/`) — and prove it type-checks under `tsc --noEmit` in a
hermetic test. This completes the fourth language path (NestJS source → TS SDK). Pure codegen: the whole
upstream pipeline (extraction → IR → lowering) already exists; this phase adds one new `Target`.

**In scope:**
- A new `crates/gnr8-core/src/tssdk/` module (mirrors `pysdk/`/`gosdk/`): `emit` (IR→TypeScript via
  `format!`, NO template engine), `bundle` (deterministic multi-file framing), `mod` (`generate` +
  `write_to_dir`). The generated SDK: built-in `fetch` (no axios), typed `interface` request/response
  models, string-literal-union enums (`type X = "a" | "b"`), a typed `ApiError` class, and a configurable
  `Client` (base URL + an injectable `fetch` so the transport is swappable for tests).
- A `TsSdk` `Target` built-in in `crates/gnr8-core/src/sdk/builtins.rs` (mirrors `GoSdk`/`PySdk`): single
  source of truth for the SDK package/module name + base path; wires into the `Pipeline`.
- A hermetic acceptance test (mirrors `tests/pysdk_compile.rs`/`sdk_compile.rs`): build the IR from a
  fixture → generate the TS SDK → `write_to_dir` into a unique stdlib temp dir → run `tsc --noEmit
  --strict` over the generated SDK using the VENDORED `typescript` (`tsextract/node_modules` from Phase 4 —
  no new install). Assert exit 0 (type-checks). Skip gracefully if node/typescript absent (mirror the
  Go/Python toolchain skips).
- A determinism guarantee: generating twice over the same IR is byte-identical (mirror the determinism test).

**Out of scope:** changing the IR/lowering/extraction (frozen); cross-language examples/docs (Phase 6);
any new source frontend.
</domain>

<decisions>
## Implementation Decisions

### Locked (from PROJECT.md / REQUIREMENTS / STATE — non-negotiable)
- **Dependency-free generated SDK:** built-in `fetch` (Node 18+/browser global), typed `interface`
  models + string-literal-union enums, typed `ApiError`, configurable `Client`. NO third-party runtime
  deps (axios/node-fetch forbidden — REQUIREMENTS Out of Scope).
- **gnr8-core takes ZERO new OSS crates** (rule 2). The `tssdk/` module is pure Rust stdlib + the existing
  SDK seam. `format!`-based emission, no template engine (mirror gosdk/pysdk — D-05).
- **IR is the single source of truth** (rule 3/4): SDK base path + package/module name come from the
  `TsSdk` target config / the graph's `base_path` (the SAME source OpenAPI lowering uses), never re-derived.
  Pure IR→string twin: no per-language branches added to lowering. Exhaustive Type-enum handling (no `_=>`).
- **Deterministic, byte-identical output** across runs (TSSDK-03).
- **Config is code:** enablement via the `.gnr8/` `TsSdk` `Target` built-in, not a data file (rule 4).
- **The `typescript` carve-out is TOOLCHAIN-ONLY:** `tsc` is used to TYPECHECK the generated SDK in the
  test; the generated SDK itself has ZERO runtime dependencies. `typescript` is already vendored from
  Phase 4 — do NOT add a new dependency; reuse `tsextract/node_modules`.

### Recommended defaults (auto-accepted; Claude's discretion at plan/exec, guided by RESEARCH)
- **No formatter pass:** like `pysdk` (and unlike `gosdk`'s real `gofmt`), `tssdk::emit` produces
  deterministic, correctly-formatted TypeScript DIRECTLY via `format!` — NO prettier/eslint (third-party).
- **Type mapping:** neutral Type enum → TS: Primitive→`string`/`number`/`boolean`; WellKnown(date-time)→
  `string` (document); Array→`T[]`; Map→`Record<string, T>`; Named→interface/type ref; Object→inline
  `{...}` or a named interface; Enum→string-literal-union `type` (named) or inline union; Union→`A | B`;
  optional→`field?:`; nullable→`| null`; Any→`unknown` (prefer `unknown` over `any` for strictness).
  Exhaustive match, no `_=>`. Reconcile named-vs-inline with how the IR/lowering treats them.
- **Client shape:** a `Client` class/factory taking `{ baseUrl, fetch? }` (injectable `fetch` defaulting to
  the global) so the hermetic test can typecheck without a live server; one method per operation;
  `ApiError` thrown on non-2xx carrying status + body. Mirror the GoSdk/PySdk client ergonomics.
- **Hermetic test:** build IR from the richest fixture for type coverage (the `nestjs-bookstore` IR via
  tsextract completes the NestJS→TS path, OR the `fastapi-bookstore` IR — both exercise unions/enums/
  optional/nullable; planner picks). Run `node tsextract/node_modules/typescript/bin/tsc --noEmit --strict
  <generated>` (or the ts API) over the generated SDK; assert exit 0. Skip if node/typescript absent.
- **Determinism:** fixed import/preamble; sorted; two-run byte-identical (mirror the SDK determinism tests).

### Claude's Discretion
Exact `tssdk/` module split, the generated SDK's file layout + interface/method shape, the precise
`tsc --noEmit` invocation + minimal tsconfig/flags, and the `TsSdk` target surface — all at Claude's
discretion, guided by the `pysdk/`/`gosdk/` twins, the `PySdk`/`GoSdk` targets, and the
`tests/pysdk_compile.rs`/`sdk_compile.rs` hermetic-test pattern.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets / Analogs (the twins to mirror)
- `crates/gnr8-core/src/pysdk/` (mod/emit/bundle — the most recent SDK twin, no-formatter pattern) and
  `crates/gnr8-core/src/gosdk/` — `tssdk/` is their structural twin (TypeScript emission).
- `crates/gnr8-core/src/sdk/builtins.rs` `PySdk`/`GoSdk` `Target` — clone to `TsSdk`.
- `crates/gnr8-core/src/sdk/mod.rs` `trait Target` (`generate(&self, ir, out, cx)`) — the seam TsSdk impls.
- `crates/gnr8-core/tests/pysdk_compile.rs` — the hermetic generate→write→typecheck pattern to mirror
  (stdlib temp dir, toolchain-skip-if-absent). For TS: `tsc --noEmit` instead of py_compile + http.server.
- `crates/gnr8-core/src/graph/mod.rs` — the `ApiGraph` IR (Operation, neutral Type enum, schemas).
- `tsextract/node_modules/typescript` (vendored, v5.9.3) — the `tsc` used to typecheck the generated SDK.
- `fixtures/nestjs-bookstore/` (+ Phase-4 tsextract) and `fixtures/fastapi-bookstore/` — IR sources for
  the hermetic typecheck.

### Established Patterns
- IR → `Target::generate` → `Artifacts` → `.gnr8/` Pipeline writes files (lifecycle/manifest ownership).
- Generation byte-identical across runs (determinism test); typed errors, no production panic; reuse
  `CoreError::SdkGen` (no error.rs edit expected).
- The pysdk hermetic test caught real codegen bugs (f-string, forward-ref) that string unit tests missed —
  the `tsc --noEmit` typecheck plays the same load-bearing role here: it catches emitted-TS that doesn't compile.

### Integration Points
- New `tssdk/` module + `TsSdk` Target in builtins.rs (+ prelude export) + a hermetic `tssdk_compile.rs` test.
- Node v24 + vendored typescript present; the generated SDK targets a `fetch`-having runtime (Node 18+/browser).

</code_context>

<specifics>
## Specific Ideas

- TsSdk is a pure IR→string twin of the Go/Python SDKs; dependency-free `fetch` runtime; `typescript` is a
  TEST-ONLY typechecker (already vendored), not a runtime dep of the generated SDK.
- The `tsc --noEmit --strict` typecheck is the load-bearing acceptance gate (mirrors pysdk's compile+import).
- Prefer `unknown` over `any`; string-literal-union enums; injectable `fetch` for hermetic typechecking.

</specifics>

<deferred>
## Deferred Ideas

- Cross-language examples + `docs/USAGE.md` envelope + doctor/watch parity — Phase 6.
- SDKs with third-party HTTP deps (axios) — permanently out of scope (dependency-free mandate).
- Rust SDK target / other languages — out of v2.0.

</deferred>
