<!-- generated-by: gsd-doc-writer -->
# OpenAPI compatibility

[Agent docs index](../agents/index.md)

`gnr8 compat openapi` is an exact semantic comparator for JSON/YAML Swagger 2.0 and OpenAPI 3.0/3.1
documents. “Compatible” means no consumer-visible difference after supported cross-version
normalization—not merely “no breaking changes.” Additions also count as differences.

## Run the gate

```bash
gnr8 compat openapi \
  --old baseline/swagger.yaml \
  --new generated/openapi.yaml \
  --policy exact
```

`--policy exact` is optional because `exact` is currently the only policy.

```bash
gnr8 --json compat openapi --old old.json --new new.yaml
status=$?
```

Exit `0` means no differences. Exit `1` means a valid comparison found differences. Other nonzero
statuses mean an input, parse, reference, or configuration error.

## Why it does not compare `ApiGraph`

The generation graph intentionally retains only facts gnr8 can generate. Exact compatibility uses a
separate canonical contract so unsupported generation vocabulary cannot disappear and produce a false
match.

## Compared surface

The canonical contract includes:

- Document metadata, servers, extensions, and top-level security.
- Security schemes and ordered OR/AND requirements.
- Path-item metadata and every HTTP operation.
- Operation ID, summary, description, tags, deprecation, external docs, servers, callbacks, and
  extensions.
- Parameters: name/location, description, required/deprecated flags, style, explode,
  `allowReserved`, `allowEmptyValue`, schema/content, examples, and extensions.
- Request bodies: description, requiredness, media types, schemas, examples, encoding, extensions.
- Responses: status/default entries, description, headers, media, schemas, examples, links,
  extensions.
- Component schemas and reusable component sections.
- Webhooks, local references, and relative external references.

Swagger 2.0 concepts are normalized to their OpenAPI 3 equivalents where representation is
unambiguous: host/basePath/schemes, body/formData, response schemas, security definitions, consumes/
produces, and supported collection formats. Formatting, key order, JSON versus YAML, and equivalent
cross-version syntax do not create differences.

## JSON result

The result includes:

```json
{
  "language": "openapi",
  "policy": "exact",
  "old": "baseline/swagger.yaml",
  "new": "generated/openapi.yaml",
  "compatible": false,
  "old_version": "swagger-2.0",
  "new_version": "openapi-3.1",
  "differences": [
    {
      "code": "parameter.schema.type.changed",
      "location": "/paths/~1books/get/parameters/query:limit/schema/type",
      "operation": "GET /books",
      "name": "limit",
      "old": "integer",
      "new": "string"
    }
  ]
}
```

Each difference has a stable dotted `code`, canonical JSON Pointer `location`, optional operation,
name and response status, plus optional `old`/`new` values. Differences are sorted by location, code,
operation, name, and status.

## Agent remediation loop

1. Save the complete JSON result; do not fix only the first human-readable line.
2. Group differences by operation/schema and stable code.
3. Decide whether each difference is an extraction gap, graph policy change, target-only rendering
   difference, or intended baseline change.
4. Fix source or `.gnr8/src/main.rs` with the narrowest transform/patch.
5. Regenerate and rerun the comparator until `differences` is empty.
6. Use `--accept-generated-baseline` only when the new contract is intentionally replacing the old
   one, and review that decision in version control.

There is no allowance file for OpenAPI exact mode. If additions are intentionally acceptable, update
the baseline or use a separate project policy outside this exact gate.

## Common interpretation

| Difference family | Likely control |
|---|---|
| `metadata.*`, `server.*` | `OpenApiMetadata` |
| operation ID/name | `RenameOperation`, `DocumentOperation` |
| missing/changed parameter | `ApiOverrides::parameter` |
| request body | typed `ApiOverrides` request-body helper |
| response status/schema/media | `ResponseOverride` or `SetOperationSuccessResponse` |
| schema name | `RenameType` or `OpenApiSchemaAliases` |
| schema field constraints/extensions | `OpenApiSchemaPatch` |
| security requirement | `ApplySecurity` or `SecurityOverride` |

Related: [OpenAPI generation](generation.md), [Transforms and overrides](../pipeline/transforms.md),
and [Diagnostics reference](../diagnostics/reference.md).
