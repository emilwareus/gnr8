<!-- generated-by: gsd-doc-writer -->
# Sources and extraction

[Agent docs index](../agents/index.md)

A `Source` statically converts one input into the shared `ApiGraph`. Code sources invoke language
sidecars but never import, start, or call the application. Ambiguous facts produce structured
diagnostics instead of inferred guesses.

## Source matrix

| Source | Configuration | Required project toolchain | Current input count |
|---|---|---|---:|
| Go + Gin | `GoGin::new().inputs(["."])` | Go | 1 |
| FastAPI | `FastApi::new().inputs(["."])` | Python 3 | 1 |
| Flask typed-envelope | `Flask::new().inputs(["."])` | Python 3 | 1 |
| NestJS | `NestJs::new().inputs(["src"])` | Node.js and project `typescript` | 1 |
| Swagger/OpenAPI | `OpenApi::new().input("openapi.yaml")` | none | 1 file |

Input paths are relative to the application root. Zero or multiple code-source inputs are
configuration errors.

## Go + Gin

```rust
.source(
    GoGin::new()
        .inputs(["."])
        .route_packages(["./cmd/api/...", "./internal/http/..."])
        .schema_packages(["./internal/dto/..."]),
)
```

`.packages(patterns)` applies the same `go/packages` scopes to routes and schemas. Empty scopes load
the whole module (`./...`). Package scopes reduce analysis work and keep unrelated binaries out of
the graph.

Recognized route facts include:

- Static nested `Group` prefixes and Gin HTTP method registrations.
- Handler functions and bounded, cycle-safe helper traversal across packages.
- Constant arguments propagated through helper calls.
- `Param` path parameters.
- `Query`, `DefaultQuery`, `GetQuery`, array/map query accessors.
- `GetHeader` and `Request.Header.Get` headers, `Cookie` cookies.
- `PostForm`, `DefaultPostForm`, `GetPostForm`, and `FormFile` form values.
- `ShouldBindJSON`/`BindJSON`; generic bind variants for typed form, multipart, query, and header
  structs.
- JSON responses, response status/media facts, Go structs, nested types, and string enums.

Dynamic route strings are skipped with a diagnostic. A dynamic group prefix is omitted and reported.
Dynamic parameter names, direct multipart map access, untraversable helpers, or ambiguous handlers are
reported rather than guessed. Use `gnr8 inspect graph` to identify the exact route/schema before
adding a transform.

## FastAPI

```rust
.source(FastApi::new().inputs(["."]))
```

The Python sidecar parses AST and does not import the app. It recognizes:

- Decorated routes and typed path/query/header/body parameters.
- Pydantic models and dataclasses.
- `response_model` and `status_code`.
- `Literal`, `Enum`, optional/union/collection annotations.
- Typed request and response models with source provenance.

Runtime-built routes, dynamic decorator values, and types that cannot be resolved statically become
diagnostics.

## Flask typed-envelope

```rust
.source(Flask::new().inputs(["."]))
```

Flask extraction intentionally requires typed envelopes. It recognizes decorated routes, typed
function parameters/returns, dataclasses/Pydantic-like model declarations, enums, unions, and typed
response shapes. Untyped `request.json`, unannotated query reads, or missing return annotations are
diagnosed. Add types in application code or use a narrow explicit transform; do not expect runtime
introspection.

## NestJS

```rust
.source(NestJs::new().inputs(["src"]))
```

The sidecar uses the target project's TypeScript compiler. It recognizes controllers, route method
decorators, parameter/query/body decorators, class DTOs, enums, arrays, optional/nullable fields, and
unions. It does not treat Swagger decorators, zod schemas, or class-validator metadata as a second
source of truth. The project must provide a usable `typescript` package.

## Swagger/OpenAPI artifact source

```rust
.source(OpenApi::new().input("specs/openapi.yaml"))
```

Accepts JSON or YAML Swagger 2.0, OpenAPI 3.0, and OpenAPI 3.1. It normalizes supported document facts
into the same graph used by code sources:

- All HTTP operations, parameters, request bodies, responses, media types, and status codes.
- Named schemas, arrays, maps, enums, nullable types, object `allOf`, and schema references.
- Metadata, tags, servers, security schemes/requirements, and operation documentation.
- Swagger body/formData/file parameters and supported `collectionFormat` serialization.
- Local references and relative external-file references contained within the project root.

The graph is deliberately smaller than the full OpenAPI vocabulary. Unrepresentable source facts
emit `source.openapi.unrepresentable`; escaping external references are rejected. Use
`gnr8 compat openapi`—not a graph round trip—when exact preservation of the entire document is the
gate.

## Source-to-target example

```rust
Pipeline::new()
    .source(OpenApi::new().input("legacy/swagger.yaml"))
    .transform(RenameOperation::new("getBooksUsingGET", "listBooks"))
    .target(OpenApi31::new().to("generated/openapi.yaml"))
    .target(
        TsSdk::new()
            .module("@acme/books")
            .to("generated/typescript")
            .profile(SdkProfile::typescript_fetch_compat()),
    );
```

## Inspect before correcting

```bash
gnr8 inspect routes
gnr8 inspect schemas
gnr8 --json inspect graph
gnr8 doctor
```

Use the diagnostic's operation, schema, subject, file, and line to make the smallest correction. Put
`DiagnosticPolicy` after corrections to turn unresolved facts into a project-specific gate.

Related: [Transforms and overrides](../pipeline/transforms.md),
[Diagnostics reference](../diagnostics/reference.md), and
[OpenAPI generation](../openapi/generation.md).
