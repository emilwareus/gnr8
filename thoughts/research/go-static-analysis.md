# Go Static Analysis Strategy

## Goal

Generate API facts from Go code without requiring users to duplicate every route, request, response, and schema detail in strings or comments.

Comments and explicit annotations should exist as escape hatches, not as the primary data model.

The native extraction research is expanded in [Native Go code to OpenAPI](native-go-to-openapi.md).

## Two-Tier Analysis

### Fast Syntax Tier

Use a Rust parser path for cheap facts:

- Files and package declarations.
- Type declarations.
- Struct fields.
- Tags such as `json`.
- Function and method declarations.
- Obvious route registration calls.
- Handler references in simple call expressions.

Tree-sitter is attractive for this tier because it is an incremental parsing library that can update syntax trees as source edits occur.

Sources:

- <https://tree-sitter.github.io/>
- <https://github.com/tree-sitter/tree-sitter>

### Semantic Tier

Use Go-native tooling for facts that require real Go semantics:

- Type aliases.
- Embedded fields.
- Imports and package scopes.
- Method sets.
- Generic instantiations.
- Interface satisfaction.
- Build tags.
- Framework adapter functions.
- Handler references passed through variables or wrappers.

The semantic tier should likely be a small Go helper or long-running sidecar, not a Rust reimplementation of Go's type checker.

Relevant Go tooling:

- `gopls` is the official Go language server developed by the Go team.
- `golang.org/x/tools/go/packages` loads Go packages for inspection and analysis, including parsing and type checking according to the selected load mode.
- The wider `golang.org/x/tools` module includes static analysis building blocks such as `go/packages`, `go/analysis`, SSA, call graphs, and AST inspection.

Sources:

- <https://go.dev/gopls/>
- <https://pkg.go.dev/golang.org/x/tools/gopls>
- <https://pkg.go.dev/golang.org/x/tools/go/packages>
- <https://pkg.go.dev/golang.org/x/tools>

## Route Discovery Problem

Route registration is framework-specific. The first research task is to inventory route patterns by framework and classify them by analysis difficulty.

Examples:

- Direct method calls: `r.Get("/users/{id}", handler)`.
- Gin-style uppercase calls: `r.GET("/users/:id", handler)`.
- Standard library: `http.HandleFunc("/users", handler)`.
- Gorilla-style chained calls: `r.HandleFunc("/users", handler).Methods("GET")`.
- Wrapper/adaptor calls: `r.Post("/users", Adapt(CreateUser))`.
- Generated or table-driven route registration.

The product should expose route recognizers as plugins or extension points rather than hardcoding every framework into the core.

## Schema Discovery Problem

Basic struct extraction is not enough.

The research must cover:

- JSON tags and `omitempty`.
- Pointers and nullability.
- Slices and maps.
- Embedded structs.
- Aliases and newtypes.
- Generics.
- `time.Time` and common custom scalar types.
- Validation tags.
- Enum-like const groups.
- Discriminated unions, where Go has no direct native model.

## Handler Contract Problem

Many Go services do not expose typed request/response contracts in function signatures. The handler may decode request bodies and write responses inside the function body.

Inference levels:

- Level 1: Typed application handlers, easy to infer.
- Level 2: Framework context methods with obvious `Bind`, `JSON`, or `Decode` calls.
- Level 3: Helper functions and adapters.
- Level 4: Arbitrary control flow, reflection, middleware, or dynamic response shapes.

The first slice should only promise Level 1 and selected Level 2 cases.

## Research Output Needed Before Implementation

- Framework route-pattern matrix.
- Handler contract inference matrix.
- Type mapping table from Go types to internal schema facts.
- Lossiness table for OpenAPI 3.2, 3.1, and 3.0 outputs.
- Representative fixture repositories for benchmarking.
