# SDK Semantic Model

> `SdkModel` is **host-internal** as of 0.9.0. It needs the generation projection, which lives in
> the engine, so it is not part of the published `gnr8` SDK and is not in the prelude. A custom
> `Target` reads [`ApiGraph`](reference/public-api.md) directly. This page describes how gnr8's own
> emitters are organized.

`SdkModel` is the SDK planning boundary between the source-owned `ApiGraph` and language-specific SDK
emitters. It is built once per SDK target from the frozen graph plus target configuration such as package
name and layout.

The model exists to keep semantic decisions from drifting across Go, Python, and TypeScript generation.
The graph remains the source of HTTP and schema facts; `SdkModel` records how those facts will be exposed
as an SDK package.

## Current Facts

`SdkModel` currently carries:

- Package/module name and API base path.
- Services/groups and their operation ids.
- Operations with method, path template, handler id, service, auth requirements, request media/schema
  choices, success responses, error responses, and declared response headers.
- Schemas with stable graph id, generated name, and neutral shape kind.
- API-key auth header metadata.
- File layout policy.
- Error response plan with a neutral base error concept.
- Runtime policy boundary with conservative no-op defaults.
- Docs metadata for API title, base path, operation tags, and schema names.

## Ownership Boundary

`ApiGraph` owns source facts: operations, params, schemas, request media/schema choices, responses,
response headers, security scheme declarations, provenance, title, and base path. The original request
body fields remain the primary choice; `request_body_variants` is the deterministically sorted set of
additional choices. Together they form one request-content collection.

`SdkModel` owns SDK planning facts: package surface, service grouping, per-operation auth/error/success
classification, file layout, docs metadata, and runtime-policy defaults.

Language emitters own syntax and idioms only: type spelling, imports, file contents, runtime code, and
language-specific package metadata. New SDK adoption features should be added to `SdkModel` first when
they affect more than one target.

## Non-Goals

The model does not generate server stubs, support older OpenAPI output profiles, implement generic
template overrides, or infer auth/pagination/runtime behavior from conventions. Later phases should add
explicit transforms that populate model fields before emitters render language-specific code.
