# Brownfield OpenAPI Generator Migration

Use this when replacing OpenAPI Generator without changing the source contract first. The clean path is:

```text
OpenApi/Swagger artifact -> ApiGraph -> compatibility SDK profile -> compat diff
```

## Before

Typical OpenAPI Generator commands look like:

```bash
openapi-generator-cli generate -i openapi.yaml -g typescript-fetch -o old-typescript-sdk
openapi-generator-cli generate -i openapi.yaml -g go -o old-go-sdk --package-name books
```

Keep those old outputs available as compatibility baselines until the gnr8 migration is accepted.

## After

Create or edit `.gnr8/src/main.rs`:

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(OpenApi::new().input("openapi.yaml"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(
                TsSdk::new()
                    .module("@acme/books")
                    .to("generated/typescript")
                    .profile(SdkProfile::typescript_fetch_compat())
                    .docs(SdkDocs::openapi_generator_compat().dir("docs")),
            )
            .target(
                GoSdk::new()
                    .module("example.com/acme/books")
                    .to("generated/go")
                    .profile(SdkProfile::go_openapi_generator_compat())
                    .docs(SdkDocs::both()),
            ),
    )
}
```

Then run:

```bash
gnr8 generate --json
gnr8 compat typescript --old old-typescript-sdk --new generated/typescript
gnr8 compat go --old old-go-sdk --new generated/go
```

## Review Workflow

1. Inspect `gnr8 generate --json`.
   The `cleanup` section lists owned files, stale generated files deleted by gnr8, protected hand-edited files, generated-looking unowned files, likely OpenAPI Generator package/config remnants, and old generator dependencies to remove.

2. Inspect compatibility diffs.
   TypeScript checks exports, API classes/factories, request aliases, method signatures, operation return wrappers, model optionality/nullability/type drift, and package entry points. Go checks exported types, exported functions, exported methods, normalized signatures, docs presence, and `go.mod` metadata.

3. Fix at the profile/config layer.
   Prefer `SdkProfile::typescript_fetch_compat()`, `SdkProfile::typescript_axios_compat()`, `SdkProfile::go_openapi_generator_compat()`, `SdkOperationAliases`, `OperationSelector`, `ApiOverrides`, and explicit output layout/profile controls over postprocessing generated files.

4. Accept only explainable drift.
   The clean migration gate is: SDKs generate from the OpenAPI source without postprocessing; strict/minimal SDK outputs stay unchanged; remaining compatibility differences are either eliminated by profiles or reported by `gnr8 compat`.

## Supported Import Surface

The `OpenApi` source accepts Swagger 2.0, OpenAPI 3.0, and OpenAPI 3.1 in JSON or YAML. It resolves local and relative `$ref`s, imports tags as operation groups, handles `allOf`, nullable fields, maps, arrays, enums, Swagger `formData`, multipart file uploads, dotted definition names, and naming collisions.

Unsupported constructs are reported as diagnostics on the graph instead of guessed.

## Layout and Metadata Controls

Keep clean SDK defaults for new consumers. Use compatibility controls only for migration outputs:

```rust
TsSdk::new()
    .module("@acme/books")
    .to("generated/typescript")
    .profile(SdkProfile::typescript_axios_compat())
    .layout(
        SdkFileLayout::split()
            .operation_file_template("apis/{service_kebab}/{operation_kebab}.ts")
            .model_file_template("models/{schema_kebab}.ts"),
    )
    .package_metadata(true)
    .docs(SdkDocs::both());

GoSdk::new()
    .module("example.com/acme/books")
    .to("generated/go")
    .profile(SdkProfile::go_openapi_generator_compat())
    .layout(SdkFileLayout::split().root_operations().root_models())
    .docs(SdkDocs::openapi_generator_compat().dir("docs"));
```

Use `.source_only()` when a parent package or monorepo already owns docs/package metadata. Use
`.without_docs()` or `.package_metadata(false)` when only one of those categories should be suppressed.
Existing `.docs(true)` and `.docs(false)` calls still work; pass `SdkDocs` when the docs layout itself
is part of the migration contract.

Generated documentation modes:

```rust
GoSdk::new()
    .docs(SdkDocs::reference()); // README.md + reference.md

TsSdk::new()
    .docs(SdkDocs::openapi_generator_compat().dir("docs")); // docs/BooksApi.md, docs/Book.md

GoSdk::new()
    .docs(SdkDocs::both().readme_links(true)); // both layouts with README links
```

`SdkDocs::openapi_generator_compat()` emits one markdown file per API group and one per model under
the configured docs directory. Filenames are derived from generated SDK symbols, so a `books` group
gets `docs/BooksApi.md` and a `CreateBookRequest` model gets `docs/CreateBookRequest.md`.

## Docs Compatibility Contracts

`gnr8 compat` reports missing docs separately from code API breaks. A migration can intentionally
accept docs-layout churn without accepting code API breakage by passing a TOML contract:

```toml
[allow]
docs_layout_migration = true
```

or allow only known removed docs:

```toml
[allow]
missing_docs = ["docs/InternalDebugApi.md"]
```

Then run:

```bash
gnr8 compat typescript --old old-typescript-sdk --new generated/typescript --contract compat.toml
gnr8 compat go --old old-go-sdk --new generated/go --contract compat.toml
```

## Production Migration Patches

Prefer graph transforms over postprocessing generated files:

```rust
let active_school = OperationSelector::any([
    OperationSelector::path_prefix("/v1/schools/active/"),
    OperationSelector::path_prefix("/v1/import-jobs/"),
]);
let mutating = OperationSelector::methods(["POST", "PUT", "PATCH", "DELETE"]);

Pipeline::new()
    .source(OpenApi::new().input("openapi.yaml"))
    .transform(
        ApplySecurity::api_key("ActiveSchoolAuth", "X-Plint-School-Id")
            .when(active_school.clone()),
    )
    .transform(
        ApplySecurity::api_key("CSRFAuth", "X-CSRF-Token")
            .when(OperationSelector::all([active_school, mutating])),
    )
    .transform(
        ApiOverrides::new()
            .query_param(
                "GET",
                "/v1/schools/active/schedule/week",
                QueryParam::new("startDate").date().required(),
            )
            .binary_response("GET", "/v1/schools/active/files/{fileId}/download", 200),
    )
    .transform(
        SdkOperationAliases::new()
            .operation("GET", "/v1/schools/active/files/{fileId}/download")
            .tag("files")
            .name("downloadSchoolFile"),
    )
    .target(
        TsSdk::new()
            .module("@acme/books")
            .to("generated/typescript")
            .profile(SdkProfile::typescript_fetch_compat()),
    )
```

For project-specific generated support files, put hand-authored files under source control and copy
them through the lifecycle:

```rust
StaticFiles::new()
    .from("sdk-static/typescript")
    .to("generated/typescript")
    .include(["README.md", "docs/**"]);
```

## CI Gate

Run generation and drift checks on every PR after the generated outputs are committed:

```yaml
name: gnr8
on: [pull_request]
jobs:
  generated:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with: { go-version: '1.23' }
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - run: gnr8 generate
      - run: gnr8 check
      - run: gnr8 compat typescript --old old-typescript-sdk --new generated/typescript
      - run: gnr8 compat go --old old-go-sdk --new generated/go
```
