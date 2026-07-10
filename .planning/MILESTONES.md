# Milestones

## v3.0 Production-ready SDK adoption (Shipped: 2026-07-09)

**Phases completed:** 5 phases, 13 plans

Full detail: `.planning/milestones/v3.0-ROADMAP.md` · Requirements:
`.planning/milestones/v3.0-REQUIREMENTS.md` · Audit:
`.planning/milestones/v3.0-MILESTONE-AUDIT.md`

**Key accomplishments:**

- Introduced a shared SDK semantic planning layer so Go, Python, and TypeScript SDK emitters derive
  package, service/group, operation, schema, auth, error, runtime policy, docs metadata, and file-plan
  decisions from the same facts.
- Added graph-driven auth and typed API errors across OpenAPI and generated SDKs, with runtime smoke
  coverage for outgoing credentials and decoded declared error responses.
- Stabilized operation IDs, SDK method names, grouping selectors, `gnr8 compat` facts, `doctor --json`
  SDK readiness, generated package metadata, local package validation, and publishing recipes.
- Added production SDK runtime ergonomics: pagination helpers, client/per-request timeouts and retries,
  safe retry gating, idempotency-key preservation, and request/response/error hooks.
- Added operation documentation transforms for summaries, descriptions, tags, deprecation, named examples,
  response descriptions, and documented JSON error responses, propagated into OpenAPI and SDK docs.
- Added common request media support across generated SDKs for JSON, text, form-urlencoded, multipart, and
  binary uploads while keeping server stubs and older spec profiles out of scope.

---

## v2.0 Multi-language: TypeScript & Python (Shipped: 2026-06-26)

**Phases completed:** 6 phases, 19 plans, 43 tasks

**Key accomplishments:**

- Promoted the stringly-typed `SchemaType { kind: String }` to a closed, adjacently-tagged neutral `Type` enum (objects, arrays, enums, unions, maps, $ref, well-knowns, any) with independent optional/nullable axes, threaded byte-identically across the Rust serde facts DTO, the Go json-tag DTO, and the IR.
- SDK (`sdk/models.go`): one field changed.
- Authored three static, type-rich fixture services — FastAPI, Flask, and NestJS — that encode the v2.0 acceptance vocabulary (objects, arrays, cross-language enums, unions, all four optional×nullable combinations) in each language's OWN type system, plus six committed intended-green graph+OpenAPI snapshots that are RED by design (`#[ignore]`-gated, failing honestly at the `.expect()` because no extractor exists yet) so Phases 2/4 flip them green with zero snapshot edits; the red set is kept out of the green `make check` gate while remaining visible via `make red`.
- Language-aware `build_graph`/`collect` dispatch (one deterministic `detect_language`) plus the `python3 -m pyextract` subprocess driver, `PythonToolchainMissing` typed error, and `FastApi`/`Flask` Source built-ins — the host switch that routes a Python target through the SAME facts → ApiGraph → lowering path the Go sidecar uses, with zero new crates.
- A stdlib-only `pyextract/` sidecar that statically parses a Python service tree (`ast.parse`, never importing it), resolves names through an OWNED cross-module symbol table, maps Python annotations to the byte-exact neutral Type vocabulary with correct four-axis optional/nullable fields, and marshals a sorted, byte-stable facts JSON whose schema half reproduces `snapshot_fastapi_graph` — all proven by a `python3 -m unittest` harness that needs no Rust host.
- `pyextract/routes.py` recognizes the full FastAPI envelope — `@app`/`@router` decorator routes, group-relative paths with the `APIRouter(prefix=)` recorded separately (never folded, rule 1), path/query params from the typed signature, Pydantic/`@dataclass` request bodies, and `response_model=`/`status_code=` responses (including a `Union`-alias response) — and the two committed FastAPI snapshots (`snapshot_fastapi_graph`, `snapshot_fastapi_openapi`) flip from `#[ignore]` red to GREEN through REAL extraction with ZERO snapshot edits, after reconciling the fixture's source lines to the snapshot's asserted anchors and teaching the reused lowering to emit a union-bodied named component.
- `pyextract/routes.py` now recognizes the HONEST Flask typed envelope — `@bp.route`/`@app.route` with `methods=` (one route per method), `Blueprint(url_prefix=)` recorded separately (never folded, rule 1), the `<int:order_id>` converter lowered to a `/{order_id}` int64 path param, method-derived status (typed POST→201, typed non-POST→200, a CODE fact never read from a docstring), and typed DTO request bodies / response refs — while every UNTYPED surface (raw `request.json`, unannotated `request.args.get(...)`, missing return annotation) emits an exact-string DIAGNOSTIC and OMITS the fact (rule 3, no fallback). Both committed Flask snapshots (`snapshot_flask_graph`, `snapshot_flask_openapi`) flip from `#[ignore]` red to GREEN through REAL extraction with ZERO snapshot edits, after reconciling the fixture so all route/param/schema spans and the three diagnostics anchor to the snapshot's exact lines (42/69/78).
- 1. [Rule 1 - Bug] Named non-object/non-enum schema body must emit a type alias, not a typed error
- 1. [Rule 1 - Bug] Generated templated-path f-string did not compile (SyntaxError) on Python 3.9-3.11
- `Lang::TypeScript` is now first-class end-to-end — a single deterministic 3-way `detect_language`, a typed `TypeScriptToolchainMissing` error, a discrete-arg `run_tsextract` node driver wired into both dispatch seams, a prelude-exported `NestJs` Source, and a hermetic `tsextract/` package (pinned + vendored `typescript`) that emits a valid empty facts envelope.
- The tsextract extractor CORE is live — a static-only `ts.Program` + `TypeChecker` loader, a TS-type to neutral-Type mapper that strips the optional/nullable axes and pins the named-vs-inline enum predicate empirically (`aliasSymbol` on the full pre-strip type), and a fixpoint schema collector that emits all 8 NestJS DTOs byte-correct (`OutOfStockDto` reached only through the `BookOrError` union arm, `SortOrder` correctly absent), proven green by node-native golden + type + schema unit tests.
- tsextract now recognizes the full NestJS routing envelope — `routes.js` walks the `@Controller` class + `@Get`/`@Post`/`@Put`/`@Patch`/`@Delete` verb decorators + `@Param`/`@Query`/`@Body` param decorators into RouteFacts (group-relative paths with the `@Controller` prefix never folded, `:name`->`{name}`, method-derived status, request_body/response refs that seed the transitive schema collection), the fixture is reconciled to the committed snapshot's asserted span lines via non-fact edits only, and BOTH nestjs snapshots (graph + OpenAPI) are GREEN through real Compiler-API extraction with ZERO snapshot edits, a node/typescript skip-guard, determinism twins, and a green `make check` gate — closing the NestJS source -> neutral IR -> OpenAPI path (TSSRC-01).
- Dependency-free IR->TypeScript SDK emitter: interface models, string-literal-union enums, a fetch-based Client with an injectable transport, and a typed ApiError extends Error — a structural clone of the pysdk twin with an exhaustive ts_type mapping and zero new crates.
- The rule-4 enablement seam: a `TsSdk` `Target` built-in (a verbatim PySdk clone wired to `crate::tssdk`) that a developer composes into a `.gnr8/` Pipeline as code, deriving the package via `sdk_package` and the base path via `ir.base_path` from the single sources of truth, with a typed Config error when unconfigured, an unsafe-name write guard, deterministic byte-identical output, and an `output_anchors` loop-safety anchor — re-exported from `sdk::prelude`.
- A hermetic `tsc --noEmit --strict --lib es2022,dom` acceptance test that generates the NestJS TS SDK, type-checks it against the vendored compiler (exit 0), grep-proves zero runtime deps, and — on first run — caught a real TS2304 codegen bug now fixed and wired into the green `make check` gate.
- Task 1 — pub source-toolchain API (gnr8-core):
- Two self-contained end-to-end examples (FastAPI→OpenAPI 3.1+Python SDK, NestJS→OpenAPI 3.1+TS SDK, both driven by a `.gnr8/` Rust Pipeline crate with REAL committed output) plus a `make examples-check` gate that proves byte-identical cross-language determinism across all three examples.
- `docs/USAGE.md` now documents the honest per-language source envelope — Gin (Go, full), FastAPI (Python, full), Flask (Python, typed-envelope second-class with its untyped-surface gaps stated plainly: `request.json`/`request.args`/missing-return → diagnostic, NEVER inferred), and NestJS (TypeScript, class-DTO scope; bare interfaces erased; never reads `@nestjs/swagger`/`zod`/`class-validator`) — with the Source/Target built-in tables extended (`FastApi`/`Flask`/`NestJs`, `PySdk`/`TsSdk`), the build/watch/doctor wording generalized to the source language, and the two new examples pointed to; `CLAUDE.md` records the JUST-un-vendored `typescript` as a REQUIRED USER TOOLCHAIN (`tsextract` borrows the user's own from the target project via `ts.js`, exactly as `goextract` uses `go` / `pyextract` uses `python3`) — gnr8 ships ZERO OSS so rule 2 holds LITERALLY (not a loosening), with `PROJECT.md` reworded to agree; gnr8-core is asserted to add zero NEW OSS deps (Cargo.toml byte-unchanged vs the v1 baseline; `cargo tree` adds nothing; the four debt crates stay tracked, not retired); WR-02/WR-04 are recorded as ROADMAP backlog 999.x; and `make check` (incl. the cross-language `examples-check`) is GREEN end-to-end — closing Phase 6 and v2.0.

---

## v1.0 PoC: Go to OpenAPI to Go SDK (Shipped: 2026-06-24)

**Phases completed:** 5 phases, 14 plans, 38 tasks

**Key accomplishments:**

- Two-crate Cargo workspace (gnr8-core lib + gnr8 clap CLI) with a thiserror typed-error boundary, five stubbed router-agnostic module seams returning NotYetImplemented, a clippy-clean six-command CLI that exits cleanly without panicking, and a committed PoC contract locking Go->OpenAPI->Go-SDK scope.
- 1. [Rule 3 - Blocking] Re-pinned gin in go.mod after `go mod tidy` pruned it
- Four insta contract tests (graph/openapi/sdk/diagnostics) that fail clearly today via a panicking `.expect()` on the NotYetImplemented gnr8-core seams, plus a Makefile and a GitHub Actions CI that reconcile RUST-03 (blocking gates green) with FIX-04 (contract suite visibly red) by splitting them into separate jobs — the red suite promoted to blocking in Phase 3.
- A go/packages-based `goextract` helper that emits a deterministic sorted JSON facts document (8 DTO schemas + TargetDirection enum + float64/free-form-map diagnostics) for the goalservice fixture, plus the Rust serde mirror, a typed-error subprocess driver, and the Makefile/CI gate.
- goextract now emits 4 Gin route facts (method, normalized path, handler, secured, request/response/param refs, source spans) recognized via go/types Info.Selections, with handler request/response inference (go/constant status mapping) and a swaggo doc-comment escape hatch (operationId goalUuidPut, aggregation Enums, ApiKeyAuth security, @Router override) merged code-primary — plus exactly the 3 untyped-query WARN diagnostics.
- Router-agnostic Rust `ApiGraph` built from the goextract facts with deterministic stable operation/schema ids and fully sorted serialization, `build_graph` + `diagnostics::collect` implemented, three `inspect` renderers (table + `--json`), and the two Phase-1 contract tests (`snapshot_graph` + `snapshot_diagnostics`) flipped GREEN with reviewed `.snap` files plus an end-to-end determinism test — `snapshot_openapi`/`snapshot_sdk` stay red-by-design for Phase 3.
- `lower::to_openapi` lowers the Phase-2 ApiGraph into a valid OpenAPI 3.1.0 YAML document — typed Rust structs + a deterministic key-ordered hand-rolled YAML writer, absolute `/goal/...` paths joined from a const, free-form maps surfaced as `additionalProperties: true`, dangling `$ref`/unknown-kind as typed `CoreError::Lowering` — flipping `snapshot_openapi` GREEN.
- `sdk::generate` emits a deterministic, gofmt-clean Go SDK from the Phase-2 ApiGraph — a functional-options `Client`, tag-grouped `context.Context`-first operation methods with path/query/body handling and a typed `APIError`, and model structs with json tags/optional pointers/enum newtypes/uuid·time·float32 mapping — serialized as one file-marker-framed String that flips `snapshot_sdk` GREEN and `go build`s clean.
- 1. [Rule 3 - Blocking] `sdk::write_to_dir` was not callable from an out-of-crate integration test
- `gnr8 init` now idempotently scaffolds a `.gnr8/` workspace (checked-in `config.toml` + auto-written `.gitignore` ignoring `/cache/`) and a typed `toml`-backed `Config` reads the documented PoC knobs (inputs/output paths/go module/naming overrides) with `deny_unknown_fields`, plus four new lifecycle `CoreError` variants for 04-02/04-03.
- A blake3-hashed ownership manifest plus a PURE `plan_writes` truth table now drive `gnr8 generate`/`gnr8 check` through the real Phase-3 pipeline, delivering the two headline guarantees — no silent clobbering (a hand-edited generated file is warned + skipped unless `--force`) and no-op = no write (a byte-identical second generate touches zero files and zero mtimes) — with naming overrides that rename referenced types without dangling any `$ref`.
- Phase:
- `gnr8 doctor` read-only health aggregator (lifecycle + stale/drift + unsupported-pattern diagnostics, human report + `--json`, exit 0 healthy / 1 actionable with informational WARNs excluded) plus a hermetic `scripts/bench.sh` producing cold/warm-no-op/single-file-edit wall-clock numbers on a scratch fixture copy.
- `docs/demo.md` — a verified-reproducible fresh-checkout walkthrough (build → scratch-copy the fixture → init → generate → doctor → add one Go field → re-generate, with only the affected OpenAPI + SDK outputs updating, all real captured output) — plus `docs/evidence.md`, the HARD-03 milestone sign-off that captures `make check` GREEN live (exit 0) and maps every v1 requirement to the concrete file/test where it is satisfied.

---
