---
phase: 02-go-analysis-and-api-graph
plan: 02
subsystem: api
tags: [go, go-ast, go-types, go-constant, gin, swaggo, annotations, rust, serde, json-contract]

# Dependency graph
requires:
  - phase: 02-go-analysis-and-api-graph (02-01)
    provides: "goextract module (go/packages LoadAllSyntax+NeedModule loader, diag accumulator, JSON facts schema incl. empty RouteFact/ParamFact/ResponseFact + Rust serde mirror, module-relative schema-id format internal/common/dto.X)"
provides:
  - "goextract Gin route recognizer (internal/routes): Group/METHOD/Use resolved via go/types Info.Selections gated on receiver pkg path github.com/gin-gonic/gin (alias-robust), group-prefix-relative paths with :param->{param} normalization, group Use(...) -> secured propagation, handler symbol + source span"
  - "goextract handler analyzer (internal/handlers): ShouldBindJSON->request_body TypeRef, JSON(http.StatusXxx, y)->responses by numeric status via go/constant, c.Param->path param, c.Query->query param; emits exactly 3 untyped-query WARN diagnostics + a defensive dynamic-response WARN path"
  - "goextract swaggo annotation parser (internal/handlers/annotations): @ID/@Summary/@Tags/@Security/@Router/@Param(Enums)/@Success/@Failure from FuncDecl.Doc, merged code-primary (operationId goalUuidPut, aggregation Enums set, ApiKeyAuth security, @Router override, 404 backfill)"
  - "Populated GoFacts.routes (4 fixture routes) with params/request/response/annotation facts; new RouteFact fields router_path + security_schemes mirrored in Rust serde DTOs (deny_unknown_fields in sync)"
affects: [02-03-apigraph-inspect, 03-openapi-sdk-generation]

# Tech tracking
tech-stack:
  added:
    - "go/constant (stdlib) — resolve http.StatusXxx const values to numeric status (no hardcoded name->number map)"
    - "go/types Info.Selections (stdlib) — semantic Gin method identity resolution (alias/version robust)"
  patterns:
    - "Route recognition gates on the RESOLVED receiver package path, never the import alias text (T-02-06)"
    - "Code inference is PRIMARY; swaggo annotations are the ESCAPE HATCH that fills gaps and adds metadata, never clobbering a code-resolved fact (TARGET-API.md thesis)"
    - "Lossy/unsupported patterns (untyped query params, dynamic responses) emit a diagnostic with file:line; never silently dropped, never panic (GO-06 / D-05)"
    - "Path storage: code-derived group-relative Path + authoritative @Router RouterPath override, both emitted so 02-03 renders the absolute /goal/... deterministically without folding the dynamic prefix"
    - "Every new Go JSON field is mirrored in the Rust serde DTO so deny_unknown_fields stays a drift detector"

key-files:
  created:
    - "goextract/internal/routes/routes.go"
    - "goextract/internal/routes/routes_test.go"
    - "goextract/internal/routes/testdata/aliasedgin/{go.mod,go.sum,app.go}"
    - "goextract/internal/handlers/handlers.go"
    - "goextract/internal/handlers/handlers_test.go"
    - "goextract/internal/handlers/annotations.go"
    - "goextract/internal/handlers/annotations_test.go"
  modified:
    - "goextract/main.go"
    - "goextract/internal/facts/facts.go"
    - "goextract/internal/diag/diag.go"
    - "crates/gnr8-core/src/analyze/facts.rs"

key-decisions:
  - "Path storage: RouteFact.Path holds the code-derived group-relative normalized template; new RouteFact.RouterPath holds the @Router annotation override. Both emitted (router-agnostic); 02-03 picks the rendered absolute path."
  - "Added two RouteFact fields (router_path: Option<String>, security_schemes: Vec<String>) to both Go json tags and the Rust serde mirror; existing empty-routes round-trip tests unaffected, two new populated-route round-trip tests added."
  - "Diagnostics keep a machine-stable rule + identity (param name + method + group-relative path); canonical per-line wording reconciliation with expected/diagnostics.txt stays a 02-03 (diagnostics::collect) concern, per RESEARCH Pitfall 4."
  - "Merge precedence: request_body/responses/params resolved from code win; annotations upgrade required-ness, add description/enums, fill missing responses (404 backfill), and supply operationId/summary/tags/security/@Router."
  - "Identity helpers (GinMethod/GinPkgPath/NormalizePath) exported from routes so the handler analyzer and annotation parser share the exact gate and path normalization."

patterns-established:
  - "Two-pass route recognition per file: pass 1 records Use(...)-secured group objects, pass 2 emits routes with security resolved regardless of source order"
  - "Annotation type refs (dto.X) resolved to module-relative schema ids via a selector->path map derived from loaded packages (AnnotationPackagesFromResult), not hardcoded fixture layout"
  - "aliasedgin testdata sub-module (own go.mod, skipped by parent go ./...) proves alias-robust recognition without perturbing goextract's go.mod/go.sum"

requirements-completed: [GO-04, GO-05, GO-06]

# Metrics
duration: 12min
completed: 2026-06-24
---

# Phase 2 Plan 02: Gin Route + Handler + Swaggo Annotation Extraction Summary

**goextract now emits 4 Gin route facts (method, normalized path, handler, secured, request/response/param refs, source spans) recognized via go/types Info.Selections, with handler request/response inference (go/constant status mapping) and a swaggo doc-comment escape hatch (operationId goalUuidPut, aggregation Enums, ApiKeyAuth security, @Router override) merged code-primary — plus exactly the 3 untyped-query WARN diagnostics.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-06-24T17:20:30Z
- **Completed:** 2026-06-24T17:32:36Z
- **Tasks:** 3
- **Files modified:** 13 (10 created, 3 modified Go/Rust + 1 facts schema)

## Accomplishments

- **Route recognition (Task 1):** `internal/routes` walks every target file's AST and recognizes `Group`/`GET`/`POST`/`PUT`/`DELETE`/`Use` by resolving the selector method's identity through `go/types` `Info.Selections` and gating on the receiver package path `github.com/gin-gonic/gin` — never the import alias (T-02-06). It records the 4 fixture routes group-relative (`/`, `/list`, `/{uuid}`), normalizes Gin `:uuid`→`{uuid}`, propagates `secured=true` from the group `Use(h.AuthMiddleware)`, and attaches a source span. An `aliasedgin` testdata sub-module (`import grouter "...gin"`) proves recognition survives aliasing.
- **Handler inference (Task 2):** `internal/handlers` matches each route's handler `FuncDecl` by symbol and walks the body for the gin-context calls (same Selections identity gate): `ShouldBindJSON(&x)` → `request_body` TypeRef; `JSON(http.StatusXxx, y)` → responses keyed by numeric status resolved via `go/constant` (201/400/200, not a hardcoded map); `Param`/`Query` → path/query params. It emits exactly the 3 untyped-query WARN diagnostics (cursor/page_size/aggregation) with file:line and has a defensive dynamic-response WARN path (not triggered by this fully-typed fixture).
- **Swaggo escape hatch (Task 3):** `internal/handlers/annotations` parses `@ID`/`@Summary`/`@Tags`/`@Security`/`@Router`/`@Param`(+`Enums(...)`)/`@Success`/`@Failure` from `FuncDecl.Doc` and merges them **code-primary**: `updateGoal` gets `operation_id=goalUuidPut`, summary, `ApiKeyAuth` security, the `@Router /{uuid}` override, and a 404 response backfilled (code only emitted 200/400); `listGoals` gets its 3 query params with required-ness and the `aggregation` `Enums` closed set `[avg,count,max,min,sum]`. Code-resolved response bodies are never clobbered.
- **Contract + determinism:** `routes` array now has 4 entries; the diagnostics set is float64×3 + free-form-map×1 + untyped-query×3 = 7 total (matching `expected/diagnostics.txt`'s structure). Two `go run` invocations are byte-identical. The Rust serde mirror gained `router_path` + `security_schemes` with two new populated-route round-trip tests; `cargo test -p gnr8-core` lib (12 pass), clippy `-D warnings`, and `fmt --check` are green.

## Task Commits

Each task was committed atomically:

1. **Task 1: Gin route recognizer (Selections + prefix chain + path normalization)** - `27c6d43` (feat)
2. **Task 2: Handler request/response/param inference + untyped-query diagnostics** - `18a8758` (feat)
3. **Task 3: Swaggo annotation parser (escape hatch) + code-primary merge** - `69282d0` (feat)

**Plan metadata:** committed separately with this SUMMARY + STATE + ROADMAP.

_Task tests and implementation landed in one commit each (Go unit tests co-developed against the live fixture)._

## Files Created/Modified

- `goextract/internal/routes/routes.go` - Gin recognizer: `GinMethod` (Info.Selections identity), two-pass Use→secured propagation, `:param`→`{param}` `NormalizePath`, handler symbol + span; exports `GinMethod`/`GinPkgPath`/`NormalizePath` for the handler/annotation packages
- `goextract/internal/routes/routes_test.go` - asserts 4 fixture routes (method/path/handler/secured), `:uuid`→`{uuid}`, and the aliased-import case
- `goextract/internal/routes/testdata/aliasedgin/{go.mod,go.sum,app.go}` - test-only module importing gin under the `grouter` alias to prove alias-robust recognition (own module; skipped by parent `go ./...`)
- `goextract/internal/handlers/handlers.go` - handler index by symbol, code inference (ShouldBindJSON/JSON-via-go/constant/Param/Query), `SetModule` for 02-01-compatible schema ids
- `goextract/internal/handlers/handlers_test.go` - request/response/param + go/constant status + 3 untyped-query diagnostics
- `goextract/internal/handlers/annotations.go` - swaggo `@...` parser + `MergeAnnotations` (code-primary), selector→module-relative path map (`AnnotationPackagesFromResult`)
- `goextract/internal/handlers/annotations_test.go` - operationId/security/@Router, aggregation Enums, 404 gap-fill without clobber, createGoal unaffected
- `goextract/main.go` - wires recognize → analyze → merge into sorted `RouteFact`s; sets module + annotation packages
- `goextract/internal/facts/facts.go` - `RouteFact.RouterPath` + `RouteFact.SecuritySchemes` fields; `sortRoute` stably orders params/responses/tags/schemes/enum_values (GRAPH-02)
- `goextract/internal/diag/diag.go` - `UntypedQueryParam` + `DynamicResponse` diagnostic helpers
- `crates/gnr8-core/src/analyze/facts.rs` - mirror `router_path`/`security_schemes`; two populated-route round-trip tests (accept new fields, reject unknown route key)

## Emitted Fact Shapes (the 02-03 contract)

- **RouteFact:** `method`, `path` (group-relative, normalized), **`router_path`** (Option, from `@Router`), `handler`, `operation_id` (Option, from `@ID`), `summary` (Option), `tags` ([]), `secured` (bool), **`security_schemes`** ([], from `@Security`), `params` ([]ParamFact), `request_body` (Option TypeRef), `responses` ([]ResponseFact), `span`.
- **ParamFact:** `name`, `location` ("path"|"query"), `required`, `schema` (SchemaType), `description` (Option), `enum_values` ([], sorted), `span`.
- **ResponseFact:** `status` (u16, from go/constant or `@Success`/`@Failure`), `body` (Option TypeRef), `description` (Option).
- **Path-storage decision:** `path` is always the code-derived group-relative template; `router_path` is the authoritative `@Router` override. 02-03 joins the `/goal` base (from the fixture call / annotation) and renders the absolute path.
- **Merge precedence:** code wins for request/response/params; annotations fill gaps + add `operation_id`/`summary`/`tags`/`security_schemes`/`router_path`, upgrade query `required`/description/`enum_values`, and add annotation-only responses (404).

## Diagnostics now emitted (for 02-03 `diagnostics::collect` reconciliation)

Total **7**, byte-stable sorted:
- **float64-narrowing ×3** (CreateGoalInput/UpdateGoalInput/GoalResponse `.TargetValue` `*float64`) — from 02-01.
- **free-form-map ×1** (GoalResponse.Metadata `map[string]any`) — from 02-01.
- **untyped-query ×3** (cursor, page_size, aggregation on GET /list) — NEW this plan, file:line at the `c.Query` call in `handlers.go`. Wording is a uniform machine-stable template (`untyped query param '<name>' on <METHOD> <route>: ... (TARGET-API.md §5.4)`); the per-param canonical phrasing in `expected/diagnostics.txt` (lines 12-14 differ slightly per param) is reconciled by `diagnostics::collect` in 02-03. The route label here is group-relative (`/list`); 02-03 joins the `/goal` prefix.
- **dynamic-response ×0** for this fixture (all `c.JSON` bodies resolve to named types); the WARN path exists and is exercised defensively (D-05), so a future dynamic handler diagnoses rather than guessing.

## Decisions Made

- **Two new RouteFact fields over reusing `path`:** `router_path` (annotation override) and `security_schemes` (scheme names) are additive; emitting both the code path and the `@Router` path keeps the facts router-agnostic and defers the absolute-prefix decision to 02-03 (RESEARCH Open Q1 recommendation: store both).
- **Selector→package map derived from loaded packages**, not hardcoded, so `@Param body dto.UpdateGoalInput` and `@Success {object} dto.X` refs qualify to the same `internal/common/dto.X` ids the 02-01 extractor emits.
- **aliasedgin as a separate testdata module** (not a package inside goextract) so pulling gin in for the alias test does not perturb `goextract/go.mod`/`go.sum`; `testdata/` is skipped by the parent `go ./...`.

## Deviations from Plan

None - plan executed exactly as written. All three tasks, their `<action>` specs, `<acceptance_criteria>`, and `<verify>` commands were implemented and pass as specified; no bugs, missing-critical, blocking, or architectural deviations were encountered.

## Known Stubs

None that block this plan's goal. Two intentional, documented boundaries (consistent with the threat model, not goal-blocking):
- The annotation selector→path resolver (`schemaRefFromAnnotation`) resolves the package selectors present in the loaded target module; generic resolution of arbitrary cross-module selectors is a documented post-PoC concern (the fixture's `dto` selector resolves correctly).
- The `dynamic-response` diagnostic path is implemented but not triggered by this fully-typed fixture (correct — there are no dynamic responses to diagnose). It is unit-reachable via the `analyzeJSON` fallback and will fire for non-typed handlers.

## Issues Encountered

None. The red-by-design Rust contract tests `snapshot_graph` and `snapshot_diagnostics` remain RED (they call `build_graph`/`diagnostics::collect`, both still `NotYetImplemented` until 02-03) — this is the plan's explicit `<verification>` NOTE, not a regression. The `gnr8-core` **lib** test suite (12 tests incl. the new populated-route round-trips), clippy `-D warnings`, and `fmt --check` are all green.

## User Setup Required

None - no external service configuration required. The Go toolchain (`go 1.26`) is the only dependency; the `aliasedgin` testdata module reuses the gin version already in the module cache.

## Next Phase Readiness

- **Ready for 02-03:** the JSON `routes` array is fully populated and deterministic — route table (4 routes), params (path/query with required/enum), request/response refs (by go/constant status), `router_path` + `security_schemes` + `operation_id` + `summary` + `tags`. 02-03 lowers this into the Rust `ApiGraph`, joins the `/goal` base path, derives stable operation ids, reconciles `diagnostics::collect` against `expected/diagnostics.txt` (the 7 lines: float64×3, free-form-map×1, untyped-query×3), and flips `snapshot_graph` + `snapshot_diagnostics` green.
- No blockers. Rust `analyze::facts` already deserializes the populated shape (round-trip tested), so 02-03 builds straight on the deserialized `GoFacts`.

---
*Phase: 02-go-analysis-and-api-graph*
*Completed: 2026-06-24*

## Self-Check: PASSED

- All 10 created files verified present on disk (`[ -f ]`): routes.go/_test, aliasedgin {go.mod,go.sum,app.go}, handlers.go/_test, annotations.go/_test, SUMMARY.md.
- All 3 task commits verified in `git log` (`27c6d43`, `18a8758`, `69282d0`).
- Plan `<verification>` re-run green: `goextract` build/vet/test (routes/handlers/types/facts all ok); `go run` emits 4 routes + 7 diagnostics (float64×3, free-form-map×1, untyped-query×3) with operationId goalUuidPut, aggregation Enums `[avg,count,max,min,sum]`, request body CreateGoalInput, responses [201,400]; two `go run` invocations byte-identical.
- Rust gates green: `cargo test -p gnr8-core --lib` (12 pass incl. 2 new populated-route round-trips), `cargo clippy -D warnings`, `cargo fmt --check`.
- `snapshot_graph` + `snapshot_diagnostics` confirmed still RED-by-design (build_graph/diagnostics::collect NotYetImplemented until 02-03) — per the plan's `<verification>` NOTE, not a regression.
