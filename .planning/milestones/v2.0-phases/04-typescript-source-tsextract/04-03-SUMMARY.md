---
phase: 04-typescript-source-tsextract
plan: 03
subsystem: api
tags: [typescript, tsextract, nestjs, routes, decorators, snapshots, determinism, acceptance]

# Dependency graph
requires:
  - phase: 04-typescript-source-tsextract (plan 01)
    provides: the Rust host seam (Lang::TypeScript, run_tsextract, NestJs Source), the tsextract skeleton + vendored typescript@5.9.3 (Option A, committed), the index.js stub
  - phase: 04-typescript-source-tsextract (plan 02)
    provides: tsextract extractor CORE (load/types/schemas/facts/diagnostics); the fixpoint Registry that follows named refs through fields AND union arms; the named-vs-inline discriminator
  - phase: 01-language-neutral-ir
    provides: the neutral facts contract (facts.rs RouteFact/ParamFact/ResponseFact, deny_unknown_fields) the sidecar JSON must satisfy
  - phase: 02-python-source-pyextract
    provides: pyextract/routes.py (the recognizer twin); the Phase-2 fixture-line-reconciliation approach + the snapshot-flip pattern (FastAPI/Flask removed from red)
provides:
  - "tsextract ROUTE recognition: routes.js recognizeNestController walks the @Controller class + @Get/@Post/@Put/@Patch/@Delete verb decorators + @Param/@Query/@Body param decorators -> RouteFacts (group-relative paths, :name->{name}, method-derived status, request_body/response refs)"
  - "the named-vs-inline discriminator HARDENED to the SYNTACTIC annotation node (a bare TypeReference to a type-alias -> named ref; a UnionType -> inline) — uniform for fields AND optional params, fixing fmt?: BookFormat (the 04-02 aliasSymbol-on-full-type discriminator failed it)"
  - "both nestjs snapshots (graph + openapi) GREEN through real Compiler-API extraction with ZERO snapshot edits; in the blocking make check gate; node/typescript skip-guard + determinism twins"
  - "the reconciled fixture: every op/param/schema span anchors to the committed snapshot's asserted line via blank-line / non-fact-comment edits only (rule 1); a golden line-assertion test locks the mapping"
affects: []  # FINAL wave of phase 04; phase ready for verification

# Tech tracking
tech-stack:
  added: []  # ZERO deps; gnr8-core Cargo.toml + Cargo.lock unchanged; tsextract sole dep stays typescript@5.9.3
  patterns:
    - "Recognize the routing envelope from @nestjs/common's framework-native decorators ONLY (rule 1); the @Controller('books') prefix is read for provenance and NEVER folded into op paths; request/response/param SHAPES come from the TypeChecker over the typed signature, never a schema-annotation dialect"
    - "Status is a SINGLE method-derived rule (typed POST -> 201, else 200), the @HttpCode(n) override read first; never a try-typed-then-fallback chain (rule 3)"
    - "Routes seed the schema Registry with every DTO referenced from a route param/body/response; buildSchemas drains that pre-seeded registry (plus the direct DTO roots) to a fixpoint, so the route roots drive the transitive collection"
    - "named-vs-inline discriminator derived from the AUTHOR's annotation node, not the resolved type (TS drops aliasSymbol once | null/| undefined is mixed in, so an optional alias param needs the syntactic source)"
    - "Fixture line reconciliation: pick one anchor convention (op=method-name line, param=param-name line, schema=decl line); insert blank lines / non-fact comments ONLY until each honest AST anchor lands on the snapshot's asserted line; ZERO snapshot edits"
    - "Shared test skip-guard module (tests/nestjs_toolchain/mod.rs): probe node + vendored typescript; absent -> eprintln + early return, mirroring the go-toolchain skip"

key-files:
  created:
    - tsextract/routes.js
    - tsextract/tests/routes.test.js
    - tsextract/tests/lines.test.js
    - crates/gnr8-core/tests/nestjs_toolchain/mod.rs
  modified:
    - tsextract/index.js
    - tsextract/schemas.js
    - tsextract/types.js
    - fixtures/nestjs-bookstore/src/books.controller.ts
    - fixtures/nestjs-bookstore/src/books.dto.ts
    - crates/gnr8-core/tests/snapshot_nestjs_graph.rs
    - crates/gnr8-core/tests/snapshot_nestjs_openapi.rs
    - crates/gnr8-core/tests/determinism.rs
    - crates/gnr8-core/src/analyze/helper.rs
    - Makefile

key-decisions:
  - "The named-vs-inline discriminator is the SYNTACTIC annotation node, NOT the resolved type's aliasSymbol. 04-02 pinned aliasSymbol-on-the-full-type, which works for fields (format keeps it; sort? loses it -> inline) but FAILS for the optional alias param fmt?: BookFormat: TS drops the aliasSymbol once | undefined is mixed in, so it inlined as an enum instead of the snapshot's required named ref. A bare TypeReference annotation -> named ref + schema; a UnionType annotation -> inline after stripping. One source (what the author wrote), one path (rule 3), uniform for fields AND params."
  - "Routes seed the schema Registry; buildSchemas accepts a pre-seeded registry and drains it (plus direct roots) to closure. Both the route-seeded path and the direct-root path converge to the same 8 schemas (the W2 invariant); the route seeding is the full-pipeline confirmation the plan asked for."
  - "Anchor convention: operation = method-name line; param = param-name line; schema = class/type-alias declaration line. The fixture is reconciled to the snapshot with blank-line/non-fact-comment edits only (rule 1) — ZERO snapshot edits; the committed .snap is the byte-exact spec."
  - "Vendoring stays Option A (committed typescript, from 04-01): no npm ci step; tests run offline. The skip-guard probes node + the vendored typescript package so a node-less box SKIPS make check rather than failing."
  - "The Makefile red target is retired to a no-op echo: all six multi-language acceptance snapshots (FastAPI/Flask/NestJS graph+openapi) are GREEN and in the blocking gates set; nothing remains #[ignore]d."

patterns-established:
  - "Harden a Compiler-API predicate against a case the prior wave's discriminator could not see (the optional alias param), moving the source of truth to the author's syntactic annotation — still one deterministic path"
  - "Route-seeded transitive schema collection: the recognizer enqueues route roots; the fixpoint follows them through fields/union arms, no core change"

requirements-completed: [TSSRC-01]

# Metrics
duration: 14min
completed: 2026-06-25
---

# Phase 4 Plan 03: NestJS Routes + Snapshot Acceptance Summary

**tsextract now recognizes the full NestJS routing envelope — `routes.js` walks the `@Controller` class + `@Get`/`@Post`/`@Put`/`@Patch`/`@Delete` verb decorators + `@Param`/`@Query`/`@Body` param decorators into RouteFacts (group-relative paths with the `@Controller` prefix never folded, `:name`->`{name}`, method-derived status, request_body/response refs that seed the transitive schema collection), the fixture is reconciled to the committed snapshot's asserted span lines via non-fact edits only, and BOTH nestjs snapshots (graph + OpenAPI) are GREEN through real Compiler-API extraction with ZERO snapshot edits, a node/typescript skip-guard, determinism twins, and a green `make check` gate — closing the NestJS source -> neutral IR -> OpenAPI path (TSSRC-01).**

## Performance

- **Duration:** ~14 min
- **Started:** 2026-06-25T22:41:14Z
- **Completed:** 2026-06-25T22:55:34Z
- **Tasks:** 3
- **Files:** 4 created + 10 modified

## Accomplishments

- **routes.js** — `recognizeNestController(loaded, diags, registry)`: for each `@Controller`-decorated class (recognized by NAME, rule 1), each method carrying an HTTP-verb decorator becomes a RouteFact. Verb map `Get->GET ... Delete->DELETE`; path = the decorator string arg with `:name`->`{name}` (a bare `/` stays `/`); the `@Controller('books')` prefix is read for provenance and **never folded**. `@Param`->path/required-true; `@Query`->query/required-from-`?`-or-default; `@Body`->`request_body` TypeRef (not a param). Response ref resolves the return type (object-union alias `BookOrError` via its aliasSymbol -> alias schema id; class -> class schema id), registering it for transitive collection. Status is method-derived (typed POST->201, else 200), `@HttpCode(n)` override read first (single rule, rule 3). All decorators via `ts.getDecorators` (Pitfall 6); spans via `getLineAndCharacterOfPosition(node.getStart(sf))`.
- **index.js** — wired `routes -> schemas -> marshal`: `recognizeNestController` runs first into a shared `Registry`, then `buildSchemas` drains it (plus direct roots) to a fixpoint, so the route roots drive the transitive collection. End-to-end: 4 routes + exactly 8 schemas + 0 diagnostics, byte-deterministic.
- **schemas.js** — `buildSchemas` now accepts an optional pre-seeded `registry`; the route-seeded path and the direct-root path converge to the same closure.
- **types.js** — `_annotationAliasSymbol`: the named-vs-inline discriminator now reads the SYNTACTIC annotation node (a bare `TypeReference` to a type-alias -> named ref; otherwise map the residual). Fixes `fmt?: BookFormat` (named), keeps `format` (named) and `sort?: SortOrder | null` (inline enum).
- **fixture reconciliation** — `books.controller.ts` + `books.dto.ts` edited with blank lines / non-fact comments only (rule 1) so every op (41/51/57/65), param (42/43/44/58/59/66) and schema (BookFormat 36, AuthorDto 41, BookDto 47, BookFilters 56, OutOfStockDto 70, BookOrError 73, CreatedMessage 75, ListBooksResponse 80) anchors to the committed snapshot line. ZERO snapshot edits.
- **tests** — `routes.test.js` (4 routes, verbs/paths/params/body+response refs, method-derived status, no `/books` folding); `lines.test.js` (golden: all 18 op/param/schema span anchors == snapshot lines).
- **snapshot flip** — `#[ignore]` removed from both `snapshot_nestjs_graph.rs` + `snapshot_nestjs_openapi.rs`; both assert against the committed `.snap` through real tsextract (openapi keeps the TEST-supplied title/base_path/`fixture_security`, rule 4). Shared `nestjs_toolchain` skip-guard (node spawns AND vendored typescript present); a node-less PATH SKIPS (verified), not fails.
- **determinism** — `determinism.rs` nestjs build_graph + to_openapi byte-identical twins (same skip-guard).
- **gate wiring** — Makefile `gates` target now includes the nestjs snapshots; `red` target retired to a no-op (all six acceptance snapshots green). `make check` is GREEN end-to-end (fmt-check, clippy `-D warnings`, full test, fixture-build, goextract-build) with go on PATH.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] named-vs-inline discriminator failed for the optional alias param `fmt?: BookFormat`**
- **Found during:** Task 2 (first graph snapshot run)
- **Issue:** The 04-02 discriminator (capture `aliasSymbol` on the FULL resolved type before stripping) works for fields but FAILS for an optional alias param: `fmt?: BookFormat` resolves to `BookFormat | undefined`, and TS drops the `aliasSymbol` once `| undefined` is mixed in — so the residual `'paperback' | 'hardcover'` literal union inlined as an `enum` instead of the snapshot's required `named` ref to `src/books.dto.BookFormat`. (The field `format: BookFormat` keeps its aliasSymbol because nothing is mixed in; the field `sort?: SortOrder | null` correctly inlines.)
- **Fix:** Derive the discriminator from the SYNTACTIC annotation node (`_annotationAliasSymbol`): a bare `TypeReference` to a type-alias -> named ref + register the alias; any other annotation (union/primitive/array) -> map the residual. This reads what the author wrote (one source, one path, rule 3) and is uniform for fields AND params: `format` named, `sort` inline, `fmt` named. The existing `golden.test.js` (field cases) still passes.
- **Files modified:** tsextract/types.js
- **Commit:** c8f2ac5

**2. [Rule 3 - Blocking] pre-existing rustfmt drift in helper.rs blocked the green fmt-check gate**
- **Found during:** Task 3 (`make check`)
- **Issue:** `helper.rs` (committed in 04-01) had a rustfmt drift on a test-only line; `make check`'s `fmt-check` failed before reaching the nestjs tests, so the green gate could not be confirmed. Out-of-task-origin, but it blocks the plan's "make check stays GREEN" success criterion.
- **Fix:** `cargo fmt --all` (the canonical format; only the 1 drifted line + the already-canonical new files). No behavior change.
- **Files modified:** crates/gnr8-core/src/analyze/helper.rs
- **Commit:** bb42d97

_Clippy lints on the NEW test code (`map_unwrap_or`->`is_ok_and`, `doc_markdown` allow on determinism.rs, `unreachable_pub`/`dead_code` allow on the shared `nestjs_toolchain` module) were fixed inline in the same Task-3 commit; these are normal in-task corrections, not deviations._

## Known Stubs

None — `routes: []` (the documented 04-02 stub) is now the real recognizer; the end-to-end run emits 4 routes + 8 schemas + 0 diagnostics.

## Threat Flags

None — no new security surface beyond the plan's threat register (the recognizer reads only @nestjs/common routing decorators; the sidecar remains static-only; the skip-guard prevents a node-less `make check` hard-fail).

## Issues Encountered

None beyond the two auto-fixed items above.

## User Setup Required

None — `node` (v24.14.1) and the vendored `typescript@5.9.3` (Option A, committed) are present; tests run offline. A node-less box SKIPS the nestjs tests rather than failing.

## Phase Readiness

- **Phase 04 is COMPLETE and ready for verification.** All six multi-language acceptance snapshots (FastAPI/Flask/NestJS x graph/openapi) are GREEN in the blocking `make check` gate; the red-by-design contract is fully retired.
- TSSRC-01 satisfied: a NestJS service extracts routes/params/request+response DTOs into the neutral IR via real Compiler-API extraction.
- Verifier checklist: `cargo test -p gnr8-core --test snapshot_nestjs_graph --test snapshot_nestjs_openapi` (green, no `#[ignore]`, zero snapshot edits); `node tsextract/index.js fixtures/nestjs-bookstore` (4 routes, 8 schemas, no `/books` folding, createBook 201 / others 200); `make check` green with go on PATH; bright-line + static-only grep gates clean; gnr8-core adds zero crates; tsextract sole dep stays typescript@5.9.3.

## Self-Check: PASSED

- Created files verified present: `tsextract/routes.js`, `tsextract/tests/routes.test.js`, `tsextract/tests/lines.test.js`, `crates/gnr8-core/tests/nestjs_toolchain/mod.rs`.
- Task commits verified in git history: `b9bf41a` (feat — routes), `c8f2ac5` (fix — reconcile + discriminator), `bb42d97` (test — snapshot flip + gate).
- `node tsextract/tests/{routes,lines,types,golden,schemas}.test.js` all exit 0; `node tsextract/index.js fixtures/nestjs-bookstore` emits 4 routes + 8 schemas + 0 diagnostics, byte-identical across two runs.
- `cargo test -p gnr8-core --test snapshot_nestjs_graph --test snapshot_nestjs_openapi --test determinism` green WITHOUT `--ignored`; `git diff --stat` on both nestjs `.snap` files is EMPTY (zero edits); skip-guard verified to SKIP (not fail) with node absent; `make check` GREEN end-to-end with go on PATH.

---
*Phase: 04-typescript-source-tsextract*
*Completed: 2026-06-25*
