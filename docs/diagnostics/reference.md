<!-- generated-by: gsd-doc-writer -->
# Diagnostics reference

Diagnostics are structured evidence that extraction or an explicit override was incomplete, lossy,
or intentional. They travel with the graph and appear in `inspect`, `generate`, `check`, and `doctor`.
Warnings do not fail generation unless `DiagnosticPolicy` denies them.

## Shape

```json
{
  "code": "response.schema.unresolved",
  "severity": "WARN",
  "category": "response",
  "message": "response schema could not be resolved",
  "file": "internal/books/handlers.go",
  "line": 42,
  "span": { "file": "internal/books/handlers.go", "start_line": 42, "end_line": 47 },
  "operation": "GET /books/{id}",
  "schema": "Book",
  "subject": "200 application/json"
}
```

`operation`, `schema`, and `subject` are present only when known. Code/category are stable policy
keys; message is explanatory text. Results are deterministically sorted.

## Categories

| JSON category | Rust variant | Meaning |
|---|---|---|
| `source` | `DiagnosticCategory::Source` | source/route pattern could not be analyzed |
| `request_parameter` | `RequestParameter` | incomplete or ambiguous request parameter |
| `request_body` | `RequestBody` | incomplete request body |
| `response` | `Response` | incomplete response fact |
| `schema` | `Schema` | incomplete or lossy schema fact |
| `security` | `Security` | incomplete or contradictory security fact |
| `override` | `Override` | explicit override replaced an extracted fact |
| `artifact` | `Artifact` | artifact ownership violation |
| `compatibility` | `Compatibility` | contract drift |

## Extraction codes

| Code | Meaning | Typical remediation |
|---|---|---|
| `source.unresolved` | generic source expression could not be resolved | simplify/type source or inspect message |
| `source.route.unresolved` | route path/group/method was dynamic | make registration static |
| `source.load.unresolved` | part of the source/package graph could not load | fix toolchain/module/import errors |
| `source.handler.ambiguous` | route handler identity was not unique | register a statically resolvable handler |
| `source.openapi.unrepresentable` | imported OpenAPI fact has no lossless graph representation | keep exact spec gate or add explicit graph policy |
| `request.parameter.unresolved` | name/location/type/default/serialization was incomplete | add source typing or typed parameter override |
| `request.body.unresolved` | request body schema/media/requiredness was incomplete | use typed request-body override |
| `response.status.unresolved` | response status was dynamic/unknown | set an exact response override |
| `response.schema.unresolved` | response schema was unknown or ambiguous | set success/response schema explicitly |
| `response.media_type.unresolved` | response media type was unknown or not representable | use `ResponseOverride` media policy |
| `schema.type.unresolved` | field/schema type could not be resolved | add source type or `SetSchemaFieldType` |
| `schema.metadata.unresolved` | schema constraint/metadata could not be preserved | type source or add target patch |
| `schema.numeric.narrowing` | numeric shape required a lossy narrowing | choose an explicit graph type |
| `schema.free_form_map` | source contains a free-form map | accept `Type::Any` explicitly or model a schema |
| `security.unresolved` | security/auth fact was incomplete | use `ApplySecurity`/`SecurityOverride` |

Not every source emits every code. The diagnostic message and span identify the exact unsupported
pattern.

## Intentional override codes

| Code | Severity | Meaning |
|---|---:|---|
| `override.parameter.replaced` | `INFO` | `ParameterOverride::replace` intentionally replaced an existing parameter |
| `override.security.replaced` | `INFO` | exact per-operation security replaced inherited/extracted security |

These are audit records, not extraction failures. A redundant or contradictory override fails as a
configuration error instead of emitting an informational diagnostic.

## Artifact ownership codes

These normally surface as typed pipeline errors:

| Code | Cause |
|---|---|
| `artifact.path_collision` | `create` targeted an already-owned artifact |
| `artifact.overlay_missing` | `overlay` targeted a path no stage created |
| `artifact.rewrite_missing` | `rewrite` targeted a path no stage created |

Fix stage ownership explicitly; do not choose a less strict method merely to suppress the error.

## Correct then gate

```rust
Pipeline::new()
    .source(GoGin::new().inputs(["."]))
    .transform(
        ApiOverrides::new()
            .json_request_body("POST", "/books", "CreateBook"),
    )
    .transform(
        DiagnosticPolicy::new()
            .deny("request.body.unresolved")
            .deny_category(DiagnosticCategory::Security),
    )
    .target(OpenApi31::new().to("generated/openapi.yaml"));
```

Supported corrections retire matching unresolved diagnostics when the operation/schema/subject is
resolved. Policy runs at its declaration point, so always put it after corrections and before
targets.

## Agent triage procedure

```bash
gnr8 --json inspect graph > graph.json
gnr8 --json doctor > doctor.json
```

1. Group by exact `code`, then operation/schema/subject.
2. Read the source span and confirm the actual runtime contract.
3. Prefer adding source types/static constants.
4. If source cannot express the contract, add the narrowest exact transform.
5. Regenerate and confirm the diagnostic is retired or intentionally remains.
6. Deny critical codes/categories only after existing findings are addressed.

`doctor` labels analysis diagnostics informational and excludes them from its actionable-problem
count. Lifecycle failure, stale output, or protected edits are actionable.

Compatibility commands return their own sorted diff records; OpenAPI difference codes are described
in [OpenAPI compatibility](../openapi/compatibility.md), and SDK diff fields in
[SDK compatibility](../sdk/compatibility.md).
