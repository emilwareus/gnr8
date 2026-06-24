# Native Go Code To OpenAPI

## Research Question

How can `gnr8` generate OpenAPI from Go application code natively, without wrapping Swaggo, oapi-codegen, OpenAPI Generator, or another external generator?

## Position

The core should be an owned Go analysis pipeline:

```text
Go source
  -> package and syntax facts
  -> semantic facts
  -> route/handler/schema graph
  -> internal API graph
  -> OpenAPI lowerer
```

OpenAPI generation should be the final lowering step, not the analysis engine.

This should stay intentionally small in the first implementation. The goal is not to design a universal source-analysis framework before proving that one native Go path can infer useful API facts.

## What "Native" Means

Native means:

- Own the extraction model.
- Own the internal API graph.
- Own OpenAPI serialization.
- Own diagnostics and source provenance.
- Own SDK backend inputs.
- Do not shell out to Swaggo, oapi-codegen, OpenAPI Generator, or similar generators as the core workflow.

Native does not necessarily mean ignoring official language tooling. Go's parser, AST, type checker, `go/packages`, or gopls-compatible package loading may be appropriate because they provide language truth rather than API-generation behavior.

Sources:

- Go tools module: <https://pkg.go.dev/golang.org/x/tools>
- `go/packages`: <https://pkg.go.dev/golang.org/x/tools/go/packages>
- gopls: <https://go.dev/gopls/>

## Why Not Comment-First

Swaggo validates demand for Go code-first API documentation, but its README describes the model as converting Go annotations to Swagger Documentation 2.0.

Source: <https://github.com/swaggo/swag>

The problem with comment-first generation:

- Route strings, response types, status codes, and schema names can drift from code.
- Comments are weakly typed.
- Refactors do not reliably update documentation.
- Users end up maintaining a second API description language inside comments.

`gnr8` should invert that:

- Code structure is primary.
- Type information is primary.
- Comments are optional escape hatches for intent that cannot be inferred.

## Facts To Extract

### Package Facts

- Module path.
- Package name.
- Build tags.
- Import graph.
- File ownership.

### Route Facts

- HTTP method.
- Path template.
- Router/framework.
- Handler symbol.
- Middleware chain if statically available.
- Source span.

### Handler Facts

- Function or method symbol.
- Receiver type.
- Parameter types.
- Return types.
- Adapter/wrapper chain.
- Body decoder calls.
- Response writer calls.
- Error mapping.
- Status code mapping.

### Schema Facts

- Struct fields.
- JSON tags.
- Validation tags.
- Embedded structs.
- Type aliases.
- Newtypes.
- Generic instantiations.
- Enum-like const groups.
- Well-known scalar mappings.

## Inference Levels

Level 1: Direct typed handlers.

Example shape:

```go
func CreateUser(ctx context.Context, req CreateUserRequest) (User, error)
```

This is easiest because request and response types are in the signature.

Level 2: Framework context binding.

Example shape:

```go
var req CreateUserRequest
if err := c.Bind(&req); err != nil { ... }
return c.JSON(http.StatusCreated, user)
```

This requires intra-function analysis.

Level 3: Adapters and wrappers.

Example shape:

```go
r.Post("/users", Adapt(CreateUser))
```

This requires resolving adapter semantics.

Level 4: Arbitrary dynamic behavior.

Examples:

- Reflection-based responses.
- Map-based JSON responses.
- Response helper functions across packages.
- Middleware-controlled status codes.

This may require explicit code configuration or annotations.

## OpenAPI Lowering

The API graph should lower into OpenAPI with explicit version targets:

- 3.2 for latest semantics.
- 3.1 for broad modern JSON Schema compatibility.
- 3.0 for downstream generator compatibility.

OpenAPI 3.2.0 is the newest version listed by the official OpenAPI spec site as of June 24, 2026.

Sources:

- <https://spec.openapis.org/oas/>
- <https://spec.openapis.org/oas/v3.2.0.html>

## Research Tasks

1. Build a matrix of route registration patterns by Go framework.
2. Build a matrix of handler contract patterns by framework.
3. Build a type mapping table from Go to internal schema facts.
4. Define when annotations are allowed.
5. Define OpenAPI lowering loss diagnostics.
6. Select real-world fixture services for validation.
7. Identify which Go-specific facts should not leak into the language-neutral graph.
