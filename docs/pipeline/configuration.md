<!-- generated-by: gsd-doc-writer -->
# Pipeline configuration

[Agent docs index](../agents/index.md)

The only gnr8 configuration is a project-local Rust binary at `.gnr8/src/main.rs`. It depends on the
`gnr8` crate, composes a `Pipeline`, and hands it to `gnr8::runner::run`.

## Minimal complete pipeline

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(FastApi::new().inputs(["."]))
            .transform(SetBasePath::new("/api"))
            .transform(SetTitle::new("Public API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(
                PySdk::new()
                    .module("example.com/public/sdk")
                    .to("generated/sdk"),
            )
            .post(Header::generated()),
    )
}
```

Required shape:

- Exactly one source must be configured.
- Transforms run in declaration order against one mutable `ApiGraph`.
- Every target reads the same final graph and adds artifacts.
- Post-processors run in declaration order after all targets.
- All configured paths are project-relative unless an API explicitly states otherwise.

## Stage traits

| Trait | Input | Output | Built-in examples |
|---|---|---|---|
| `Source` | `Cx` | `ApiGraph` | `GoGin`, `FastApi`, `Flask`, `NestJs`, `OpenApi` |
| `Transform` | mutable `ApiGraph`, `Cx` | changed graph | `SetBasePath`, `ApiOverrides` |
| `Target` | frozen `ApiGraph`, `Cx` | additions to `Artifacts` | `OpenApi31`, `GoSdk` |
| `PostProcess` | `Artifacts`, `Cx` | rewritten artifacts | `Header`, `FormatCommand` |

`Cx::project_root` is the root used to resolve relative inputs and output-related files.

## Ordering rules

Ordering is semantic. Put graph corrections before consumers and policy gates:

```rust
Pipeline::new()
    .source(GoGin::new().inputs(["."]))
    .transform(SetSchemaFieldType::new("Event", "payload", Type::Any {}))
    .transform(
        ApiOverrides::new()
            .json_request_body("POST", "/events", "CreateEvent")
            .optional(),
    )
    .transform(DiagnosticPolicy::new().deny("request.body.unresolved"))
    .target(OpenApi31::new().to("generated/openapi.yaml"));
```

The correction can retire a matching unresolved diagnostic. Placing `DiagnosticPolicy` first would
fail before the correction runs.

## Multiple targets

One graph can produce semantically aligned artifacts:

```rust
Pipeline::new()
    .source(OpenApi::new().input("openapi.yaml"))
    .transform(SetBasePath::new("/v1"))
    .target(OpenApi31Json::new().to("generated/openapi.json"))
    .target(
        GoSdk::new()
            .module("example.com/acme/client")
            .to("generated/go"),
    )
    .target(
        TsSdk::new()
            .module("@acme/client")
            .to("generated/typescript"),
    );
```

Do not duplicate the same correction in individual targets. Put API meaning in transforms; keep
target-specific file layout and compatibility policy on targets.

## Custom stages

The traits are public. A custom target must use explicit artifact ownership methods:

```rust
use gnr8::{graph::ApiGraph, CoreError};
use gnr8::sdk::prelude::*;

struct SummaryTarget;

impl Target for SummaryTarget {
    fn generate(
        &self,
        graph: &ApiGraph,
        out: &mut Artifacts,
        _cx: &Cx,
    ) -> Result<(), CoreError> {
        out.create(
            "generated/summary.txt",
            format!("operations={}\n", graph.operations.len()),
        )
    }
}
```

Use `create` for a new path, `overlay` for intentional full replacement, and `rewrite` for an
intentional in-place transformation. Collisions and missing ownership targets are errors. See
[Artifacts and CI](../operations/artifacts-and-ci.md).

## Dependency and lockfile policy

- Commit `.gnr8/Cargo.toml` and `.gnr8/Cargo.lock`.
- Keep the direct `gnr8` dependency and installed CLI on the same release.
- The host and child exchange protocol, CLI/core version, and capability fingerprints before output
  is trusted.
- When upgrading, update the dependency, regenerate the lockfile, install the same CLI version, run
  `gnr8 generate`, then `gnr8 check`.

## Determinism and caches

The graph, artifact paths, and built-in output are sorted and deterministic. gnr8 caches source
analysis, file hashes, and generated artifact bundles under `.gnr8/cache`; Rust build output is under
`.gnr8/target`. Cache hits change work performed, not output semantics.

## Next pages

- Choose a source: [Sources and extraction](../extraction/sources.md)
- Configure graph changes: [Transforms and overrides](transforms.md)
- Choose targets: [OpenAPI generation](../openapi/generation.md) and
  [SDK generation](../sdk/generation.md)
- Find symbols: [Public API map](../reference/public-api.md)
