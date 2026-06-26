# Phase 4: TypeScript Source — `tsextract` - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous) — decisions grounded in locked PROJECT/REQUIREMENTS; recommended defaults auto-accepted

<domain>
## Phase Boundary

Build a `tsextract` Node sidecar on the **`typescript` Compiler API** that turns a real **NestJS** service
into the neutral JSON facts contract, deriving every schema fact from the source's OWN TS types — so the
reused Rust lowering produces OpenAPI 3.1 for NestJS. The Phase-1 `fixtures/nestjs-bookstore/` red snapshots
(graph + OpenAPI) are the acceptance contract this phase turns GREEN.

**In scope:**
- A new top-level `tsextract/` Node sidecar (analogous to `goextract/`/`pyextract/`) whose SOLE dependency
  is `typescript` (the documented rule-2 carve-out — see below). It uses the TS Compiler API
  (`ts.createProgram` + `TypeChecker`) to resolve types statically, emits the SAME neutral facts JSON on
  stdout, and NEVER executes the target TS.
- NestJS recognition: `@Controller('base')` base path, method decorators (`@Get/@Post/@Put/@Patch/@Delete`
  + path arg), params (`@Param`/`@Query`/`@Body` + the typed signature), request/response DTO classes
  (plain TS class properties + their types; `?` optional, `| null` nullable, unions, string-literal-union
  enums, `T[]`, nested DTO classes) mapped onto the Phase-1 neutral `Type` vocabulary.
- Diagnostics for unsupported/untyped surfaces — NO fallback, NO guessed facts (rule 3).
- Rust host seam: extend `analyze::Lang` + `detect_language` with a TypeScript marker (`tsconfig.json`/
  `*.ts`), a `run_tsextract` subprocess driver (mirror `run_pyextract`), and a TS-toolchain-missing typed
  error variant (mirror `PythonToolchainMissing`).
- A `.gnr8/` `NestJs` `Source` built-in (mirror `FastApi`/`Flask`).
- Flip the 2 NestJS red-by-design snapshot tests (`snapshot_nestjs_graph`, `snapshot_nestjs_openapi`) GREEN
  via real extraction — ZERO snapshot edits (the committed snapshots are the byte-exact spec).

**Out of scope:** the TS SDK target (`TsSdk` — Phase 5); changing the IR/lowering (frozen); cross-language
examples/docs (Phase 6); Hono/Express/Fastify/tRPC sources (FUT-01..02, out of v2.0).
</domain>

<decisions>
## Implementation Decisions

### THE rule-2 carve-out (locked, documented — the single OSS-in-toolchain exception)
- `tsextract` may depend on **`typescript` ONLY** — the language's own reference compiler (the `go/types`
  analog). TS has no stdlib type-checker, so a hand-rolled TS parser is multi-month and deferred (FUT-04).
  This is the ONE documented exception to rule 2, recorded in PROJECT.md/STATE Key Decisions. It is run
  behind the JSON-facts boundary; **`gnr8-core` still takes ZERO OSS crates**; generated SDKs stay
  dependency-free.
- **Bright line (rule 1, enforced):** tsextract derives facts ONLY from the source's own TS types via the
  Compiler API. It must NEVER read `@nestjs/swagger`, `zod`, or `class-validator` annotations, and never
  consume any runtime/emitted OpenAPI. `@nestjs/common` decorators (`@Controller`/`@Get`/`@Param`/etc.)
  are recognized for ROUTING facts (they are NestJS's own routing constructs), but SCHEMA facts come from
  the TS types, not from any validation/swagger decorator. The fixture's package.json must NOT make
  schema facts depend on swagger/zod/class-validator.

### Locked (from PROJECT.md / REQUIREMENTS / STATE)
- **Static-only — never execute the target TS** (security boundary). Use `ts.createProgram` over the
  source files; resolve via `TypeChecker`. No `require()`/`import()`/`eval` of target modules at analysis.
- **Unresolved → diagnostic, never a fallback** (rule 3): unsupported/untyped surfaces emit diagnostics;
  facts are omitted, never guessed.
- **One neutral facts contract:** emit the SAME JSON the Rust host deserializes strictly
  (`deny_unknown_fields`); reuse the Phase-1 neutral `Type` vocabulary; no TS terms leak into the IR.
- **Config is code:** enablement via the `.gnr8/` `NestJs` `Source` built-in (rule 4).
- **Deterministic, sorted, byte-identical** facts output.

### Recommended defaults (auto-accepted; Claude's discretion at plan/exec, guided by RESEARCH)
- **Sidecar packaging:** a `tsextract/` dir with a `package.json` pinning ONLY `typescript`, runnable as
  `node tsextract/<entry>.js <target_dir>` (or a tiny bin), emitting neutral facts JSON to stdout. Decide
  at plan time whether to author in plain JS or TS-compiled-to-JS (prefer the simplest deterministic path
  that needs only `typescript` at runtime). `npm install` runs once to vendor `typescript` (NOTE: requires
  network — if the sandbox lacks registry access, this is a blocker to surface, not work around).
- **Subprocess driver:** a Rust `run_tsextract`/`tsextract_dir` mirroring `run_pyextract`; typed error
  `TypeScriptToolchainMissing` (or `NodeToolchainMissing`) mirroring `PythonToolchainMissing`; resolve the
  `tsextract/` dir relative to the crate like the others (carry, don't worsen, the compile-time-path debt).
- **Language detection:** extend `detect_language` to a 3-language decision (Go / Python / TypeScript via
  `tsconfig.json`/`*.ts`). Keep it a SINGLE deterministic classification (no try-then-fallback). Update the
  ambiguity arms to reject mixed-language trees with a typed `Config` error (mirror the existing Go/Python
  ambiguity handling, now extended to three).
- **NestJS envelope:** controllers + method decorators + param decorators + class DTOs with TS types.
  Document anything outside the envelope as a diagnostic. Status codes from `@HttpCode()` if present, else
  the NestJS method-default convention (POST→201, else 200) as a code-derived fact (mirror the Flask
  method-derived-status resolution from Phase 2).

### Claude's Discretion
Exact `tsextract/` module layout, the Compiler-API traversal/visitor design, the TS-type→neutral-Type
mapping details, the diagnostic taxonomy, and the `NestJs` Source surface — all at Claude's discretion,
guided by the goextract/pyextract analogs, the Phase-1 facts contract, and the nestjs fixture's snapshots.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets / Analogs
- `goextract/` + `pyextract/` — the sidecar pattern (self-contained, stdout neutral-facts JSON, subprocess).
  `tsextract/` is their Node/`typescript` twin.
- `crates/gnr8-core/src/analyze/mod.rs` — `Lang` enum + `detect_language` (single tree-walk, Go/Python
  markers) + the `build_graph` per-language `match`. EXTEND with a TypeScript arm (the load-bearing seam).
- `crates/gnr8-core/src/analyze/helper.rs` — `run_pyextract`/`run_goextract` drivers to mirror as `run_tsextract`.
- `crates/gnr8-core/src/error.rs` — `PythonToolchainMissing` to mirror as the TS toolchain-missing variant.
- `crates/gnr8-core/src/analyze/facts.rs` — the neutral facts DTO the tsextract JSON must satisfy.
- `crates/gnr8-core/src/sdk/builtins.rs` — `FastApi`/`Flask`/`GoGin` `Source` built-ins to clone as `NestJs`.
- `fixtures/nestjs-bookstore/` (package.json, src/books.controller.ts, src/books.dto.ts) + the committed
  `snapshot_nestjs_graph`/`snapshot_nestjs_openapi` snapshots — the extraction input + byte-exact green target.

### Established Patterns
- Source sidecar → neutral JSON facts → `deny_unknown_fields` deserialize → `build_graph` → reused
  lowering/OpenAPI. The whole Rust pipeline downstream of facts is reused, never forked.
- Snapshot-driven acceptance + determinism guard; `make check` green gate. The Phase-2 approach of
  reconciling fixture source lines so spans/diagnostics anchor to the committed snapshot lines applies here too.

### Integration Points
- New `tsextract/` dir (+ `package.json` with only `typescript`) + new Rust driver + `Lang::TypeScript`
  dispatch + `NestJs` Source in builtins.rs + flip the 2 nestjs snapshot tests green.
- Toolchain: sandbox has Node v24 + npm 11. `typescript` must be `npm install`ed into `tsextract/`
  (the carve-out). The nestjs snapshot tests skip-if-node/typescript-absent (mirror the Go-toolchain skip)
  so a node-less env never hard-fails.

</code_context>

<specifics>
## Specific Ideas

- `tsextract` mirrors goextract/pyextract in role; its sole dep is `typescript` (the documented carve-out),
  bright-line-excluded from reading swagger/zod/class-validator.
- The 2 nestjs red snapshots are the spec — turn them green via real Compiler-API extraction, zero edits.
- Reconcile fixture source lines to the snapshot's asserted spans/diagnostics (fixture-side, like Phase 2).

</specifics>

<deferred>
## Deferred Ideas

- TypeScript SDK target (`TsSdk`) — Phase 5.
- Hono/typed-Express/Fastify/tRPC TS sources — out of v2.0 (FUT-01..02).
- A stdlib-pure (hand-rolled) TS extraction path that would retire the carve-out — FUT-04.

</deferred>
