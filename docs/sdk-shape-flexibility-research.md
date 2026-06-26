# SDK Shape Flexibility Research and Plan

Date: 2026-06-26

This document researches how gnr8 should support flexible SDK shapes without overfitting to one migration target. The immediate pressure came from a realistic external Go backend whose existing generated SDKs had a mature public surface. The target name and path are intentionally omitted.

## Executive Summary

gnr8 can now generate split SDK files, Pydantic v2 Python models, and fast large SDK output. That solves readability and generation speed, but it does not solve compatibility. The current SDK generators hard-code a minimal client shape per language. When replacing an existing SDK, this creates broad breaking changes even if the underlying HTTP API did not change.

The fix is not more `split_files()` knobs. We need a proper SDK generation architecture:

1. Keep `ApiGraph` as the semantic source of truth.
2. Lower `ApiGraph` into a language-neutral `SdkModel` that describes package shape, operations, schemas, auth, files, imports, aliases, and compatibility metadata.
3. Apply configurable `SdkProfile`s that decide naming, grouping, runtime style, file layout, and compatibility wrappers.
4. Render files from templates or strongly typed template modules over `SdkModel`.
5. Add compatibility tests that compare old and new public SDK surfaces, not just compile outputs.

The immediate high-priority bug is auth: OpenAPI lowering reads `ir.security`, but SDK emitters currently do not consistently use it. That must be fixed before deeper flexibility work.

## Current State

Current pipeline:

```text
Source -> ApiGraph -> Transform -> Target -> Artifacts
```

Relevant files:

- `crates/gnr8-core/src/graph/mod.rs`: semantic API graph.
- `crates/gnr8-core/src/sdk/mod.rs`: `Source`, `Transform`, `Target`, `PostProcess`, `Pipeline`.
- `crates/gnr8-core/src/sdk/builtins.rs`: built-in `GoSdk`, `PySdk`, `TsSdk`, OpenAPI targets.
- `crates/gnr8-core/src/sdk/layout.rs`: compact/split layout knobs.
- `crates/gnr8-core/src/gosdk`, `pysdk`, `tssdk`: hard-coded language emitters.

Current strengths:

- Code-as-config pipeline is flexible at the IR transform level.
- `ApiGraph` is deterministic and language-neutral.
- Targets can be custom Rust code.
- Split file output is now configurable at a basic directory level.
- Python supports Pydantic v2 and dataclass model styles.
- Large generation is now fast enough for tight iteration.

Current limitations:

- No first-class SDK model between `ApiGraph` and language emitters.
- No template abstraction.
- No service/tag/resource grouping in `ApiGraph`.
- No compatibility profile concept.
- No per-language runtime profile beyond Python model style.
- No configurable public naming policy beyond `RenameType` and `RenameOperation`.
- No generated aliases or deprecated wrappers.
- No generated compatibility shims for old file/module paths.
- No public-surface diffing tests.
- SDK auth generation is incomplete and inconsistent with graph security metadata.

## Why File Layout Alone Is Insufficient

The current `SdkFileLayout` controls only:

- compact vs split
- operation directory
- model directory

That is useful for readability, but SDK compatibility is about the public API contract. File paths are only one part of that contract.

Examples of compatibility-sensitive shape:

- Go client constructor names and config type: `NewAPIClient(Configuration)` vs `NewClient(baseURL, opts...)`.
- Go service fields: `client.GoalsAPI.GoalPost(...).Execute()` vs `client.CreateGoal(...)`.
- Python import paths: `package.api.goals_api`, `package.api_client`, `package.configuration`, `package.models.dto_create_goal_input`.
- Python model helper methods: `to_dict`, `from_dict`, `to_json`, `from_json`, `validate_assignment`.
- TypeScript runtime: Axios `Configuration`/`BaseAPI`/request builders vs fetch `Client`.
- TypeScript model names and files: `DtoCreateGoalInput` in `models/dto-create-goal-input.ts` vs `CreateGoalInput` in `models/create_goal_input.ts`.
- Auth plumbing and header names.
- Return envelope shape: raw model vs `(model, response, error)` or response wrappers.
- Generated docs/package metadata that consumers import or rely on.

Changing all of these at once is a major SDK breaking change.

## Research References

OpenAPI Generator is relevant because it separates generator inputs from user-overridable templates, and its docs describe template overrides plus vendor/spec extensions for generator-specific metadata:

- OpenAPI Generator templating: https://openapi-generator.tech/docs/templating/
- OpenAPI Generator customization docs: https://github.com/OpenAPITools/openapi-generator/blob/master/docs/customization.md
- OpenAPI/Swagger extensions: https://swagger.io/docs/specification/v3_0/openapi-extensions/

Kiota is relevant as a contrasting approach: it aims for a consistent SDK architecture across APIs and languages rather than arbitrary template compatibility:

- Kiota documentation: https://learn.microsoft.com/en-us/openapi/kiota/
- Kiota repository: https://github.com/microsoft/kiota

Implication for gnr8:

- We should support both philosophies.
- A default profile can be simple, consistent, and gnr8-native.
- Compatibility profiles must be configurable enough to preserve an existing SDK surface during migration.
- Vendor-extension-like metadata should live in gnr8 transforms/profile config, not as untyped ad hoc strings in emitters.

## Design Principles

1. Do not overfit to one existing SDK layout.

   A profile should express generic policies: grouping, naming, runtime style, file mapping, aliases, auth, and wrappers. It should not hard-code one target's package names or endpoint set.

2. Keep semantic truth separate from presentation.

   `ApiGraph` should keep describing HTTP facts and schemas. SDK surface decisions belong in `SdkModel` and `SdkProfile`.

3. Preserve deterministic generation.

   Profiles, templates, aliases, and file plans must produce stable sorted artifacts.

4. Make compatibility explicit.

   Compatibility output should be named as such, e.g. `.profile(SdkProfile::openapi_generator_compat())`, not hidden in default behavior.

5. Prefer typed configuration over unstructured templates.

   Raw template overrides are powerful but easy to break. The core should first expose typed knobs and a typed render model. Template escape hatches should be layered on top.

6. Generate shims before forcing rewrites.

   For migrations, preserving old imports and method chains as deprecated wrappers is often better than asking every consumer to update immediately.

7. Public SDK surface needs tests.

   Compile tests are not enough. We need snapshot/diff tests for exported symbols, import paths, method names, constructor shape, and auth behavior.

## Proposed Architecture

### 1. Add `SdkModel`

Create a language-neutral model between `ApiGraph` and emitters.

Suggested shape:

```rust
pub struct SdkModel {
    pub package: PackageModel,
    pub auth: Vec<AuthModel>,
    pub services: Vec<ServiceModel>,
    pub operations: Vec<OperationModel>,
    pub schemas: Vec<SchemaModel>,
    pub files: FilePlan,
}
```

Core concepts:

- `PackageModel`: module/import/package metadata.
- `ServiceModel`: resource grouping such as `Goals`, `Billing`, `Users`.
- `OperationModel`: generated method names, old/new aliases, path/query/body/response metadata.
- `SchemaModel`: generated type names, old aliases, file names, helper-method policy.
- `AuthModel`: scheme id, header/query/cookie name, option/config shape.
- `FilePlan`: planned files and ownership before rendering.

`SdkModel` should be language-neutral but not language-blind. It can carry normalized facts that every language needs, while each language maps them into its own syntax.

### 2. Add `SdkProfile`

Profiles define how to turn `ApiGraph` into `SdkModel`.

Example:

```rust
GoSdk::new()
    .module("example.com/acme/sdk")
    .profile(SdkProfile::minimal())
    .to("sdk/go");

GoSdk::new()
    .module("example.com/acme/sdk")
    .profile(SdkProfile::openapi_generator_compat())
    .to("sdk/go");
```

Generic profile dimensions:

- `client_style`: flat client, service client, request builder, fluent builder.
- `runtime_style`: stdlib/fetch/axios/urllib3/requests/httpx, language dependent.
- `model_style`: Pydantic v2, dataclass, Go plain structs, TS interfaces/classes.
- `operation_grouping`: flat, by source module, by path prefix, by tag/resource transform.
- `naming`: schema prefix/suffix, operation casing, file casing, acronym policy.
- `file_layout`: compact, split, grouped, custom path templates.
- `auth_style`: constructor option, context value, configuration object, per-request option.
- `return_style`: model only, response wrapper, tuple, `(model, http_response, error)`.
- `compat_aliases`: old type names, old import paths, deprecated method wrappers.
- `support_files`: README, package files, configuration files, ignore files, docs.

### 3. Add `SdkGrouping`

The biggest missing fact for compatibility is operation grouping. Existing mature SDKs usually group operations by tag/resource/service. gnr8 removed annotation tags, which is reasonable for code-first extraction, but the SDK still needs grouping.

Add explicit grouping as a transform/profile concept:

```rust
.transform(GroupOperations::by_path_prefix([
    ("/billing", "Billing"),
    ("/goal", "Goals"),
]))
```

or:

```rust
.transform(GroupOperations::by_source_package([
    ("internal/goal/ports", "Goals"),
]))
```

The graph or `SdkModel` should then carry:

```rust
operation.group = Some("Goals")
```

This remains generic: any project can group by path, source, explicit operation ids, or custom transform.

### 4. Add Naming Policies and Aliases

Current `RenameType` changes the canonical schema name. That is not enough for compatibility because we often need both:

- canonical internal name
- old public alias
- old import file path

Add:

```rust
SchemaNaming {
    canonical: "CreateGoalInput",
    public: "DtoCreateGoalInput",
    aliases: ["CreateGoalInput"],
    file_stem: "dto-create-goal-input",
}
```

For Go:

```go
type DtoCreateGoalInput = CreateGoalInput
```

or generate the compatibility name as canonical and the clean name as alias, depending on profile.

For Python:

```python
DtoCreateGoalInput = CreateGoalInput
```

and optionally generate module shims:

```python
# models/dto_create_goal_input.py
from .create_goal_input import CreateGoalInput as DtoCreateGoalInput
```

For TypeScript:

```ts
export type DtoCreateGoalInput = CreateGoalInput;
```

and optionally generate old path shims:

```ts
// models/dto-create-goal-input.ts
export type { CreateGoalInput as DtoCreateGoalInput } from "./create_goal_input";
```

### 5. Add Template/File Path Mapping

Current split config cannot express paths like:

- `api/{service_snake}_api.py`
- `api/{service-kebab}-api.ts`
- `api_{service_snake}.go`
- `models/{schema_snake}.py`
- `models/{schema-kebab}.ts`
- `docs/{symbol}.md`

Add typed file templates:

```rust
SdkFileLayout::custom()
    .operation_file("api/{service_snake}_api.py")
    .model_file("models/{schema_snake}.py")
    .service_file("api/{service_snake}_api.py")
    .support_file("configuration.py");
```

Path template variables should be a fixed typed set:

- `{service}`
- `{service_snake}`
- `{service_kebab}`
- `{operation}`
- `{operation_snake}`
- `{operation_kebab}`
- `{schema}`
- `{schema_snake}`
- `{schema_kebab}`
- `{language}`

Reject unknown variables at config time.

### 6. Add Render Templates Carefully

There are two viable layers:

1. Built-in typed renderers over `SdkModel`.
2. Optional user template overrides over the same `SdkModel`.

Do not start with unconstrained arbitrary templates as the only solution. They will make compatibility possible but hard to validate.

Recommended path:

- First introduce `SdkModel` and built-in profile-driven renderers.
- Then add optional template overrides for selected files.
- Template context is serialized from `SdkModel`.
- Template renders must still pass language compile/typecheck gates.

Rust template engine candidates:

- `minijinja`: expressive, maintained, good for structured templates.
- `handlebars`: simpler, Mustache-like ecosystem.
- Keep manual emitters for default profile if dependency minimization is critical, but still render from `SdkModel`.

### 7. Fix Auth as a First-Class Part of SDK Model

The SDK generators must read `ir.security`, not hard-code headers.

Auth requirements:

- Header name must come from `SecurityScheme.name`.
- Scheme id must be available for configuration/context lookup.
- TypeScript must attach auth headers.
- Go/Python/TS should support profile-specific auth placement:
  - constructor option
  - configuration object
  - context value
  - per-request option

This is a correctness fix, not merely compatibility work.

### 8. Add Compatibility Surface Tests

Add a test harness that can snapshot the generated SDK public surface.

For Go:

- exported identifiers via `go doc` or a small parser
- expected constructors and service fields
- expected model aliases
- generated package compiles

For Python:

- import probes for old and new paths
- `dir(package)` symbol checks
- Pydantic model helper method checks
- runtime auth request test

For TypeScript:

- `tsc` import probes for old and new paths
- generated public type exports
- constructor/config type checks
- runtime fetch auth test

This test harness should be generic: feed it a compatibility contract file, not target-specific code.

Example contract:

```toml
[go]
symbols = ["APIClient", "Configuration", "GoalsAPIService", "DtoCreateGoalInput"]
imports = ["github.com/acme/sdk"]

[python]
imports = [
  "pkg.api_client.ApiClient",
  "pkg.configuration.Configuration",
  "pkg.models.dto_create_goal_input.DtoCreateGoalInput",
]

[typescript]
imports = [
  "./api/goals-api",
  "./configuration",
  "./models/dto-create-goal-input",
]
types = ["GoalsApi", "Configuration", "DtoCreateGoalInput"]
```

## Capability Matrix

| Capability | Current | Needed |
| --- | --- | --- |
| Compact vs split files | Partial | Keep, extend with path templates |
| Per-language model style | Python only | All languages need runtime/model profiles |
| Pydantic v2 | Yes | Add OpenAPI-compatible helper method style |
| Operation grouping | No | Group by path/source/custom transform |
| Service clients | No | Needed for compatibility profiles |
| Request builders | No | Needed for OpenAPI-style Go/TS compatibility |
| Config object generation | No | Needed for many SDK surfaces |
| Auth from graph security | Incomplete | Required |
| Model aliases | No | Required |
| Import/module shims | No | Required |
| File path templates | No | Required |
| Support files/docs/package metadata | No | Required |
| Template overrides | No | Useful after typed model exists |
| Public surface diff tests | No | Required |

## Phased Plan

### Phase 0: Correctness Fixes

Goal: make current generated SDKs respect existing semantic facts.

Tasks:

- Thread `ir.security` into Go/Python/TypeScript SDK generation.
- Generate auth headers from `SecurityScheme`.
- Add tests proving configured header names are used.
- Add TypeScript auth support.
- Update docs to say SDK auth is graph-driven.

Acceptance:

- OpenAPI and all SDKs agree on auth scheme/header.
- Existing compile tests still pass.
- Runtime smoke test confirms auth header injection in all SDKs.

### Phase 1: Introduce `SdkModel`

Goal: split semantic SDK planning from language rendering.

Tasks:

- Add `crates/gnr8-core/src/sdk/model.rs`.
- Lower `ApiGraph` to `SdkModel`.
- Include package, auth, operation, schema, response, file-plan basics.
- Keep current emitters working by adapting them to read `SdkModel`.
- Preserve current output byte-for-byte where possible for the default profile.

Acceptance:

- Current compact/split generated SDKs remain equivalent.
- Unit tests cover `ApiGraph -> SdkModel` determinism.
- No language emitter directly performs graph-level grouping/naming decisions that belong in `SdkModel`.

### Phase 2: Profiles and Naming Policies

Goal: expose generic shape controls.

Tasks:

- Add `SdkProfile` with minimal/default profile.
- Add `NamingPolicy` for schemas, operations, services, fields, files.
- Add alias support for schemas and operations.
- Add deprecation wrapper generation primitives.
- Add file path template validation.

Acceptance:

- A profile can add `Dto` schema prefixes without changing source extraction.
- A profile can generate model aliases in Go/Python/TS.
- A profile can place files with configurable path templates.

### Phase 3: Operation Grouping and Service Surfaces

Goal: support grouped SDKs without relying on OpenAPI tags.

Tasks:

- Add `GroupOperations` transform(s):
  - by path prefix
  - by source path/package
  - by explicit operation id map
  - custom closure/trait implementation
- Add `ServiceModel` to `SdkModel`.
- Add service-style emitters:
  - Go service fields and service methods
  - Python `api/*_api.py` classes
  - TypeScript `api/*-api.ts` classes or functions

Acceptance:

- Generated SDK can expose grouped service clients.
- Grouping policy is generic and configurable.
- Flat client remains available.

### Phase 4: Runtime Style Profiles

Goal: make runtime shape selectable.

Tasks:

- Go:
  - minimal functional-options client
  - config object client
  - optional request-builder style
- Python:
  - minimal urllib client
  - OpenAPI-like `ApiClient`/`Configuration`/`exceptions`
  - Pydantic helper methods profile
- TypeScript:
  - fetch client
  - Axios API classes
  - optional function/request-argument creator style

Acceptance:

- Each language supports at least `minimal` and one compatibility-oriented runtime profile.
- Runtime style is selected by profile, not by editing emitter internals.

### Phase 5: Template Override Layer

Goal: allow project-specific shaping without forking gnr8.

Tasks:

- Choose template engine.
- Serialize `SdkModel` as template context.
- Allow per-file template overrides:
  - built-in fallback templates
  - user template dirs
  - strict missing-variable behavior
- Add template diagnostics and snapshot tooling.

Acceptance:

- A user can override a model, operation, service, or support-file template.
- Unknown template variables fail loudly.
- Template output still participates in lifecycle ownership and no-op writes.

### Phase 6: Compatibility Contract Tests

Goal: prevent accidental SDK breaking changes.

Tasks:

- Add a contract file format for expected public surface.
- Add `gnr8 sdk-diff` or test helper that checks generated SDKs against a contract.
- Include import probes and type probes per language.
- Add auth behavior probes.

Acceptance:

- A migration can state the SDK surface it must preserve.
- CI can fail on accidental public surface breaks.
- Contracts are generic and reusable across projects.

## Recommended Immediate Work

1. Fix SDK auth generation from `ir.security`.
2. Add a generic compatibility research fixture with a small API and an expected old-style SDK surface.
3. Implement `SdkModel` and keep current default output stable.
4. Add schema alias generation.
5. Add file path templates.
6. Add grouping transforms.
7. Build one compatibility profile per language as a proof point, but keep it generic and config-driven.

## Non-Goals

- Recreating every OpenAPI Generator feature.
- Making OpenAPI annotations the source of truth again.
- Hard-coding one external target's package names or directory tree into gnr8.
- Replacing code-as-config with a large static config file.
- Allowing arbitrary templates to bypass typed semantic validation.

## Open Questions

1. Should compatibility profiles target OpenAPI Generator surfaces specifically, or should they be decomposed into smaller reusable profile pieces?

   Recommendation: decompose into reusable pieces, then provide convenience constructors.

2. Should `ApiGraph` carry service groups, or should grouping live only in `SdkModel`?

   Recommendation: grouping should be a transform-owned metadata layer that `SdkModel` consumes. Avoid making grouping look like a source-extracted HTTP fact.

3. Should default gnr8 SDKs remain minimal?

   Recommendation: yes. Minimal SDKs should be fast, readable, and dependency-light. Compatibility should be explicit.

4. Should template overrides be available before `SdkModel`?

   Recommendation: no. Without `SdkModel`, templates would depend on unstable emitter internals.

