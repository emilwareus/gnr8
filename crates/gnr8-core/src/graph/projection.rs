//! Build the direction-specific graph consumed by artifact generators.
//!
//! A schema used in both directions can remain shared only while its complete transitive contract is
//! identical. When presence/null behavior differs on the schema or on a referenced schema, this pass
//! creates input/output components and rewrites roots and references accordingly. Targets therefore
//! consume one unambiguous graph instead of each inventing its own split policy.

use super::direction::{directions_of, schema_directions, SchemaDirections};
use super::{ApiGraph, Schema, SchemaRef, SchemaUse, Type};
use crate::CoreError;
use std::collections::{BTreeMap, BTreeSet};

const INPUT_ID_SUFFIX: &str = "::input";
const OUTPUT_ID_SUFFIX: &str = "::output";
const INPUT_NAME_SUFFIX: &str = "Input";
const OUTPUT_NAME_SUFFIX: &str = "Output";

/// Clone `graph` into the exact directional contract artifact generators consume.
pub(crate) fn for_generation(graph: &ApiGraph) -> Result<ApiGraph, CoreError> {
    let directions = schema_directions(graph);
    let by_id: BTreeMap<&str, &Schema> = graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), schema))
        .collect();

    let mut split = BTreeSet::new();
    for schema in &graph.schemas {
        let reached = directions_of(&directions, &schema.id);
        if reached.request && reached.response && own_contract_differs(schema) {
            split.insert(schema.id.clone());
        }
    }

    // A shared parent cannot point at two different child components. Propagate splitting to a
    // fixpoint through every named reference in a both-direction schema.
    loop {
        let mut changed = false;
        for schema in &graph.schemas {
            if split.contains(&schema.id) {
                continue;
            }
            let reached = directions_of(&directions, &schema.id);
            if reached.request && reached.response && type_references_any(&schema.body, &split) {
                changed |= split.insert(schema.id.clone());
            }
        }
        if !changed {
            break;
        }
    }

    if split.is_empty() {
        return Ok(graph.clone());
    }

    validate_projected_identities(graph, &split)?;
    let mut projected = graph.clone();
    projected.schemas.clear();
    for schema in &graph.schemas {
        if split.contains(&schema.id) {
            projected
                .schemas
                .push(project_schema(schema, SchemaUse::Input, &split)?);
            projected
                .schemas
                .push(project_schema(schema, SchemaUse::Output, &split)?);
            continue;
        }
        let reached = directions_of(&directions, &schema.id);
        let use_ = match (reached.request, reached.response) {
            (_, false) => Some(SchemaUse::Input),
            (false, true) => Some(SchemaUse::Output),
            (true, true) => None,
        };
        let mut unchanged = schema.clone();
        unchanged.body = rewrite_type(&schema.body, use_, &split)?;
        projected.schemas.push(unchanged);
    }
    projected
        .schemas
        .sort_by(|left, right| left.id.cmp(&right.id));

    for operation in &mut projected.operations {
        if let Some(body) = &mut operation.request_body {
            rewrite_schema_ref(body, SchemaUse::Input, &split);
        }
        for param in &mut operation.params {
            param.schema = rewrite_type(&param.schema, Some(SchemaUse::Input), &split)?;
            if let Some(content) = &mut param.openapi_content {
                rewrite_preserved_refs(content, SchemaUse::Input, &split);
            }
            for (_, value) in &mut param.openapi_fields {
                rewrite_preserved_refs(value, SchemaUse::Input, &split);
            }
        }
        for response in &mut operation.responses {
            if let Some(body) = &mut response.body {
                rewrite_schema_ref(body, SchemaUse::Output, &split);
            }
        }
    }
    for root in &mut projected.schema_uses {
        if split.contains(&root.schema_id) {
            root.schema_id = directional_id(&root.schema_id, root.use_);
        }
    }

    // Ensure every rewritten id still names a schema. This also catches an unexpected dangling
    // reference introduced by a future graph shape at the projection boundary.
    let projected_ids: BTreeSet<&str> = projected
        .schemas
        .iter()
        .map(|schema| schema.id.as_str())
        .collect();
    for id in split {
        if !by_id.contains_key(id.as_str())
            || !projected_ids.contains(directional_id(&id, SchemaUse::Input).as_str())
            || !projected_ids.contains(directional_id(&id, SchemaUse::Output).as_str())
        {
            return Err(CoreError::Config {
                message: format!("could not project directional schema {id:?}"),
            });
        }
    }
    Ok(projected)
}

fn own_contract_differs(schema: &Schema) -> bool {
    let Type::Object(fields) = &schema.body else {
        return false;
    };
    fields.iter().any(|field| {
        SchemaDirections::input_field_is_required(field)
            != SchemaDirections::output_field_is_required(field)
            || SchemaDirections::input_field_is_nullable(field)
                != SchemaDirections::output_field_is_nullable(field)
    })
}

fn validate_projected_identities(
    graph: &ApiGraph,
    split: &BTreeSet<String>,
) -> Result<(), CoreError> {
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for schema in &graph.schemas {
        let name_candidates = if split.contains(&schema.id) {
            vec![
                format!("{}{INPUT_NAME_SUFFIX}", schema.name),
                format!("{}{OUTPUT_NAME_SUFFIX}", schema.name),
            ]
        } else {
            vec![schema.name.clone()]
        };
        let id_candidates = if split.contains(&schema.id) {
            vec![
                directional_id(&schema.id, SchemaUse::Input),
                directional_id(&schema.id, SchemaUse::Output),
            ]
        } else {
            vec![schema.id.clone()]
        };
        for name in name_candidates {
            if !names.insert(name.clone()) {
                return Err(CoreError::Config {
                    message: format!(
                        "directional schema projection produces duplicate public name {name:?}; rename a source type"
                    ),
                });
            }
        }
        for id in id_candidates {
            if !ids.insert(id.clone()) {
                return Err(CoreError::Config {
                    message: format!(
                        "directional schema projection produces duplicate internal id {id:?}; rename a source type"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn project_schema(
    schema: &Schema,
    use_: SchemaUse,
    split: &BTreeSet<String>,
) -> Result<Schema, CoreError> {
    let mut projected = schema.clone();
    projected.id = directional_id(&schema.id, use_);
    projected.name = match use_ {
        SchemaUse::Input => format!("{}{INPUT_NAME_SUFFIX}", schema.name),
        SchemaUse::Output => format!("{}{OUTPUT_NAME_SUFFIX}", schema.name),
    };
    projected.body = rewrite_type(&schema.body, Some(use_), split)?;
    Ok(projected)
}

fn directional_id(id: &str, use_: SchemaUse) -> String {
    match use_ {
        SchemaUse::Input => format!("{id}{INPUT_ID_SUFFIX}"),
        SchemaUse::Output => format!("{id}{OUTPUT_ID_SUFFIX}"),
    }
}

fn rewrite_schema_ref(reference: &mut SchemaRef, use_: SchemaUse, split: &BTreeSet<String>) {
    if split.contains(&reference.ref_id) {
        reference.ref_id = directional_id(&reference.ref_id, use_);
    }
}

fn rewrite_preserved_refs(
    value: &mut serde_json::Value,
    use_: SchemaUse,
    split: &BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            let rewritten = object
                .get("$ref")
                .and_then(serde_json::Value::as_str)
                .and_then(|reference| rewrite_preserved_ref(reference, use_, split));
            if let Some(reference) = rewritten {
                object.insert("$ref".to_string(), serde_json::Value::String(reference));
            }
            for child in object.values_mut() {
                rewrite_preserved_refs(child, use_, split);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rewrite_preserved_refs(item, use_, split);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn rewrite_preserved_ref(
    reference: &str,
    use_: SchemaUse,
    split: &BTreeSet<String>,
) -> Option<String> {
    let (id, suffix) = super::split_local_component_ref(reference)?;
    if !split.contains(&id) {
        return None;
    }
    let id = directional_id(&id, use_)
        .replace('~', "~0")
        .replace('/', "~1");
    Some(suffix.map_or_else(
        || format!("#/components/schemas/{id}"),
        |suffix| format!("#/components/schemas/{id}/{suffix}"),
    ))
}

fn rewrite_type(
    ty: &Type,
    use_: Option<SchemaUse>,
    split: &BTreeSet<String>,
) -> Result<Type, CoreError> {
    Ok(match ty {
        Type::Primitive(value) => Type::Primitive(value.clone()),
        Type::WellKnown(value) => Type::WellKnown(value.clone()),
        Type::Array(items) => Type::Array(Box::new(rewrite_type(items, use_, split)?)),
        Type::Map { key, value } => Type::Map {
            key: Box::new(rewrite_type(key, use_, split)?),
            value: Box::new(rewrite_type(value, use_, split)?),
        },
        Type::Named(id) if split.contains(id) => {
            let Some(use_) = use_ else {
                return Err(CoreError::Config {
                    message: format!(
                        "unregistered schema references direction-specific schema {id:?}; register the parent as an input or output root"
                    ),
                });
            };
            Type::Named(directional_id(id, use_))
        }
        Type::Named(id) => Type::Named(id.clone()),
        Type::Object(fields) => Type::Object(
            fields
                .iter()
                .cloned()
                .map(|mut field| {
                    field.schema = rewrite_type(&field.schema, use_, split)?;
                    Ok(field)
                })
                .collect::<Result<Vec<_>, CoreError>>()?,
        ),
        Type::Enum(members) => Type::Enum(members.clone()),
        Type::Union(variants) => Type::Union(
            variants
                .iter()
                .map(|variant| rewrite_type(variant, use_, split))
                .collect::<Result<Vec<_>, CoreError>>()?,
        ),
        Type::Any {} => Type::Any {},
    })
}

fn type_references_any(ty: &Type, ids: &BTreeSet<String>) -> bool {
    match ty {
        Type::Named(id) => ids.contains(id),
        Type::Array(items) => type_references_any(items, ids),
        Type::Map { key, value } => {
            type_references_any(key, ids) || type_references_any(value, ids)
        }
        Type::Object(fields) => fields
            .iter()
            .any(|field| type_references_any(&field.schema, ids)),
        Type::Union(variants) => variants
            .iter()
            .any(|variant| type_references_any(variant, ids)),
        Type::Primitive(_) | Type::WellKnown(_) | Type::Enum(_) | Type::Any {} => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::unwrap_used)]

    use super::ApiGraph;
    use crate::graph::Type;

    fn graph_with_unwired_parent() -> ApiGraph {
        let facts = serde_json::from_slice(
            br#"{
              "module": "app",
              "routes": [
                {
                  "method": "POST", "path": "/input", "handler": "input",
                  "operation_id": "input", "params": [],
                  "request_body": { "ref_id": "app.Child" },
                  "responses": [ { "status": 204, "body": null } ],
                  "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
                },
                {
                  "method": "GET", "path": "/output", "handler": "output",
                  "operation_id": "output", "params": [], "request_body": null,
                  "responses": [ { "status": 200, "body": { "ref_id": "app.Child" } } ],
                  "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
                }
              ],
              "schemas": [
                {
                  "id": "app.Child", "name": "Child",
                  "body": { "type": "object", "of": [
                    {
                      "json_name": "value",
                      "serializer_may_omit": true,
                      "deserializer_accepts_absent": false,
                      "deserializer_accepts_null": false,
                      "serializer_may_emit_null": false,
                      "validator_requires_presence": false,
                      "validator_rejects_null": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null
                    }
                  ] },
                  "span": { "file": "/root/models.go", "start_line": 1, "end_line": 1 }
                },
                {
                  "id": "app.Parent", "name": "Parent",
                  "body": { "type": "object", "of": [
                    {
                      "json_name": "child",
                      "serializer_may_omit": false,
                      "deserializer_accepts_absent": false,
                      "deserializer_accepts_null": false,
                      "serializer_may_emit_null": false,
                      "validator_requires_presence": false,
                      "validator_rejects_null": false,
                      "schema": { "type": "named", "of": "app.Child" },
                      "description": null, "example": null
                    }
                  ] },
                  "span": { "file": "/root/models.go", "start_line": 2, "end_line": 2 }
                }
              ],
              "diagnostics": []
            }"#,
        )
        .unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    #[test]
    fn an_unwired_parent_uses_input_references_when_its_child_splits_elsewhere() {
        let projected = graph_with_unwired_parent()
            .project_for_generation()
            .unwrap();
        let parent = projected
            .schemas
            .iter()
            .find(|schema| schema.id == "app.Parent")
            .unwrap();
        let Type::Object(fields) = &parent.body else {
            panic!("parent must remain an object")
        };
        assert!(matches!(
            fields.as_slice(),
            [field] if field.schema == Type::Named("app.Child::input".to_string())
        ));
        assert!(projected
            .schemas
            .iter()
            .any(|schema| schema.id == "app.Child::input"));
        assert!(projected
            .schemas
            .iter()
            .any(|schema| schema.id == "app.Child::output"));
    }
}
