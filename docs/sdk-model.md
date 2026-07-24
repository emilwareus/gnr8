# SDK Semantic Model

`SdkModel` is the SDK planning boundary between the source-owned `ApiGraph` and language-specific SDK
emitters. It is built once per SDK target from the frozen graph plus target configuration such as package
name, layout, aliases, and profile.

The model exists to keep semantic decisions from drifting across Go, Python, and TypeScript generation.
The graph remains the source of HTTP and schema facts; `SdkModel` records how those facts will be exposed
as an SDK package.

## Current Facts

`SdkModel` currently carries:

- Package/module name and API base path.
- Services/groups and their operation ids.
- Operations with method, path template, handler id, service, auth requirements, request schema,
  success responses, and error responses.
- Schemas with stable graph id, generated name, and neutral shape kind.
- API-key auth header metadata.
- Compatibility aliases and profile metadata.
- File layout policy.
- Error response plan with a neutral base error concept.
- Runtime policy boundary with conservative no-op defaults.
- Docs metadata for API title, base path, operation tags, and schema names.

## Ownership Boundary

`ApiGraph` owns source facts: operations, params, schemas, responses, security scheme declarations,
provenance, title, and base path.

`SdkModel` owns SDK planning facts: package surface, service grouping, per-operation auth/error/success
classification, file layout, optional aliases, docs metadata, and runtime-policy defaults.

Language emitters own syntax and idioms only: type spelling, imports, file contents, runtime code, and
language-specific package metadata. New SDK adoption features should be added to `SdkModel` first when
they affect more than one target.

## Non-Goals

The model does not generate server stubs, support older OpenAPI output profiles, implement generic
template overrides, or infer auth/pagination/runtime behavior from conventions. Later phases should add
explicit transforms that populate model fields before emitters render language-specific code.
