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

## Field presence in generated models

Whether a model lets a key be left out is answered from the direction its schema is reached from, the
same walk that answers the OpenAPI
[`required` array](../openapi/generation.md#a-component-schemas-required-array).

| The schema is reached from | The model may leave the key out when |
|---|---|
| requests only (a request body, a parameter, or a schema one of those reaches) | the source states no validation rule requiring it |
| responses only | the source's serializer may leave the key out |
| both | the exact answer for each generated input/output model; gnr8 splits the model when they differ |
| registered non-HTTP use | the corresponding input/output answer |

A validation rule says what your server rejects an inbound payload for lacking, so a model reads it
exactly where the model is inbound and only inbound. In Go that matters most: an omission option
governs marshalling and a server unmarshals a request DTO rather than marshalling one, so a field
written `json:"name,omitempty"` with `binding:"required"` is required in the request model —
previously a caller who set it to the zero value sent nothing and the server rejected the call.

Everywhere else the model is the decode side, and demanding a key the server may omit would reject a
valid response. A type reached from both directions is projected into distinct `TypeInput` and
`TypeOutput` models whenever its own or a nested contract differs.

Nullability is selected separately. It changes only the value hint: a required nullable response is
`field: T | null` in TypeScript and an `Optional[T]` Python field with no omission default. An
optional non-null response is `field?: T`; it does not gain `| null`. Request nullability reads what
decoding accepts after validation, independently of what the same source type can serialize.

### How each language spells it

`TsSdk` uses `?:` and `PySdk` a `= None` default for absence. Nullable values use `| null` and
`Optional[...]` respectively; those value hints do not add an absence default.

`GoSdk` combines a pointer representation with `,omitempty` for an optional value type. Nil means the
key is absent, while a non-nil pointer preserves an explicit zero value such as `0`, `""`, or a zero
struct. A required nullable value type is also a pointer but has no omission tag. Slices and maps are
already nil-capable, so their direction-selected tag supplies the presence behavior without another
pointer layer.

FastAPI, Flask, NestJS, and an imported OpenAPI document normally state the same contract in both
directions, so they do not split unless configured facts make the uses differ.

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

## Go target controls

```rust
GoSdk::new()
    .module("github.com/acme/books")
    .go_version("1.23")
    .to("generated/go");
```

The generated Go SDK uses one ctx-first typed method surface, functional client options, explicit
request structs, and graph-derived wire behavior.

Exported Go identifiers are CamelCase of the wire token with Go initialisms applied, including when
the initialism is pluralized:

| Wire token | Go identifier |
|---|---|
| `uuid` | `UUID` |
| `stepUuids` | `StepUUIDs` |
| `primaryFileId` | `PrimaryFileID` |
| `labelIds` | `LabelIDs` |
| `siteUrls` | `SiteURLs` |
| `publicApis` | `PublicAPIs` |

This spelling is Go-local. The json tag, query key, path template, and OpenAPI property name keep the
wire token exactly, and the TypeScript and Python targets keep their own language-native casing
(`stepUuids`, `step_uuids`). Use `RenameType` / `RenameOperation` when you want a different canonical
name.

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
    .to("generated/typescript");
```

The generated TypeScript SDK preserves graph optionality and nullability exactly and returns decoded
response data through the native fetch client.

### TypeScript call shape

Each operation takes its path parameters positionally, then the typed request body, then ONE params
object carrying every remaining request parameter, then `RequestOptions`:

```ts
export type GetItemsPaginatedParams = {
  cursor?: string;
  kinds?: string[];
  pageSize?: number;
};

await client.getItemsPaginated({ cursor, pageSize: 50 });
await client.getItem(itemId, { verbose: true });
await client.createItem(body, { notify: true });
await client.replaceItem(itemId, body, { dryRun: true });
```

| Operation shape | Signature |
|---|---|
| Query/header params, all optional | `op(params?: OpParams, options?: RequestOptions)` |
| Query/header params, any required | `op(params: OpParams, options?: RequestOptions)` |
| Path + params | `op(id: string, params?: OpParams, options?: RequestOptions)` |
| Body + params | `op(body: Body, params?: OpParams, options?: RequestOptions)` |
| Path + body + params | `op(id: string, body: Body, params?: OpParams, options?: RequestOptions)` |
| No request parameters | `op(options?: RequestOptions)` — no params argument |

The params type is named `{OperationId}Params` in PascalCase, is declared in `client.ts` next to
`RequestOptions`, and is re-exported from the package root. Header parameters ride the same object;
cookies and browser-forbidden headers stay with the fetch transport and never appear on it. Wire
names, `style`, and `explode` are unaffected — the object changes how a caller writes the call, not
what goes on the wire.

A `{OperationId}Params` name that would collide with a schema name, with another operation's params
type, or with a symbol `client.ts` already exports is a typed generation error, not a broken emit.

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
