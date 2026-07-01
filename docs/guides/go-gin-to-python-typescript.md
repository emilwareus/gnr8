# Guide: Go/Gin Backend to Python and TypeScript SDKs

Use this when the application is a Go service using Gin and the useful output is OpenAPI plus SDKs for
non-Go consumers. This is the common "backend in Go, clients in app/frontend languages" setup.

## Start

Run from the Go service root:

```bash
gnr8 init --source go-gin --sdk typescript
```

Then edit `.gnr8/src/main.rs` to add the Python SDK target as a second SDK target.

## Pipeline Example

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(GoGin::new().inputs(["."]))
            .transform(SetBasePath::new("/api"))
            .transform(SetTitle::new("Public API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .transform(RenameOperation::new("listGoals", "list_goals"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/public/python-sdk").to("generated/python"))
            .target(TsSdk::new().module("example.com/public/typescript-sdk").to("generated/typescript"))
            .post(Header::generated()),
    )
}
```

## Agent Checklist

- Keep `GoGin::new().inputs(["."])` pointed at the Go module root unless the service code lives in a
  clear subdirectory.
- Static nested Gin groups such as `api := r.Group("/api")` and `books := api.Group("/books")` are
  folded into operation paths. Dynamic route paths are skipped with diagnostics; dynamic group prefixes
  are omitted with diagnostics.
- Use `SetBasePath` for router mount prefixes such as `/api` or `/v1`.
- Use `ApplySecurity::api_key(...)` when handlers expect a shared auth header.
- Use `RenameOperation` only for public SDK compatibility or awkward generated names.
- Put generated outputs under one stable directory, usually `generated/`.
- Do not edit generated SDK files. Edit Go source or `.gnr8/src/main.rs`, then regenerate.

## Validate

```bash
gnr8 generate
gnr8 doctor
gnr8 check
```

Read `generated/python/README.md`, `generated/python/reference.md`,
`generated/typescript/README.md`, and `generated/typescript/reference.md` before wiring clients.
