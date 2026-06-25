//! `OpenAPI` lowering seam (Phase 3): lowers the API graph to an `OpenAPI` 3.1.0 document.
//!
//! The graph is the source of truth; the `OpenAPI` document is an artifact serialized from typed
//! structs (PROJECT constraint / D-01). [`to_openapi`] is a pure graph→typed-doc transform (no
//! re-analysis — D-02): it builds a [`model::OpenApiDoc`] from the [`crate::graph::ApiGraph`] and
//! serializes it with the deterministic key-ordered writer in [`yaml`].
//!
//! ## Resolved Open Question A3 — the absolute base-path prefix (from code-as-config)
//!
//! The Phase-2 graph stores **group-relative** operation paths (`/`, `/list`, `/{uuid}`) and carries
//! NO explicit service base path; 02-03 deferred joining the dynamic `"/" + basePath` prefix to
//! Phase-3 lowering (see `graph::Operation::path`). That prefix is the Gin group argument — often a
//! *runtime* value the analyzer cannot constant-fold — so it is NOT scraped: it is the graph's
//! `base_path` (the single source of truth; CLAUDE.md rules 3 & 4), set by a `SetBasePath` transform in
//! the user's `.gnr8/` pipeline and threaded into [`to_openapi`], joined to each operation's
//! group-relative path with slash-collapse. With `base_path = "/goal"` this yields `/goal/`,
//! `/goal/list`, `/goal/{uuid}` (never `/goal//list` and never a dropped prefix). A multi-group
//! generalization is deferred (D-02).
//!
//! ## Diagnostics (OAPI-03)
//!
//! Lowering does NOT re-derive diagnostics. The graph already carries the byte-locked Phase-2
//! diagnostics (float64 narrowing, free-form maps, untyped query params); they are surfaced through
//! the existing `diagnostics::collect` path (the green `snapshot_diagnostics`). `to_openapi` is
//! non-fatal on diagnostics — a graph with a non-empty `diagnostics` vector still lowers successfully
//! — and it never drops or recomputes them. The representational decision the diagnostics describe
//! (a free-form map → `additionalProperties: true`) is applied here.

mod model;
mod yaml;

use crate::graph::{
    ApiGraph, Field, Operation as GraphOp, Prim, Schema, SecurityScheme as GraphSecurityScheme,
    Type, WellKnown,
};
use model::{
    Components, Info, OpenApiDoc, Operation, Parameter, PathItem, RequestBody, ResponseObj,
    SchemaObject, SecurityRequirement, SecurityScheme,
};
use std::collections::BTreeMap;

/// The only `apiKey` location the `PoC` supports (the fixture's `X-API-Key` header).
const SUPPORTED_API_KEY_LOCATION: &str = "header";

/// The only security scheme kind the `PoC` supports.
const SUPPORTED_SCHEME_KIND: &str = "apiKey";

/// Lower the [`crate::graph::ApiGraph`] to an `OpenAPI` 3.1.0 document (serialized YAML).
///
/// A pure graph→typed-doc transform (D-02): builds a [`model::OpenApiDoc`] and serializes it via the
/// deterministic [`yaml::write`] writer. Operation paths are joined with the `base_path` prefix (Open Q
/// A3 — the single source of truth for the service prefix, set by a `SetBasePath` transform, CLAUDE.md
/// rules 3 & 4); every schema `$ref` is resolved against `graph.schemas` to its bare
/// component name. The `security` requirement and `components.securitySchemes` are built ENTIRELY from
/// `security` (the [`crate::graph::SecurityScheme`]s an `ApplySecurity` transform set on the graph) —
/// the single source of truth for security (`CLAUDE.md` rule 4); the graph carries no security facts
/// otherwise. The `PoC` policy applies every scheme to all operations (top-level `security`).
///
/// # Errors
///
/// Returns [`crate::CoreError::Lowering`] when a graph fact cannot be represented — a dangling `$ref`
/// (a `request_body`/`response.body` whose `ref_id` is not among `graph.schemas`) or a neutral
/// [`crate::graph::Type`] the `OpenAPI` target cannot express — or when a security scheme uses an
/// unsupported `kind`/`location` (so a misconfiguration is a clear error, never a silently dropped
/// scheme). Never panics and never `unwrap`s (RUST-04 / T-03-01-01).
pub fn to_openapi(
    graph: &ApiGraph,
    title: &str,
    base_path: &str,
    security: &[GraphSecurityScheme],
) -> Result<String, crate::CoreError> {
    // ref_id (pkg-qualified) -> bare component name, for resolving $refs to local schema names.
    let ref_to_name: BTreeMap<&str, &str> = graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), schema.name.as_str()))
        .collect();

    let paths = build_paths(graph, base_path, &ref_to_name)?;
    let schemas = build_component_schemas(&graph.schemas, &ref_to_name)?;
    let security = build_security(security)?;

    let doc = OpenApiDoc {
        openapi: "3.1.0",
        info: Info {
            title: title.to_string(),
            version: "0.1.0".to_string(),
            description: None,
        },
        security: security.requirements,
        paths,
        components: Components {
            security_schemes: security.schemes,
            schemas,
        },
    };

    Ok(yaml::write(&doc))
}

/// The lowered security: the top-level `security` requirements and the `components.securitySchemes`,
/// both built from the graph's schemes. Bundled into one struct so the [`build_security`] return type
/// stays simple.
struct LoweredSecurity {
    /// Top-level `security` requirements (one per scheme, sorted by id).
    requirements: Vec<SecurityRequirement>,
    /// `components.securitySchemes` entries, keyed by scheme id, sorted by id.
    schemes: Vec<(String, SecurityScheme)>,
}

/// Build the top-level `security` requirements + `components.securitySchemes` from the graph's
/// [`crate::graph::SecurityScheme`]s (the single source of truth for security — CLAUDE.md rule 4, set
/// by an `ApplySecurity` transform). The `PoC` `apply_to_all` policy adds every scheme to the top-level
/// requirement, sorted by scheme id for determinism.
///
/// # Errors
///
/// Returns [`crate::CoreError::Lowering`] for a scheme whose `kind`/`location` the `PoC` does not
/// support, so an unsupported scheme is a clear error rather than a silently dropped one.
fn build_security(security: &[GraphSecurityScheme]) -> Result<LoweredSecurity, crate::CoreError> {
    // Sort by scheme id so the emitted requirement + schemes are deterministic regardless of input
    // order (GRAPH-02), and reject a duplicate id rather than silently collapsing one.
    let mut schemes: Vec<&GraphSecurityScheme> = security.iter().collect();
    schemes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut requirements = Vec::with_capacity(schemes.len());
    let mut components = Vec::with_capacity(schemes.len());
    for scheme in schemes {
        if scheme.kind != SUPPORTED_SCHEME_KIND || scheme.location != SUPPORTED_API_KEY_LOCATION {
            return Err(crate::CoreError::Lowering {
                message: format!(
                    "unsupported security scheme '{}': the PoC supports kind=\"{SUPPORTED_SCHEME_KIND}\" \
                     in=\"{SUPPORTED_API_KEY_LOCATION}\" only (got kind=\"{}\" location=\"{}\")",
                    scheme.id, scheme.kind, scheme.location
                ),
            });
        }
        if components.iter().any(|(id, _)| id == &scheme.id) {
            return Err(crate::CoreError::Lowering {
                message: format!(
                    "duplicate security scheme id '{}' (an ApplySecurity transform added it twice)",
                    scheme.id
                ),
            });
        }
        requirements.push(SecurityRequirement {
            scheme: scheme.id.clone(),
            scopes: vec![],
        });
        components.push((
            scheme.id.clone(),
            SecurityScheme {
                kind: scheme.kind.clone(),
                location: scheme.location.clone(),
                name: scheme.name.clone(),
            },
        ));
    }
    Ok(LoweredSecurity {
        requirements,
        schemes: components,
    })
}

/// Group operations sharing an absolute path into one [`PathItem`] (so PUT + DELETE on
/// `/goal/{uuid}` coexist), preserving graph order and keying paths in first-seen (sorted) order.
fn build_paths(
    graph: &ApiGraph,
    base_path: &str,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<Vec<(String, PathItem)>, crate::CoreError> {
    // The graph sorts operations by (path, method); joining the base path preserves that order, so a
    // simple ordered accumulator keeps the output deterministic without re-sorting.
    let mut paths: Vec<(String, PathItem)> = Vec::new();
    for op in &graph.operations {
        let abs_path = join_base(base_path, &op.path);
        let operation = lower_operation(op, ref_to_name)?;
        // Find the existing path-item index (the graph's (path, method) sort keeps same-path
        // operations adjacent, so this stays deterministic), else append a fresh one.
        let index = if let Some(index) = paths.iter().position(|(p, _)| *p == abs_path) {
            index
        } else {
            paths.push((abs_path, PathItem::default()));
            paths.len() - 1
        };
        let Some((_, item)) = paths.get_mut(index) else {
            return Err(crate::CoreError::Lowering {
                message: "internal: path accumulator index out of range".to_string(),
            });
        };
        place_operation(item, &op.method, operation)?;
    }
    Ok(paths)
}

/// Slot an [`Operation`] into its method field on the [`PathItem`], rejecting unknown/duplicate
/// methods with a typed error (never a panic).
fn place_operation(
    item: &mut PathItem,
    method: &str,
    operation: Operation,
) -> Result<(), crate::CoreError> {
    let slot = match method {
        "GET" => &mut item.get,
        "POST" => &mut item.post,
        "PUT" => &mut item.put,
        "DELETE" => &mut item.delete,
        other => {
            return Err(crate::CoreError::Lowering {
                message: format!("unsupported HTTP method '{other}' for operation lowering"),
            });
        }
    };
    if slot.is_some() {
        return Err(crate::CoreError::Lowering {
            message: format!("duplicate {method} operation on a single path"),
        });
    }
    *slot = Some(operation);
    Ok(())
}

/// Lower one graph [`GraphOp`] into a typed [`Operation`] (operationId, params, body, responses).
///
/// Query params lower to a bare `string` schema, never required, with no enum (those were annotation
/// facts and are gone — CLAUDE.md rules 1 & 3). There is no summary/tags. Response descriptions use a
/// stable default since the graph carries none.
fn lower_operation(
    op: &GraphOp,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<Operation, crate::CoreError> {
    let parameters = op
        .params
        .iter()
        .map(|param| {
            Ok(Parameter {
                name: param.name.clone(),
                location: param.location.clone(),
                required: param.required,
                schema: lower_schema_type(&param.schema, ref_to_name)?,
            })
        })
        .collect::<Result<Vec<_>, crate::CoreError>>()?;

    let request_body = match &op.request_body {
        Some(body) => Some(RequestBody {
            required: true,
            schema_ref: resolve_ref(&body.ref_id, ref_to_name)?,
        }),
        None => None,
    };

    let responses = op
        .responses
        .iter()
        .map(|resp| {
            let schema_ref = match &resp.body {
                Some(body) => Some(resolve_ref(&body.ref_id, ref_to_name)?),
                None => None,
            };
            Ok((
                resp.status.to_string(),
                ResponseObj {
                    description: default_response_description(resp.status),
                    schema_ref,
                },
            ))
        })
        .collect::<Result<Vec<_>, crate::CoreError>>()?;

    Ok(Operation {
        operation_id: op.id.clone(),
        parameters,
        request_body,
        responses,
    })
}

/// Map each graph [`Schema`] to a component [`SchemaObject`], keyed by its bare local name.
fn build_component_schemas(
    schemas: &[Schema],
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<Vec<(String, SchemaObject)>, crate::CoreError> {
    schemas
        .iter()
        .map(|schema| {
            let object = lower_named_schema(schema, ref_to_name)?;
            Ok((schema.name.clone(), object))
        })
        .collect()
}

/// Lower one named graph [`Schema`] (an [`Type::Object`] or [`Type::Enum`] body) into a
/// [`SchemaObject`]. A named schema's body is a neutral [`Type`]; only `Object`/`Enum` are valid
/// component bodies, every other variant is an explicit typed error (a named schema is never a bare
/// scalar/array/ref). The match is exhaustive — no `_ =>`/`other =>` arm — so a future [`Type`] variant
/// fails to compile here until it is handled (T-03).
fn lower_named_schema(
    schema: &Schema,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    match &schema.body {
        Type::Enum(members) => {
            let mut enum_values = members.clone();
            enum_values.sort();
            Ok(SchemaObject {
                type_name: Some("string".to_string()),
                enum_values,
                ..SchemaObject::default()
            })
        }
        Type::Object(fields) => lower_object(fields, ref_to_name),
        // A named component is always a struct/class (Object) or a string enum (Enum). Any other
        // neutral type as a *named* body is a contract error — an explicit arm, not a catch-all (T-03).
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Array(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Union(_)
        | Type::Any {} => Err(crate::CoreError::Lowering {
            message: format!(
                "named schema '{}' has a non-object/non-enum body that cannot be a component",
                schema.id
            ),
        }),
    }
}

/// Lower an object's fields into an `object` [`SchemaObject`] with a sorted `required` list and sorted
/// properties. Optionality is the `required`-omission axis (a field is `required` iff `field.required`);
/// nullability is rendered independently on each property's schema (the 3.1 `[T,"null"]` array form).
fn lower_object(
    fields: &[Field],
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    let mut required: Vec<String> = fields
        .iter()
        .filter(|field| field.required)
        .map(|field| field.json_name.clone())
        .collect();
    required.sort();
    let mut properties: Vec<(String, SchemaObject)> = fields
        .iter()
        .map(|field| {
            // The NULLABLE axis (independent of optional) renders on the property schema.
            let mut prop = lower_field_schema(&field.schema, field.nullable, ref_to_name)?;
            // Attach a field description when the graph carries one and the schema is not a bare $ref
            // / oneOf (a $ref node ignores sibling keys per JSON Schema).
            if prop.schema_ref.is_none() && prop.one_of.is_empty() {
                if let Some(desc) = &field.description {
                    prop.description = Some(desc.clone());
                }
            }
            Ok((field.json_name.clone(), prop))
        })
        .collect::<Result<Vec<_>, crate::CoreError>>()?;
    properties.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(SchemaObject {
        type_name: Some("object".to_string()),
        required,
        properties,
        ..SchemaObject::default()
    })
}

/// Lower a field's neutral [`Type`] applying the field's `nullable` axis: a nullable scalar/array/map
/// renders as the 3.1 `type: ["<type>", "null"]` array form; a nullable `$ref` renders as the
/// `oneOf: [ {$ref}, {type: "null"} ]` form (a `$ref` cannot carry a sibling `type`). A non-nullable
/// field is the plain lowered schema.
fn lower_field_schema(
    ty: &Type,
    nullable: bool,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    let lowered = lower_schema_type(ty, ref_to_name)?;
    if !nullable {
        return Ok(lowered);
    }
    // A bare `$ref` (or an already-composed `oneOf`) cannot carry a sibling `type` key — wrap it in a
    // `oneOf` with an explicit null schema (the 3.1 / JSON-Schema-2020-12 nullable-reference form).
    if lowered.schema_ref.is_some() || !lowered.one_of.is_empty() {
        return Ok(SchemaObject {
            one_of: vec![lowered, null_schema()],
            ..SchemaObject::default()
        });
    }
    // A typed schema renders the array form via the `nullable` flag (the writer emits `[T, "null"]`).
    Ok(SchemaObject {
        nullable: true,
        ..lowered
    })
}

/// The bare `{ type: "null" }` schema (the null member of a nullable `oneOf`).
fn null_schema() -> SchemaObject {
    SchemaObject {
        type_name: Some("null".to_string()),
        ..SchemaObject::default()
    }
}

/// Map a neutral graph [`Type`] to a [`SchemaObject`], NEUTRALLY — no Go (or any language) type name
/// appears here; lowering emits only `OpenAPI`/`JSON Schema` primitive names from the neutral type
/// (IR-03). The match is exhaustive (no `_ =>`/`other =>` arm) so a new [`Type`] variant fails to
/// compile here until handled (T-03). Nullability is NOT applied here — it is a field-position axis
/// applied by [`lower_field_schema`].
fn lower_schema_type(
    ty: &Type,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    match ty {
        Type::Primitive(prim) => Ok(SchemaObject::primitive(openapi_primitive(prim), None)),
        // A well-known scalar lowers to its canonical `string` + `format` (uuid, date-time, ...). The
        // format string is the neutral wire token, never a language type name (IR-03).
        Type::WellKnown(well_known) => Ok(SchemaObject::primitive(
            "string",
            Some(openapi_format(well_known).to_string()),
        )),
        Type::Array(items) => Ok(SchemaObject {
            type_name: Some("array".to_string()),
            items: Some(Box::new(lower_schema_type(items, ref_to_name)?)),
            ..SchemaObject::default()
        }),
        // A keyed map lowers to an object whose additionalProperties is the lowered value schema.
        Type::Map { value, .. } => Ok(SchemaObject {
            type_name: Some("object".to_string()),
            items: None,
            additional_properties_schema: Some(Box::new(lower_schema_type(value, ref_to_name)?)),
            ..SchemaObject::default()
        }),
        Type::Named(ref_id) => Ok(SchemaObject::reference(resolve_ref(ref_id, ref_to_name)?)),
        // An inline (anonymous) object lowers to a full object schema with its own properties.
        Type::Object(fields) => lower_object(fields, ref_to_name),
        Type::Enum(members) => {
            let mut enum_values = members.clone();
            enum_values.sort();
            Ok(SchemaObject {
                type_name: Some("string".to_string()),
                enum_values,
                ..SchemaObject::default()
            })
        }
        // A sum type lowers to the 3.1 `oneOf` of its lowered variants.
        Type::Union(variants) => {
            let one_of = variants
                .iter()
                .map(|variant| lower_schema_type(variant, ref_to_name))
                .collect::<Result<Vec<_>, crate::CoreError>>()?;
            Ok(SchemaObject {
                one_of,
                ..SchemaObject::default()
            })
        }
        // A free-form value lowers to `additionalProperties: true` (the OAPI-03 representational
        // decision for an untyped map).
        Type::Any {} => Ok(SchemaObject {
            type_name: Some("object".to_string()),
            additional_properties: Some(true),
            ..SchemaObject::default()
        }),
    }
}

/// Map a neutral [`Prim`] to its `OpenAPI`/`JSON Schema` primitive type name (neutral — no language
/// type name; the width/sign of an integer or float is a *target* concern, not a spec primitive).
fn openapi_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String | Prim::Bytes => "string",
        Prim::Bool => "boolean",
        Prim::Int { .. } => "integer",
        Prim::Float { .. } => "number",
    }
}

/// Map a neutral [`WellKnown`] to its canonical `OpenAPI`/`JSON Schema` `format` token (the neutral
/// wire form, e.g. `uuid`, `date-time`); these are spec format strings, never language type names.
fn openapi_format(well_known: &WellKnown) -> &'static str {
    match well_known {
        WellKnown::Uuid => "uuid",
        WellKnown::DateTime => "date-time",
        WellKnown::Date => "date",
        WellKnown::Duration => "duration",
        WellKnown::Decimal => "decimal",
        WellKnown::Email => "email",
        WellKnown::Uri => "uri",
    }
}

/// Resolve a pkg-qualified `ref_id` to its bare component name, erroring on a dangling reference
/// (a `ref_id` not among `graph.schemas`) — never an `unwrap` (RESEARCH Pitfall 6 / T-03-01-01).
fn resolve_ref(
    ref_id: &str,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<String, crate::CoreError> {
    match ref_to_name.get(ref_id) {
        Some(name) => Ok((*name).to_string()),
        None => Err(crate::CoreError::Lowering {
            message: format!("dangling $ref '{ref_id}': no schema with that id in the graph"),
        }),
    }
}

/// Join the `base_path` prefix with a group-relative operation path, collapsing the seam slash:
/// `/goal` + `/` → `/goal/`, `/goal` + `/list` → `/goal/list`, `/goal` + `/{uuid}` → `/goal/{uuid}`
/// (never `/goal//list`, never a dropped prefix). Open Q A3.
fn join_base(base: &str, relative: &str) -> String {
    let base = base.trim_end_matches('/');
    if relative == "/" {
        return format!("{base}/");
    }
    let suffix = relative.strip_prefix('/').unwrap_or(relative);
    format!("{base}/{suffix}")
}

/// A stable default response description used when the graph carries none, so the document never
/// emits an empty `description` (required by the spec) and stays deterministic.
fn default_response_description(status: u16) -> String {
    format!("Response {status}")
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the
    // allow to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{join_base, to_openapi};
    use crate::graph::{ApiGraph, SecurityScheme};

    /// The fixture's security schemes (the SINGLE source of truth for security — CLAUDE.md rule 4):
    /// one `ApiKeyAuth` / `X-API-Key` scheme applied to all operations. Graph-owned `SecurityScheme`s,
    /// as an `ApplySecurity` transform would set them.
    fn security_config() -> Vec<SecurityScheme> {
        vec![SecurityScheme {
            id: "ApiKeyAuth".to_string(),
            kind: "apiKey".to_string(),
            location: "header".to_string(),
            name: "X-API-Key".to_string(),
        }]
    }

    /// A facts document covering the cases the mapper must handle (code-first shape — no annotation
    /// facts): a POST under `/`, a GET under `/list` with two query params, a PUT + DELETE coexisting
    /// under `/{uuid}`, an object schema with a uuid field, a free-form-map field, a code-defined enum
    /// schema, and a diagnostic.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "handler": "createGoal",
          "operation_id": "createGoal", "params": [],
          "request_body": { "ref_id": "internal/dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "internal/dto.CommandMessage" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listGoals",
          "operation_id": "listGoals",
          "params": [
            {
              "name": "aggregation", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 }
            }
          ],
          "request_body": null,
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.GoalResponse" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "DELETE", "path": "/{uuid}", "handler": "deleteGoal",
          "operation_id": "deleteGoal",
          "params": [
            {
              "name": "uuid", "location": "path", "required": true,
              "schema": { "type": "well_known", "of": "uuid" },
              "span": { "file": "/root/h.go", "start_line": 3, "end_line": 3 }
            }
          ],
          "request_body": null,
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.CommandMessage" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 3, "end_line": 3 }
        },
        {
          "method": "PUT", "path": "/{uuid}", "handler": "updateGoal",
          "operation_id": "updateGoal",
          "params": [
            {
              "name": "uuid", "location": "path", "required": true,
              "schema": { "type": "well_known", "of": "uuid" },
              "span": { "file": "/root/h.go", "start_line": 4, "end_line": 4 }
            }
          ],
          "request_body": { "ref_id": "internal/dto.CreateGoalInput" },
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.CommandMessage" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 4, "end_line": 4 }
        }
      ],
      "schemas": [
        {
          "id": "internal/dto.CreateGoalInput", "name": "CreateGoalInput",
          "body": { "type": "object", "of": [
            {
              "json_name": "name", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": "Goal name", "example": null
            },
            {
              "json_name": "metadata", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "any", "of": {} },
              "description": null, "example": null
            },
            {
              "json_name": "uuid", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "well_known", "of": "uuid" },
              "description": null, "example": null
            }
          ] },
          "span": { "file": "/root/dto.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "internal/dto.CommandMessage", "name": "CommandMessage",
          "body": { "type": "object", "of": [
            {
              "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null
            }
          ] },
          "span": { "file": "/root/dto.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "internal/dto.GoalResponse", "name": "GoalResponse",
          "body": { "type": "object", "of": [
            {
              "json_name": "direction", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "named", "of": "internal/dto.TargetDirection" },
              "description": null, "example": null
            }
          ] },
          "span": { "file": "/root/dto.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "internal/dto.TargetDirection", "name": "TargetDirection",
          "body": { "type": "enum", "of": ["lte", "gte"] },
          "span": { "file": "/root/dto.go", "start_line": 4, "end_line": 4 }
        }
      ],
      "diagnostics": [
        {
          "severity": "WARN",
          "message": "free-form map field: CreateGoalInput.Metadata (map[string]any) lowers to additionalProperties: true",
          "file": "/root/dto.go", "line": 1
        }
      ]
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    #[test]
    fn join_base_collapses_the_seam_slash() {
        assert_eq!(join_base("/goal", "/"), "/goal/");
        assert_eq!(join_base("/goal", "/list"), "/goal/list");
        assert_eq!(join_base("/goal", "/{uuid}"), "/goal/{uuid}");
        // A trailing slash on the base is collapsed, never doubled.
        assert_eq!(join_base("/goal/", "/list"), "/goal/list");
    }

    #[test]
    fn paths_are_keyed_absolutely_under_goal() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        assert!(yaml.contains("'/goal/':"), "{yaml}");
        assert!(yaml.contains("'/goal/list':"), "{yaml}");
        assert!(yaml.contains("'/goal/{uuid}':"), "{yaml}");
        assert!(!yaml.contains("/goal//"), "no doubled slash:\n{yaml}");
    }

    #[test]
    fn put_and_delete_coexist_on_one_path() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        // Both methods must render under the single /goal/{uuid} path item.
        let uuid_block = yaml
            .split("'/goal/{uuid}':")
            .nth(1)
            .expect("uuid path present");
        assert!(uuid_block.contains("put:"), "{uuid_block}");
        assert!(uuid_block.contains("delete:"), "{uuid_block}");
    }

    #[test]
    fn operation_ids_are_handler_symbols() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        // operationIds are the handler-symbol-derived ids — no annotation override (e.g. updateGoal,
        // not goalUuidPut).
        assert!(yaml.contains("operationId: createGoal"), "{yaml}");
        assert!(yaml.contains("operationId: updateGoal"), "{yaml}");
        assert!(yaml.contains("operationId: deleteGoal"), "{yaml}");
        assert!(yaml.contains("operationId: listGoals"), "{yaml}");
        assert!(
            !yaml.contains("goalUuidPut"),
            "operation id is the handler symbol:\n{yaml}"
        );
        // No summary/tags survive (they were annotation facts).
        assert!(!yaml.contains("summary:"), "no summary:\n{yaml}");
        assert!(!yaml.contains("tags:"), "no tags:\n{yaml}");
    }

    #[test]
    fn query_params_are_plain_string_not_required_no_enum() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        // The aggregation query param lowers to a bare string, not required, with no enum.
        let list_block = yaml.split("'/goal/list':").nth(1).expect("list path");
        let list_block = list_block
            .split("'/goal/{uuid}':")
            .next()
            .unwrap_or(list_block);
        assert!(list_block.contains("name: aggregation"), "{list_block}");
        assert!(list_block.contains("required: false"), "{list_block}");
        assert!(
            !list_block.contains("enum:"),
            "no enum on query param:\n{list_block}"
        );
    }

    #[test]
    fn dangling_request_body_ref_returns_lowering_error() {
        let mut graph = sample_graph();
        // Point a request body at a ref_id that is not among the schemas.
        graph.operations[0].request_body = Some(crate::graph::SchemaRef {
            ref_id: "internal/dto.DoesNotExist".to_string(),
        });
        let err = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap_err();
        let crate::CoreError::Lowering { message } = err else {
            panic!("expected Lowering, got {err:?}");
        };
        assert!(message.contains("DoesNotExist"), "{message}");
    }

    #[test]
    fn named_schema_with_non_object_body_returns_lowering_error() {
        use crate::graph::Type;
        let mut graph = sample_graph();
        // A named component whose body is a bare array (not Object/Enum) cannot be a component schema:
        // an EXPLICIT typed error arm, not a swallowed catch-all (T-03).
        graph.schemas[1].body = Type::Array(Box::new(Type::Primitive(crate::graph::Prim::String)));
        let err = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap_err();
        let crate::CoreError::Lowering { message } = err else {
            panic!("expected Lowering, got {err:?}");
        };
        assert!(message.contains("non-object/non-enum body"), "{message}");
    }

    #[test]
    fn nullable_field_renders_type_array_and_stays_in_required() {
        use crate::graph::{Field, Type};
        // A nullable-but-NOT-optional field: it appears in `required` (optionality axis) AND its type
        // is the 3.1 `["string", "null"]` array form (nullability axis) — the two are independent.
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "label".to_string(),
            required: true,
            optional: false,
            nullable: true,
            schema: Type::Primitive(crate::graph::Prim::String),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(
            block.contains("type: [string, null]"),
            "nullable field must render the 3.1 type array form:\n{block}"
        );
        assert!(
            block.contains("required: [label]"),
            "a nullable-not-optional field must still be in required:\n{block}"
        );
    }

    #[test]
    fn optional_not_nullable_field_is_scalar_and_omitted_from_required() {
        use crate::graph::{Field, Type};
        // An optional-but-NOT-nullable field: omitted from `required` (optionality), but its type is a
        // PLAIN scalar (no `["T","null"]`), proving optionality does not leak into the type.
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "note".to_string(),
            required: false,
            optional: true,
            nullable: false,
            schema: Type::Primitive(crate::graph::Prim::String),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        // `note`'s schema is a plain `type: string` (not an array form).
        assert!(
            block.contains("note:") && block.contains("type: string"),
            "optional-not-nullable field must be a plain scalar:\n{block}"
        );
        assert!(
            !block.contains("type: [string, null]"),
            "optional alone must NOT produce the null array form:\n{block}"
        );
        // `required:` is omitted entirely (no required fields).
        let block_until_next = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(
            !block_until_next.contains("required:"),
            "an optional-only field must be omitted from required:\n{block_until_next}"
        );
    }

    #[test]
    fn nullable_ref_field_renders_oneof_with_null() {
        use crate::graph::{Field, Type};
        // A nullable `$ref` cannot carry a sibling `type`; it renders as `oneOf: [{$ref}, {type:null}]`.
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "direction".to_string(),
            required: false,
            optional: false,
            nullable: true,
            schema: Type::Named("internal/dto.TargetDirection".to_string()),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(
            block.contains("oneOf:"),
            "nullable $ref must use oneOf:\n{block}"
        );
        assert!(
            block.contains("$ref: '#/components/schemas/TargetDirection'"),
            "{block}"
        );
        assert!(block.contains("type: null"), "{block}");
    }

    #[test]
    fn union_field_lowers_to_oneof_of_variants() {
        use crate::graph::{Field, Prim, Type};
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "either".to_string(),
            required: true,
            optional: false,
            nullable: false,
            schema: Type::Union(vec![
                Type::Primitive(Prim::String),
                Type::Primitive(Prim::Int {
                    bits: 64,
                    signed: true,
                }),
            ]),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(
            block.contains("oneOf:"),
            "union must lower to oneOf:\n{block}"
        );
        assert!(
            block.contains("- type: string") && block.contains("- type: integer"),
            "oneOf must carry each lowered variant:\n{block}"
        );
    }

    #[test]
    fn nullable_union_field_lowers_to_nested_oneof_with_null() {
        use crate::graph::{Field, Prim, Type};
        // A NULLABLE union: `lower_schema_type` already returns a non-empty `one_of` for the union, so
        // the nullable wrap hits the `!one_of.is_empty()` branch and produces a NESTED `oneOf`
        // (`oneOf: [ <union oneOf>, {type: null} ]`) — the deterministic shape the writer renders. This
        // locks the contract CR-01's fixture snapshots must encode.
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "rating".to_string(),
            required: true,
            optional: false,
            nullable: true,
            schema: Type::Union(vec![
                Type::Primitive(Prim::Int {
                    bits: 64,
                    signed: true,
                }),
                Type::Primitive(Prim::Float { bits: 64 }),
            ]),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        // Byte-exact nested form (a union member nested under the outer nullable oneOf).
        assert!(
            block.contains(
                "rating:\n          oneOf:\n          - oneOf:\n            - type: integer\n            - type: number\n          - type: null\n"
            ),
            "nullable union must render as a NESTED oneOf with a null member:\n{block}"
        );
    }

    #[test]
    fn nullable_enum_field_lowers_to_type_array_with_enum() {
        use crate::graph::{Field, Type};
        // A NULLABLE enum: an enum lowers to `type: string` + `enum` (neither `$ref` nor `one_of`), so
        // the nullable wrap falls through to the `nullable: true, ..lowered` branch and renders the 3.1
        // type-array form `type: [string, null]` alongside the `enum` keys (valid JSON-Schema-2020-12).
        // This locks the contract CR-02's fixture snapshots must encode.
        let mut graph = sample_graph();
        graph.schemas[1].body = Type::Object(vec![Field {
            json_name: "sort".to_string(),
            required: false,
            optional: false,
            nullable: true,
            schema: Type::Enum(vec!["asc".to_string(), "desc".to_string()]),
            description: None,
            example: None,
        }]);
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml.split("CreateGoalInput:").nth(1).expect("schema block");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(
            block.contains("sort:\n          type: [string, null]\n          enum: [asc, desc]\n"),
            "nullable enum must render the 3.1 type-array form with enum keys:\n{block}"
        );
    }

    #[test]
    fn response_less_operation_renders_empty_responses_map() {
        use crate::graph::Operation;
        // An operation with NO responses must render a valid, deterministic `responses: {}` (the empty
        // OpenAPI responses object) — never a bare `responses:` (a YAML null, which is invalid OpenAPI).
        // Locks CR-03's writer fix.
        let mut graph = sample_graph();
        let op: &mut Operation = &mut graph.operations[0];
        op.responses.clear();
        let yaml = to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap();
        assert!(
            yaml.contains("responses: {}"),
            "a response-less operation must render `responses: {{}}`:\n{yaml}"
        );
        assert!(
            !yaml.contains("responses:\n      content"),
            "must not emit a bare null responses:\n{yaml}"
        );
    }

    #[test]
    fn well_known_uuid_field_lowers_to_string_with_uuid_format() {
        // A WellKnown(Uuid) field lowers NEUTRALLY to `type: string` + `format: uuid` — no language
        // type name leaks into lowering (IR-03).
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        let block = yaml
            .split("CreateGoalInput:")
            .nth(1)
            .expect("CreateGoalInput schema");
        let block = block.split("GoalResponse:").next().unwrap_or(block);
        assert!(block.contains("uuid:"), "{block}");
        assert!(block.contains("format: uuid"), "{block}");
    }

    #[test]
    fn api_key_security_is_emitted_from_config_top_level_and_in_components() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        assert!(yaml.contains("security:"), "top-level security:\n{yaml}");
        assert!(yaml.contains("- ApiKeyAuth: []"), "{yaml}");
        assert!(yaml.contains("securitySchemes:"), "{yaml}");
        assert!(yaml.contains("type: apiKey"), "{yaml}");
        assert!(yaml.contains("in: header"), "{yaml}");
        assert!(yaml.contains("name: X-API-Key"), "{yaml}");
    }

    #[test]
    fn no_security_config_emits_no_security() {
        // With an empty security config the document carries no security — proving security is
        // ENTIRELY config-driven, never derived from the graph (CLAUDE.md rule 4).
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &[]).unwrap();
        assert!(
            !yaml.contains("ApiKeyAuth"),
            "no scheme without config:\n{yaml}"
        );
        assert!(!yaml.contains("securitySchemes:"), "{yaml}");
    }

    #[test]
    fn unsupported_security_scheme_kind_returns_lowering_error() {
        let config = vec![SecurityScheme {
            id: "OAuth".to_string(),
            kind: "oauth2".to_string(),
            location: "header".to_string(),
            name: "Authorization".to_string(),
        }];
        let err = to_openapi(&sample_graph(), "goalservice", "/goal", &config).unwrap_err();
        let crate::CoreError::Lowering { message } = err else {
            panic!("expected Lowering, got {err:?}");
        };
        assert!(message.contains("unsupported security scheme"), "{message}");
    }

    #[test]
    fn free_form_map_field_lowers_to_additional_properties_true() {
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        assert!(
            yaml.contains("additionalProperties: true"),
            "free-form map must lower to additionalProperties: true:\n{yaml}"
        );
    }

    #[test]
    fn code_defined_enum_is_preserved() {
        // A code-defined Go enum (TargetDirection, from go/types) must still render as a string enum —
        // it comes from CODE, not annotations (CLAUDE.md rule on keeping code-defined enums).
        let yaml = to_openapi(&sample_graph(), "goalservice", "/goal", &security_config()).unwrap();
        let td = yaml
            .split("TargetDirection:")
            .nth(1)
            .expect("TargetDirection schema");
        assert!(
            td.contains("enum: [gte, lte]"),
            "code enum preserved:\n{td}"
        );
    }

    #[test]
    fn lowering_succeeds_even_when_diagnostics_are_non_empty() {
        let graph = sample_graph();
        // The sample carries a diagnostic; lowering must still succeed (diagnostics are advisory).
        assert!(!graph.diagnostics.is_empty());
        assert!(to_openapi(&graph, "goalservice", "/goal", &security_config()).is_ok());
    }

    #[test]
    fn to_openapi_is_byte_identical_across_two_runs() {
        let graph = sample_graph();
        assert_eq!(
            to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap(),
            to_openapi(&graph, "goalservice", "/goal", &security_config()).unwrap()
        );
    }
}
