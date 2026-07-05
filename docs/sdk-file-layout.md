# SDK File Layout

gnr8 SDK targets support compact output for small APIs and split output for larger or compatibility
focused SDKs. File layout is configured on each SDK target with `SdkFileLayout`.

The same layout policy is available for Go, Python, and TypeScript:

- `SdkFileLayout::compact()` keeps operation methods and models in compact aggregate files.
- `SdkFileLayout::split()` splits models and, by default, splits client operation files by tag/group.
- `.operations_per_tag()` emits one operation file per operation tag/group.
- `.operations_per_endpoint()` emits one operation file per endpoint/operation.
- `.operation_dir("apis")` places operation files under a directory.
- `.model_dir("models")` places model files under a directory.
- `.operation_file_template(...)` customizes split operation file names.
- `.model_file_template(...)` customizes split model file names.

In gnr8, an OpenAPI "tag" is represented internally as `Operation.group`. It can come from an
OpenAPI source document's first `tags[]` value, or from code-as-config transforms such as
`GroupOperations`.

## Compact Layout

Compact is the default for all built-in SDK targets.

```rust
GoSdk::new()
    .module("example.com/acme/sdk")
    .to("generated/go");

PySdk::new()
    .module("example.com/acme/sdk")
    .to("generated/python");

TsSdk::new()
    .module("@acme/sdk")
    .to("generated/typescript");
```

Typical output:

```text
generated/go/
  client.go
  errors.go
  operations.go
  models.go
  go.mod

generated/python/
  __init__.py
  client.py
  errors.py
  models.py

generated/typescript/
  client.ts
  errors.ts
  index.ts
  models.ts
```

## Split By Tag

`SdkFileLayout::split()` defaults operation files to one file per tag/group. Untagged operations use
the synthetic `default` group.

```rust
Pipeline::new()
    .source(GoGin::new().input("."))
    .transform(
        GroupOperations::new()
            .by_path_prefix("/books", "Books")
            .by_path_prefix("/authors", "Authors"),
    )
    .target(
        PySdk::new()
            .module("example.com/acme/sdk")
            .to("generated/python")
            .layout(
                SdkFileLayout::split()
                    .operation_dir("apis")
                    .model_dir("models"),
            ),
    );
```

The same split-by-tag mode is configured on each target:

```rust
GoSdk::new()
    .module("example.com/acme/sdk")
    .to("generated/go")
    .layout(SdkFileLayout::split().operation_dir("apis"));

PySdk::new()
    .module("example.com/acme/sdk")
    .to("generated/python")
    .layout(SdkFileLayout::split().operation_dir("apis").model_dir("models"));

TsSdk::new()
    .module("@acme/sdk")
    .to("generated/typescript")
    .layout(SdkFileLayout::split().operation_dir("apis").model_dir("models"));
```

Typical output:

```text
generated/go/
  client.go
  errors.go
  facades.go
  apis/api_books.go
  apis/api_authors.go
  model_book.go
  model_author.go
  go.mod

generated/python/
  __init__.py
  client.py
  errors.py
  apis/__init__.py
  apis/api_books.py
  apis/api_authors.py
  models/__init__.py
  models/book.py
  models/author.py

generated/typescript/
  client.ts
  errors.ts
  index.ts
  apis/index.ts
  apis/api_books.ts
  apis/api_authors.ts
  models/index.ts
  models/book.ts
  models/author.ts
```

Python and TypeScript split operation modules are loaded by `client.py` / `client.ts` and attach the
same operation methods to `Client`, so existing `client.create_book(...)` or `client.createBook(...)`
call sites keep working.

## Split By Endpoint

Use `.operations_per_endpoint()` when each endpoint should get its own operation file.

```rust
TsSdk::new()
    .module("@acme/sdk")
    .to("generated/typescript")
    .layout(
        SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_dir("apis")
            .model_dir("models"),
    );
```

The endpoint split mode is also available on every SDK target:

```rust
GoSdk::new()
    .module("example.com/acme/sdk")
    .to("generated/go")
    .layout(
        SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_dir("apis"),
    );

PySdk::new()
    .module("example.com/acme/sdk")
    .to("generated/python")
    .layout(
        SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_dir("apis")
            .model_dir("models"),
    );

TsSdk::new()
    .module("@acme/sdk")
    .to("generated/typescript")
    .layout(
        SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_dir("apis")
            .model_dir("models"),
    );
```

Typical output:

```text
generated/go/
  client.go
  errors.go
  facades.go
  apis/api_create_book.go
  apis/api_list_books.go
  apis/api_list_authors.go
  model_book.go
  model_author.go
  go.mod

generated/python/
  __init__.py
  client.py
  errors.py
  apis/__init__.py
  apis/api_create_book.py
  apis/api_list_books.py
  apis/api_list_authors.py
  models/__init__.py
  models/book.py
  models/author.py

generated/typescript/
  client.ts
  errors.ts
  index.ts
  apis/index.ts
  apis/api_create_book.ts
  apis/api_list_books.ts
  apis/api_list_authors.ts
  models/index.ts
  models/book.ts
  models/author.ts
```

## File Templates

Operation file templates support these placeholders in per-endpoint mode:

- `{operation}`
- `{operation_snake}`
- `{operation_kebab}`
- `{service}`
- `{service_snake}`
- `{service_kebab}`

Per-tag mode supports the service placeholders because one file contains all operations for that
group:

- `{service}`
- `{service_snake}`
- `{service_kebab}`

Model file templates support:

- `{schema}`
- `{schema_snake}`
- `{schema_kebab}`

Examples:

```rust
SdkFileLayout::split()
    .operations_per_endpoint()
    .operation_file_template("apis/{service_snake}/{operation_snake}.ts")
    .model_file_template("models/{schema_snake}.ts");

SdkFileLayout::split()
    .operations_per_tag()
    .model_dir("schemas")
    .operation_file_template("apis/{service_snake}.py")
    .model_file_template("schemas/{schema_snake}.py");
```

If a per-tag operation template uses an operation placeholder such as `{operation_snake}`, generation
fails with a typed SDK generation error because there is no single operation name for a grouped file.
Generation also fails if two generated files render to the same path; adjust the split mode or
template so every operation and model file has a unique path.

For Python SDK operation files, every package and module path segment rendered by
`.operation_file_template(...)` must be a valid Python identifier. Use snake-case placeholders such as
`{service_snake}` or `{operation_snake}` rather than kebab-case placeholders for Python operation
modules.

## Keeping Operations Compact

You can split models while keeping operations in the compact client/operations file:

```rust
PySdk::new()
    .module("example.com/acme/sdk")
    .to("generated/python")
    .layout(
        SdkFileLayout::split()
            .compact_operations()
            .model_dir("models"),
    );
```

For a fully compact SDK, omit `.layout(...)` or use `SdkFileLayout::compact()`.
