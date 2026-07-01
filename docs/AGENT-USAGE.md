# gnr8 Agent Usage Guide

This guide is for coding agents using gnr8 inside an application repository. It is not contributor
guidance for the gnr8 repository.

## What gnr8 Does

gnr8 reads service source code, builds an API graph, and generates OpenAPI 3.1 plus typed client SDKs.
Configuration is a Rust file at `.gnr8/src/main.rs`; there is no YAML/TOML config file.

Supported source frontends:

| Source | Init flag | Pipeline stage | Required toolchain |
|--|--|--|--|
| Go + Gin | `--source go-gin` | `GoGin::new().inputs(["."])` | `go` |
| FastAPI | `--source fastapi` | `FastApi::new().inputs(["."])` | `python3` |
| Flask typed-envelope | `--source flask` | `Flask::new().inputs(["."])` | `python3` |
| NestJS class DTOs | `--source nestjs` | `NestJs::new().inputs(["src"])` | `node` + project `typescript` |
| OpenAPI/Swagger artifact | n/a | `OpenApi::new().input("openapi.yaml")` | none |

Supported targets:

| Target | Init flag | Pipeline stage |
|--|--|--|
| OpenAPI 3.1 YAML | always scaffolded | `OpenApi31::new().to("openapi.yaml")` |
| Go SDK | `--sdk go` | `GoSdk::new().module("example.com/yourservice/sdk").to("sdk")` |
| Python SDK | `--sdk python` | `PySdk::new().module("example.com/yourservice/sdk").to("sdk")` |
| TypeScript SDK | `--sdk typescript` | `TsSdk::new().module("example.com/yourservice/sdk").to("sdk")` |

## Standard Workflow

Run these commands from the root of the service repository:

```bash
gnr8 init --source fastapi --sdk python
# edit .gnr8/src/main.rs: title, base path, security, output paths, custom transforms
gnr8 generate
gnr8 doctor
gnr8 check
```

## Scenario Guides

Run `gnr8 guide <topic>` for concrete examples beyond this basic workflow:

| Topic | Use When |
|--|--|
| `go-gin-to-python-typescript` | Complex Go/Gin backend that should publish OpenAPI plus Python and TypeScript SDKs. |
| `python-apis-to-python-sdk` | FastAPI or Flask service that should publish a Python SDK. |
| `nestjs-to-typescript-sdk` | NestJS service with class DTOs that should publish a TypeScript SDK. |

## Editing `.gnr8/src/main.rs`

The scaffolded file contains a `Pipeline`. The source stage reads the service. Transform stages add
facts that source code cannot express reliably. Target stages write artifacts.

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(FastApi::new().inputs(["."]))
            .transform(SetBasePath::new("/api"))
            .transform(SetTitle::new("Public API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/public/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

Common changes:

- Change `.source(...)` when the service frontend changes.
- Change `SetBasePath` when the API is mounted under a prefix.
- Change `SetTitle` for OpenAPI `info.title`.
- Add `ApplySecurity::api_key(...)` for header API-key auth.
- Add `RenameOperation` or `RenameType` for compatibility.
- Change target output paths to keep generated files under `generated/` or `sdk/`.

## Source-Specific Notes

Go + Gin recognizes static nested Gin route groups, route methods, path/query params,
`ShouldBindJSON`, `c.JSON` responses, structs, nested structs, and string enums. Current limit: one
input directory and Gin-oriented patterns; dynamic route paths are skipped with diagnostics, and dynamic
group prefixes are omitted with diagnostics rather than guessed.

FastAPI is static: it reads Python AST and never imports/runs the app. It recognizes typed params,
Pydantic/dataclass models, `response_model`, `status_code`, `Literal`, `Enum`, and unions.

Flask is intentionally typed-envelope only. Untyped `request.json`, unannotated query reads, and
missing return annotations become diagnostics instead of guesses.

NestJS reads TypeScript through the target project's own `typescript` package. It recognizes controller
methods, params/query/body decorators, DTO classes, enums, and unions. It does not read swagger, zod, or
class-validator metadata.

OpenAPI/Swagger source reads JSON or YAML Swagger 2.0, OpenAPI 3.0, and OpenAPI 3.1 artifacts into
the same API graph used by code-first sources. Use it for brownfield generator replacement, then target
`OpenApi31`, `TsSdk`, `GoSdk`, or other SDK targets from that one graph.

```rust
Pipeline::new()
    .source(OpenApi::new().input("openapi.yaml"))
    .target(
        TsSdk::new()
            .module("@acme/books")
            .to("generated/typescript")
            .profile(SdkProfile::typescript_fetch_compat()),
    )
    .target(
        GoSdk::new()
            .module("example.com/acme/books")
            .to("generated/go")
            .profile(SdkProfile::go_openapi_generator_compat()),
    )
```

Compatibility checks compare old and new generated SDK surfaces:

```bash
gnr8 compat typescript --old old-typescript-sdk --new generated/typescript
gnr8 compat go --old old-go-sdk --new generated/go
```

For OpenAPI Generator migrations, use compatibility profiles first and clean profiles later:

```rust
TsSdk::new()
    .module("@acme/books")
    .to("generated/typescript")
    .profile(SdkProfile::typescript_fetch_compat())
    .layout(SdkFileLayout::split().model_file_template("models/{schema_kebab}.ts"));

GoSdk::new()
    .module("example.com/acme/books")
    .to("generated/go")
    .profile(SdkProfile::go_openapi_generator_compat());
```

`gnr8 generate --json` includes a `cleanup` section for migration review: files gnr8 owns, stale
generated files removed, generated-looking unowned files, protected hand edits, legacy package files,
and old generator dependencies to remove.

## Generated SDKs

SDK targets write `README.md` and `reference.md` in the SDK output directory. Read those files before
calling the client. They list operation IDs, paths, request schemas, response statuses, schemas, and
diagnostics from the generation run.

The SDK output is generated. Do not patch generated client files by hand; update service source or
`.gnr8/src/main.rs`, then run `gnr8 generate`.

## Failure Recovery

| Symptom | Action |
|--|--|
| `no .gnr8/ workspace` | Run `gnr8 init --source ... --sdk ...` from the service root. |
| Pipeline compile error | Fix `.gnr8/src/main.rs`; it is ordinary Rust. |
| Missing source toolchain | Install `go`, `python3`, or `node` plus project `typescript`. |
| Generated file skipped as user-edited | Inspect the edit; run `gnr8 generate --force` only if overwrite is intended. |
| `gnr8 check` exits 1 | Run `gnr8 generate`; commit updated generated artifacts. |
| Diagnostics in `doctor` | Prefer typed source/config changes over guessing undocumented behavior. |

## CI

Use `gnr8 check` as the gate after generated files have been committed:

```bash
gnr8 generate
gnr8 check
```

`check` exits non-zero when outputs are stale or protected by user edits.

GitHub Actions example:

```yaml
name: gnr8
on: [pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with: { go-version: '1.23' }
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - run: gnr8 generate
      - run: gnr8 check
```
