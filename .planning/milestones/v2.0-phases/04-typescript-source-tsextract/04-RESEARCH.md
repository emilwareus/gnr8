# Phase 4: TypeScript Source — `tsextract` - Research

**Researched:** 2026-06-25
**Domain:** Static TypeScript extraction via the `typescript` Compiler API (`ts.createProgram` + `TypeChecker`) → the frozen language-neutral JSON facts contract; Rust subprocess seam for a third language.
**Confidence:** HIGH — everything is codebase-grounded. The frozen facts DTO, the goextract/pyextract analogs, the host seams, and the two byte-exact acceptance snapshots are all committed and read in full; the TS Compiler API behavior against the actual fixture was empirically probed (typescript 5.9.3, Node v24.14.1) and every neutral-Type mapping was confirmed against the committed snapshot.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**THE rule-2 carve-out (the single OSS-in-toolchain exception):**
- `tsextract` may depend on **`typescript` ONLY** — the language's own reference compiler (the `go/types` analog). TS has no stdlib type-checker; a hand-rolled TS parser is multi-month and deferred (FUT-04). This is the ONE documented exception to rule 2 (recorded in PROJECT.md / STATE Key Decisions). It runs behind the JSON-facts boundary; **`gnr8-core` still takes ZERO OSS crates**; generated SDKs stay dependency-free.
- **Bright line (rule 1, enforced):** tsextract derives facts ONLY from the source's own TS types via the Compiler API. It must NEVER read `@nestjs/swagger`, `zod`, or `class-validator` annotations, and never consume any runtime/emitted OpenAPI. `@nestjs/common` decorators (`@Controller`/`@Get`/`@Param`/etc.) are recognized for ROUTING facts (they are NestJS's own routing constructs), but SCHEMA facts come from the TS types, not from any validation/swagger decorator. The fixture's `package.json` must NOT make schema facts depend on swagger/zod/class-validator.

**Locked (from PROJECT.md / REQUIREMENTS / STATE):**
- **Static-only — never execute the target TS** (security boundary). Use `ts.createProgram` over the source files; resolve via `TypeChecker`. No `require()`/`import()`/`eval` of target modules at analysis.
- **Unresolved → diagnostic, never a fallback** (rule 3): unsupported/untyped surfaces emit diagnostics; facts are omitted, never guessed.
- **One neutral facts contract:** emit the SAME JSON the Rust host deserializes strictly (`deny_unknown_fields`); reuse the Phase-1 neutral `Type` vocabulary; no TS terms leak into the IR.
- **Config is code:** enablement via the `.gnr8/` `NestJs` `Source` built-in (rule 4).
- **Deterministic, sorted, byte-identical** facts output.

### Claude's Discretion
- **Sidecar packaging:** a `tsextract/` dir with a `package.json` pinning ONLY `typescript`, runnable as `node tsextract/<entry>.js <target_dir>`, emitting neutral facts JSON to stdout. Decide at plan time whether to author in plain JS or TS-compiled-to-JS (prefer the simplest deterministic path that needs only `typescript` at runtime). `npm install` runs once to vendor `typescript` (requires network — verified available in this sandbox).
- **Subprocess driver:** a Rust `run_tsextract`/`tsextract_dir` mirroring `run_pyextract`; typed error `TypeScriptToolchainMissing` (or `NodeToolchainMissing`) mirroring `PythonToolchainMissing`; resolve the `tsextract/` dir relative to the crate like the others (carry, don't worsen, the compile-time-path debt).
- **Language detection:** extend `detect_language` to a 3-language decision (Go / Python / TypeScript via `tsconfig.json`/`*.ts`). Keep it a SINGLE deterministic classification (no try-then-fallback). Update the ambiguity arms to reject mixed-language trees with a typed `Config` error.
- **NestJS envelope:** controllers + method decorators + param decorators + class DTOs with TS types. Document anything outside the envelope as a diagnostic. Status codes from `@HttpCode()` if present, else the NestJS method-default convention (POST→201, else 200) as a code-derived fact (mirror the Flask method-derived-status resolution from Phase 2).
- Exact `tsextract/` module layout, the Compiler-API traversal/visitor design, the TS-type→neutral-Type mapping details, the diagnostic taxonomy, and the `NestJs` Source surface — all at Claude's discretion.

### Deferred Ideas (OUT OF SCOPE)
- TypeScript SDK target (`TsSdk`) — Phase 5.
- Hono/typed-Express/Fastify/tRPC TS sources — out of v2.0 (FUT-01..02).
- A stdlib-pure (hand-rolled) TS extraction path that would retire the carve-out — FUT-04.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| TSSRC-01 | Extract routes, params, request/response DTOs from a NestJS service (`@nestjs/common` decorators + DTO classes) into the IR | Decorator AST recognition (Architecture Pattern 2) + TypeChecker type resolution (Pattern 3) + the byte-exact graph snapshot mapping (Code Examples). Empirically verified against the fixture: all 4 routes, all params, all 8 DTOs resolve. |
| TSSRC-02 | TS sidecar uses the `typescript` Compiler API; derives every schema fact from the source's own types — never from `@nestjs/swagger`/`zod`/`class-validator` | The carve-out (`typescript` 5.9.3, the sole dep). `TypeChecker.getTypeAtLocation` reads property types directly (Pattern 3). The fixture has NO swagger/zod/class-validator on its DTOs — confirmed. Bright line in Project Constraints + Security Domain. |
| TSSRC-03 | Unsupported or untyped surfaces produce diagnostics, never guessed facts (no fallback) | `DiagnosticFact` emission + omit (rule 3); diagnostic taxonomy in Common Pitfalls. NOTE: the committed snapshot has `diagnostics: []` — the in-envelope fixture produces zero diagnostics; the taxonomy is for the unit-test surface, not the acceptance snapshot. |
| TSSRC-04 | Enable NestJS extraction from `.gnr8/` code via a `NestJs` `Source` built-in | Clone `FastApi`/`Flask` Source in `builtins.rs` (Don't Hand-Roll + Code Examples). Calls the SAME `build_graph`; language dispatch by target detection. |
</phase_requirements>

## Summary

This phase builds `tsextract`, a Node sidecar whose **sole dependency is `typescript`** (the documented rule-2 carve-out), that statically reads a NestJS service via `ts.createProgram` + `TypeChecker` and emits the **same** neutral JSON facts document the Go (`goextract/`) and Python (`pyextract/`) sidecars emit. The Rust host deserializes that JSON with `deny_unknown_fields` and runs it through the *unchanged* `ApiGraph::from_facts` → lowering → OpenAPI pipeline. The acceptance target is byte-exact and already committed: two red-by-design snapshots (`snapshot_nestjs_graph`, `snapshot_nestjs_openapi`) flip green the moment the extractor produces the right facts — **with zero snapshot edits**.

The work splits into three concerns, identical in shape to Phase 2: (1) a Node sidecar that recognizes NestJS routing decorators (`@Controller`/`@Get`/`@Post`/`@Put`/`@Param`/`@Query`/`@Body`) for ROUTING facts and resolves param/body/return DTO types via the `TypeChecker` for SCHEMA facts, mapping TS types onto the frozen neutral `Type` vocabulary; (2) the Rust seam — a `run_tsextract` driver analogous to `run_pyextract`, a `Lang::TypeScript` arm extending the now-**3-language** `detect_language`, and the dispatch arm added in **both** `analyze::build_graph` AND `diagnostics::collect`; and (3) a `NestJs` `Source` built-in in `builtins.rs` cloned from `FastApi`.

The single most important constraint is that **the committed snapshots ARE the specification**. Every field name, sort order, `prim: float bits: 64` width (TS `number` → float64), `named` vs inline enum decision, optional/nullable axis, and span line number is already fixed. This RESEARCH maps each snapshot fact back to the precise TS construct that produces it — empirically verified by running the Compiler API against the real fixture (typescript 5.9.3, Node v24.14.1). The one genuine work item beyond extraction is **fixture line reconciliation**: the snapshot's asserted spans do NOT match the fixture's current line numbers (the offsets are large and non-uniform), so the fixture source must be edited — inserting blank lines / non-fact comments only — so each honest AST anchor lands on its snapshot line, exactly as Phase 2 did (resolved Q2).

**Primary recommendation:** Build `tsextract/` as a Node package whose only `package.json` dependency is `typescript`; author it in plain JS so no compile step is needed at runtime (`node tsextract/<entry>.js <target_dir>`). Drive correctness exclusively against the two committed snapshots: strip `null`/`undefined` arms to derive the optional/nullable axes (NOT union members), map `number`→float64, detect named type-alias references via `type.aliasSymbol` (do NOT inline string-literal-union aliases), and reconcile the fixture lines to the snapshot. Add `run_tsextract` + the `Lang::TypeScript` arm in both host seams + a `NestJs` Source.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Parse target TS, build a Program + TypeChecker | `tsextract/` (Node sidecar, `typescript` dep) | — | The Compiler API IS the language's reference type-checker; the sidecar owns all TS-specific knowledge |
| NestJS route recognition (decorators) | `tsextract/` route module | — | `@nestjs/common` decorators are NestJS's own routing constructs; the sidecar reads their AST shape, never their runtime |
| Type resolution (param/body/return → neutral Type) | `tsextract/` via `TypeChecker` | — | `getTypeAtLocation` resolves types statically; never imports/executes target |
| Neutral facts JSON emission (sorted, byte-stable) | `tsextract/` facts module | — | Contract boundary; the sidecar marshals, the host deserializes strictly |
| Subprocess invocation + typed errors | `analyze::helper` (Rust) | — | Mirrors `run_pyextract`; the host never parses TS |
| Language dispatch (Go vs Python vs TS) | `analyze::build_graph` + `diagnostics::collect` (Rust) | `analyze::detect_language` | A single deterministic 3-way detector; the same entry serves all three languages |
| Facts → `ApiGraph` lowering, sorting, relativization | `graph::ApiGraph::from_facts` (Rust) | — | **Reused unchanged** — the v2.0 bet; TS facts flow through the same code |
| `.gnr8/` enablement (which dirs, NestJS) | `sdk::builtins` `NestJs` Source | — | Config-is-code (rule 4); thin wrapper over `build_graph` |
| OpenAPI 3.1 generation | `lower::to_openapi` (Rust) | — | Reused unchanged; language-agnostic |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `typescript` (Compiler API) | 5.9.3 [VERIFIED: `npm install typescript@5` in sandbox → `ts.version` = 5.9.3] | `ts.createProgram` + `TypeChecker` to parse the target TS and resolve types statically | The ONE permitted external dependency this phase — the documented rule-2 carve-out (the language's own reference compiler, the `go/types` analog). TS has no stdlib type-checker, so a hand-rolled parser is deferred (FUT-04). |
| Node.js runtime | v24.14.1 [VERIFIED: `node --version` on sandbox] | Runs the `tsextract` sidecar; provides `process.argv`, `fs`, `path`, JSON | The host for the `typescript` library; the TS analog of `python3`/`go`. Only the runtime + `typescript` — no other npm package. |
| Rust `std::process::Command` | std | Spawn `node` as a subprocess with discrete args | Already the goextract/pyextract pattern; no shell, no interpolation (T-02-01 mirror) |
| serde / serde_json | existing workspace dep (debt) | Deserialize facts JSON in the host | The same deserialize path goextract/pyextract use; this phase adds **zero** new Rust deps |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ts.createProgram(rootFiles, options)` | typescript 5.9 | Build a Program over the discovered `*.ts` files | Entry point; pass `experimentalDecorators: true` + `strictNullChecks: true` (see Pitfall 1) |
| `program.getTypeChecker()` | typescript 5.9 | Resolve types, walk unions, resolve named symbols | The core resolution engine |
| `checker.getTypeAtLocation(node)` | typescript 5.9 | Get the resolved `Type` of a property/parameter/return | Maps onto neutral Type |
| `ts.getDecorators(node)` | typescript 5.9 (replaces deprecated `node.decorators`) | Read class/method/param decorators | Use `ts.getDecorators`, NOT `node.decorators` (removed in TS 5.x AST without the helper) — see Pitfall 6 |
| `type.aliasSymbol` | typescript 5.9 | Detect a named type-alias reference (e.g. `BookFormat`) so it is emitted as a `named` ref, not inlined | Critical for the named-vs-inline enum decision (Pitfall 4) |
| `sourceFile.getLineAndCharacterOfPosition(node.getStart(sf))` | typescript 5.9 | 1-based line numbers for spans/diagnostics | Provenance; reconcile to snapshot lines |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `typescript` Compiler API | A hand-rolled Rust TS parser (stdlib-pure) | **Deferred (FUT-04)** — multi-month; TS has no stdlib type-checker. The carve-out exists precisely because this is infeasible now. |
| `typescript` Compiler API | `@nestjs/swagger` runtime schema / emitted OpenAPI / `class-validator` metadata | **FORBIDDEN (rule 1)** — third-party annotation/validation tools and runtime output. The whole product premise is deriving from the source's own types. Not an option. |
| Plain JS sidecar | TS-compiled-to-JS sidecar | Plain JS needs no build step and only `typescript` at runtime; prefer it (CONTEXT recommends the simplest deterministic path). A `.ts` sidecar would need a compile step in `make check` — avoidable complexity. |
| `ts.transpileModule` / emit | Static `Program` + `TypeChecker` only | Emitting = a transform step we don't need and risks executing nothing but adds surface; static type queries are sufficient and safer. |

**Installation (the ONLY install this phase):**
```bash
cd tsextract && npm install typescript@5
# vendors typescript 5.9.3 (23M, ZERO transitive deps — single package) into tsextract/node_modules
```
[VERIFIED: in-sandbox install succeeded; `node_modules` = 23M, all of it `typescript`, `0 vulnerabilities`, `added 1 package`.]

**Version verification:** `node --version` → `v24.14.1`; `npm --version` → `11.11.0`; after install, `node -e "console.log(require('typescript').version)"` → `5.9.3` [all VERIFIED via Bash on sandbox]. Pin `typescript` to a specific version in `package.json` (e.g. `"typescript": "5.9.3"`) for determinism — a floating `^5` could resolve to a newer patch that changes `typeToString` formatting.

## Package Legitimacy Audit

> This phase installs exactly ONE external package — `typescript` — the documented carve-out. slopcheck could not be installed in this sandbox session; per the graceful-degradation protocol, `typescript` is treated as `[VERIFIED via official source]` rather than slopcheck-`[OK]`, because it is the canonical, first-party Microsoft compiler (the de facto standard, not a registry discovery), verified directly: `npm install typescript@5` succeeded, resolved to 5.9.3, added exactly 1 package with 0 transitive deps and 0 vulnerabilities, and `require('typescript').version` confirms it loads.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `typescript` | npm | ~12 yrs | ~60M+/wk | github.com/microsoft/TypeScript | n/a (unavailable; first-party canonical) | Approved (the carve-out) |
| `@nestjs/common` (fixture `package.json` only) | npm | — | — | github.com/nestjs/nest | n/a | **NOT installed** — the fixture lists it for documentation only; tsextract never installs or reads it at runtime. Confirmed by the fixture `package.json` `"//"` note. |

**Postinstall check:** `npm install typescript` added the package with no flagged postinstall network/filesystem activity in the install log (`added 1 package, audited 2 packages`). The `typescript` package has no malicious postinstall — it is the reference compiler.

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*Any task that proposes a SECOND npm dependency in `tsextract/`, or a new Rust crate in `gnr8-core`, is a rule-2 defect and must be rejected. The carve-out is exactly one package: `typescript`.*

## Architecture Patterns

### System Architecture Diagram

```
.gnr8/ Pipeline  (user code, rule 4)
   │  NestJs::new().inputs(["."])
   ▼
Source::load(cx)                       [crates/gnr8-core/src/sdk/builtins.rs]  ← NEW NestJs Source
   │  resolves input against cx.project_root
   ▼
analyze::build_graph(target_dir)       [crates/gnr8-core/src/analyze/mod.rs]
   │  ── LANGUAGE DISPATCH (NOW 3-WAY) ──┐
   │   detect Go vs Python vs TypeScript │  (also in diagnostics::collect — SAME detector)
   ▼            ▼                        ▼
run_goextract  run_pyextract     run_tsextract(dir)   [analyze/helper.rs]  ← NEW driver
 (existing)     (existing)         │  Command::new("node")
                                  │    .args([<entry.js>, target_dir])
                                  │    .current_dir(tsextract_dir())
                                  │    capture stdout → bytes
                                  ▼
              ┌──────────  tsextract  (Node, `typescript` ONLY) ─────────────┐
              │  entry:    argv → target_dir                                  │
              │     ▼                                                         │
              │  load:     discover *.ts under target; ts.createProgram(...)   │
              │            with experimentalDecorators + strictNullChecks      │
              │     ▼      checker = program.getTypeChecker()                  │
              │  routes:   walk @Controller class → @Get/@Post/@Put/@Param/    │
              │            @Query/@Body decorators (ts.getDecorators); verb +   │
              │            method-path; params from signature; body/return via   │
              │            checker.getTypeAtLocation                            │
              │     ▼                                                         │
              │  types:    TS Type → neutral Type (strip null/undefined arms   │
              │            for axes; number→float64; aliasSymbol→named ref)     │
              │     ▼                                                         │
              │  schemas:  every transitively-referenced DTO class/alias →     │
              │            SchemaFact (object / enum / union body)             │
              │     ▼                                                         │
              │  diag:     out-of-envelope / untyped surface → DiagnosticFact   │
              │            (rule 3 — omit fact, never guess)                    │
              │     ▼                                                         │
              │  facts:    sort every array; JSON.stringify → stdout           │
              └───────────────────────────────────────────────────────────────┘
                                  │  JSON facts document (same shape as goextract/pyextract)
                                  ▼
serde_json::from_slice::<facts::GoFacts>  (deny_unknown_fields)   [analyze/facts.rs]
   ▼
ApiGraph::from_facts(facts, module_root)   ── REUSED UNCHANGED ──  [graph/mod.rs]
   │  sorts ops/schemas/params/fields/enums; relativizes spans vs module_root
   ▼
lower::to_openapi(ir, …)                    ── REUSED UNCHANGED ──  [lower/]
   ▼
OpenAPI 3.1 document  +  ApiGraph  →  the two committed nestjs snapshots (turn GREEN)
```

### Recommended Project Structure
```
tsextract/                       # new top-level dir (mirrors goextract/, pyextract/)
├── package.json                 # name + "dependencies": { "typescript": "5.9.3" } ONLY
├── package-lock.json            # committed lockfile (deterministic install)
├── node_modules/                # see "node_modules vendoring decision" — likely .gitignore'd + installed in make check
├── index.js                     # entry: argv → target_dir → run() → JSON to stdout; nonzero exit on error
├── load.js                      # discover *.ts under target; ts.createProgram + getTypeChecker
├── routes.js                    # @Controller + method/param decorator recognition (AST)
├── types.js                     # TS Type → neutral Type dict (strip axes, number→float64, aliasSymbol→named)
├── schemas.js                   # DTO class / type-alias → SchemaFact (object/enum/union body); transitive collection
├── diagnostics.js               # diagnostic accumulator (severity/message/file/line)
└── facts.js                     # neutral facts dict builders + deterministic JSON.stringify marshal
```
(Module names are Claude's discretion per CONTEXT; this mirrors `pyextract/{load,routes,types,schemas,diagnostics,facts}`.)

### Pattern 1: `typescript`-only sidecar → neutral JSON → strict deserialize (the v2.0 narrow waist)
**What:** The sidecar emits exactly the JSON the Rust `facts::GoFacts` DTO accepts. The DTO is already language-neutral — its module doc says *"every language sidecar emits this one shared facts contract"* — and pyextract already proved a non-Go sidecar feeds it cleanly.
**When to use:** Always — this is the whole contract.
**Key shape rules (from `facts.rs`, all `deny_unknown_fields`) — identical to what pyextract emits:**
- `Type` is adjacently tagged: `{"type":"<variant>","of":<payload>}`. `Any` is `{"type":"any","of":{}}` (empty object).
- `Prim` is internally tagged on `prim`: `{"prim":"string"}`, `{"prim":"bool"}`, `{"prim":"int","bits":64,"signed":true}`, `{"prim":"float","bits":64}`, `{"prim":"bytes"}`.
- `WellKnown` is a plain snake_case string payload: `{"type":"well_known","of":"uuid"}` (not used by this fixture, but available).
- `Named` payload is the bare id string: `{"type":"named","of":"src/books.dto.AuthorDto"}`.
- `Enum` payload is a string array: `{"type":"enum","of":["asc","desc"]}`.
- `Union` payload is an array of `Type`: `{"type":"union","of":[ … ]}`.
- `FieldFact` keys EXACTLY: `json_name, required, optional, nullable, schema, description, example` — `description`/`example` are ALWAYS `null` here.
- `request_body`/`response.body` are `{"ref_id":"<id>"}` (a `TypeRef`), or `null`.
- A `DiagnosticFact` has EXACTLY `severity, message, file, line` (line is a single `u32`, NOT a span).
- Top-level keys EXACTLY: `module, routes, schemas, diagnostics`.

### Pattern 2: NestJS route recognition (decorator AST shape — empirically verified)
**What:** Walk the `@Controller`-decorated class; for each method with an HTTP-verb decorator, read the verb + path arg, then the parameter decorators and the return type.
**AST shapes (verified against the fixture via `ts.getDecorators` + `ts.isCallExpression`):**
```
@Controller('books')           → Decorator → CallExpression(expression=Identifier "Controller",
                                              arguments=[StringLiteral "books"])
@Get('/')                      → Decorator → CallExpression(expression=Identifier "Get",
                                              arguments=[StringLiteral "/"])    ; verb = "GET"
@Param('bookId') bookId: number→ parameter with Decorator CallExpression(expr="Param", args=["bookId"])
@Query('genre') genre: string  → parameter Decorator CallExpression(expr="Query", args=["genre"])
@Body() book: BookDto          → parameter Decorator CallExpression(expr="Body", args=[])  ; no name arg
```
**Verb map:** `Get→GET, Post→POST, Put→PUT, Patch→PATCH, Delete→DELETE`.
**Path conversion:** NestJS `'/:bookId'` → neutral `'/{bookId}'` (`:name` → `{name}`); the `@Controller('books')` prefix is **NOT folded** into the operation path (rule 1) — operation paths are group-relative (`/`, `/{bookId}`); the snapshot shows `base_path: /` because no transform sets it.
**Param location:** `@Param` → `location: "path"`, `required: true`; `@Query` → `location: "query"`, `required` derived from `?`/default (see Pitfall 3); `@Body` → `request_body` (a `TypeRef`), not a param.
**operation_id / handler:** the method name verbatim (`listBooks`, `createBook`, `getBook`, `updateBook`) — the snapshot shows `id: listBooks`, `handler: listBooks`.

### Pattern 3: TS Type → neutral Type via the TypeChecker (the core of types.js — empirically verified)
**What:** Resolve each property/param/return type with `checker.getTypeAtLocation(node)`, then map. **The critical move is stripping the `null` and `undefined` arms FIRST** to compute the optional/nullable axes; whatever remains is the actual schema type.
```
number                        → {"prim":"float","bits":64}   ← VERIFIED: snapshot shows every `number` as float64
string                        → {"prim":"string"}
boolean                       → {"prim":"bool"}
T[]                           → {"type":"array","of":map(T)}        (checker.isArrayType / getNumberIndexType)
field?: T   (questionToken)   → optional = true; the resolved type also carries a `| undefined` arm → STRIP it
field: T | null               → nullable = true; STRIP the `null` arm
field?: T | null              → optional=true AND nullable=true; STRIP both → leaves T (a SINGLE type, NOT a union)
A | B  (no null/undefined)    → {"type":"union","of":[map(A),map(B)]}   (source order kept)
string-literal-union ALIAS    → if type.aliasSymbol is set → {"type":"named","of":"<id>"} (a REF; do NOT inline)
inline string-literal-union   → {"type":"enum","of": sorted([...])}     (no aliasSymbol; inline)
DTO class type                → {"type":"named","of":"<id>"} + emit the class as a SchemaFact
```
**VERIFIED behaviors (probed against the fixture, typescript 5.9.3, strictNullChecks):**
- `rating?: number | null` resolves to `number | null | undefined`. Strip `undefined` (optional) + `null` (nullable) → leaves `number` → `{"prim":"float","bits":64}`. The snapshot confirms `rating` is a single float64 primitive with `optional:true, nullable:true` — **NOT a union**. This is the most important reconciliation rule.
- `format: BookFormat` (an alias to `'paperback'|'hardcover'`) carries `type.aliasSymbol.getName() === "BookFormat"` → emit `{"type":"named","of":"src/books.dto.BookFormat"}` (snapshot confirms `named`), AND emit `BookFormat` as a separate enum schema with **sorted** members `["hardcover","paperback"]`.
- `sort?: SortOrder | null` (alias `'asc'|'desc'`) — the snapshot shows this as an **inline** `enum: [asc, desc]` with `optional:true, nullable:true`, NOT a `named` ref. ⚠️ This contradicts the `format` rule above — see **Pitfall 4** and **Open Question 1** for the exact reconciliation (the snapshot is authoritative; the rule is likely "a string-literal-union resolves to an inline enum when it appears alongside a stripped null/undefined arm, but to a named ref when it is the sole type" — must be pinned at plan time).
- `bio: string | null` → strip null → `string` primitive, `nullable:true` (snapshot confirms).
- `tags?: string[]` → strip undefined → `string[]` → array of string, `optional:true, nullable:false` (snapshot confirms).
- `author: AuthorDto` → `getSymbol()` resolves to the `AuthorDto` class declaration in `books.dto.ts` → `named` ref to `src/books.dto.AuthorDto` (snapshot confirms).

### Pattern 4: Schema id + module derivation
**What:** Schema ids are `src/books.dto.<Name>` — the target-relative file path (with `/` kept, `.ts` dropped) joined by `.` to the declared name. VERIFIED from the snapshot: `id: src/books.dto.AuthorDto`, `id: src/books.dto.BookFormat`, etc. Note the id keeps the slash form `src/books.dto` (it is `relpath(file).removesuffix('.ts')` then `+ "." + name`), NOT a dotted module path.
**module:** the snapshot shows `module: nestjs-bookstore` = `basename(target_dir)` (VERIFIED — same rule pyextract uses; `from_facts` copies `facts.module` verbatim).
**Transitive schema collection:** every DTO referenced from any route param/body/response — AND transitively from any DTO field or union arm — must be emitted. VERIFIED: `OutOfStockDto` appears in the snapshot only because it is a union arm of `BookOrError` (a `getBook` response); the sidecar must transitively collect it. All 8 schemas (`AuthorDto, BookDto, BookFilters, BookFormat, BookOrError, CreatedMessage, ListBooksResponse, OutOfStockDto`) appear, sorted by id (the host re-sorts, but sort in the sidecar too for internal determinism).

### Pattern 5: Status code — code-derived (mirror the Flask method-derived-status resolution)
**What:** The snapshot shows POST `createBook` → `status: 201`; every other route → `status: 200`. The fixture has NO `@HttpCode()` decorator. VERIFIED from the snapshot: this is exactly the Flask Phase-2 rule — **typed POST → 201, else → 200**, derived from the HTTP method (which is in the code). If a `@HttpCode(n)` decorator IS present on a method, it overrides (a code fact); else apply the method-default convention. This is a single deterministic rule, not a fallback (rule 3).
**Response body:** the return type → `{"ref_id":"<id>"}`. `listBooks→ListBooksResponse@200`, `createBook→CreatedMessage@201`, `getBook→BookOrError@200`, `updateBook→CreatedMessage@200` (VERIFIED).

### Anti-Patterns to Avoid
- **Reading `@nestjs/swagger`/`zod`/`class-validator`** — every schema fact comes from the TS property type (rule 1, the whole premise). The fixture deliberately has none.
- **Folding the `@Controller('books')` prefix into the path** — operation paths must stay group-relative (`/`, `/{bookId}`) (rule 1; the prefix is a base path).
- **Treating `T | null` / `T | undefined` as Union members** — they are the nullable/optional AXES; strip them. A real union (`A | B` of two non-null types) is the only `Type::Union`.
- **`number` → `int`** — TS `number` is IEEE double → `{"prim":"float","bits":64}` (snapshot is unambiguous on every numeric field/param).
- **Inlining a named type-alias when the snapshot expects a `named` ref** (and vice-versa) — `format` is `named`, `sort` is inline `enum`; reconcile against the snapshot (Pitfall 4 / Open Question 1).
- **Emitting declaration order for enum members** — sort lexically: `BookFormat` declared `'paperback' | 'hardcover'` → snapshot `["hardcover","paperback"]`.
- **`node.decorators` / `node.getStart()` without the SourceFile** — use `ts.getDecorators(node)` and `node.getStart(sf)` / `node.getText(sf)` (Pitfall 6).
- **Executing the target** — never `require`/`import`/`transpile-and-run`; static `Program`/`TypeChecker` queries only (security boundary).
- **A fallback when a type can't be resolved** — emit a diagnostic and OMIT the fact (rule 3); never guess.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parsing + type-checking TypeScript | A hand-rolled TS parser/checker | the `typescript` Compiler API (the carve-out) | TS's type system is enormous (unions, conditional types, generics, narrowing); the reference compiler IS the only correct source. A hand-rolled parser is FUT-04 (multi-month). |
| Facts → graph lowering | A second TS→graph path in Rust | `ApiGraph::from_facts` (reused) | The neutral contract is language-agnostic; forking lowering breaks the v2.0 bet and IR-03. pyextract already proved the path is reused unchanged. |
| Sorting / relativizing in the host | Re-sorting TS facts differently | `from_facts` already sorts ops/schemas/params/fields/enums + relativizes spans | The Rust side normalizes; the sidecar needs only internal determinism. |
| Subprocess error taxonomy | New ad-hoc error strings | a `TypeScriptToolchainMissing` (or `NodeToolchainMissing`) variant mirroring `PythonToolchainMissing` + reuse `HelperExit`/`FactsParse` | Typed errors, no panics (RUST-04); consistent with the Go/Python drivers. |
| Language dispatch | A try-Go-then-Py-then-TS chain | a SINGLE deterministic `detect_language` returning `Lang::{Go,Python,TypeScript}` | rule 3 — one decision from the scanned markers, mixed-tree → typed `Config` error. |
| JSON serialization in the sidecar | A custom JSON writer | `JSON.stringify` with sorted arrays + stable key order | Node built-in; deterministic once arrays are pre-sorted and objects are built key-ordered. |
| `.gnr8/` NestJS enablement | A new Source extraction path | clone the `FastApi` Source as `NestJs` (calls the SAME `build_graph`) | rule 4 — thin wrapper; language is detected from the target, not the Source. |

**Key insight:** Identical to Phase 2 — the entire Rust pipeline downstream of the facts JSON is *already written and tested* for Go and Python. This phase's only Rust work is (a) a ~30-line `run_tsextract` driver, (b) extending `detect_language` to 3 languages and adding the `Lang::TypeScript` arm in **both** `build_graph` AND `diagnostics::collect`, (c) a `NestJs` Source, and (d) a `TypeScriptToolchainMissing`/`NodeToolchainMissing` error variant. All real new logic lives in the Node sidecar, whose correctness target is fully specified by the two committed snapshots.

## Runtime State Inventory

> This phase ADDS a sidecar and extends a seam; it renames/migrates nothing. Most categories are N/A; the host-side dispatch seams and the node_modules vendoring decision are the load-bearing items.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — facts flow stdout→host only. Verified: no datastore. | none |
| Live service config | None — no external service holds state. | none |
| OS-registered state | None — no scheduled tasks/daemons. | none |
| Secrets/env vars | None new. `CI=true` / `INSTA_UPDATE=no` already govern snapshot acceptance. | none |
| Build artifacts | (a) The 4 `*.snap.new` files present (`snapshot_nestjs_graph__nestjs_graph.snap.new`, `…_openapi.snap.new`) are STALE insta artifacts from earlier red runs (the graph `.snap.new` shows a `go/packages load error` from BEFORE language dispatch existed — it is obsolete). Clean them when the tests turn green. (b) `tsextract/node_modules/typescript` is a vendored dependency — see the vendoring decision below; it is `.gitignore`'d under the existing `node_modules` rule? **NO — `.gitignore` does NOT list `node_modules`** (only `target`, `*.snap.new`). The planner MUST add `tsextract/node_modules` to `.gitignore` OR commit it (decision below). | clean `*.snap.new`; decide+implement node_modules vendoring; update `.gitignore` |
| **Host-side dispatch seams (code edit — NOT new files)** | `analyze::build_graph` (mod.rs:129) AND `diagnostics::collect` (diagnostics/mod.rs:37) BOTH have a 2-arm `match detect_language` (`Python`/`Go`) that must become 3-arm with a `Lang::TypeScript => run_tsextract` arm. `detect_language` itself (mod.rs:48) is a 2-boolean decision that must become a 3-marker decision. `helper.rs` has only `goextract_dir`/`pyextract_dir` + their drivers; add `tsextract_dir` + `run_tsextract`. | Edit `detect_language` (3-way), add the `TypeScript` arm in BOTH seams, add `run_tsextract`/`tsextract_dir`, add the error variant. |

**The key question — after every file is updated, what still ignores TypeScript?** `detect_language` (2 booleans), `build_graph` (2 arms), `diagnostics::collect` (2 arms). These are the load-bearing edits; the snapshots cannot turn green until `build_graph` routes the nestjs target to `run_tsextract`. (Note: `diagnostics::collect` has no nestjs diagnostics-text snapshot this phase since `diagnostics: []`, but it MUST still get the arm for consistency and to avoid a `Config` error if exercised.)

### node_modules vendoring decision (OPEN — recommend below; pin at plan time)
The `.gitignore` currently lists only `target` and `*.snap.new` — it does NOT ignore `node_modules`. Three options:
1. **Commit `tsextract/node_modules/typescript` (vendor into git).** Pro: fully hermetic, no network in `make check`, deterministic. Con: +23M in the repo (one package, no transitive deps — manageable); some teams dislike committed deps. Aligns with CLAUDE.md's "stand on our own legs" ethos and the "no network in test" goal.
2. **`.gitignore tsextract/node_modules` + `npm ci` in `make check`/CI before the nestjs tests.** Pro: clean repo. Con: needs network at test time (the sandbox HAS network — verified — but CI hermeticity weakens); add a `tsextract-install` Make target gated before the nestjs gate.
3. **`.gitignore` + a one-time `npm install` documented as a prerequisite** (like the Go toolchain). Pro: simplest. Con: a fresh checkout's nestjs tests fail until the dev runs install — but the **skip-if-toolchain-absent** guard (below) turns this into a graceful skip, not a hard failure.
**Recommendation:** Option 1 (commit the single vendored `typescript`) for hermetic determinism and the "own our pipeline" ethos, OR Option 2 with a committed `package-lock.json` + an `npm ci` Make step if repo size is a concern. EITHER WAY, the nestjs snapshot tests MUST **skip-if-(node OR typescript)-absent** (mirror the Go-toolchain skip) so a node-less / not-yet-installed env never hard-fails `make check`. Flagged in Open Questions.

## Common Pitfalls

### Pitfall 1: `strictNullChecks` MUST be on, or `| null`/`| undefined` arms vanish
**What goes wrong:** Without `strictNullChecks`, `string | null` resolves to just `string` and the `nullable` axis is lost; `field?: T` may not surface the `undefined` arm. Every nullable field in the snapshot diffs.
**Why it happens:** TS collapses `null`/`undefined` into every type when `strictNullChecks` is off.
**How to avoid:** Build the Program with `{ strictNullChecks: true, experimentalDecorators: true }` (and ideally `strict: true`). VERIFIED: under `strictNullChecks: true`, `bio: string | null` → `string | null`, `rating?: number | null` → `number | null | undefined`, `published: number | null` → `number | null`. Do NOT depend on the fixture's own `tsconfig.json` (it may not exist or differ) — synthesize the CompilerOptions in the sidecar so the analysis is deterministic regardless of the target's config.

### Pitfall 2: TS `number` → float64, not int (every numeric field/param)
**What goes wrong:** Emitting `{"prim":"int",...}` for `id: number`, `bookId: number`, `total: number`.
**Why it happens:** Intuition says "id is an integer."
**How to avoid:** TS `number` is IEEE double. The snapshot maps EVERY `number` to `{"prim":"float","bits":64}` — fields (`id`, `total`, `published`, `rating`) and path params (`bookId`). Hardcode `number → float64`. In OpenAPI this lowers to `type: number` (snapshot `bookId` → `type: number`).

### Pitfall 3: optional vs nullable are INDEPENDENT axes (the four-way matrix) — strip arms to compute them
**What goes wrong:** Conflating `?` with `| null`, or treating the stripped arm as a union member.
**How to avoid:** VERIFIED rules against the fixture + snapshot:
- `optional = true` iff the property has a `questionToken` (`field?:`) OR (for query params) a default/`?`. The resolved type carries a `| undefined` arm — strip it.
- `nullable = true` iff the type admits `null` (`T | null`) — strip the `null` arm.
- `required = !optional` in the snapshot (note `nullable` does NOT affect `required` — `published: number | null` with no `?` is `required:true, optional:false, nullable:true`).
- Worked rows (BookFilters, VERIFIED): `genre: string` (req,!opt,!null); `inStock?: boolean` (!req,opt,!null); `published: number | null` (req,!opt,null); `sort?: SortOrder | null` (!req,opt,null).
- Worked rows (BookDto): `tags?: string[]` (!req,opt,!null); `rating?: number | null` (!req,opt,null) → single float64 after stripping BOTH arms.

### Pitfall 4: named vs inline enum — `format` is `named`, `sort` is inline (the subtle one)
**What goes wrong:** Treating all string-literal-union aliases the same.
**Why it happens:** Both `BookFormat` and `SortOrder` are `type X = 'a' | 'b'` aliases, but the snapshot renders them differently.
**VERIFIED snapshot facts:**
- `BookDto.format: BookFormat` → `{"type":"named","of":"src/books.dto.BookFormat"}` (a **named ref**), and `BookFormat` is a **separate enum schema** `["hardcover","paperback"]`.
- `getBook` query param `fmt?: BookFormat` → also `named` ref `src/books.dto.BookFormat`.
- `BookFilters.sort?: SortOrder | null` → **inline** `{"type":"enum","of":["asc","desc"]}` with `optional:true, nullable:true` — NOT a named ref, and `SortOrder` does NOT appear as a separate schema.
**The reconciliation (likely rule, must be pinned at plan time — Open Question 1):** when a type-alias appears as the *sole* type of a property/param (`format: BookFormat`, `fmt?: BookFormat`), it is emitted as a `named` ref + its own schema; when the alias appears with a stripped `null`/`undefined` arm such that the alias's `aliasSymbol` is lost on the residual `Type` (`sort?: SortOrder | null` → after stripping, the residual is the bare literal union whose `aliasSymbol` may be absent), it inlines as an `enum`. The probe confirmed `sort` resolves to `'asc' | 'desc' | null | undefined` (4 arms, with the literals directly present, NOT carrying `SortOrder` aliasSymbol on the residual). **Pin the exact rule against the snapshot at plan time and add a golden test for both `format` (named) and `sort` (inline).**
**How to avoid:** Check `type.aliasSymbol` on the *residual* type (after stripping null/undefined). If present → `named` ref + emit schema. If absent (a bare literal union) → inline `enum` (sorted). Verify both fixture cases reproduce the snapshot exactly.

### Pitfall 5: union-of-OBJECTS → `Type::Union` of `named`; union-of-literals → `Type::Enum`
**What goes wrong:** Rendering `BookOrError = BookDto | OutOfStockDto` as an enum, or sorting union members.
**VERIFIED:** `BookOrError` (alias `BookDto | OutOfStockDto`) → a **schema** whose body is `{"type":"union","of":[{"named":BookDto},{"named":OutOfStockDto}]}` (id `src/books.dto.BookOrError`), referenced by `getBook`'s 200 response. Union members keep **source order** (BookDto then OutOfStockDto — NOT sorted). A string-literal union → `Type::Enum` (sorted); an object union → `Type::Union` (source order). The discriminator is whether the arms are string literals (→enum) or named object types (→union).

### Pitfall 6: TS 5.x AST — use `ts.getDecorators`, and pass the SourceFile to `getStart`/`getText`
**What goes wrong:** `node.decorators` is `undefined` (the field moved behind `ts.getDecorators(node)` in TS 4.8+/5.x); and `node.getStart()` / `node.getText()` throw `Cannot read properties of undefined (reading 'text')` when called on certain nodes without the SourceFile arg.
**Why it happens:** EMPIRICALLY HIT during research: `decorator.getStart()` and `identifier.getText()` threw until the SourceFile was passed (`node.getStart(sf)`, `node.getText(sf)`). Iterating via `program.getSourceFiles()` and using `node.name.escapedText` / `getLineAndCharacterOfPosition(node.getStart(sf))` worked reliably.
**How to avoid:** Always `ts.getDecorators(node)` (with `ts.canHaveDecorators(node)` guard) and always pass `sf` to position/text APIs. Use `node.name.escapedText` for identifiers where possible (no SourceFile needed).

### Pitfall 7: `build_graph` AND `diagnostics::collect` both need the TS arm; `detect_language` is 2-boolean today
**What goes wrong:** Adding the `Lang::TypeScript` arm in only one seam, or a non-exhaustive `match` (compile error — `Lang` is a closed enum, which is the point).
**How to avoid:** Extend `Lang` with `TypeScript`; both `match detect_language(...)?` sites (mod.rs:129, diagnostics/mod.rs:37) get a `Lang::TypeScript => helper::run_tsextract(&target)?` arm (exhaustiveness forces it). Extend `scan_markers` to record a TS marker (`tsconfig.json` OR `*.ts`) and `detect_language` to decide among 3 markers — a tree with markers for >1 language → typed `Config` error naming the ambiguity (extend the existing Go/Python ambiguity handling to three; e.g. count how many languages are present, ambiguous if >1). ⚠️ The nestjs fixture has a `package.json` but NO `tsconfig.json` and HAS `*.ts` — so the marker MUST include `*.ts` (a `tsconfig.json`-only marker would miss it). It has no `.go`/`.py`, so it classifies cleanly as TypeScript.

### Pitfall 8: fixture line reconciliation — snapshot spans do NOT match current fixture lines
**What goes wrong:** The graph snapshot asserts span line numbers that do NOT match the fixture's current source (large, non-uniform offsets). The graph snapshot will diff on every `provenance` block until the fixture is reconciled.
**VERIFIED offsets (AST line → snapshot-asserted line):**
- Controller operations (anchor candidate vs snapshot): listBooks name=40→**41**, createBook name=50→**51**, getBook decorator=57→**57**, updateBook decorator=66→**65**. Params: genre 41→42, sort 42→43, cursor 43→44, book(body) — , bookId(get) 59→58, fmt 60→59, bookId(put) 68→66. The deltas are inconsistent (+1, +1, ~0, −2) → the **fixture must be edited** so one consistent anchor rule lands each on its snapshot line.
- DTO schemas (class/alias AST line → snapshot line): `BookFormat` 21→**36** (+15), `AuthorDto` 29→**41** (+12), `BookDto` 37→**47** (+10), `BookFilters` 48→**56** (+8), `OutOfStockDto` 60→**70** (+10), `BookOrError` 65→**73** (+8), `CreatedMessage` 67→**75** (+8), `ListBooksResponse` 72→**80** (+8).
**How to avoid (the Phase-2 resolution, Q2):** Pick ONE consistent anchor convention (e.g. operation = method-name line; param = param-name line; schema = class/type-alias-name line), then EDIT the fixture source — inserting blank lines and non-fact comments ONLY (rule 1: never add a swagger/zod/class-validator annotation or any API-fact-bearing construct) — so every honest AST anchor lands on the snapshot's asserted line. Add a golden test asserting `produced line == snapshot line`. The snapshot is authoritative for shape; correcting a snapshot line is the documented fallback ONLY if a line is genuinely unreachable (it should not be — there is ample freedom to insert blank lines). This is exactly what Phase 2 did for FastAPI/Flask.
**Warning signs:** every `provenance:` block in the graph snapshot diffs; the OpenAPI snapshot (which has no line numbers) may pass while the graph snapshot fails.

### Pitfall 9: the OpenAPI snapshot uses `title: bookstore` + `base_path: /books` from the TEST, not the sidecar
**What goes wrong:** Expecting the sidecar to emit the title/base_path.
**How to avoid:** The `snapshot_nestjs_openapi` harness calls `to_openapi(&graph, "bookstore", "/books", &fixture_security())` — the title, base_path, and the `ApiKeyAuth`/`X-API-Key` security are supplied by the TEST (code-as-config, rule 4), NOT scraped. The sidecar emits `base_path: /` and group-relative paths; the test injects `/books`. Do not touch this.

## Code Examples

### Building the Program + walking decorators (verified shape)
```javascript
// Source: empirically verified against fixtures/nestjs-bookstore (typescript 5.9.3, Node v24.14.1)
const ts = require('typescript');
const files = discoverTsFiles(targetDir);                // *.ts under target, sorted
const program = ts.createProgram(files, {
  target: ts.ScriptTarget.ES2020,
  experimentalDecorators: true,                          // REQUIRED for NestJS decorators
  strictNullChecks: true,                                // REQUIRED so | null / | undefined survive (Pitfall 1)
  skipLibCheck: true,
});
const checker = program.getTypeChecker();

function decoratorsOf(node) {
  return (ts.canHaveDecorators(node) ? ts.getDecorators(node) : undefined) || [];
}
// A method decorator @Get('/') is a Decorator wrapping a CallExpression:
for (const d of decoratorsOf(method)) {
  const e = d.expression;
  if (ts.isCallExpression(e)) {
    const name = e.expression.getText(sf);               // "Get" | "Post" | ... | "Param" | "Query" | "Body"
    const args = e.arguments.map(a => a.getText(sf));     // ["'/'"] etc. (strip quotes)
  }
}
```

### TS Type → neutral Type (strip axes, the core rule — verified)
```javascript
// Source: verified mapping against the committed snapshot
function mapType(tsType, checker) {
  let optional = false, nullable = false;
  let arms = (tsType.isUnion && tsType.isUnion()) ? [...tsType.types] : [tsType];
  arms = arms.filter(a => {
    if (a.flags & ts.TypeFlags.Undefined) { optional = true; return false; } // strip | undefined
    if (a.flags & ts.TypeFlags.Null)      { nullable = true; return false; } // strip | null
    return true;
  });
  // residual: if 1 arm -> that type; if >1 string-literal arms -> enum; if >1 object arms -> union
  // number -> {"prim":"float","bits":64}; string -> {"prim":"string"}; boolean -> {"prim":"bool"}
  // residual carrying aliasSymbol (e.g. BookFormat) -> {"type":"named","of":id} + emit schema
  // ... (see Pattern 3 / Pitfall 4 for the named-vs-inline reconciliation)
  return { schema, optional, nullable };
}
```

### Rust seam: 3-way dispatch + tsextract driver (sketch)
```rust
// Source: design mirroring analyze/helper.rs run_pyextract + analyze/mod.rs build_graph
// analyze/mod.rs:
pub(crate) enum Lang { Go, Python, TypeScript }      // add the variant

pub(crate) fn detect_language(target_dir: &str) -> Result<Lang, CoreError> {
    let (mut go, mut py, mut ts) = (false, false, false);
    scan_markers(Path::new(target_dir), &mut go, &mut py, &mut ts); // ts: tsconfig.json OR *.ts
    let n = [go, py, ts].iter().filter(|b| **b).count();
    match (n, go, py, ts) {
        (1, true, _, _) => Ok(Lang::Go),
        (1, _, true, _) => Ok(Lang::Python),
        (1, _, _, true) => Ok(Lang::TypeScript),
        (0, ..) => Err(CoreError::Config { message: "no Go/Python/TypeScript source …".into() }),
        _       => Err(CoreError::Config { message: "ambiguous: multiple languages …".into() }),
    }
}
// build_graph AND diagnostics::collect:  add  Lang::TypeScript => helper::run_tsextract(&target)?,

// helper.rs:
pub(crate) fn tsextract_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tsextract"))  // dir holding index.js + node_modules
}
pub(crate) fn run_tsextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let output = Command::new("node")
        .args(["index.js", target_dir])                  // discrete args; no shell (T-02-01)
        .current_dir(tsextract_dir())
        .output()
        .map_err(|source| CoreError::TypeScriptToolchainMissing { source })?; // NEW variant
    if !output.status.success() { /* HelperExit { code, stderr } */ }
    serde_json::from_slice(&output.stdout).map_err(|source| CoreError::FactsParse { source })
}
```

### `NestJs` Source built-in (clone of FastApi — Code-as-config, rule 4)
```rust
// Source: verbatim clone of sdk/builtins.rs FastApi, differing only in the proper noun in errors.
// Calls the SAME crate::analyze::build_graph — language is detected from the target, not the Source.
pub struct NestJs { inputs: Vec<String> }
impl Source for NestJs {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => return Err(CoreError::Config { message: "NestJs source has no inputs …".into() }),
            many => return Err(CoreError::Config { message: format!("NestJs source lists {} inputs …", many.len()) }),
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph(&resolved.to_string_lossy())
    }
}
```

### Skip-if-toolchain-absent test guard (mirror the Go-toolchain skip)
```rust
// Source: the nestjs snapshot tests must SKIP (not fail) when node or vendored typescript is absent,
// exactly as the Go fixture tests skip when `go` is absent — so make check stays green on a node-less box.
// Probe: Command::new("node").arg("--version").output().is_ok() AND tsextract/node_modules/typescript exists.
// If absent: eprintln a skip note and `return;` BEFORE build_graph (do not assert).
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `build_graph`/`collect` dispatch Go vs Python (2-way) | dispatch Go vs Python vs TypeScript (3-way) | This phase | The single seam serves three languages; the harness is unchanged |
| `Lang` is a 2-variant enum | `Lang` adds `TypeScript`; `scan_markers` tracks a 3rd marker | This phase | Exhaustiveness forces both seams to handle TS |
| `node.decorators` (TS ≤4.7) | `ts.getDecorators(node)` (TS 4.8+/5.x) | TS 4.8 (2022); confirmed in 5.9.3 | The sidecar must use the helper, not the removed AST field (Pitfall 6) |
| TypeScript services unsupported | NestJS (class-DTO envelope) extracted statically via the carve-out | This phase | TSSRC-01..04 satisfied |

**Deprecated/outdated:**
- `node.decorators` / `node.modifiers` direct AST access — replaced by `ts.getDecorators` / `ts.getModifiers` in TS 4.8+. Use the helpers.
- Any notion of reading `@nestjs/swagger` runtime schema, `class-validator` metadata, or emitted OpenAPI — permanently out (rule 1, REQUIREMENTS "Out of Scope").

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | TS `number` → `{"prim":"float","bits":64}` for ALL numeric fields/params | Pitfall 2 | LOW — VERIFIED against every numeric value in the snapshot (id, total, published, rating, bookId). |
| A2 | The optional/nullable axes are computed by stripping `\| undefined` / `\| null` arms; the residual is the schema (e.g. `rating?: number\|null` → single float64, NOT a union) | Pattern 3 / Pitfall 3 | LOW — VERIFIED via Compiler-API probe + snapshot cross-check. |
| A3 | `format: BookFormat` → `named` ref + separate enum schema; `sort?: SortOrder\|null` → inline `enum` (no separate schema) | Pitfall 4 | **MEDIUM** — both snapshot facts are VERIFIED, but the *general rule* distinguishing them (aliasSymbol on the residual type) needs pinning + a golden test for both cases. See Open Question 1. |
| A4 | Status code is method-derived: typed POST → 201, else → 200 (override by `@HttpCode()` if present) | Pattern 5 | LOW — VERIFIED against the snapshot (createBook=201, rest=200); identical to the Phase-2 Flask resolution (Q1 resolved there). No `@HttpCode()` in the fixture. |
| A5 | Schema id = `src/<relpath>.<Name>` (slash form kept, `.ts` dropped, `.Name` appended); `module = basename(target_dir)` = `nestjs-bookstore` | Pattern 4 | LOW — VERIFIED directly from the snapshot ids + module value. |
| A6 | The fixture source must be EDITED (insert blank lines / non-fact comments) to reconcile spans to the snapshot's asserted lines; the snapshot is authoritative | Pitfall 8 | LOW (mechanism) / MEDIUM (exact final positions) — the offsets are VERIFIED; the exact edits are an iterate-to-green task (the Phase-2 Q2 resolution). |
| A7 | `tsconfig.json` OR `*.ts` is the TS detection marker; the nestjs fixture (no tsconfig, has `*.ts`) classifies as TypeScript | Pitfall 7 | LOW — VERIFIED the fixture has `*.ts`, no `.go`/`.py`; marker MUST include `*.ts` (tsconfig-only would miss it). |
| A8 | Plain-JS sidecar invoked as `node index.js <target>` (no TS compile step at runtime) | Standard Stack | LOW — CONTEXT recommends the simplest path needing only `typescript` at runtime; plain JS achieves it. |
| A9 | node_modules is vendored (committed) OR installed via `npm ci` in `make check`; tests skip-if-toolchain-absent | Runtime State Inventory | **MEDIUM** — a packaging decision (Open Question 2). The skip guard de-risks a fresh checkout regardless. |
| A10 | Build CompilerOptions in the sidecar (`strictNullChecks` + `experimentalDecorators`) rather than reading the target's tsconfig | Pitfall 1 | LOW — VERIFIED the axis behavior depends on `strictNullChecks`; synthesizing options makes analysis deterministic regardless of the target. |

## Open Questions

1. **Named-vs-inline enum rule (`format` → named, `sort` → inline)**
   - What we know: VERIFIED — `BookDto.format: BookFormat` and `getBook` param `fmt?: BookFormat` are `named` refs to a separate `BookFormat` enum schema; `BookFilters.sort?: SortOrder | null` is an **inline** `enum` with no separate `SortOrder` schema.
   - What's unclear: the precise predicate that decides named-vs-inline. The probe shows `format` retains `aliasSymbol === BookFormat`, while `sort`'s residual (after stripping null/undefined) is a bare `'asc'|'desc'` literal union whose aliasSymbol is not carried. The likely rule: "if the residual `Type` has an `aliasSymbol` → named ref + schema; else inline enum."
   - Recommendation: Pin this predicate at plan time by checking `aliasSymbol` on the *residual* type and add a golden test asserting BOTH `format` (named) and `sort` (inline) reproduce the snapshot. The snapshot is authoritative.

2. **`typescript` vendoring strategy (node_modules) + determinism/CI**
   - What we know: `.gitignore` does NOT currently ignore `node_modules`; `npm install typescript@5` works in-sandbox (network available) → 5.9.3, 23M, zero transitive deps. `make check` must stay green and as hermetic as possible.
   - What's unclear: commit the vendored `typescript` (Option 1, hermetic, +23M) vs `.gitignore` + `npm ci` in a Make step (Option 2, needs network at test time) vs documented prerequisite install (Option 3).
   - Recommendation: Option 1 (commit the single vendored `typescript`) for hermetic determinism and the "own our pipeline" ethos, with a committed `package-lock.json`; OR Option 2 with an `npm ci` Make target if repo size is a concern. EITHER WAY add the **skip-if-(node|typescript)-absent** guard to the nestjs tests (mirror the Go-toolchain skip) and pin `typescript` to an exact version. Update `.gitignore` accordingly. Decide at plan time.

3. **Exact fixture span reconciliation positions**
   - What we know: VERIFIED the offsets between AST lines and snapshot-asserted lines (Pitfall 8); the mechanism (insert blank lines / non-fact comments) is the resolved Phase-2 Q2 approach.
   - What's unclear: the precise per-line edits to land every anchor — an iterate-to-green task, not a derivation.
   - Recommendation: pick one anchor convention, edit the fixture, run the graph snapshot test, adjust until green; add a golden line-assertion test. Never edit the snapshot (rule: zero snapshot edits).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `node` (Node.js runtime) | tsextract sidecar | ✓ | v24.14.1 [VERIFIED] | none — `TypeScriptToolchainMissing`/`NodeToolchainMissing` error + test skip-guard |
| `npm` | one-time `typescript` install | ✓ | 11.11.0 [VERIFIED] | none needed (install runs once) |
| `typescript` (Compiler API) | tsextract type resolution | ✓ (installs cleanly; network available) | 5.9.3 [VERIFIED: installed in-sandbox] | none — vendor it (the carve-out); test skip-guard if absent |
| network (for `npm install typescript`) | vendoring step | ✓ [VERIFIED: install succeeded] | — | vendor into git (Option 1) removes the test-time network need |
| Rust + cargo + insta | host build/test | ✓ (Phase-1/2/3 green) | — | — |
| `go` toolchain | existing goextract tests | ✗ on this sandbox PATH (Go installed elsewhere; see MEMORY sandbox-toolchains) | — | Go fixture tests skip gracefully when `go` is absent (existing pattern to mirror for node) |
| `@nestjs/common` / nest packages | NOT required (never installed/imported) | n/a | — | n/a — static type analysis only; the fixture's `package.json` is documentation |

**Missing dependencies with no fallback:** none. Node + npm are present; `typescript` installs cleanly.
**Missing dependencies with fallback:** `go` is not on the sandbox PATH, which is irrelevant to this phase except that it confirms the **skip-if-toolchain-absent** test pattern is already a project norm — apply it to the node/typescript guard so `make check` never hard-fails on a box lacking the new toolchain.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` + `insta` snapshot tests (host); the Node sidecar testable via direct `node tsextract/index.js <fixture>` → committed golden JSON |
| Config file | `crates/gnr8-core/Cargo.toml`; insta snapshots under `crates/gnr8-core/tests/snapshots/`; `Makefile` `check`/`gates`/`red` targets |
| Quick run command | `cargo test -p gnr8-core --test snapshot_nestjs_graph -- --ignored` (and `--test snapshot_nestjs_openapi`) — remove `#[ignore]` as they turn green |
| Full suite command | `make check` (clippy `-D warnings` + tests; `CI=true` keeps `INSTA_UPDATE=no`) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TSSRC-01 | NestJS routes/params/bodies/responses/status → graph | snapshot | `cargo test -p gnr8-core --test snapshot_nestjs_graph` (remove `#[ignore]`) | ✅ committed snapshot (byte-exact spec) |
| TSSRC-01 | NestJS → OpenAPI 3.1 | snapshot | `cargo test -p gnr8-core --test snapshot_nestjs_openapi` | ✅ committed snapshot |
| TSSRC-02 | `typescript` Compiler API; facts from own TS types; no swagger/zod/class-validator | unit (Node) + snapshot | `node tsextract/index.js fixtures/nestjs-bookstore` → golden JSON; no swagger/zod/class-validator imported (assert) | ❌ Wave 0 (sidecar golden test) |
| TSSRC-03 | Untyped/out-of-envelope → diagnostics, no fallback | unit (Node) | sidecar diagnostic unit tests (the acceptance snapshot has `diagnostics: []`; taxonomy tested separately) | ❌ Wave 0 (sidecar diagnostic tests) |
| TSSRC-04 | `.gnr8/` `NestJs` Source built-in | unit (Rust) | `cargo test -p gnr8-core builtins` (new tests mirroring `FastApi` zero/many-input errors) | ❌ Wave 0 (builtins tests) |
| (dispatch) | `detect_language` 3-way; nestjs → TypeScript; mixed → Config error | unit (Rust) | `cargo test -p gnr8-core --lib analyze` (extend `detect_language` tests to TS + 3-way ambiguity) | ❌ Wave 0 (extend existing tests) |
| (toolchain) | node/typescript absent → typed error, tests skip | unit (Rust) | `run_tsextract_with("nonexistent-node", …)` → `TypeScriptToolchainMissing` (mirror the goextract test) | ❌ Wave 0 |
| (determinism) | byte-identical across two runs | integration | `cargo test -p gnr8-core --test determinism` (extend to the nestjs fixture) | ✅ test exists (Go/Py today) |
| (span lines) | produced span line == snapshot line | golden | a line-assertion test after fixture reconciliation | ❌ Wave 0 (Pitfall 8) |

### Sampling Rate
- **Per task commit:** the affected snapshot test (`--test snapshot_nestjs_graph` or `_openapi`) + `cargo clippy -D warnings`; for sidecar tasks, `node tsextract/index.js fixtures/nestjs-bookstore` vs golden.
- **Per wave merge:** both nestjs snapshot tests + the new Rust unit tests + determinism (extended) + the full existing Go/Python suite (no regression).
- **Phase gate:** `make check` fully green with both nestjs `#[ignore]` attributes removed; the 4 stale `*.snap.new` files cleaned; `tsextract/` lints clean; the toolchain skip-guard verified (tests skip, not fail, when node/typescript absent).

### Wave 0 Gaps
- [ ] `tsextract/` package (`package.json` pinning `typescript` only; `package-lock.json`) with a golden harness (`node tsextract/index.js <fixture>` → compare to a committed golden JSON, so the sidecar is testable without the Rust host).
- [ ] `CoreError::TypeScriptToolchainMissing` (or `NodeToolchainMissing`) variant + its `Display` (mirror `PythonToolchainMissing`) + a `run_tsextract` toolchain-missing test.
- [ ] Extend `Lang`/`scan_markers`/`detect_language` to 3 languages + new unit tests (TS classification, 3-way ambiguity → `Config`).
- [ ] Add the `Lang::TypeScript` arm in BOTH `analyze::build_graph` AND `diagnostics::collect`.
- [ ] `NestJs` Source built-in + tests (mirror `FastApi` zero/many-input errors).
- [ ] Fixture line reconciliation + a golden line-assertion test (Pitfall 8).
- [ ] Remove `#[ignore]` from `snapshot_nestjs_graph.rs` / `snapshot_nestjs_openapi.rs`; add the node/typescript skip-guard; update `Makefile` `red`/`gates` accordingly.
- [ ] node_modules vendoring decision implemented (commit vs `npm ci` Make step) + `.gitignore` updated.

## Security Domain

> `security_enforcement` not found as `false` in config — treated as enabled. The surface is narrow but real: a subprocess spawn + parsing untrusted source + ONE external dependency (the carve-out).

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth in the extractor |
| V3 Session Management | no | Stateless subprocess |
| V4 Access Control | no | — |
| V5 Input Validation | yes | Host deserializes sidecar JSON under `deny_unknown_fields` (rejects malformed/forward-incompatible output, T-01-05); the sidecar treats target source as untrusted *data* (parses via `ts.createProgram`, NEVER executes) |
| V6 Cryptography | no | None |
| V10 Malicious Code / Supply Chain | yes | The carve-out adds exactly ONE dependency (`typescript`, the canonical Microsoft compiler, zero transitive deps). Pin an exact version + commit a lockfile; no second npm dep; `gnr8-core` adds zero crates. Verify the install log shows no malicious postinstall (it does not). |
| V12 Files/Resources | yes | Sidecar only reads `*.ts` under the target dir; never writes; never follows the source into execution |

### Known Threat Patterns for {typescript-compiler-API sidecar + node subprocess seam}
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Arbitrary code execution via importing/transpiling-and-running target TS | Elevation of Privilege | **Static `Program`/`TypeChecker` only — NEVER `require`/`import`/`eval`/run the target** (TSSRC-03 static-only); the load-bearing security invariant |
| Shell/argument injection in subprocess spawn | Tampering | `Command::new("node").args([entry, target_dir])` — discrete args, no `sh -c`, no interpolation (mirrors goextract/pyextract T-02-01) |
| Malicious/typosquatted dependency in the carve-out | Malicious Code | Exactly ONE dep, the canonical `typescript`; pin exact version + committed lockfile; no transitive deps to audit; reject any second npm package as a rule-2 defect |
| Malformed sidecar output trusted blindly | Tampering | `serde_json` + `deny_unknown_fields`; drift hard-fails → typed `FactsParse`, never a partial graph |
| Toolchain absence → panic/DoS | Denial of Service | `TypeScriptToolchainMissing`/`NodeToolchainMissing` typed error; tests skip-guard; no `unwrap`/`expect`/`panic` in production (RUST-04) |
| Reading third-party schema annotations (swagger/zod/class-validator) | Tampering / Spoofing of facts | Bright line (rule 1): facts ONLY from the source's own TS types via the TypeChecker; never parse those decorators or consume emitted OpenAPI |

## Project Constraints (from CLAUDE.md)

These OVERRIDE convenience. The planner must verify every task against them:
- **Rule 1 — no coupling to another tool's conventions:** SCHEMA facts come ONLY from the source's own TS property types via the TypeChecker. FORBIDDEN: reading `@nestjs/swagger`, `zod`, `class-validator`, or any emitted/runtime OpenAPI. `@nestjs/common` routing decorators (`@Controller`/`@Get`/`@Param`/etc.) ARE read — they are NestJS's own routing constructs (the Gin analog), used for ROUTING facts only. The `@Controller('books')` prefix is NOT folded into the path.
- **Rule 2 — the documented carve-out:** `tsextract` may depend on `typescript` ONLY (the language's reference compiler — the single documented exception). `gnr8-core` adds ZERO new crates. NO second npm dependency. STRONGLY PREFER hand-rolled in-repo logic for everything `typescript` doesn't directly provide (the route recognition, the type→neutral mapping, the facts marshal). There is no approval path for a second OSS dep.
- **Rule 3 — no fallback / no dual control flow:** exactly ONE deterministic source per fact. Unresolvable/untyped/out-of-envelope → a diagnostic and the fact is OMITTED, never guessed, never "try A then B". Language dispatch is a single deterministic 3-way detection, not a try-each chain. Status code is one method-derived rule (POST→201/else 200, `@HttpCode()` overrides), not a fallback.
- **Rule 4 — config is code:** NestJS extraction is enabled via the `.gnr8/` `NestJs` `Source` built-in (Rust method calls), never a data file. Title/base_path/security are supplied by the pipeline (the test injects them), never scraped.
- **Determinism:** identical input ⇒ byte-identical output. Sort every array in the sidecar; the host re-sorts and relativizes. Pin `typescript` to an exact version (floating `^5` could change `typeToString` formatting). No production `unwrap`/`expect`/`panic` (RUST-04) — typed `CoreError` everywhere.
- **Known debt (do not worsen):** the compile-time-baked sidecar path (`goextract_dir`/`pyextract_dir`); `tsextract_dir` may carry the same debt forward but must not deepen it. `diagnostics::collect` is a redundant test-only seam — give it the TS arm for consistency but do not expand its role.

## Sources

### Primary (HIGH confidence)
- `crates/gnr8-core/src/analyze/facts.rs` — the frozen neutral facts DTO (every field name, tag, enum representation) — the exact JSON contract tsextract must emit.
- `crates/gnr8-core/tests/snapshots/snapshot_nestjs_graph__nestjs_graph.snap` — the byte-exact graph acceptance target (every fact, sort order, `float64` width, named-vs-inline enum, optional/nullable axis, span line). Read in full.
- `crates/gnr8-core/tests/snapshots/snapshot_nestjs_openapi__nestjs_openapi.snap` — the byte-exact OpenAPI 3.1 target (`type: number`, `oneOf`, `type: [T, null]`, enum lowering). Read in full.
- `crates/gnr8-core/tests/snapshot_nestjs_graph.rs` + `snapshot_nestjs_openapi.rs` — the `#[ignore]` red-by-design harness (calls `build_graph(FIXTURE_DIR)`; injects title/base_path/security in the OpenAPI test).
- `crates/gnr8-core/src/analyze/mod.rs` — `Lang`, `detect_language` (2-boolean, to extend to 3), `scan_markers`, `build_graph` (the 2-arm dispatch to extend).
- `crates/gnr8-core/src/diagnostics/mod.rs` — `collect` (the SECOND 2-arm dispatch site that also needs the TS arm).
- `crates/gnr8-core/src/analyze/helper.rs` — `run_pyextract`/`run_goextract`, `pyextract_dir`/`goextract_dir`, `resolve_target` — the driver to mirror as `run_tsextract`/`tsextract_dir`.
- `crates/gnr8-core/src/error.rs` — `PythonToolchainMissing` (to mirror), `HelperExit`, `FactsParse`, `Config`.
- `crates/gnr8-core/src/sdk/builtins.rs` — `FastApi`/`Flask`/`GoGin` Source (to clone as `NestJs`).
- `fixtures/nestjs-bookstore/src/{books.controller.ts,books.dto.ts}` + `package.json` — the static source whose constructs map to the snapshot facts. Read in full.
- `.planning/phases/02-python-source-pyextract/02-RESEARCH.md` — the prior source-extractor research (snapshot-as-spec, line-reconciliation Q2, method-derived-status Q1, dispatch pattern) reused here.
- **Empirical Compiler-API probe** against the real fixture: `npm install typescript@5` (→ 5.9.3), `ts.createProgram` + `getTypeChecker`, decorator/type resolution — every neutral-Type mapping, the `number→float64` width, the strip-null/undefined axis rule, `aliasSymbol` named-vs-inline behavior, and `ts.getDecorators`/`getStart(sf)` gotchas were VERIFIED via Bash on the sandbox.
- Sandbox toolchain: `node --version` → v24.14.1; `npm --version` → 11.11.0 [VERIFIED via Bash].
- `CLAUDE.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/phases/04-typescript-source-tsextract/04-CONTEXT.md`, `Makefile`, `.gitignore`.

### Secondary (MEDIUM confidence)
- TypeScript Compiler API behavior (Program/TypeChecker/Type flags, `isUnion`, `aliasSymbol`, `getNumberIndexType`, decorator AST) — cross-checked against the empirical probe output; the `ts.getDecorators` (TS 4.8+) and `getStart(sf)` requirements were directly observed (errors reproduced and fixed).

### Tertiary (LOW confidence)
- None. Every claim is grounded in committed repo files or the empirical in-sandbox probe; the two genuine unknowns (the exact named-vs-inline predicate; the node_modules vendoring choice) are raised as Open Questions, not asserted.

## Metadata

**Confidence breakdown:**
- Standard stack / carve-out: HIGH — `typescript` 5.9.3 install + load VERIFIED in-sandbox; the sole dependency by mandate.
- Architecture / seam: HIGH — the goextract/pyextract analogs, the facts DTO, `from_facts`, and BOTH dispatch sites read in full; the 3-way extension is concretely identified.
- Snapshot-as-spec mapping: HIGH — both snapshots read end-to-end AND the Compiler API was run against the real fixture to confirm every type mapping reproduces the snapshot.
- Named-vs-inline enum predicate (`format` vs `sort`): MEDIUM — both snapshot facts VERIFIED; the general rule needs a golden test to pin (Open Question 1).
- node_modules vendoring: MEDIUM — a packaging decision (Open Question 2); de-risked by the skip-if-absent test guard.
- Fixture line reconciliation: HIGH (mechanism, offsets measured) / MEDIUM (exact final edits — iterate-to-green).

**Research date:** 2026-06-25
**Valid until:** 2026-07-25 (stable — the facts contract is frozen by Phase 1 and the snapshots are committed; only the two open questions can move. `typescript` is fast-moving in general but pinned to an exact version here, so the mapping stays valid.)
