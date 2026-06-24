---
phase: 01-foundation-and-fixtures
plan: 02
subsystem: testing
tags: [go, gin, fixtures, openapi, swaggo, uuid, sdk, diagnostics]

# Dependency graph
requires:
  - phase: 01-foundation-and-fixtures (plan 01-01)
    provides: Rust workspace skeleton + PoC contract; this plan adds the Go fixture the analyzer will read
provides:
  - Compilable Go Gin fixture module fixtures/goalservice/ (own go.mod, pinned gin v1.12.0 + uuid v1.6.0)
  - CRUD + list-with-query-filters route surface (POST/GET-list/PUT/DELETE) mirroring TARGET-API.md §6
  - DTO schema source-of-truth covering every FIX-02 feature incl. the map[string]any unsupported pattern
  - Hand-authored expected/ acceptance targets (openapi.yaml 3.1.0, sdk/*, diagnostics.txt) per D-15
affects: [02-go-analysis-and-api-graph, 03-openapi-and-go-sdk-generation, 01-03 (red-by-design snapshot tests)]

# Tech tracking
tech-stack:
  added: [github.com/gin-gonic/gin v1.12.0, github.com/google/uuid v1.6.0, Go 1.26.2 module]
  patterns:
    - "Hexagonal Gin layout: internal/<domain>/ports/http.go (routes) + handlers.go, internal/common/dto (schemas)"
    - "Code-first API facts (route registration, ShouldBindJSON, c.Param/c.Query, struct tags) with swaggo comments as escape hatch only"
    - "expected/ snapshots as reviewable hand-authored acceptance contracts, separate from the fixture source"

key-files:
  created:
    - fixtures/goalservice/go.mod
    - fixtures/goalservice/go.sum
    - fixtures/goalservice/internal/common/dto/common.go
    - fixtures/goalservice/internal/common/dto/goal.go
    - fixtures/goalservice/internal/goal/ports/http.go
    - fixtures/goalservice/internal/goal/ports/handlers.go
    - fixtures/goalservice/expected/openapi.yaml
    - fixtures/goalservice/expected/sdk/client.go
    - fixtures/goalservice/expected/sdk/goals.go
    - fixtures/goalservice/expected/sdk/models.go
    - fixtures/goalservice/expected/sdk/errors.go
    - fixtures/goalservice/expected/diagnostics.txt
  modified: []

key-decisions:
  - "Fixture is a standalone Go module under fixtures/ (not in the Cargo workspace); cargo never compiles it, go build/go vet (and CI in 01-03) do"
  - "createGoal is fully code-inferable (no annotations) while updateGoal/listGoals carry swaggo blocks — the fixture forces BOTH the inference path and the annotation escape hatch"
  - "expected/sdk/* are authored as valid compiling Go (package goalservice) even though the plan only requires them to be illustrative — keeps the module's go build ./... green and makes the SDK shape concretely reviewable"

patterns-established:
  - "Pattern: one dense request DTO (CreateGoalInput) concentrates every schema feature so each FIX-02 concern has at least one fact to extract"
  - "Pattern: diagnostics.txt lists one human-readable WARN per line, each tagged with its TARGET-API.md §5 source"

requirements-completed: [FIX-01, FIX-02]

# Metrics
duration: 5min
completed: 2026-06-24
---

# Phase 1 Plan 2: Go Gin Fixture Service + Expected Acceptance Targets Summary

**Realistic standalone Go Gin fixture module (goalservice) — 4-route CRUD+list surface with an auth-middleware group, FIX-02 DTOs (nested/embedded structs, enum newtype, uuid.UUID/time.Time/[]uuid.UUID, *float64, and an unsupported map[string]any), plus hand-authored expected OpenAPI 3.1.0, Go SDK shape, and diagnostics contracts.**

## Performance

- **Duration:** 5 min
- **Started:** 2026-06-24T16:15:52Z
- **Completed:** 2026-06-24T16:21:30Z
- **Tasks:** 3
- **Files modified:** 12 (all created)

## Accomplishments
- Initialized `fixtures/goalservice/` as its own Go 1.26.2 module with pinned `gin-gonic/gin v1.12.0` + `google/uuid v1.6.0`, compiling and vetting clean.
- Authored the DTO schema source-of-truth exercising every FIX-02 feature: json field renaming, `binding:"required"`, optional pointer/omitempty fields, nested struct (`GoalAnalyticsQuery`), embedded struct composition (`CommandMessageWithUUID` promotes `message`), `TargetDirection` string-enum newtype, `uuid.UUID`/`time.Time` well-known types, `[]uuid.UUID` array, `HttpError` error envelope, and the UNSUPPORTED `map[string]any` (`Metadata`) free-form map for diagnostics.
- Wrote the Gin route group with auth middleware + 4 routes (`POST /goal/`, `GET /goal/list`, `PUT /goal/:uuid`, `DELETE /goal/:uuid`) and handlers exercising `ShouldBindJSON`, `c.Param("uuid")`, untyped `c.Query`, success/error `c.JSON` statuses, a fully code-inferable handler, and swaggo annotation blocks incl. inline `Enums(count,sum,avg,min,max)`.
- Hand-authored the `expected/` acceptance targets (D-15): an OpenAPI 3.1.0 doc applying the §4 type map, a 4-file Go SDK shape sketch (functional-options `Client`, context-first tag-grouped ops, generated models, typed `APIError`), and a `diagnostics.txt` listing the §5 warnings.

## Task Commits

Each task was committed atomically:

1. **Task 1: Init Go Gin module + DTO schema source-of-truth** - `c41bb3e` (feat)
2. **Task 2: Gin routes + handlers (code inference + annotation hatches)** - `15e5c7c` (feat)
3. **Task 3: expected/ acceptance-target scaffolds (openapi/sdk/diagnostics)** - `6153ea3` (feat)

**Plan metadata:** _(this commit)_ (docs: complete plan)

## Files Created/Modified
- `fixtures/goalservice/go.mod` / `go.sum` - Standalone module pinning gin v1.12.0 + uuid v1.6.0.
- `fixtures/goalservice/internal/common/dto/common.go` - HttpError, CommandMessage, embedded CommandMessageWithUUID, TargetDirection string-enum newtype.
- `fixtures/goalservice/internal/common/dto/goal.go` - CreateGoalInput/UpdateGoalInput/GoalResponse/ListGoalsOutput; nested struct, `*float64`, `time.Time`, `[]uuid.UUID`, and the `map[string]any` unsupported pattern.
- `fixtures/goalservice/internal/goal/ports/http.go` - HttpServer + setupRoutes: group + `api.Use(AuthMiddleware)` + 4 routes.
- `fixtures/goalservice/internal/goal/ports/handlers.go` - createGoal/listGoals/updateGoal/deleteGoal with ShouldBindJSON, c.Param, c.Query, c.JSON, and swaggo annotation blocks.
- `fixtures/goalservice/expected/openapi.yaml` - Hand-authored OpenAPI 3.1.0 acceptance target.
- `fixtures/goalservice/expected/sdk/{client,goals,models,errors}.go` - D-05 SDK shape sketch (acceptance target).
- `fixtures/goalservice/expected/diagnostics.txt` - Expected analyzer warnings (float64→float32 narrowing, free-form map, untyped query params).

## Decisions Made
- Authored `expected/sdk/*` as valid, compiling Go (`package goalservice`) rather than non-compiling sketches. The plan only required them to be illustrative, but keeping them compilable means `go build ./...`/`go vet ./...` stay green across the whole module and the SDK shape is concretely reviewable. They are header-marked "EXPECTED SDK SHAPE — acceptance target" so their role as targets (not the real generator output) is unambiguous.
- Mapped `[]uuid.UUID` → `[]string` and `*float64` → `*float32` in `expected/sdk/models.go` to faithfully reflect the §4 type-table behavior (and the narrowing the diagnostics flag), not to prescribe the final generator choice — gnr8 may instead keep float64 and warn (noted inline).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Re-pinned gin in go.mod after `go mod tidy` pruned it**
- **Found during:** Task 1 (module init)
- **Issue:** Task 1's acceptance criterion requires `grep -q 'gin-gonic/gin v1.12.0' go.mod`, but `go get gin@v1.12.0` followed by `go mod tidy` removed gin from go.mod because no `.go` file imported it yet (gin is first imported in Task 2's handlers). The acceptance grep would fail.
- **Fix:** Re-ran `go get github.com/gin-gonic/gin@v1.12.0` after tidy to restore the pinned entry (initially `// indirect`); Task 2's imports then promoted it to a direct require via the subsequent `go mod tidy`.
- **Files modified:** fixtures/goalservice/go.mod, fixtures/goalservice/go.sum
- **Verification:** `grep -q 'gin-gonic/gin v1.12.0' go.mod` passes; `go build ./... && go vet ./...` exit 0.
- **Committed in:** c41bb3e (Task 1 commit), finalized in 15e5c7c (Task 2, go.sum amended).

---

**Total deviations:** 1 auto-fixed (1 blocking).
**Impact on plan:** The fix only restores a dependency the plan explicitly intended to pin; no scope creep, no behavior change. Everything else executed exactly as written.

## Issues Encountered
None. The `bc`/`grep -c` multi-file output produced cosmetic parse noise during the verification matrix only; recomputed counts (32 json tags, 20 omitempty, 8 uuid.UUID, embedded CommandMessage confirmed) verified full FIX-02 coverage.

## Known Stubs
The handler bodies contain no real persistence (e.g. `createGoal` returns `uuid.New()`, `listGoals` returns an empty page). This is intentional and required by the plan: the fixture is analyzer INPUT, never a runnable production service (threat T-02-01/T-02-03 = accept). The binding/response patterns are fully exercised so the analyzer has facts to extract; no future plan needs to "fill in" the bodies.

## User Setup Required
None - no external service configuration required. (Local Go 1.26.2 toolchain is already present.)

## Next Phase Readiness
- `fixtures/goalservice/` compiles + vets clean and encodes all FIX-01/FIX-02 acceptance cases incl. the `map[string]any` unsupported pattern — ready as the primary input for Phase 2's analyzer.
- `expected/` files are the reviewable acceptance contracts (OpenAPI 3.1.0, Go SDK shape, diagnostics) Phases 2–3 must satisfy.
- Plan 01-03 will wire the red-by-design snapshot tests (insta) and CI/Make gates that compile this module and assert against the `expected/` targets.

## Self-Check: PASSED

- All 12 fixture files verified present on disk.
- All three task commits (c41bb3e, 15e5c7c, 6153ea3) present in git log.
- `cd fixtures/goalservice && go build ./... && go vet ./...` exit 0; `gofmt -l .` clean.
- All per-task `<acceptance_criteria>` re-run and passing; plan-level `<verification>` satisfied.

---
*Phase: 01-foundation-and-fixtures*
*Completed: 2026-06-24*
