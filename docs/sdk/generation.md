<!-- generated-by: gsd-doc-writer -->
# SDK generation

[Agent docs index](../agents/index.md)

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
| TypeScript | minimal fetch-based client | compact | README + `reference.md` | off by default |

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
.docs(SdkDocs::none())
```

- `reference`: output-root `README.md` and `reference.md`.
- `none`: no generated SDK docs.

Docs are part of the generated SDK surface. Prefer an explicit policy for stable output.

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
```

The minimal profile is gnr8's native generated surface. Configure any intentional public API changes
with the explicit target controls below.

## Go target controls

```rust
GoSdk::new()
    .module("github.com/acme/books")
    .go_version("1.23")
    .to("generated/go")
    .error_model("ApiError")
    .required_pointer_constructor_policy(
        RequiredPointerConstructorPolicy::value_param(),
    )
    .query_time_format(QueryTimeFormat::date_only_at_midnight_else_rfc3339())
    .request_builder_scope(GoRequestBuilderScope::operation());
```

Additional public-surface controls:

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
    .model_property_policy(TsModelPropertyPolicy::openapi_required())
    .nullable_policy(TsNullablePolicy::explicit_null())
    .response_policy(TsResponsePolicy::data_only())
    .request_body_param_name("body")
    .init_override_function(true);
```

Policies:

- Model presence: `strict` or `openapi_required`.
- Nullability: `explicit_null`, `omit_null_from_optional_properties`, `omit_null`.
- Response: `data_only`.
- Barrel: `star`.

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
style/explode, `allowReserved`, and defaults. If a generated request differs from the service
contract, correct the graph parameter/body/security fact rather than patching one emitter.

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

Related: [Transforms](../pipeline/transforms.md) and [Artifacts and CI](../operations/artifacts-and-ci.md).
