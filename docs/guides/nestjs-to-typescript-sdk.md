# Guide: NestJS Backend to TypeScript SDK

Use this when the service is a NestJS app with controller methods and class DTOs, and the desired output
is OpenAPI plus a fetch-based TypeScript SDK.

## Start

Run from the NestJS project root:

```bash
gnr8 init --source nestjs --sdk typescript
```

## Pipeline Example

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(NestJs::new().inputs(["src"]))
            .transform(SetBasePath::new("/api"))
            .transform(SetTitle::new("NestJS API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(TsSdk::new().module("example.com/nestjs-service/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

## OpenAPI Generator Compatibility

For migrations from an existing `typescript-axios` OpenAPI Generator SDK, use the compatibility profile.
It emits Axios transport files, runtime enum objects, request aliases, schema-required model
properties, and Axios response wrappers by default:

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(NestJs::new().inputs(["src"]))
            .target(
                TsSdk::new()
                    .module("example.com/nestjs-service/sdk")
                    .to("generated/sdk")
                    .compatibility(TsCompatibility::OpenApiGenerator),
            ),
    )
}
```

The compatibility defaults can be overridden per target:

```rust
TsSdk::new()
    .module("example.com/nestjs-service/sdk")
    .to("generated/sdk")
    .compatibility(TsCompatibility::OpenApiGenerator)
    .model_property_policy(TsModelPropertyPolicy::Strict)
    .nullable_policy(TsNullablePolicy::OmitNullFromOptionalProperties)
    .response_policy(TsResponsePolicy::DataOnly)
```

## Agent Checklist

- Ensure the project has `typescript` installed where the NestJS app already builds. gnr8 uses the
  project's TypeScript compiler API for extraction.
- Keep controller and DTO code under `src` unless you intentionally change `inputs(["src"])`.
- Prefer DTO classes, enums, unions, and explicit controller method return types.
- gnr8 does not read Swagger decorators, zod schemas, or class-validator metadata as the source of
  truth. Put API shape in TypeScript types.
- Keep SDK output under a generated directory and consume the generated README/reference before writing
  client code.

## Validate

```bash
gnr8 generate
gnr8 doctor
gnr8 check
npx tsc --noEmit
```

Read `generated/sdk/README.md` and `generated/sdk/reference.md` before wiring frontend calls.
