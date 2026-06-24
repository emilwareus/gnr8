# Target API Research: Real-World Gin Service Shape

**Generated:** 2026-06-24
**Purpose:** Define the concrete Go API surface that gnr8's PoC must extract from, derived from a
representative production Gin codebase. This document anchors the fixtures so we build for a *real*
target shape without overfitting to one repo's quirks.

---

## 1. Why this document exists

The PROJECT contract says: generate accurate OpenAPI and a Go SDK from real Go source, with minimal
duplicated API description. To do that well we need to know what "real source" actually looks like in a
production service — not a toy `net/http` example.

This research models a representative target: a multi-domain HTTP backend built on **Gin**, whose
OpenAPI today is produced by **comment annotations** (swaggo/swag style) and whose SDKs are produced by
running an external **openapi-generator** over the resulting swagger JSON.

That existing toolchain is exactly the pain gnr8 wants to displace:

```text
Today (the target's current pipeline):
  Gin handlers + heavy // @annotation comments
    -> swag init (per domain)        -> <domain>_swagger.json
    -> normalize/patch scripts        (fix generator quirks)
    -> openapi-generator-cli          -> Go / TS / Python SDKs

gnr8's bet:
  Gin routes + typed handlers + struct tags   (code is the source of truth)
    -> native extraction -> API graph -> OpenAPI + Go SDK
    -> comments / .gnr8 overrides used ONLY as escape hatches
```

The key insight for fixtures: **most API facts in the target are already present in the code**
(route registration, path/query access, request-body structs, response structs, validation tags).
The annotation comments mostly *restate* what the types already encode. gnr8's differentiator is to
derive those facts from code structure and treat annotations as optional override, not requirement.

---

## 2. The target's shape, abstracted

Observed characteristics of the representative target (numbers rounded, identities removed):

| Dimension | Observed in target | Implication for PoC |
|---|---|---|
| Web framework | Gin (`gin-gonic/gin`) | Primary router to support |
| Architecture | Hexagonal: `internal/<domain>/ports/http.go` per domain | Extractor must follow route registration across files, not assume one file |
| Domains / services | ~9 domains, each its own router + swagger instance | Support multiple base paths / route groups |
| Routes | ~140 routes (≈58 GET, ≈34 POST, ≈6 PUT, ≈8 DELETE) | Cover GET/POST/PUT/DELETE; PATCH optional |
| Annotated handlers | ~186 handlers across ~29 files | Annotations are heavily used today — must read them as escape hatch, not ignore |
| Request decode | `c.ShouldBindJSON(&input)` (≈45 uses) | JSON body binding is the dominant request pattern |
| Path params | `c.Param("uuid")`, `c.Param("id")`, etc. | Map Gin `:param` to OpenAPI `{param}` |
| Query params | `c.Query("cursor")`, `c.Query("page_size")` | Extract query params (annotation-assisted in target) |
| Validation | `binding:"required"` struct tags | Drives `required` in schema |
| Well-known types | `uuid.UUID`, `time.Time`, `*float64`, `[]uuid.UUID` | Need a type-mapping table |
| Enums | Typed `string` newtypes with a fixed value set | Map to OpenAPI string enums |
| SDK targets | Go, TypeScript, Python today | PoC scope = **Go SDK only**; design graph language-neutral |

**Do not overfit.** Gin is the first router we support, but the extraction model should describe
*HTTP route facts* (method, path template, params, request body type, response type+status), not Gin
internals. A second router (chi, echo, net/http mux) should later plug into the same graph without
reshaping it. The fixtures below are therefore written in idiomatic Gin but annotated with "what a
generic extractor sees."

---

## 3. Concrete examples (the fixture spec)

These examples are a faithful, anonymized reduction of the target's real patterns. The PoC's fixtures
should mirror **this structure**, not invent a simpler one. A single fixture service with **two
resources** (a "goals"-like CRUD resource and a "list with query filters" read resource) exercises every
extraction concern the target throws at us.

### 3.1 Route registration (what defines the route table)

```go
// internal/goal/ports/http.go
package ports

func (h HttpServer) setupRoutes(basePath string) {
	api := h.Router.Group("/" + basePath)   // base path => OpenAPI server/prefix
	api.Use(server.RequestBudget(60 * time.Second))
	api.Use(h.AuthMiddleware)                // security applies to the whole group
	{
		api.POST("/", h.createGoal)          // POST   /goal/
		api.GET("/list", h.listGoals)        // GET    /goal/list
		api.PUT("/:uuid", h.updateGoal)      // PUT    /goal/{uuid}
		api.DELETE("/:uuid", h.deleteGoal)   // DELETE /goal/{uuid}
	}
}
```

**Extractor must derive, from code alone:**
- 4 routes with method + full path template (group prefix + segment).
- `:uuid` is a path parameter named `uuid`, type string/uuid.
- The group has an auth middleware → security requirement on every operation in the group.
- Each route maps to a handler function (`h.createGoal`, …) to analyze next.

### 3.2 Handler + request/response binding (the operation contract)

```go
// POST /goal/ — create
func (h HttpServer) createGoal(c *gin.Context) {
	var input dto.CreateGoalInput            // <-- request body schema
	if err := c.ShouldBindJSON(&input); err != nil {
		c.JSON(http.StatusBadRequest, dto.HttpError{...})   // <-- 400 response schema
		return
	}
	result, err := h.app.Commands.CreateGoal.Handle(ctx, cmd)
	// ...
	c.JSON(http.StatusCreated, dto.CommandMessageWithUUID{...}) // <-- 201 response schema
}
```

**Extractor must derive:**
- Request body type = `dto.CreateGoalInput` (from `ShouldBindJSON(&input)` where `input` is that type).
- Success response = `201` with body `dto.CommandMessageWithUUID` (from the `c.JSON(http.StatusCreated, …)` call).
- Error response = `400` with body `dto.HttpError`.

> Note for the gnr8 thesis: today the target *also* writes a comment block restating all of this
> (`@Param body body dto.CreateGoalInput`, `@Success 201 {object} dto.CommandMessageWithUUID`,
> `@Failure 400 {object} dto.HttpError`). gnr8 should derive it from the code and only *fall back* to
> the comment when the code is ambiguous (e.g. a handler that builds responses dynamically). The fixture
> must include both an example the code fully describes and one that needs an annotation escape hatch.

### 3.3 The annotation escape hatch (must still be supported)

The current pipeline's source of truth is swaggo-style comments. gnr8 must *read* these where present,
because real handlers sometimes can't be fully inferred (dynamic status codes, query params accessed via
`c.Query` with no struct). Representative annotation block:

```go
// @Summary      Update goal
// @Description  Update a goal including workflow links
// @Tags         Goals
// @Accept       json
// @Produce      json
// @Param        uuid  path   string               true  "Goal UUID"
// @Param        body  body   dto.UpdateGoalInput   true  "Goal fields to update"
// @Success      200   {object} dto.CommandMessage  "Goal updated"
// @Failure      400   {object} dto.HttpError        "Invalid input"
// @Failure      404   {object} dto.HttpError        "Goal not found"
// @ID           goalUuidPut
// @Router       /{uuid} [put]
// @Security     ApiKeyAuth
func (h HttpServer) updateGoal(c *gin.Context) { /* ... */ }
```

And query parameters, which in the target are predominantly annotation-described because they're read
loosely via `c.Query`:

```go
// @Param  cursor      query string false "Cursor UUID for pagination"
// @Param  page_size   query string false "Page size"
// @Param  actionTypes query string false "Action types (comma-separated)"
// @Param  aggregation query string true  "Aggregation" Enums(count,sum,avg,min,max)
func (h HttpServer) listActions(c *gin.Context) {
	cursor := c.Query("cursor")
	// ...
}
```

**Extraction rules the fixtures must cover:**
- Path params: prefer code (`:uuid` + `c.Param("uuid")`); annotation refines type/description.
- Query params: code gives names via `c.Query("x")`; annotation gives required-ness, type, enums, docs.
- An inline `Enums(...)` on a query param is a closed value set → OpenAPI enum.

### 3.4 DTO structs (the schema source of truth)

```go
// CreateGoalInput is the request body for creating a goal.
type CreateGoalInput struct {
	Name             string             `json:"name" binding:"required" description:"Short human-readable goal name"`
	Description      string             `json:"description" description:"Longer explanation"`
	AnalyticsQuery   GoalAnalyticsQuery `json:"analyticsQuery" binding:"required"`   // nested struct
	TargetValue      *float64           `json:"targetValue,omitempty"`               // optional (pointer + omitempty)
	TargetDirection  *TargetDirection   `json:"targetDirection,omitempty"`           // optional enum
	WorkflowChainIDs []uuid.UUID        `json:"workflowChainIds,omitempty"`          // array of uuid
}

type HttpError struct {
	Message string   `json:"message" example:"error message" binding:"required"`
	Slug    string   `json:"slug,omitempty" example:"error-slug"`
	Hints   []string `json:"hints,omitempty" example:"hint 1,hint 2"`
}

// Embedded struct (composition) — fields are flattened into the schema.
type CommandMessage struct {
	Message string `json:"message" binding:"required"`
}
type CommandMessageWithUUID struct {
	CommandMessage              // embedded -> "message" promoted
	UUID uuid.UUID `json:"uuid" binding:"required"`
}
```

**Struct → schema rules the fixtures must cover:**
- Field name comes from the `json:"..."` tag (not the Go field name).
- `binding:"required"` → field is in the schema's `required` list.
- Pointer (`*T`) and/or `,omitempty` → optional field.
- `description:"..."` and `example:"..."` tags → schema metadata (optional polish).
- Embedded struct → promoted/flattened fields.
- Nested struct field → `$ref` to that struct's schema.
- `[]T` → array; `map[string]T` → object with additionalProperties.

### 3.5 Enum newtypes

```go
// TargetDirection is a closed vocabulary: gte (higher is better) or lte (lower is better).
type TargetDirection string
const (
	TargetDirectionGte TargetDirection = "gte"
	TargetDirectionLte TargetDirection = "lte"
)
```

**Rule:** a named `string` type with a set of typed const values → OpenAPI `string` enum with those
values. (The target additionally hand-writes `JSONSchema()` methods for some of these — that's an escape
hatch gnr8 can honor later, but the const-set inference should be the default path.)

---

## 4. Type mapping table (Go → OpenAPI → Go SDK)

Derived from how the target's existing generator behaves; the PoC needs at least these:

| Go type | OpenAPI schema | Generated Go SDK type | Notes |
|---|---|---|---|
| `string` | `type: string` | `string` | |
| `bool` | `type: boolean` | `bool` | |
| `int`, `int64` | `type: integer, format: int64` | `int64` | |
| `float64` | `type: number` | `float32` | ⚠ generator narrows `number`→`float32` by default; surface as a diagnostic |
| `uuid.UUID` | `type: string, format: uuid` | `string` | well-known type mapping |
| `time.Time` | `type: string, format: date-time` | `time.Time` | well-known type mapping |
| `*T` / `,omitempty` | optional (not in `required`) | pointer field `*T` | optionality |
| `[]T` | `type: array, items: T` | `[]T` | |
| `map[string]T` | `object, additionalProperties: T` | `map[string]T` | see pitfall §5 |
| named `string` + consts | `string` enum | typed string newtype | enum inference |
| nested struct | `$ref` | nested model type | |
| embedded struct | flattened fields | flattened fields | composition |

---

## 5. Known pitfalls (diagnostics the PoC should emit)

These come straight from the workarounds the target's pipeline had to bolt on — each is a concrete
"unsupported pattern / lossy mapping" the gnr8 `doctor`/diagnostics should catch instead of silently
mishandling:

1. **`map[string]any` / `interface{}`** → the external toolchain emitted `additionalProperties: {}`,
   which downstream generators mis-typed (double-wrapped). They needed a normalize step. gnr8 should
   emit `additionalProperties: true` and/or warn on free-form maps.
2. **`float64` → `float32` narrowing** in the Go SDK loses precision. gnr8 should either map to
   `float64` or emit a compatibility diagnostic.
3. **Dynamic responses** — handlers that compute status codes or bodies at runtime can't be fully
   inferred from code; gnr8 must fall back to annotations and diagnose when neither is conclusive.
4. **Query params with no struct** — `c.Query("x")` gives a name but no type/required-ness; without an
   annotation the param is under-specified. Diagnose as "param type unknown."
5. **Per-domain swagger instances** — the target splits into ~9 independent OpenAPI docs (one per
   domain) to keep them small. gnr8 should support a single graph that can emit one combined doc *and/or*
   per-group docs, but the PoC can start with one combined doc per service.
6. **Stale-generation detection** — the target gates CI on "SDK artifacts are up to date." This validates
   gnr8's no-op/idempotent-generation requirement: regenerating unchanged source must produce byte-identical
   output.

---

## 6. Fixture plan for the PoC (concrete deliverable)

Build **one** Gin fixture service (a Go module under `fixtures/`) that encodes the above. Suggested shape:

```text
fixtures/goalservice/
  go.mod
  internal/
    goal/ports/http.go        # routes: POST /goal/, GET /goal/list, PUT /goal/:uuid, DELETE /goal/:uuid
    goal/ports/handlers.go     # createGoal/listGoals/updateGoal/deleteGoal with mixed inference + annotations
    common/dto/goal.go         # CreateGoalInput, UpdateGoalInput, GoalResponse, ListGoalsOutput
    common/dto/common.go       # HttpError, CommandMessage, CommandMessageWithUUID, TargetDirection enum
  expected/
    openapi.yaml               # snapshot: the OpenAPI gnr8 must produce
    sdk/                       # snapshot: the Go SDK shape (api_goals.go-style + model_*.go-style)
    diagnostics.txt            # snapshot: expected warnings (float64 narrowing, untyped query param, etc.)
```

**Acceptance cases the fixture must force** (maps to ROADMAP success criteria):

| Case | Exercises |
|---|---|
| `POST /goal/` with `CreateGoalInput` body, `201 CommandMessageWithUUID` | body binding, success status, nested + enum + array fields, required tags |
| `GET /goal/list` returning `ListGoalsOutput` | response-only operation, array of nested structs |
| `GET` with `?cursor=&page_size=` (annotation-described) | query param extraction + escape-hatch annotations |
| `PUT /goal/:uuid` | path param mapping `:uuid`→`{uuid}`, full annotation block |
| `DELETE /goal/:uuid` returning `CommandMessage` | path param + simple response |
| any handler returning `HttpError` on 400/404 | error responses, `example`/`omitempty` tags |
| a field of type `map[string]any` or `*float64` | pitfall diagnostics (§5) |
| auth middleware on the group | security requirement propagation |
| second run with no source change | idempotent / no-op generation |

**Deliberately out of scope for the PoC fixture** (kept generic, deferred — do not build): TypeScript/Python
SDK targets, non-Gin routers, websocket/streaming routes, file-upload multipart, OAuth flows. The graph
should *allow* these later without reshaping, but the fixture stays Go-source → OpenAPI → Go-SDK.

---

## 7. Summary for planners

- **Target = a multi-domain Gin service** whose OpenAPI is today produced from swaggo comment
  annotations and whose SDKs come from an external openapi-generator. gnr8 replaces both with native
  code-first extraction.
- **Support Gin first, model generically.** Extract HTTP route facts (method, path template, params,
  request type, response type+status), not Gin internals.
- **Code is the primary source of truth; annotations are an escape hatch.** Inference from route
  registration + handler binding + struct tags covers the common case; read annotations to fill gaps and
  diagnose when both are silent.
- **Fixtures must mimic the real shape** (CRUD + list-with-filters, nested/enum/array DTOs, well-known
  types, validation tags, error responses, auth group) and ship with expected OpenAPI, expected Go SDK,
  and expected diagnostics snapshots.
- **The pitfalls (§5) are the diagnostics backlog** — each real-world workaround the target needed is a
  pattern gnr8 should detect and report rather than mishandle.
