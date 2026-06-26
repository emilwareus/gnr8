# Phase 5: TypeScript Target — `TsSdk` - Research

**Researched:** 2026-06-25
**Domain:** Deterministic IR→TypeScript code generation (pure string emission) + hermetic `tsc --noEmit` typecheck
**Confidence:** HIGH (every load-bearing claim verified against the in-repo twins and the vendored `tsc`)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **Dependency-free generated SDK:** built-in `fetch` (Node 18+/browser global), typed `interface` models + string-literal-union enums, typed `ApiError`, configurable `Client`. NO third-party runtime deps (axios/node-fetch forbidden — REQUIREMENTS Out of Scope).
- **gnr8-core takes ZERO new OSS crates** (rule 2). The `tssdk/` module is pure Rust stdlib + the existing SDK seam. `format!`-based emission, no template engine (mirror gosdk/pysdk — D-05).
- **IR is the single source of truth** (rule 3/4): SDK base path + package/module name come from the `TsSdk` target config / the graph's `base_path` (the SAME source OpenAPI lowering uses), never re-derived. Pure IR→string twin: no per-language branches added to lowering. Exhaustive Type-enum handling (no `_=>`).
- **Deterministic, byte-identical output** across runs (TSSDK-03).
- **Config is code:** enablement via the `.gnr8/` `TsSdk` `Target` built-in, not a data file (rule 4).
- **The `typescript` carve-out is TOOLCHAIN-ONLY:** `tsc` is used to TYPECHECK the generated SDK in the test; the generated SDK itself has ZERO runtime dependencies. `typescript` is already vendored from Phase 4 — do NOT add a new dependency; reuse `tsextract/node_modules`.

### Claude's Discretion
- **No formatter pass:** like `pysdk` (and unlike `gosdk`'s real `gofmt`), `tssdk::emit` produces deterministic, correctly-formatted TypeScript DIRECTLY via `format!` — NO prettier/eslint (third-party).
- **Type mapping:** Primitive→`string`/`number`/`boolean`; WellKnown(date-time)→`string` (document); Array→`T[]`; Map→`Record<string, T>`; Named→interface/type ref; Object→inline `{...}` or a named interface; Enum→string-literal-union `type` (named) or inline union; Union→`A | B`; optional→`field?:`; nullable→`| null`; Any→`unknown`. Exhaustive match, no `_=>`. Reconcile named-vs-inline with how the IR/lowering treats them.
- **Client shape:** a `Client` class/factory taking `{ baseUrl, fetch? }` (injectable `fetch` defaulting to the global); one method per operation; `ApiError` thrown on non-2xx carrying status + body. Mirror the GoSdk/PySdk client ergonomics.
- **Hermetic test:** build IR from the richest fixture for type coverage; run `tsc --noEmit --strict` over the generated SDK; assert exit 0. Skip if node/typescript absent.
- **Determinism:** fixed import/preamble; sorted; two-run byte-identical.
- Exact `tssdk/` module split, the generated SDK's file layout + interface/method shape, the precise `tsc --noEmit` invocation + minimal tsconfig/flags, and the `TsSdk` target surface — all at Claude's discretion, guided by the twins.

### Deferred Ideas (OUT OF SCOPE)
- Cross-language examples + `docs/USAGE.md` envelope + doctor/watch parity — Phase 6.
- SDKs with third-party HTTP deps (axios) — permanently out of scope.
- Rust SDK target / other languages — out of v2.0.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| TSSDK-01 | Dependency-free TS SDK: built-in `fetch`, typed `interface` models + string-literal-union enums, typed `ApiError`, configurable `Client`. | "Standard Stack" + "IR→TypeScript Type Mapping" + "Generated SDK File Layout" — every shape mapped from a verified pysdk analog; `fetch`/`Response`/`RequestInit` declared by vendored `lib.dom.d.ts` `[VERIFIED]`. |
| TSSDK-02 | Generated TS SDK type-checks (`tsc --noEmit`) hermetically; zero runtime deps; no axios. | "Hermetic Test Mechanics" — `tsc --noEmit --strict --lib es2022,dom` invocation **empirically proven exit 0** on a fetch-based SDK using only the vendored compiler `[VERIFIED]`. Supply-chain grep mirrors pysdk_compile.rs. |
| TSSDK-03 | `.gnr8/` `TsSdk` Target built-in; deterministic byte-identical output. | "The TsSdk Target" — verbatim clone of `PySdk`/`GoSdk` (single source of truth via `sdk_package` + `ir.base_path`); determinism inherited from fixed headers + graph-sorted iteration (same pattern as pysdk). |
</phase_requirements>

## Summary

Phase 5 is a **pure additive twin**: a new `crates/gnr8-core/src/tssdk/` module structurally identical to `crates/gnr8-core/src/pysdk/` (the most recent twin), a `TsSdk` `Target` cloned verbatim from `PySdk` in `sdk/builtins.rs`, and a hermetic `tests/tssdk_compile.rs` cloned from `tests/pysdk_compile.rs` with `tsc --noEmit --strict` swapped in for `py_compile`+`http.server`. The upstream pipeline (extraction→IR→lowering) is frozen and already produces the IR this phase consumes; nothing outside the three new artifacts changes. The IR is even MORE faithfully expressible in TypeScript than in Python — TS has real string-literal unions, real `A | B` sum types, and structural optional (`?:`) / nullable (`| null`) axes, so every place where `pysdk` had to work around a Python limitation (`Literal[..]`, eager-alias forward refs, dataclass required-first ordering, 3.9 f-string backslash bans) **simply disappears** in the TS emitter. There is no analog of those pitfalls.

The single genuinely new mechanic is the typecheck. `pysdk` proves correctness with `python3 -m py_compile` + `import` + a stdlib `http.server` round-trip; `tssdk` proves it with `tsc --noEmit --strict` over the generated `.ts` files. The load-bearing question — *how do you typecheck `fetch` usage without pulling in the third-party `@types/node`?* — is **resolved and empirically verified**: the vendored `typescript` 5.9.3 package ships `lib.dom.d.ts`, which declares `fetch`, `Response`, `RequestInit`, and `RequestInfo`. Invoking `node tsextract/node_modules/typescript/bin/tsc --noEmit --strict --target es2022 --module esnext --moduleResolution bundler --lib es2022,dom <files>` typechecks a fetch-based SDK to exit 0 using **only** the already-vendored compiler — zero new dependencies, fully hermetic. I verified both the positive case (exit 0 with `dom`) and the negative case (`error TS2304: Cannot find name 'fetch'` without `dom`), so the lib setting is proven load-bearing, not assumed.

**Primary recommendation:** Clone `pysdk/{mod,emit,bundle}.rs` → `tssdk/{mod,emit,bundle}.rs`, clone `PySdk` → `TsSdk`, clone `pysdk_compile.rs` → `tssdk_compile.rs`. Map the IR with the verified table below. Emit `interface` models, `type X = "a" | "b"` enums, a fetch `Client`, and an `ApiError extends Error`. Typecheck with the flags-only `tsc` invocation above (no tsconfig file needed). Use the **nestjs-bookstore** fixture IR (completes the NestJS→TS path; node is present) with a graceful skip if node/tsc absent.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| IR→TypeScript string emission | `gnr8-core` library (`tssdk/emit.rs`) | — | Pure function; same tier as `pysdk::emit` / `gosdk::emit`. No I/O, no subprocess. |
| Multi-file framing / determinism | `gnr8-core` library (`tssdk/bundle.rs`) | — | Verbatim twin of `pysdk::bundle` — marker-framed `SdkBundle`, `parse`, `Display`. |
| `generate` + `write_to_dir` orchestration | `gnr8-core` library (`tssdk/mod.rs`) | — | Twin of `pysdk::mod` — assembles files, frames, materializes safely. |
| Config surface (package name, output dir, base path) | `.gnr8/` Pipeline via `TsSdk` Target (`sdk/builtins.rs`) | `gnr8-core` (`sdk_package` helper, reused) | Rule 4: enablement is code. Single source of truth = `sdk_package(module)` + `ir.base_path`, both reused verbatim. |
| Typecheck acceptance (the proof) | Test tier (`tests/tssdk_compile.rs`) | Vendored `tsc` subprocess (toolchain-only) | TSSDK-02. The compiler is a TEST dependency, never linked into `gnr8-core` and never a runtime dep of the generated SDK. |
| HTTP transport at SDK runtime | Generated SDK (browser/Node `fetch` global) | injectable `fetch` for tests | Dependency-free mandate; injectable so a future round-trip test can swap transports without a live server. |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust `std` only | (workspace toolchain) | `tssdk/` emitter — `format!`, `std::fmt::Write`, `std::fs`, `std::path` | CLAUDE.md rule 2: gnr8-core takes ZERO crates. `pysdk` uses exactly this set. `[VERIFIED: crates/gnr8-core/src/pysdk/emit.rs imports]` |
| Generated SDK runtime deps | **none** | The emitted `.ts` uses the `fetch` global + structural types only | TSSDK-01/02: dependency-free. `fetch` is a platform global (Node 18+, all browsers), not an import. `[CITED: 05-CONTEXT.md Locked Decisions]` |

### Supporting (TEST-ONLY — not a gnr8-core dependency)
| Tool | Version | Purpose | When to Use |
|------|---------|---------|-------------|
| `typescript` (vendored `tsc`) | 5.9.3 | `tsc --noEmit --strict` typecheck of the generated SDK in `tests/tssdk_compile.rs` | TSSDK-02 acceptance gate ONLY. Already at `tsextract/node_modules/typescript/bin/tsc`. `[VERIFIED: tsextract/node_modules/typescript/package.json version 5.9.3; node tsextract/node_modules/typescript/bin/tsc --version → "Version 5.9.3"]` |
| `node` | v24.14.1 (any ≥18) | Runs `tsc` (and is the tsextract sidecar runtime for the nestjs IR path) | Test harness only; skip gracefully if absent. `[VERIFIED: node --version → v24.14.1]` |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `--lib es2022,dom` for fetch types | `@types/node` | FORBIDDEN — `@types/node` is a third-party package (rule 2) and is not vendored. `lib.dom` ships INSIDE the `typescript` package, so it adds zero deps. `[VERIFIED: grep "declare function fetch" tsextract/node_modules/typescript/lib/lib.dom.d.ts → line 39142]` |
| `--lib es2022,dom` | Hand-rolled `declare function fetch(...)` ambient block in the test | Unnecessary — `dom` already declares the exact surface (`fetch`/`Response`/`RequestInit`/`RequestInfo`). Hand-rolling an ambient block would be MORE code and risk drifting from the real signatures. Use `dom`. |
| string-literal-union `type` enums | TS `enum` | A `type X = "a" \| "b"` is a pure structural type that erases to strings and JSON-round-trips identically to the wire value (the twin of pysdk's `str, enum.Enum` mixin and gosdk's `type X string`). A TS `enum` emits runtime code and does not erase — avoid. `[CITED: 05-CONTEXT.md type mapping]` |
| `interface` models | `class` models | Interfaces are zero-runtime structural types — a decoded `await res.json()` is assignable directly with a cast, no constructor/from_dict needed (unlike Python @dataclass which needs `from_dict`). Interfaces are simpler AND dependency-free. Use `interface`. |
| `unknown` for `Type::Any` | `any` | CONTEXT prefers `unknown` for strictness (it forces a narrow at use sites; `any` defeats `--strict`). `[CITED: 05-CONTEXT.md specifics]` |

**Installation:** None. No package is installed in this phase. `typescript` is already vendored (Phase 4); `node` is present.

**Version verification:**
- `typescript`: `[VERIFIED]` 5.9.3 — confirmed via `tsextract/node_modules/typescript/package.json` AND `tsc --version`.
- `node`: `[VERIFIED]` v24.14.1 — confirmed via `node --version`.
- No npm registry lookup applies — nothing is installed.

## Package Legitimacy Audit

> This phase installs **no** packages. `gnr8-core` adds zero crates (rule 2); the generated SDK has zero runtime deps; the only tool used (`typescript`) was vendored in Phase 4 and is reused unchanged.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| typescript (reused, not installed) | npm | mature (5.9.3) | ~70M/wk | github.com/microsoft/TypeScript | n/a (pre-vendored, offline) | Reused as-is — no new install |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*No new dependency is introduced in this phase, so the slopcheck gate is not applicable. The single tool is the already-committed, lockfile-pinned `typescript` 5.9.3 from Phase 4.*

## Architecture Patterns

### System Architecture Diagram

```
                       (frozen, pre-existing pipeline)
  source tree ──► analyze::build_graph ──► ApiGraph (IR) ──► [transforms set base_path/title]
   (NestJS .ts)        │                      │
                       └─ tsextract            │  ir.base_path, ir.schemas, ir.operations
                          (node, Phase 4)      │
                                               ▼
                          ┌──────────────────────────────────────────┐
                          │  TsSdk::generate(ir, out, cx)  [NEW]      │   sdk/builtins.rs
                          │   package = sdk_package(self.module)      │   (clone of PySdk)
                          │   base_path = ir.base_path                │
                          └───────────────────┬──────────────────────┘
                                              ▼ calls
                          ┌──────────────────────────────────────────┐
                          │  tssdk::generate(graph, package, base)    │   tssdk/mod.rs  [NEW]
                          │    push client.ts  ← emit_client+emit_ops │
                          │    push errors.ts  ← emit_errors          │
                          │    push index.ts   ← emit_index           │
                          │    push models.ts  ← emit_models          │
                          │    frame → SdkBundle.to_string()          │   tssdk/bundle.rs [NEW]
                          └───────────────────┬──────────────────────┘
                                              ▼
                            String bundle ─► split_bundle ─► Artifacts.write("<dir>/<name>")
                                              │
                                              └─(test path)─► write_to_dir(<tmp>/<pkg>/)
                                                                       │
                                                                       ▼
                                       tsc --noEmit --strict --lib es2022,dom *.ts
                                       (vendored typescript, exit 0 == TSSDK-02 pass)
```

The IR enters via the frozen pipeline; the only new components are the boxed-`[NEW]` stages. Data flow for the primary use case (a NestJS DTO → a typechecked TS SDK file) is traceable left-to-right.

### Recommended Project Structure
```
crates/gnr8-core/src/tssdk/        # NEW — structural twin of pysdk/
├── mod.rs       # generate(graph, package, base_path) + split_bundle + write_to_dir + path-safety
├── emit.rs      # ts_type(), emit_models(), emit_client(), emit_errors(), emit_operations(), emit_index()
└── bundle.rs    # SdkBundle / SdkFile / parse — VERBATIM clone of pysdk::bundle (marker is already `//`)

crates/gnr8-core/src/sdk/builtins.rs   # ADD `TsSdk` Target (clone of PySdk); reuse sdk_package()
crates/gnr8-core/src/sdk/mod.rs        # ADD TsSdk to `prelude` re-export
crates/gnr8-core/src/lib.rs            # ADD `pub mod tssdk;` (next to `pub mod pysdk;` line 15)
crates/gnr8-core/tests/tssdk_compile.rs # NEW — clone of pysdk_compile.rs; tsc --noEmit instead of py_compile
```

### Pattern 1: The pysdk→tssdk module twin (file-by-file map)
**What:** Each `pysdk` file has an exact `tssdk` counterpart. The generated FILE names change from Python to TypeScript.
**When to use:** This is the whole phase.

| pysdk file/emitter | tssdk counterpart | What changes |
|--------------------|-------------------|--------------|
| `mod.rs::generate` pushes `__init__.py`, `client.py`, `errors.py`, `models.py` | `mod.rs::generate` pushes `index.ts`, `client.ts`, `errors.ts`, `models.ts` | File names + extensions; same fixed alpha push order; same `SdkBundle` framing |
| `bundle.rs` (marker `// ==== gnr8:file <name> ====`) | **verbatim clone** | The marker is ALREADY a `//` comment (valid in TS too) — copy unchanged. `[VERIFIED: pysdk/bundle.rs MARKER_PREFIX = "// ==== gnr8:file "]` |
| `emit_models` → `@dataclass` + `class X(str, enum.Enum)` | `emit_models` → `export interface X {}` + `export type X = "a" \| "b"` | Interfaces (no from_dict needed); literal-union enums |
| `emit_client` → urllib `Client` + `OpenerDirector` | `emit_client` → fetch `Client` + injectable `fetch` | `fetch` replaces urllib; `{ baseUrl, fetch? }` replaces `(base_url, *, opener=)` |
| `emit_errors` → `class ApiError(Exception)` | `emit_errors` → `export class ApiError extends Error` | JS-native Error subclass carrying `status`/`body` |
| `emit_operations` → `def m(self, ...)` methods | `emit_operations` → `async m(...): Promise<T>` methods | `async`/`await fetch`; same path-token / query / success-status logic |
| `emit_init` → `__init__.py` re-exports + `__all__` | `emit_index` → `export * from "./client"` etc. (or named re-exports) | TS re-export syntax |
| `py_type(schema, nullable, graph)` | `ts_type(schema, nullable, graph)` | The type-mapping table below; exhaustive `match`, no `_=>` |
| `snake()` for method names | `camelCase()` for method names | TS convention: `createBook` (camel), not `create_book` |

### Pattern 2: Determinism by construction (inherited unchanged)
**What:** Fixed file headers (no computed import set — TS doesn't even need imports for `fetch`), iteration in the graph's already-sorted order, no `HashMap`.
**When to use:** Every emitter.
**Example (the determinism invariant pysdk relies on, mirror it):**
```rust
// Source: crates/gnr8-core/src/pysdk/emit.rs (module docs, verified)
// "every collection is consumed in the graph's already-sorted order, and each
//  file's import header is a FIXED string (no computed import set, no HashMap iteration)."
```
For TS this is even simpler: there is NO import header to compute at all (interfaces and `fetch` need no imports within a file; cross-file model references resolve via the bundle / a single `models.ts` import). Emit a fixed preamble comment, then the schemas in `graph.schemas` order, then operations in `graph.operations` order.

### Pattern 3: Injectable `fetch` (the swappable transport seam)
**What:** The `Client` takes an optional `fetch` defaulting to the global, exactly as pysdk's `Client` takes an optional `OpenerDirector` defaulting to `build_opener()`.
**When to use:** The `Client` constructor — it makes the SDK testable without a live server AND keeps it dependency-free.
**Example (verified to typecheck — from the empirical PoC):**
```typescript
// Source: empirically typechecked PoC (tsc --noEmit --strict --lib es2022,dom → exit 0)
export interface ClientOptions { baseUrl: string; fetch?: typeof fetch; }
export class Client {
  private baseUrl: string;
  private fetchFn: typeof fetch;
  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.fetchFn = opts.fetch ?? fetch;   // global fetch default; `typeof fetch` needs lib.dom
  }
}
```

### Anti-Patterns to Avoid
- **Emitting a TS `enum`:** generates runtime code, does not erase, and does not JSON-round-trip as the bare wire string. Use `export type X = "a" | "b"`.
- **Using `any` for `Type::Any`:** defeats `--strict`. Use `unknown` (CONTEXT).
- **Adding `@types/node`, `node-fetch`, `axios`, or any `import` of an external module:** violates rule 2 AND TSSDK-01/02. The SDK must use only the platform `fetch` global and structural types. The test greps for these and fails (mirror pysdk's banned-import grep).
- **Writing a `tsconfig.json` into the generated SDK output:** the output is dependency-free SDK SOURCE only. The typecheck flags are passed to `tsc` by the TEST, not committed as a config file in the SDK (mirrors how pysdk emits no `setup.py`). A throwaway tsconfig in the temp dir is acceptable but unnecessary — flags-only works (verified).
- **A non-exhaustive `ts_type` match (`_ =>`):** rule 3. Match every `Type` variant explicitly so a future IR variant fails to compile (the pysdk `py_type` discipline).
- **Re-deriving the package name or base path:** rule 3/4. Reuse `sdk_package(self.module)` and `ir.base_path` exactly as `PySdk` does — no second derivation.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Multi-file framing / round-trip parse | A new bundle format | **Clone `pysdk::bundle` verbatim** | The `//`-comment marker is already TS-valid; it's proven + tested. Zero divergence. |
| Package-name derivation | A TS-specific name sanitizer | Reuse `sdk_package(module)` from `sdk/builtins.rs` | Single source of truth (rule 3); already handles last-segment + leading-digit. `[VERIFIED: sdk/builtins.rs:708]` |
| Base-path joining | A TS `join_path` | Mirror `pysdk::emit::join_path` (trivially portable) OR reuse the same logic | Same `ir.base_path` source as OpenAPI lowering (rule 3/4). `[VERIFIED: pysdk/emit.rs:575]` |
| Path-token extraction / set-equality check | A new path parser | Mirror `pysdk::emit::path_tokens` + the token/param set-equality guard | Already catches dangling tokens as a typed `SdkGen` error (WR-03 analog). `[VERIFIED: pysdk/emit.rs:647, 783-794]` |
| success-status / body-model resolution | New resolvers | Mirror `pysdk::emit::success_of` / `body_model_of` | Identical dangling-`$ref`→`SdkGen` semantics. `[VERIFIED: pysdk/emit.rs:594, 628]` |
| fetch typing | `@types/node` / ambient `declare` | `tsc --lib es2022,dom` | `lib.dom` ships in `typescript`; zero deps. `[VERIFIED]` |
| JSON encode/decode in the SDK | A serializer | `JSON.stringify` / `await res.json()` (platform built-ins) | Dependency-free; `res.json()` returns `Promise<any>`, cast to the model interface. |
| Error type | A custom error shape | `class ApiError extends Error` (JS built-in) | Native, dependency-free; carries `status` + `body` like pysdk's `ApiError`. |

**Key insight:** Almost nothing is genuinely new. The Rust-side scaffolding (bundle, package derivation, path tokens, success resolution, path-safety on write, the Target wrapper) is a structural copy of `pysdk`. The ONLY new authored logic is (a) the `ts_type` mapping table, (b) the TS string templates in the four emitters, and (c) the `tsc` invocation in the test. Everything else is a rename-and-clone.

## IR→TypeScript Type Mapping

This is the heart of `ts_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError>`. Cross-checked against `pysdk::emit::py_type` (the exhaustive analog) so the match arms line up one-to-one. `[VERIFIED: pysdk/emit.rs:149-216 py_type; facts.rs:148-229 Type/Prim/WellKnown enums]`

| IR `Type` variant | pysdk emits | **tssdk should emit** | Notes |
|-------------------|-------------|------------------------|-------|
| `Primitive(String)` | `str` | `string` | |
| `Primitive(Bool)` | `bool` | `boolean` | |
| `Primitive(Int{..})` | `int` | `number` | TS has one numeric type; width is irrelevant (matches tsextract's `number->float64` extraction, STATE 04-02). |
| `Primitive(Float{..})` | `float` | `number` | |
| `Primitive(Bytes)` | `bytes` | `string` | No byte type on the wire; document as base64 string (parity with WellKnown carrying as string). |
| `WellKnown(_)` (date-time/uuid/email/uri/…) | `str` | `string` | A7: every well-known scalar carries as a string in a dependency-free SDK (no `Date`/luxon import). Document date-time as RFC-3339 string. |
| `Array(inner)` | `List[..]` | `${ts_type(inner)}[]` | e.g. `string[]`, `BookDto[]`. |
| `Map{key,value}` | `Dict[str, Any]` | `Record<string, unknown>` | A keyed/free-form map. (pysdk collapses to `Dict[str, Any]`; mirror with `Record<string, unknown>` for `--strict`.) |
| `Named(ref_id)` | resolved schema `name` | resolved schema `name` (interface/type ref) | Resolve via `graph.schemas.find(|s| s.id == ref_id)`; dangling → typed `SdkGen` error (mirror pysdk). |
| `Enum(members)` (INLINE) | `Literal["a", "b"]` | `"a" \| "b"` (string-literal union) | Members in graph-sorted order. |
| `Union(variants)` | `Union[A, B]` | `A \| B` | Recurse each variant; join with ` \| `. The case Go rejects; TS expresses it natively. |
| `Object(fields)` (INLINE) | typed `SdkGen` ERROR | **typed `SdkGen` error** (parity) | Every object in the IR is a named `$ref`; keep the explicit error arm, NOT a catch-all (rule 3). |
| `Any {}` | `Any` | `unknown` | CONTEXT prefers `unknown` over `any`. |

**The `nullable` axis** (the `nullable: bool` param): pysdk wraps in `Optional[..]`; tssdk appends `| null`:
```
if nullable { format!("{base} | null") } else { base }
```
**The `optional` axis** (key-may-be-absent) is NOT handled in `ts_type` — exactly like pysdk it lives at the FIELD declaration: an optional field emits `name?: T` (the `?:` marker) rather than widening the type. Keep the two axes distinct (the pysdk Pitfall-4 lesson, but cleaner in TS):

| field flags | TS field declaration |
|-------------|----------------------|
| required, non-nullable | `name: T` |
| required, nullable | `name: T \| null` |
| optional, non-nullable | `name?: T` |
| optional, nullable | `name?: T \| null` |

This is strictly simpler than pysdk, which had to widen optional-non-nullable to `Optional[T] = None` to avoid a default-vs-type mismatch. TS's `?:` is purely structural — no default, no widening, no `from_dict`. `[VERIFIED: facts.rs:117-135 FieldFact has independent optional+nullable bools; graph/mod.rs test field_nullable_axis_is_carried_distinctly_from_optional]`

### Named-vs-inline enum reconciliation
The IR resolves this UPSTREAM (frozen). A named enum is a top-level `Schema` whose `body` is `Type::Enum` (→ `export type X = "a" | "b"` in `models.ts`, referenced elsewhere as `Type::Named("...X")`). An inline enum is a `Type::Enum` appearing directly in a field/param `schema` (→ an inline `"a" | "b"` literal union at the use site). `ts_type` therefore handles `Type::Enum` as the INLINE form (literal union) and `Type::Named` as the reference form — identical to how `py_type` splits `Literal[..]` (inline) vs a named-enum class. `emit_models` decides per-schema: a `Type::Enum` body → `export type X = ...`; a `Type::Object` body → `export interface X {}`; a named `Type::Union` body → `export type X = A | B`. `[VERIFIED: this exactly mirrors pysdk/emit.rs:252-284 emit_models match; STATE 04-02/04-03 confirm the IR's named-vs-inline discriminator is already settled at extraction time]`

A **named non-object/non-enum** schema (e.g. `BookOrError = BookDto | OutOfStockDto`, a named union; or a scalar/array alias) → `export type X = <ts_type(body)>`. This is the load-bearing divergence pysdk made from gosdk (Go rejected named unions); TS expresses it natively as a `type` alias — even cleaner than pysdk's PEP-484 string-forward-ref workaround (TS type aliases are order-independent; NO forward-ref hack needed).

## Generated SDK File Layout

Four files, fixed alpha push order (mirrors pysdk's `__init__.py`/`client.py`/`errors.py`/`models.py`):

| File | Contents | pysdk analog |
|------|----------|--------------|
| `client.ts` | `import { ApiError } from "./errors"; import * as models from "./models";` (or named) + the `Client` class (constructor `{ baseUrl, fetch? }`, a private `request()` helper) + one `async` method per operation appended by `emit_operations`. | `client.py` |
| `errors.ts` | `export class ApiError extends Error { constructor(public status: number, public body: unknown) { super(...) } isNotFound() { return this.status === 404; } }` | `errors.py` |
| `index.ts` | re-export surface: `export * from "./client"; export * from "./errors"; export * from "./models";` (or explicit named re-exports for determinism — prefer named, graph-sorted, to mirror pysdk's `__all__`). | `__init__.py` |
| `models.ts` | one `export interface X { ... }` per object schema; one `export type X = "a" \| "b"` per named enum; one `export type X = A \| B` per named union/alias. Schemas in graph id-sorted order. | `models.py` |

**Operation method shape (verified-to-typecheck pattern):**
```typescript
// Source: empirically typechecked PoC (exit 0). Method body mirrors pysdk's emit_operation flow.
async createBook(body: BookDto): Promise<CreatedMessage> {
  const path = `/books`;                                    // join_path(base_path, op.path)
  const res = await this.fetchFn(`${this.baseUrl}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status !== 201) {                                  // success_of(op) real status
    throw new ApiError(res.status, await res.json().catch(() => null));
  }
  return (await res.json()) as CreatedMessage;              // typed return; bodyless op → void/unknown
}
```
- **Path params:** template-interpolate with `encodeURIComponent(String(x))` (the `urllib.parse.quote(safe='')` analog / V5 path-injection mitigation). Reuse the path-token set-equality guard.
- **Query params:** build a `URLSearchParams`; required always appended, optional guarded by `if (x !== undefined)` (the WR-01 analog). Append `?${qs}` when non-empty.
- **Bodyless / no-2xx-body ops:** `Promise<void>` or `Promise<unknown>` (mirror pysdk's `Any` return when no model).
- **camelCase** method names from `op.handler` (TS convention; the `snake()` analog is a `camel()` helper).

## Common Pitfalls

### Pitfall 1: Typechecking `fetch` pulls in a forbidden dependency
**What goes wrong:** Reaching for `@types/node` (or `node-fetch`) to make `tsc` recognize `fetch`/`Response` — a rule-2 violation and not vendored.
**Why it happens:** `fetch` is a global, and the default `tsc` lib set (driven by `--target`) does NOT include DOM types, so a bare `--strict --target es2022` run fails with `TS2304: Cannot find name 'fetch'`.
**How to avoid:** Pass `--lib es2022,dom`. `lib.dom.d.ts` ships inside the vendored `typescript` package and declares `fetch`, `Response`, `RequestInit`, `RequestInfo` — zero extra deps. `[VERIFIED: positive case exit 0 with dom; negative case TS2304 without dom]`
**Warning signs:** Any `@types/*` in the temp dir; `TS2304: Cannot find name 'fetch'` in the test output.

### Pitfall 2: A string snapshot looks right but the TS doesn't compile
**What goes wrong:** Unit tests assert substrings (`out.contains("export interface BookDto")`) and pass, but the emitted TS has a real type error (a dangling ref, a malformed union, a method returning the wrong type).
**Why it happens:** Exactly the pysdk lesson — `pysdk` shipped an f-string backslash bug and a forward-ref `NameError` that string tests MISSED and only `py_compile`/`import` caught. `[VERIFIED: 05-CONTEXT Established Patterns; STATE Phase-03 notes]`
**How to avoid:** The `tsc --noEmit --strict` gate IS the real acceptance bar (TSSDK-02). Treat it as load-bearing, not a nicety. Run it over the FULL generated SDK from a real fixture, not a synthetic minimal graph.
**Warning signs:** Green unit tests but a red `tssdk_compile`.

### Pitfall 3: `tsc` picks up an ambient/global type accidentally
**What goes wrong:** A stray `node_modules/@types` or a global tsconfig leaks types into the typecheck, making it pass for the wrong reason (or fail spuriously).
**Why it happens:** `tsc` auto-includes `@types/*` from any reachable `node_modules` unless told otherwise.
**How to avoid:** Materialize the SDK into a UNIQUE stdlib temp dir (no `node_modules` nearby) and pass `--types` empty / rely on flags-only invocation with `--lib es2022,dom` (no `@types` present in the temp dir → nothing to leak). `[VERIFIED: no @types dir in tsextract/node_modules; flags-only PoC ran clean from /tmp]` Optionally add `--types ""` defensively.
**Warning signs:** The typecheck passes even when you deliberately break a type.

### Pitfall 4: Non-deterministic output from unsorted iteration
**What goes wrong:** Iterating a `HashMap` or recomputing an import set yields a different byte order across runs (TSSDK-03 fail).
**Why it happens:** Any non-`Vec` collection or computed set.
**How to avoid:** Iterate `graph.schemas` / `graph.operations` (already sorted by the IR) directly; emit a FIXED preamble; no `HashMap`. TS needs no computed import header at all. Mirror pysdk's determinism test (two-run byte-identical) at the `generate`, `emit_models`, and `bundle` levels.
**Warning signs:** A flaky determinism test.

### Pitfall 5: Node/tsc absent hard-fails the suite
**What goes wrong:** `tssdk_compile` panics on a missing toolchain in a CI without node.
**Why it happens:** No skip guard.
**How to avoid:** Early-return skip if node/tsc absent, exactly like `pysdk_compile`'s `python_available()` and `sdk_compile`'s `go` check. Map a spawn failure to `CoreError::TypeScriptToolchainMissing` (already exists). `[VERIFIED: error.rs:59 TypeScriptToolchainMissing; pysdk_compile.rs:47-54 skip pattern]`
**Warning signs:** A red suite on a node-less machine.

### Pitfall 6 (NON-pitfall — explicitly absent): Python-specific workarounds
The pysdk emitter carries four workarounds that have **NO TS analog** — do NOT port them:
- Dataclass required-first field ordering (TS `?:` is order-free) — skip the partition.
- `from __future__ import annotations` / lazy-annotation header — TS has no import header.
- PEP-484 string forward refs for named aliases — TS type aliases are order-independent.
- f-string backslash `SyntaxError` (`safe=''`) — TS template literals + `encodeURIComponent` have no such restriction.
Porting these would be cargo-culting. The TS emitter is simpler than the Python one.

## Code Examples

### The verified hermetic typecheck invocation (the load-bearing one)
```bash
# Source: empirically run in this session — exit 0 on a fetch-based SDK using ONLY the vendored compiler.
node <repo>/tsextract/node_modules/typescript/bin/tsc \
  --noEmit --strict \
  --target es2022 --module esnext --moduleResolution bundler \
  --lib es2022,dom \
  <tmpdir>/sdk/*.ts
# echo $? -> 0
# Negative control (omit ",dom"): -> "error TS2304: Cannot find name 'fetch'", exit 2.
```
In Rust the test invokes this as discrete `Command::new("node").args([...])` args (never a shell string — the pysdk_compile V13 discipline), with `current_dir` the temp dir.

### `ts_type` skeleton (exhaustive, no `_=>`)
```rust
// Source: structural twin of crates/gnr8-core/src/pysdk/emit.rs:149 py_type (verified exhaustive).
pub(crate) fn ts_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(p) => ts_primitive(p).to_string(),     // string/number/boolean/string(bytes)
        Type::WellKnown(_) => "string".to_string(),
        Type::Array(items) => format!("{}[]", ts_type(items, false, graph)?),
        Type::Map { .. } => "Record<string, unknown>".to_string(),
        Type::Any {} => "unknown".to_string(),
        Type::Named(ref_id) => resolve_name(ref_id, graph)?,   // dangling -> SdkGen error
        Type::Enum(members) => members.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(" | "),
        Type::Union(vs) => { let mut p = Vec::new(); for v in vs { p.push(ts_type(v, false, graph)?); } p.join(" | ") }
        Type::Object(_) => return Err(CoreError::SdkGen {
            message: "inline object type is unsupported by the TypeScript SDK target (expected a named $ref)".to_string(),
        }),
    };
    Ok(if nullable { format!("{base} | null") } else { base })
}
```

### `ApiError` (errors.ts) — verified to typecheck
```typescript
// Source: PoC pattern (compiles under --strict --lib es2022,dom). Twin of pysdk's ApiError(Exception).
export class ApiError extends Error {
  constructor(public readonly status: number, public readonly body: unknown) {
    super(`HTTP ${status}`);
    this.name = "ApiError";
  }
  isNotFound(): boolean { return this.status === 404; }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SDKs ship `axios`/`node-fetch` | Built-in `fetch` global (Node ≥18, all browsers) | Node 18 (2022) made `fetch` global stable | Zero-dependency HTTP is now the default; no transport library needed. `[CITED: 05-CONTEXT; Node fetch is global since 18]` |
| `@types/node` for global types | `--lib dom` (ships in `typescript`) for `fetch`/`Response` | always available; load-bearing here for rule 2 | Typecheck fetch with zero extra deps. `[VERIFIED]` |
| TS `enum` for closed string sets | string-literal union `type X = "a" \| "b"` | modern TS idiom | Erases to strings, JSON-round-trips, zero runtime. |

**Deprecated/outdated:** none relevant — the IR, SDK seam, and vendored `tsc` are all current and frozen for this phase.

## Runtime State Inventory

> Not a rename/refactor/migration phase — this is purely additive (new module + new Target + new test). No stored data, live service config, OS-registered state, secrets, or build artifacts carry a string this phase changes.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — verified: no datastore keys on any renamed string (nothing is renamed). | none |
| Live service config | None — verified: no external service config touched. | none |
| OS-registered state | None — verified: no scheduled tasks / daemons. | none |
| Secrets/env vars | None — verified: the typecheck is offline; no secrets. | none |
| Build artifacts | One forward-looking note: the GENERATED SDK output dir (`generated/sdk-ts` or similar) becomes a loop-safety anchor via `TsSdk::output_anchors()` — but that is per-`.gnr8/`-config, not a repo artifact this phase commits. No stale artifact exists yet. | Implement `output_anchors()` (clone of PySdk) so a re-run never re-ingests the generated `.ts`. |

## Validation Architecture

> nyquist_validation is enabled (no `.planning/config.json` opt-out found; absent ⇒ enabled).

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` (unit tests inline per module; integration test in `crates/gnr8-core/tests/`). The typecheck uses the vendored `tsc` via `std::process::Command`. No third-party test crate (rule 2; `insta` is debt, not used for tssdk). |
| Config file | none (cargo discovers tests) |
| Quick run command | `cargo test -p gnr8-core tssdk` |
| Full suite command | `cargo test -p gnr8-core` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TSSDK-01 | `ts_type` maps every IR variant; emitters produce interface/enum/union/ApiError/Client | unit | `cargo test -p gnr8-core tssdk::emit` | ❌ Wave 0 (`tssdk/emit.rs` tests, clone of pysdk emit tests) |
| TSSDK-01 | bundle frames 4 files + round-trips | unit | `cargo test -p gnr8-core tssdk::bundle` | ❌ Wave 0 (clone pysdk::bundle tests) |
| TSSDK-02 | generated SDK typechecks `tsc --noEmit --strict` hermetically; no banned imports | integration | `cargo test -p gnr8-core --test tssdk_compile` | ❌ Wave 0 (`tests/tssdk_compile.rs`, clone of pysdk_compile.rs) |
| TSSDK-02 | invalid TS → captured `CoreError`, not a panic | integration | `cargo test -p gnr8-core --test tssdk_compile invalid` | ❌ Wave 0 |
| TSSDK-03 | `TsSdk` Target writes under output dir; two runs byte-identical; output_anchors | unit | `cargo test -p gnr8-core builtins::tests::tssdk` | ❌ Wave 0 (clone of pysdk_target_writes_under_the_output_dir_and_is_deterministic) |
| TSSDK-03 | `generate` byte-identical two-run; `mod.rs` four-marker | unit | `cargo test -p gnr8-core tssdk` | ❌ Wave 0 (clone pysdk/mod.rs tests) |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core tssdk` (unit emit/bundle/mod + Target tests; sub-second, no toolchain).
- **Per wave merge:** `cargo test -p gnr8-core` (includes the `tssdk_compile` integration test which invokes node/tsc).
- **Phase gate:** full suite green + `clippy -D warnings` (RUST-04 no unwrap/expect/panic in production) before `/gsd:verify-work`.

### Wave 0 Gaps
- [ ] `crates/gnr8-core/src/tssdk/bundle.rs` (+ tests) — verbatim clone of `pysdk/bundle.rs`.
- [ ] `crates/gnr8-core/src/tssdk/emit.rs` (+ tests) — `ts_type` + 5 emitters; clone+adapt pysdk emit tests for TS shapes (no required-first / forward-ref tests needed).
- [ ] `crates/gnr8-core/src/tssdk/mod.rs` (+ tests) — `generate`/`split_bundle`/`write_to_dir`; clone pysdk/mod.rs tests (file names → `.ts`).
- [ ] `crates/gnr8-core/tests/tssdk_compile.rs` — clone pysdk_compile.rs; swap `py_compile`+`http.server` for `tsc --noEmit --strict --lib es2022,dom`; banned-import grep for `axios`/`node-fetch`/`@types`; skip if node/tsc absent.
- [ ] `TsSdk` Target tests in `sdk/builtins.rs` — clone the `PySdk` Target tests (unconfigured-error, writes-under-dir, determinism, output_anchors).
- [ ] Framework install: none — cargo + vendored tsc + node already present.

## Security Domain

> security_enforcement enabled (absent ⇒ enabled). This phase generates code + runs a typechecker; the relevant surface is the generated SDK's request construction and the test's subprocess hygiene.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | The generated SDK carries no auth scheme this phase (security lives in OpenAPI lowering, not the SDK; CONTEXT defers it). |
| V3 Session Management | no | Stateless SDK. |
| V4 Access Control | no | N/A to a codegen target. |
| V5 Input Validation / Output Encoding | yes | Path params interpolated via `encodeURIComponent(String(x))` (URL-injection mitigation, the `urllib.parse.quote(safe='')` analog). Query via `URLSearchParams` (auto-encoded). `[VERIFIED: pysdk/emit.rs:852 the V5 analog]` |
| V6 Cryptography | no | None. |
| V12/V13 (subprocess/command) | yes | The test invokes `node`/`tsc` with DISCRETE `Command` args + `current_dir`, NEVER a shell string; the temp dir is PID+nanosecond-unique with no user-supplied component; the driver/files are written to disk and run by path. `[VERIFIED: pysdk_compile.rs:59-95 the exact discipline to mirror]` |

### Known Threat Patterns for {Rust codegen + Node typecheck subprocess}
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Bundle frame name with `..`/`/` escapes temp dir on write | Tampering / Elevation | `write_to_dir` + Target reject any name containing `/`,`\`,`..`,empty (clone pysdk's guard). `[VERIFIED: pysdk/mod.rs:100; sdk/builtins.rs:622]` |
| URL/path injection via an unescaped path param | Tampering | `encodeURIComponent` on every path token (V5). |
| Command injection via the tsc invocation | Tampering | Discrete args, no `sh -c` (V13). |
| Supply-chain: a third-party HTTP/types dep sneaks into the generated SDK | Tampering | Test greps every emitted `.ts` for `axios`/`node-fetch`/`@types`/`from "http"` and fails if present (clone pysdk's banned-import assertion). `[VERIFIED: pysdk_compile.rs:144-151]` |
| Panic on malformed IR / missing toolchain | DoS | Typed `CoreError` everywhere (`SdkGen`/`Config`/`TypeScriptToolchainMissing`); no production unwrap/expect/panic (RUST-04). |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Emitting `interface` models (vs `class`) needs no decode/from_dict — `await res.json() as Model` is sufficient. | Generated SDK File Layout | LOW. If a future requirement needs runtime validation, an interface won't enforce it — but TSSDK-01 asks only for typed models, and the SDK is dependency-free (no validator). Matches the "typed interface models" wording exactly. |
| A2 | The nestjs-bookstore fixture is the right primary for the typecheck (vs fastapi-bookstore). | Hermetic Test | LOW. Both fixtures yield equivalent-rich IR (named enum, inline enum, named union, nested object, array, all 4 optional/nullable combos — verified in both DTO/model sources). nestjs "completes the NestJS→TS path" (the milestone's narrative) and node is present. Planner may pick fastapi if a node-less but python-present CI is a concern; either typechecks identically since the IR is language-neutral. Flagged for the planner's pick (CONTEXT explicitly leaves this to the planner). |
| A3 | `index.ts` should use named graph-sorted re-exports (vs `export *`) for determinism. | Generated SDK File Layout | LOW. `export *` is also deterministic if files are fixed; named re-exports mirror pysdk's `__all__` more closely and read better. Either is byte-stable. Planner's discretion. |
| A4 | `Map` → `Record<string, unknown>` (not `Record<string, T>` with a typed value). | Type Mapping | LOW. Mirrors pysdk's `Dict[str, Any]` collapse. The IR's `Map{key,value}` carries a value type, so a stricter `Record<string, ts_type(value)>` is ALSO available and arguably better — planner may choose the typed form. Both typecheck. |

**Note:** A2 and A4 are genuine planner-discretion choices CONTEXT already delegated; they are logged for visibility, not because they block. A1/A3 are low-risk style calls grounded in the pysdk twin.

## Open Questions

1. **Single value-typed `Record` vs `Record<string, unknown>` for `Type::Map`.**
   - What we know: IR `Map{key,value}` carries both; pysdk collapses to `Dict[str, Any]`.
   - What's unclear: whether to emit the stricter `Record<string, ${ts_type(value)}>`.
   - Recommendation: emit the stricter typed form `Record<string, ts_type(value)>` if it's trivial (it is — `value` is in hand); fall back to `Record<string, unknown>` only for a free-form map. Either passes `--strict`. Decide at plan/exec.

2. **Fixture choice for the typecheck (nestjs vs fastapi).** See A2. Recommendation: nestjs-bookstore (completes the milestone path, node present); keep the skip guard so a node-less env doesn't fail.

3. **Whether to also add an optional round-trip test (like pysdk's http.server driver).** TSSDK-02 asks ONLY for `tsc --noEmit` (typecheck), not a runtime round-trip. Recommendation: scope to the typecheck (the requirement) for this phase; a runtime round-trip is not required and adds a fetch-vs-stdlib-server complication. Defer any round-trip to Phase 6 if ever wanted. (snapshot_tssdk is also optional per CONTEXT — add only if it mirrors cleanly; the typecheck is the real gate.)

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `node` | tssdk_compile (runs tsc + nestjs IR via tsextract) | ✓ | v24.14.1 | Skip the integration test gracefully (mirror pysdk skip). |
| vendored `tsc` | tssdk_compile typecheck (TSSDK-02) | ✓ | 5.9.3 | none needed — committed in tsextract/node_modules. |
| `lib.dom.d.ts` (fetch types) | typecheck of `fetch` usage | ✓ | ships in typescript 5.9.3 | none — it's inside the vendored package. |
| `python3` | (only if planner picks the fastapi fixture for the IR) | ✓ | 3.9.25 | If fastapi fixture chosen and python absent → skip; nestjs fixture avoids this entirely. |
| `cargo`/rustc | building + unit tests | ✓ | workspace toolchain | none. |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none blocking — node/tsc/python3 all present; the only "fallback" is the standard toolchain-absent test skip.

## Project Constraints (from CLAUDE.md)

| Directive | How this phase complies |
|-----------|--------------------------|
| Rule 1 — never couple to another tool's conventions/output | SDK derives purely from the IR; no annotation/sidecar reading. |
| Rule 2 — no third-party/OSS deps (stdlib only); prefer hand-rolled | gnr8-core adds ZERO crates; generated SDK has ZERO runtime deps (fetch global only); `tsc` is reused (vendored, test-only), not newly added; `lib.dom` avoids `@types/node`. |
| Rule 3 — one deterministic source per fact; no fallback; exhaustive match | `ts_type` is an exhaustive `Type` match (no `_=>`); package = `sdk_package(module)` only; base = `ir.base_path` only; no dual paths. |
| Rule 4 — what source can't express comes from user code-as-config | Enablement is the `.gnr8/` `TsSdk` Target (code), not a data file; base_path/title set by transforms. |
| Typed errors; no production unwrap/expect/panic; deterministic byte-identical | Reuse `CoreError::SdkGen`/`Config`/`TypeScriptToolchainMissing`; `sink()` folds fmt errors; fixed headers + sorted iteration → byte-identical; `#[allow(clippy::unwrap_used,...)]` scoped to test modules only. |

## Sources

### Primary (HIGH confidence — verified this session)
- `crates/gnr8-core/src/pysdk/mod.rs` + `emit.rs` + `bundle.rs` — the twin to clone (read in full).
- `crates/gnr8-core/src/sdk/mod.rs` + `builtins.rs` — `Target` trait, `PySdk`/`GoSdk`, `sdk_package`, prelude.
- `crates/gnr8-core/tests/pysdk_compile.rs` — the hermetic-test pattern to mirror (skip guard, unique temp dir, discrete-arg subprocess, banned-import grep).
- `crates/gnr8-core/src/graph/mod.rs` + `analyze/facts.rs` — the IR `ApiGraph`/`Type`/`Prim`/`WellKnown`/`FieldFact` (optional+nullable independent axes).
- `crates/gnr8-core/src/error.rs` — `SdkGen` / `Config` / `GoBuild` / `TypeScriptToolchainMissing` variants (lines 107/159/131/59).
- `tsextract/node_modules/typescript/` — version 5.9.3 (`package.json` + `tsc --version`); `lib.dom.d.ts:39142` declares `fetch`.
- **Empirical tsc PoC** — `tsc --noEmit --strict --target es2022 --module esnext --moduleResolution bundler --lib es2022,dom` on a fetch SDK → exit 0; same without `dom` → `TS2304: Cannot find name 'fetch'` exit 2. `node --version` v24.14.1.
- `fixtures/nestjs-bookstore/src/books.dto.ts` + `fixtures/fastapi-bookstore/app/models.py` — confirmed both encode named/inline enum, named union, nested object, array, all four optional/nullable combos.

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` Phase-03/04 decision log — pysdk emit divergences, tsextract vendoring (Option A, typescript 5.9.3 pinned), named-vs-inline discriminator settled at extraction.

### Tertiary (LOW confidence)
- none — every claim is grounded in an in-repo read or an empirical run.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — versions verified by tool; zero new packages.
- Architecture (twin map, type mapping): HIGH — line-by-line cross-check against the verified pysdk twin and the IR enums.
- Hermetic typecheck (the new mechanic): HIGH — the exact `tsc` invocation was run and produced exit 0 (positive) and TS2304 (negative control).
- Pitfalls: HIGH — drawn from the documented pysdk pitfalls + empirically confirmed fetch-lib behavior.

**Research date:** 2026-06-25
**Valid until:** 2026-07-25 (stable — IR, SDK seam, and vendored tsc are frozen for this milestone).
