<!-- generated-by: gsd-doc-writer -->
# SDK generation

`GoSdk`, `PySdk`, and `TsSdk` render one shared graph into language-native clients, models, runtime
support, generated reference docs, and optional package metadata. Configure API meaning in transforms;
configure file/public-surface policy on the target.

## Minimal targets

```rust
.target(
    GoSdk::new()
        .module("github.com/acme/books-sdk-go")
        .to("generated/go"),
)
.target(
    PySdk::new()
        .module("acme-books")
        .to("generated/python"),
)
.target(
    TsSdk::new()
        .module("@acme/books")
        .to("generated/typescript"),
)
```

Every SDK target requires `module` and `to`. The module/import path is the single source used to
derive the generated package name unless package metadata supplies a registry name.

## Defaults

| Target | Model/runtime default | Layout | Docs | Package metadata |
|---|---|---|---|---|
| Go | Go 1.23, minimal client | compact | README + `reference.md` | `go.mod` + `PUBLISHING.md` |
| Python | Pydantic v2, minimal client | compact | README + `reference.md` | `pyproject.toml` + `PUBLISHING.md` |
| TypeScript | minimal fetch-based client | compact | README + `reference.md` | off for minimal; on for fetch/axios compatibility profiles |

Package metadata defaults to version `0.1.0`. `source_only()` disables generated docs and package
metadata. `without_docs()` disables docs only; `package_metadata(bool)` controls metadata files.

## File layouts

```rust
SdkFileLayout::compact()

SdkFileLayout::split()
    .operations_per_tag()
    .operation_dir("apis")
    .model_dir("models")
    .operation_file_template("apis/{service_snake}/{operation_snake}.ts")
    .model_file_template("models/{schema_snake}.ts")
```

Split operation choices are `compact_operations`, `operations_per_tag` (the split default), and
`operations_per_endpoint`. Use `root_operations`/`root_models` to keep split files at package root.
Placeholders:

- Operation: `{operation}`, `{operation_snake}`, `{operation_kebab}`, `{service}`,
  `{service_snake}`, `{service_kebab}`.
- Model: `{schema}`, `{schema_snake}`, `{schema_kebab}`.

`service` comes from the operation group/tag; ungrouped operations use `default`. Unsafe paths or
unknown placeholders fail generation. Target shortcuts `.split_files()` choose per-endpoint operations
and a `models` directory.

## Generated documentation

```rust
.docs(SdkDocs::reference())
.docs(SdkDocs::openapi_generator_compat().dir("docs"))
.docs(SdkDocs::both().readme_links(true))
.docs(SdkDocs::none())
```

- `reference`: output-root `README.md` and `reference.md`.
- `openapi_generator_compat`: per-API/per-model Markdown under `docs/` (or `.dir(...)`).
- `both`: both layouts and README links.
- `none`: no generated SDK docs.

Docs are part of SDK compatibility surface. Prefer explicit docs policy during generator migrations.

## Package metadata

```rust
let package = SdkPackageMetadata::new()
    .registry_name("@acme/books")
    .version("2.3.0")
    .description("Typed Books API client")
    .license("MIT")
    .repository("https://github.com/acme/books")
    .homepage("https://example.com/books")
    .documentation("https://docs.example.com/books")
    .keywords(["books", "sdk"]);

TsSdk::new()
    .module("@acme/books")
    .to("generated/typescript")
    .package(package);
```

`name` aliases `registry_name`; `keyword` adds one value. Go and Python metadata are enabled by
default. Calling `.package(...)` enables TypeScript metadata unless explicitly overridden.

## Profiles

```rust
SdkProfile::minimal()
SdkProfile::typescript_fetch_compat()
SdkProfile::typescript_axios_compat()
SdkProfile::openapi_generator_compat() // TypeScript axios compatibility alias
SdkProfile::go_openapi_generator_compat()
```

Compatibility profiles reproduce common OpenAPI Generator public/runtime layouts. Use them while
replacing an existing generator, prove the public surface with `gnr8 compat`, then remove legacy
shims deliberately. Do not apply a TypeScript profile to a Go target or vice versa.

## Go target controls

```rust
GoSdk::new()
    .module("github.com/acme/books")
    .go_version("1.23")
    .to("generated/go")
    .profile(SdkProfile::go_openapi_generator_compat())
    .error_model("ApiError")
    .required_pointer_constructor_policy(
        RequiredPointerConstructorPolicy::value_param(),
    )
    .query_time_format(QueryTimeFormat::date_only_at_midnight_else_rfc3339())
    .request_builder_scope(GoRequestBuilderScope::operation());
```

Brownfield-only controls:

- `GoRequestBuilderAliases`: preserve selected body/query setter names by request type, operation ID,
  or route.
- `GoQuerySetterArgumentPolicy`: typed, `any`, or selected `any` setters.
- `GoExecuteCompatibility`: preserve selected legacy `Execute` signatures.
- `module_path` is an alias for `module`.

```rust
.request_builder_aliases(
    GoRequestBuilderAliases::new()
        .operation("POST", "/books")
        .body("Book")
        .operation("GET", "/books")
        .query("PageSize", "pageSize"),
)
.query_setter_argument_policy(
    GoQuerySetterArgumentPolicy::typed().any_for_queries(["sort", "filter"]),
)
.execute_compatibility(
    GoExecuteCompatibility::preserve_legacy().route("GET", "/books"),
)
```

## Python target controls

```rust
PySdk::new()
    .module("acme-books")
    .to("generated/python")
    .pydantic()
    .package_version("2.3.0");
```

`pydantic()` is the default and emits Pydantic v2 models. `dataclasses()` emits stdlib dataclasses for
no-dependency consumers. `PyModelStyle` exposes the same choice when a reusable value is needed.

## TypeScript target controls

```rust
TsSdk::new()
    .module("@acme/books")
    .to("generated/typescript")
    .profile(SdkProfile::typescript_fetch_compat())
    .model_property_policy(TsModelPropertyPolicy::openapi_required())
    .nullable_policy(TsNullablePolicy::explicit_null())
    .response_policy(TsResponsePolicy::data_only())
    .request_body_param_name("body")
    .init_override_function(true)
    .barrel_exports(TsBarrelExports::openapi_generator_compat());
```

Policies:

- Model presence: `strict`, `openapi_required`, `openapi_generator_loose`.
- Nullability: `explicit_null`, `omit_null_from_optional_properties`, `omit_null`.
- Response: `data_only`, `axios_response_wrapper`.
- Barrel: `star`, `openapi_generator_compat`.
- `compatibility(TsCompatibility::OpenApiGenerator)` is the concise legacy-profile API.

## Type aliases

Expose old public names without renaming canonical graph schemas:

```rust
let aliases = SdkTypeAliases::new()
    .type_alias("Book", "BookResponse")
    .source_prefix_alias("/transport/", "Transport");

GoSdk::new()
    .module("github.com/acme/books")
    .to("generated/go")
    .aliases(aliases);
```

Each target also has `.type_alias(schema, alias)` as a one-off shortcut. Missing, ambiguous,
duplicate, or colliding aliases fail.

## Request wire behavior

All built-in SDKs share graph semantics for path, query, header, cookie, body, security,
style/explode, `allowReserved`, and defaults. Executable tests cover standard Go, Go compatibility,
standard TypeScript, axios/fetch compatibility, and Python profiles. If a generated request differs
from the service contract, correct the graph parameter/body/security fact rather than patching one
emitter.

## Static companion files

`StaticFiles` copies declared files into the artifact set:

```rust
.target(
    StaticFiles::new()
        .from("sdk-static")
        .to("generated/typescript")
        .include(["LICENSE", "templates/**"]),
)
```

An include ending in `/**` copies a directory tree; other includes name exact files. Artifact path
collisions fail unless an explicit custom overlay/rewrite owns the transition.

Related: [SDK compatibility](compatibility.md), [Transforms](../pipeline/transforms.md), and
[Artifacts and CI](../operations/artifacts-and-ci.md).
