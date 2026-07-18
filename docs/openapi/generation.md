<!-- generated-by: gsd-doc-writer -->
# OpenAPI generation

[Agent docs index](../agents/index.md)

`OpenApi31` emits deterministic OpenAPI 3.1 YAML. `OpenApi31Json` emits the same lowered document as
pretty JSON. Both consume the final shared graph, so transforms also affect generated SDKs.

## Minimal targets

```rust
.target(OpenApi31::new().to("generated/openapi.yaml"))
.target(OpenApi31Json::new().to("generated/openapi.json"))
```

Each target requires a project-relative output file. Configure only one of them unless the project
intentionally publishes both formats.

## Complete example

```rust
Pipeline::new()
    .source(FastApi::new().inputs(["."]))
    .transform(SetBasePath::new("/v1"))
    .transform(
        OpenApiMetadata::new()
            .title("Books API")
            .version("1.4.0")
            .description("Public books endpoints")
            .server("https://api.example.com"),
    )
    .transform(ApplySecurity::bearer("BearerAuth"))
    .target(
        OpenApi31::new()
            .to("generated/openapi.yaml")
            .schema_aliases(
                OpenApiSchemaAliases::new()
                    .alias("Book", "BookResponse")
                    .clone_alias("Book", "LegacyBook"),
            )
            .schema_patch(
                OpenApiSchemaPatch::new("Book").field(
                    OpenApiFieldPatch::new("status")
                        .description("Lifecycle state")
                        .enum_values_in_order(["draft", "published"])
                        .example_string("draft")
                        .extension_bool("x-public", true),
                ),
            ),
    );
```

## What is emitted

The lowerer writes graph-backed OpenAPI facts including:

- `openapi: 3.1.0`, `info`, servers, tags, paths, and operation metadata.
- Path/query/header/cookie parameters with style, explode, defaults, and requiredness.
- Request bodies, responses, content types, examples, and descriptions.
- Component schemas, references, constraints, nullable/union shapes, arrays, maps, and enums.
- Security schemes plus document- and operation-level requirements.
- Stable operation IDs and deterministic component/order output.

YAML and JSON targets are semantic equivalents. Use JSON when downstream tooling benefits from an
unambiguous machine format; use YAML for a human-reviewed artifact.

## Metadata

`OpenApiMetadata` sets title, version, description, terms of service, contact, license, and one or more
servers. `SetTitle` remains a title-only shortcut. Metadata transforms belong before the target.

```rust
OpenApiMetadata::new()
    .title("Orders API")
    .version("2.0.0")
    .contact(
        OpenApiContact::new()
            .name("Platform")
            .email("platform@example.com"),
    )
    .license(OpenApiLicense::new("Apache-2.0"))
    .described_server("https://api.example.com", "production");
```

## Schema aliases

```rust
OpenApiSchemaAliases::new()
    .alias("CanonicalBook", "Book")
    .clone_alias("CanonicalBook", "LegacyBook")
```

- `alias` emits an alias schema that points at the canonical schema with `$ref`.
- `clone_alias` emits a separate copy of the canonical schema body for consumers that require an
  independent component shape.

`canonical` must be the exact emitted component key; graph schema IDs are not resolved here. Unknown
canonical names, alias collisions, and duplicate aliases fail generation.

## Field patches

Patches are target-specific presentation policy. They do not mutate the graph or other targets.

| Feature | Methods |
|---|---|
| String bounds | `min_length`, `max_length` |
| Numeric bounds | `minimum`, `maximum` |
| Enum members | `enum_values` (sorted), `enum_values_in_order` |
| Description | `description` |
| Defaults | `default_string`, `default_number`, `default_bool` |
| Examples | `example_string`, `example_number`, `example_bool`, `example_null` |
| Extensions | `extension_string`, `extension_number`, `extension_bool`, `extension_null` |

Every extension name must be an `x-...` key. Use transforms for semantic corrections shared by SDKs;
use target patches only for OpenAPI-only compatibility polish.

## Generated artifact validation

`gnr8 doctor` parses generated OpenAPI, checks that it is OpenAPI 3.x, verifies local `$ref` targets,
and checks stable operation/schema naming. It is a readiness check, not an exact baseline comparison.

```bash
gnr8 generate
gnr8 doctor
gnr8 check
```

For a migration with a reference specification:

```bash
gnr8 compat openapi \
  --old legacy/openapi.yaml \
  --new generated/openapi.yaml
```

See [OpenAPI compatibility](compatibility.md) for exact semantics.
