//! `OpenAPI` lowering seam (Phase 3): lowers the API graph to an `OpenAPI` 3.1.0 document.
//!
//! The graph is the source of truth; the `OpenAPI` document is an artifact serialized from typed
//! structs (PROJECT constraint / D-01). [`to_openapi`] is a pure graph→typed-doc transform (no
//! re-analysis — D-02): it builds a [`model::OpenApiDoc`] from the [`crate::graph::ApiGraph`] and
//! serializes it with the deterministic key-ordered writer in [`yaml`].
//!
//! ## Resolved Open Question A3 — the absolute `/goal` base-path prefix
//!
//! The Phase-2 graph stores **group-relative** operation paths (`/`, `/list`, `/{uuid}`) and carries
//! NO explicit service base path; 02-03 deferred joining the dynamic `"/" + basePath` prefix to
//! Phase-3 lowering (see `graph::Operation::path`). Per RESEARCH recommendation (a) — lower from a
//! known constant for the single-group `PoC` rather than reshaping the Phase-2 graph (which would be
//! an out-of-scope Phase-2 change) — this module defines a private [`BASE_PATH`] and joins it to each
//! operation's group-relative path with slash-collapse, yielding `/goal/`, `/goal/list`,
//! `/goal/{uuid}` (never `/goal//list` and never a dropped prefix). A multi-group generalization is
//! deferred (D-02).
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

use crate::graph::{ApiGraph, Operation as GraphOp, Schema, SchemaType};
use model::{
    Components, Info, OpenApiDoc, Operation, Parameter, PathItem, RequestBody, ResponseObj,
    SchemaObject, SecurityRequirement, SecurityScheme,
};
use std::collections::BTreeMap;

/// The absolute service base-path prefix joined to every group-relative operation path (Open Q A3).
///
/// The fixture is a single route group mounted at `/goal`; the graph cannot constant-fold the dynamic
/// `basePath` prefix, so lowering supplies it deterministically here. See the module doc for the full
/// rationale.
const BASE_PATH: &str = "/goal";

/// The single API-key security scheme name the fixture annotates (`@Security ApiKeyAuth`).
const API_KEY_SCHEME: &str = "ApiKeyAuth";

/// The HTTP header the generated API-key scheme reads.
const API_KEY_HEADER: &str = "X-API-Key";

/// Lower the [`crate::graph::ApiGraph`] to an `OpenAPI` 3.1.0 document (serialized YAML).
///
/// A pure graph→typed-doc transform (D-02): builds a [`model::OpenApiDoc`] and serializes it via the
/// deterministic [`yaml::write`] writer. Operation paths are joined with the absolute [`BASE_PATH`]
/// prefix (Open Q A3); every schema `$ref` is resolved against `graph.schemas` to its bare component
/// name; security schemes are collected from the operations that carry them.
///
/// # Errors
///
/// Returns [`crate::CoreError::Lowering`] when a graph fact cannot be represented — a dangling `$ref`
/// (a `request_body`/`response.body` whose `ref_id` is not among `graph.schemas`) or an unknown
/// [`crate::graph::SchemaType`] `kind`. Never panics and never `unwrap`s (RUST-04 / T-03-01-01).
pub fn to_openapi(graph: &ApiGraph) -> Result<String, crate::CoreError> {
    // ref_id (pkg-qualified) -> bare component name, for resolving $refs to local schema names.
    let ref_to_name: BTreeMap<&str, &str> = graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), schema.name.as_str()))
        .collect();

    let paths = build_paths(graph, &ref_to_name)?;
    let schemas = build_component_schemas(&graph.schemas, &ref_to_name)?;
    let any_secured = graph
        .operations
        .iter()
        .any(|op| op.security_schemes.iter().any(|s| s == API_KEY_SCHEME));

    let security = if any_secured {
        vec![SecurityRequirement {
            scheme: API_KEY_SCHEME.to_string(),
            scopes: vec![],
        }]
    } else {
        vec![]
    };
    let security_schemes = if any_secured {
        vec![(
            API_KEY_SCHEME.to_string(),
            SecurityScheme {
                kind: "apiKey",
                location: "header",
                name: API_KEY_HEADER.to_string(),
            },
        )]
    } else {
        vec![]
    };

    let doc = OpenApiDoc {
        openapi: "3.1.0",
        info: Info {
            title: "goalservice".to_string(),
            version: "0.1.0".to_string(),
            description: None,
        },
        security,
        paths,
        components: Components {
            security_schemes,
            schemas,
        },
    };

    Ok(yaml::write(&doc))
}

/// Group operations sharing an absolute path into one [`PathItem`] (so PUT + DELETE on
/// `/goal/{uuid}` coexist), preserving graph order and keying paths in first-seen (sorted) order.
fn build_paths(
    graph: &ApiGraph,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<Vec<(String, PathItem)>, crate::CoreError> {
    // The graph sorts operations by (path, method); joining the base path preserves that order, so a
    // simple ordered accumulator keeps the output deterministic without re-sorting.
    let mut paths: Vec<(String, PathItem)> = Vec::new();
    for op in &graph.operations {
        let abs_path = join_base(BASE_PATH, &op.path);
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

/// Lower one graph [`GraphOp`] into a typed [`Operation`] (summary, id, tags, params, body, responses).
fn lower_operation(
    op: &GraphOp,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<Operation, crate::CoreError> {
    let parameters = op
        .params
        .iter()
        .map(|param| {
            let schema = if param.enum_values.is_empty() {
                lower_schema_type(&param.schema, ref_to_name)?
            } else {
                // Closed value set from an Enums(...) annotation → a string enum schema.
                SchemaObject {
                    type_name: Some("string".to_string()),
                    enum_values: param.enum_values.clone(),
                    ..SchemaObject::default()
                }
            };
            Ok(Parameter {
                name: param.name.clone(),
                location: param.location.clone(),
                required: param.required,
                description: param.description.clone(),
                schema,
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
            let description = resp
                .description
                .clone()
                .unwrap_or_else(|| default_response_description(resp.status));
            Ok((
                resp.status.to_string(),
                ResponseObj {
                    description,
                    schema_ref,
                },
            ))
        })
        .collect::<Result<Vec<_>, crate::CoreError>>()?;

    Ok(Operation {
        summary: op.summary.clone(),
        operation_id: op.id.clone(),
        tags: op.tags.clone(),
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

/// Lower one named graph [`Schema`] (object or enum) into a [`SchemaObject`].
fn lower_named_schema(
    schema: &Schema,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    match schema.kind.as_str() {
        "enum" => {
            let mut enum_values = schema.enum_values.clone();
            enum_values.sort();
            Ok(SchemaObject {
                type_name: Some("string".to_string()),
                enum_values,
                ..SchemaObject::default()
            })
        }
        "object" => {
            let mut required: Vec<String> = schema
                .fields
                .iter()
                .filter(|field| field.required)
                .map(|field| field.json_name.clone())
                .collect();
            required.sort();
            let mut properties: Vec<(String, SchemaObject)> = schema
                .fields
                .iter()
                .map(|field| {
                    let mut prop = lower_schema_type(&field.schema, ref_to_name)?;
                    // Attach a field description when the graph carries one and the schema is not a
                    // bare $ref (a $ref node ignores sibling keys per JSON Schema).
                    if prop.schema_ref.is_none() {
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
        other => Err(crate::CoreError::Lowering {
            message: format!("unknown schema kind '{other}' for schema '{}'", schema.id),
        }),
    }
}

/// Map a graph [`SchemaType`] to a [`SchemaObject`]. A `ref` kind resolves to a bare-name `$ref`;
/// an unknown kind is a typed error (T-03-01-01).
fn lower_schema_type(
    schema: &SchemaType,
    ref_to_name: &BTreeMap<&str, &str>,
) -> Result<SchemaObject, crate::CoreError> {
    match schema.kind.as_str() {
        "ref" => {
            let Some(ref_id) = &schema.ref_id else {
                return Err(crate::CoreError::Lowering {
                    message: "ref schema is missing a ref_id".to_string(),
                });
            };
            Ok(SchemaObject::reference(resolve_ref(ref_id, ref_to_name)?))
        }
        "array" => {
            let Some(items) = &schema.items else {
                return Err(crate::CoreError::Lowering {
                    message: "array schema is missing an items type".to_string(),
                });
            };
            Ok(SchemaObject {
                type_name: Some("array".to_string()),
                items: Some(Box::new(lower_schema_type(items, ref_to_name)?)),
                ..SchemaObject::default()
            })
        }
        "object" => {
            // A bare `object` in the graph is a free-form map (additional_properties == Some(true));
            // it lowers to additionalProperties: true (the OAPI-03 representational decision).
            Ok(SchemaObject {
                type_name: Some("object".to_string()),
                additional_properties: schema.additional_properties,
                ..SchemaObject::default()
            })
        }
        "string" | "integer" | "number" | "boolean" => {
            Ok(SchemaObject::primitive(&schema.kind, schema.format.clone()))
        }
        other => Err(crate::CoreError::Lowering {
            message: format!("unknown SchemaType kind '{other}'"),
        }),
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

/// Join the absolute [`BASE_PATH`] prefix with a group-relative operation path, collapsing the seam
/// slash: `/goal` + `/` → `/goal/`, `/goal` + `/list` → `/goal/list`, `/goal` + `/{uuid}` →
/// `/goal/{uuid}` (never `/goal//list`, never a dropped prefix). Open Q A3.
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
    use crate::graph::ApiGraph;

    /// A facts document covering the cases the mapper must handle: one secured POST under `/`, a GET
    /// under `/list` with an enum query param, a PUT + DELETE coexisting under `/{uuid}`, an object
    /// schema with a uuid field, a free-form-map field, an enum schema, and a diagnostic.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "router_path": null, "handler": "createGoal",
          "operation_id": null, "summary": null, "tags": [], "secured": true,
          "security_schemes": ["ApiKeyAuth"], "params": [],
          "request_body": { "ref_id": "internal/dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "internal/dto.CommandMessage" }, "description": null }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "router_path": "/list", "handler": "listGoals",
          "operation_id": null, "summary": "List goals", "tags": ["Goals"], "secured": true,
          "security_schemes": ["ApiKeyAuth"],
          "params": [
            {
              "name": "aggregation", "location": "query", "required": true,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": "Aggregation", "enum_values": ["sum", "count"],
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 }
            }
          ],
          "request_body": null,
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.GoalResponse" }, "description": "Goals page" }
          ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "DELETE", "path": "/{uuid}", "router_path": null, "handler": "deleteGoal",
          "operation_id": null, "summary": null, "tags": [], "secured": true,
          "security_schemes": [],
          "params": [
            {
              "name": "uuid", "location": "path", "required": true,
              "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "enum_values": [],
              "span": { "file": "/root/h.go", "start_line": 3, "end_line": 3 }
            }
          ],
          "request_body": null,
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.CommandMessage" }, "description": null }
          ],
          "span": { "file": "/root/http.go", "start_line": 3, "end_line": 3 }
        },
        {
          "method": "PUT", "path": "/{uuid}", "router_path": "/{uuid}", "handler": "updateGoal",
          "operation_id": "goalUuidPut", "summary": "Update goal", "tags": ["Goals"], "secured": true,
          "security_schemes": ["ApiKeyAuth"],
          "params": [
            {
              "name": "uuid", "location": "path", "required": true,
              "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
              "description": "Goal UUID", "enum_values": [],
              "span": { "file": "/root/h.go", "start_line": 4, "end_line": 4 }
            }
          ],
          "request_body": { "ref_id": "internal/dto.CreateGoalInput" },
          "responses": [
            { "status": 200, "body": { "ref_id": "internal/dto.CommandMessage" }, "description": "Goal updated" }
          ],
          "span": { "file": "/root/http.go", "start_line": 4, "end_line": 4 }
        }
      ],
      "schemas": [
        {
          "id": "internal/dto.CreateGoalInput", "name": "CreateGoalInput", "kind": "object",
          "fields": [
            {
              "json_name": "name", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": "Goal name", "example": null
            },
            {
              "json_name": "metadata", "required": false, "optional": true,
              "schema": { "kind": "object", "format": null, "items": null, "ref_id": null, "additional_properties": true },
              "description": null, "example": null
            },
            {
              "json_name": "uuid", "required": false, "optional": true,
              "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null
            }
          ],
          "enum_values": [],
          "span": { "file": "/root/dto.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "internal/dto.CommandMessage", "name": "CommandMessage", "kind": "object",
          "fields": [
            {
              "json_name": "message", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null
            }
          ],
          "enum_values": [],
          "span": { "file": "/root/dto.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "internal/dto.GoalResponse", "name": "GoalResponse", "kind": "object",
          "fields": [
            {
              "json_name": "direction", "required": false, "optional": true,
              "schema": { "kind": "ref", "format": null, "items": null, "ref_id": "internal/dto.TargetDirection", "additional_properties": null },
              "description": null, "example": null
            }
          ],
          "enum_values": [],
          "span": { "file": "/root/dto.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "internal/dto.TargetDirection", "name": "TargetDirection", "kind": "enum",
          "fields": [], "enum_values": ["lte", "gte"],
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
        let yaml = to_openapi(&sample_graph()).unwrap();
        assert!(yaml.contains("'/goal/':"), "{yaml}");
        assert!(yaml.contains("'/goal/list':"), "{yaml}");
        assert!(yaml.contains("'/goal/{uuid}':"), "{yaml}");
        assert!(!yaml.contains("/goal//"), "no doubled slash:\n{yaml}");
    }

    #[test]
    fn put_and_delete_coexist_on_one_path() {
        let yaml = to_openapi(&sample_graph()).unwrap();
        // Both methods must render under the single /goal/{uuid} path item.
        let uuid_block = yaml
            .split("'/goal/{uuid}':")
            .nth(1)
            .expect("uuid path present");
        assert!(uuid_block.contains("put:"), "{uuid_block}");
        assert!(uuid_block.contains("delete:"), "{uuid_block}");
    }

    #[test]
    fn dangling_request_body_ref_returns_lowering_error() {
        let mut graph = sample_graph();
        // Point a request body at a ref_id that is not among the schemas.
        graph.operations[0].request_body = Some(crate::graph::SchemaRef {
            ref_id: "internal/dto.DoesNotExist".to_string(),
        });
        let err = to_openapi(&graph).unwrap_err();
        let crate::CoreError::Lowering { message } = err else {
            panic!("expected Lowering, got {err:?}");
        };
        assert!(message.contains("DoesNotExist"), "{message}");
    }

    #[test]
    fn unknown_schema_type_kind_returns_lowering_error() {
        let mut graph = sample_graph();
        // Corrupt a field's schema kind to an unrepresentable value.
        graph.schemas[1].fields[0].schema.kind = "tuple".to_string();
        let err = to_openapi(&graph).unwrap_err();
        let crate::CoreError::Lowering { message } = err else {
            panic!("expected Lowering, got {err:?}");
        };
        assert!(message.contains("tuple"), "{message}");
    }

    #[test]
    fn api_key_security_is_emitted_top_level_and_in_components() {
        let yaml = to_openapi(&sample_graph()).unwrap();
        assert!(yaml.contains("security:"), "top-level security:\n{yaml}");
        assert!(yaml.contains("- ApiKeyAuth: []"), "{yaml}");
        assert!(yaml.contains("securitySchemes:"), "{yaml}");
        assert!(yaml.contains("type: apiKey"), "{yaml}");
        assert!(yaml.contains("in: header"), "{yaml}");
        assert!(yaml.contains("name: X-API-Key"), "{yaml}");
    }

    #[test]
    fn free_form_map_field_lowers_to_additional_properties_true() {
        let yaml = to_openapi(&sample_graph()).unwrap();
        assert!(
            yaml.contains("additionalProperties: true"),
            "free-form map must lower to additionalProperties: true:\n{yaml}"
        );
    }

    #[test]
    fn lowering_succeeds_even_when_diagnostics_are_non_empty() {
        let graph = sample_graph();
        // The sample carries a diagnostic; lowering must still succeed (diagnostics are advisory).
        assert!(!graph.diagnostics.is_empty());
        assert!(to_openapi(&graph).is_ok());
    }

    #[test]
    fn to_openapi_is_byte_identical_across_two_runs() {
        let graph = sample_graph();
        assert_eq!(to_openapi(&graph).unwrap(), to_openapi(&graph).unwrap());
    }
}
