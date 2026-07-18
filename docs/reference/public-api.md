<!-- generated-by: gsd-doc-writer -->
# Public API map

[Agent docs index](../agents/index.md) ·
[latest rustdoc](https://docs.rs/gnr8/latest/gnr8/sdk/prelude/index.html)

Application pipelines should normally import:

```rust
use gnr8::sdk::prelude::*;
```

`gnr8::prelude` is an alias. This page maps every symbol currently exported by the SDK prelude. Use
the feature pages for behavior and examples; use rustdoc for complete method signatures.

## Pipeline core

| Symbol | Use |
|---|---|
| `Pipeline` | compose one source, ordered transforms, targets, and post-processors |
| `Source` | trait for project source/artifact → `ApiGraph` |
| `Transform` | trait for ordered graph mutation |
| `Target` | trait for graph → artifacts |
| `PostProcess` | trait for artifact transformation after targets |
| `Cx` | stage context containing project root |
| `Artifact` | one project-relative UTF-8 generated file plus ownership metadata |
| `Artifacts` | sorted artifact set with create/overlay/rewrite enforcement |
| `ArtifactMetadata` | artifact path and content hash without text |
| `FileStamp` | cached path/length/mtime/hash identity |

See [Pipeline configuration](../pipeline/configuration.md) and
[Artifacts and CI](../operations/artifacts-and-ci.md).

## Sources

| Symbol | Use |
|---|---|
| `GoGin` | static Go/Gin route, request, response, and schema extraction |
| `FastApi` | static FastAPI/Python extraction |
| `Flask` | static typed-envelope Flask extraction |
| `NestJs` | static NestJS/TypeScript extraction |
| `OpenApi` | Swagger 2.0/OpenAPI 3.x JSON/YAML import into the graph |

See [Sources and extraction](../extraction/sources.md).

## General transforms

| Symbol | Use |
|---|---|
| `SetBasePath` | set API mount/base path |
| `SetTitle` | set document title |
| `OpenApiMetadata` | set public info/contact/license/server metadata |
| `RenameOperation` | rename one operation ID |
| `RenameType` | rename one schema and rewrite references |
| `SetOperationSuccessResponse` | set exact typed 2xx response |
| `SetSchemaFieldType` | replace one object field type |
| `SetEnumOrder` | choose one enum's member order |
| `EnumOrder` | lexical, source, or explicit enum-order policy |
| `GroupOperations` | assign SDK operation groups by ordered rules |
| `SdkOperationAliases` | preserve route-scoped SDK operation name/tag |
| `DiagnosticPolicy` | deny exact remaining diagnostic codes/categories |
| `DiagnosticCategory` | stable diagnostic policy/reporting category enum |

See [Transforms and overrides](../pipeline/transforms.md).

## Selectors, overrides, and security

| Symbol | Use |
|---|---|
| `OperationSelector` | reusable exact/prefix/method/middleware/boolean selector |
| `ApiOverrides` | checked field, parameter, body, response, and security corrections |
| `QueryParam` | legacy query-only override builder |
| `RequestParameter` | typed query/header/path/cookie parameter builder |
| `ParameterOverride` | add-if-missing, correct-existing, or replace semantics |
| `ResponseOverride` | exact status/body/media response replacement |
| `SecurityOverride` | exact public/OR/AND operation security replacement |
| `ApplySecurity` | define api-key, bearer, or basic scheme globally/conditionally |
| `SecurityScheme` | low-level graph security scheme representation |
| `Type` | shared graph type vocabulary and scalar/array/enum helpers |

## Runtime, pagination, and public docs

| Symbol | Use |
|---|---|
| `ConfigureSdkRuntime` | timeout, retry, unsafe-method, and hook defaults |
| `MarkIdempotent` | mark selected operations safe for retries |
| `ConfigurePagination` | configure cursor/page/offset SDK helpers |
| `DocumentOperation` | summaries, descriptions, tags, examples, documented errors |
| `PaginationMode` | cursor, page, or offset mode enum |
| `PaginationTermination` | no-next-cursor or empty-items termination enum |
| `RuntimeHookKind` | request, response, or error hook kind |
| `OpenApiContact` | contact metadata builder |
| `OpenApiLicense` | license metadata builder |
| `OpenApiServer` | server metadata builder |

## OpenAPI targets and patches

| Symbol | Use |
|---|---|
| `OpenApi31` | deterministic OpenAPI 3.1 YAML target |
| `OpenApi31Json` | deterministic pretty OpenAPI 3.1 JSON target |
| `OpenApiSchemaAliases` | `$ref` or cloned component aliases |
| `OpenApiSchemaPatch` | collect field patches for one schema |
| `OpenApiFieldPatch` | constraints, enum order, docs, default/example/extensions for one field |

See [OpenAPI generation](../openapi/generation.md).

## SDK targets and shared policy

| Symbol | Use |
|---|---|
| `GoSdk` | Go client/model/docs/package target |
| `PySdk` | Python client/model/docs/package target |
| `TsSdk` | TypeScript client/model/docs/package target |
| `SdkProfile` | minimal and generator-compatibility surface presets |
| `SdkFileLayout` | compact/split files, directories, and templates |
| `OperationFileSplit` | compact/per-tag/per-endpoint operation layout enum |
| `SdkDocs` | none/reference/OpenAPI-Generator-compatible/both docs policy |
| `SdkPackageMetadata` | registry name, version, description, URLs, license, keywords |
| `SdkTypeAliases` | explicit and source-prefix public type aliases |
| `SdkModel` | normalized target-facing SDK model built from the graph |
| `PyModelStyle` | Pydantic v2 or stdlib dataclass model policy |
| `StaticFiles` | copy exact companion files or included directory trees |

See [SDK generation](../sdk/generation.md).

## Go SDK compatibility controls

| Symbol | Use |
|---|---|
| `RequiredPointerConstructorPolicy` | pointer or value constructor arguments for required pointer fields |
| `QueryTimeFormat` | default or date-at-midnight/RFC3339 query formatting |
| `GoRequestBuilderScope` | operation-local or graph-global legacy setters |
| `GoRequestBuilderAliases` | selected legacy body/query setter aliases |
| `GoRequestBuilderOperationAliases` | route/operation-scoped alias continuation builder |
| `GoQuerySetterArgumentPolicy` | typed, `any`, or selectively widened setters |
| `GoExecuteCompatibility` | preserve selected legacy `Execute` signatures |

## TypeScript SDK compatibility controls

| Symbol | Use |
|---|---|
| `TsCompatibility` | concise OpenAPI Generator compatibility selection |
| `TsModelPropertyPolicy` | strict/OpenAPI-required/legacy-loose optional properties |
| `TsNullablePolicy` | explicit or omitted nullable unions |
| `TsResponsePolicy` | data-only or Axios response-wrapper return shape |
| `TsBarrelExports` | star or collision-aware compatibility barrel |

## Post-processors

| Symbol | Use |
|---|---|
| `Header` | rewrite generated Go files with a generated marker |
| `FormatCommand` | run an external formatter over a temporary artifact tree |

## Important public APIs outside the prelude

| Path | Use |
|---|---|
| `gnr8::runner::run` | required `.gnr8` child entry point |
| `gnr8::runner::ArtifactBundle` | versioned child/host wire envelope |
| `gnr8::runner::PROTOCOL_VERSION` | current host/child protocol number |
| `gnr8::graph::ApiGraph` | neutral extracted/transformed API graph |
| `gnr8::CoreError` | typed core error enum |
| `gnr8::sdk::compat` | Go/Python/TypeScript surface extract/diff/contract APIs |
| `gnr8::sdk::openapi_compat` | exact OpenAPI compare/report APIs |
| `gnr8::sdk::validate_openapi_artifact` | generated OpenAPI readiness validation |

Prefer the CLI for compatibility and lifecycle operations. Direct module APIs are useful for custom
tooling and tests.

## Choosing the right extension seam

| Need | Use |
|---|---|
| add/change API meaning for every target | custom `Transform` |
| emit a new artifact format | custom `Target` |
| normalize already-declared files | custom `PostProcess` or `FormatCommand` |
| preserve a legacy SDK name | target alias/profile control |
| compare released public packages | CLI `compat`, not a generation transform |

Keep custom stages deterministic, return `CoreError` instead of panicking, and use explicit artifact
ownership transitions.
