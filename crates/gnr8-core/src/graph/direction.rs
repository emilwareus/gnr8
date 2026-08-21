//! Which HTTP positions a component schema is reached from, and what each artifact reads once it
//! knows.
//!
//! Presence and value nullability are independent, and both are direction-aware. Extraction records
//! what decoding, validation, and serialization each do; this module selects the facts for the
//! schema's payload position. Nullability never changes presence.

use super::{ApiGraph, Field, Type};
use std::collections::{BTreeMap, BTreeSet};

/// The HTTP positions a component schema is reached from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SchemaDirections {
    /// Reached from a request body, a parameter, or a schema one of those reaches.
    pub(crate) request: bool,
    /// Reached from a response body, or a schema one of those reaches.
    pub(crate) response: bool,
}

impl SchemaDirections {
    /// A parameter is a request, and so is every schema inline within one.
    pub(crate) const REQUEST: Self = Self {
        request: true,
        response: false,
    };

    pub(crate) fn input_field_is_required(field: &Field) -> bool {
        !field.deserializer_accepts_absent || field.validator_requires_presence
    }

    pub(crate) fn output_field_is_required(field: &Field) -> bool {
        !field.serializer_may_omit
    }

    pub(crate) fn input_field_is_nullable(field: &Field) -> bool {
        field.deserializer_accepts_null && !field.validator_rejects_null
    }

    pub(crate) fn output_field_is_nullable(field: &Field) -> bool {
        field.serializer_may_emit_null
    }

    /// Whether `field`'s key must be present in every payload this schema describes — the `OpenAPI`
    /// `required` array.
    ///
    /// On input, the key is required when the deserializer does not accept absence or when validation
    /// requires presence. On output, it is required when the serializer cannot omit it.
    pub(crate) fn field_is_required(self, field: &Field) -> bool {
        match (self.request, self.response) {
            (true, true) => {
                Self::input_field_is_required(field) && Self::output_field_is_required(field)
            }
            (false, true) => Self::output_field_is_required(field),
            (_, false) => Self::input_field_is_required(field),
        }
    }

    /// Whether a generated SDK model may leave `field`'s key out. This is exactly the inverse of the
    /// payload's requiredness; nullability is a separate value question.
    pub(crate) fn model_field_is_optional(self, field: &Field) -> bool {
        !self.field_is_required(field)
    }

    /// Whether a present field value may be null in this payload position.
    pub(crate) fn field_is_nullable(self, field: &Field) -> bool {
        match (self.request, self.response) {
            (true, true) => {
                Self::input_field_is_nullable(field) || Self::output_field_is_nullable(field)
            }
            (false, true) => Self::output_field_is_nullable(field),
            (_, false) => Self::input_field_is_nullable(field),
        }
    }
}

/// Map every component schema id to the positions it is reached from ([`SchemaDirections`]).
///
/// Roots are an operation's request body and parameters (request) and its response bodies (response);
/// from each root the walk follows `$ref`s through schema bodies, so a nested type is reached from
/// wherever the type carrying it is. A `$ref` is a leaf of the walk — what lies beyond it is reached
/// from that schema's own body, which is how a type shared between a request DTO and a response DTO
/// comes out marked as both.
pub(crate) fn schema_directions(graph: &ApiGraph) -> BTreeMap<&str, SchemaDirections> {
    let bodies: BTreeMap<&str, &Type> = graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), &schema.body))
        .collect();
    let mut request_roots: Vec<&str> = Vec::new();
    let mut response_roots: Vec<&str> = Vec::new();
    for op in &graph.operations {
        if let Some(body) = &op.request_body {
            request_roots.push(body.ref_id.as_str());
        }
        for param in &op.params {
            collect_named_refs(&param.schema, &mut request_roots);
            // A parameter also carries imported `OpenAPI` fragments verbatim, and those can reference
            // a component the typed schema does not — the same `$ref`s the `OpenAPI` target rewrites
            // on the way out. They are parameters, so they are requests.
            for value in param
                .openapi_content
                .iter()
                .chain(param.openapi_fields.iter().map(|(_, value)| value))
            {
                collect_json_component_refs(value, &bodies, &mut request_roots);
            }
        }
        for response in &op.responses {
            if let Some(body) = &response.body {
                response_roots.push(body.ref_id.as_str());
            }
        }
    }
    for root in &graph.schema_uses {
        match root.use_ {
            super::SchemaUse::Input => request_roots.push(root.schema_id.as_str()),
            super::SchemaUse::Output => response_roots.push(root.schema_id.as_str()),
        }
    }
    let by_request = reachable_schemas(request_roots, &bodies);
    let by_response = reachable_schemas(response_roots, &bodies);
    bodies
        .keys()
        .map(|id| {
            (
                *id,
                SchemaDirections {
                    request: by_request.contains(id),
                    response: by_response.contains(id),
                },
            )
        })
        .collect()
}

/// The positions `schema_id` is reached from, or [`SchemaDirections::default`] for an id the map does
/// not carry — a schema outside the graph occupies no position, the same answer an unwired one gets.
pub(crate) fn directions_of(
    directions: &BTreeMap<&str, SchemaDirections>,
    schema_id: &str,
) -> SchemaDirections {
    directions.get(schema_id).copied().unwrap_or_default()
}

/// The transitive closure of `roots` over the `$ref`s in each reached schema's body.
///
/// A root naming no known schema is skipped: a dangling `$ref` is the lowering's error to report, and
/// reporting it twice from two places would give one defect two messages.
fn reachable_schemas<'a>(
    mut queue: Vec<&'a str>,
    bodies: &BTreeMap<&'a str, &'a Type>,
) -> BTreeSet<&'a str> {
    let mut reached = BTreeSet::new();
    while let Some(id) = queue.pop() {
        let Some(body) = bodies.get(id).copied() else {
            continue;
        };
        if !reached.insert(id) {
            continue;
        }
        collect_named_refs(body, &mut queue);
    }
    reached
}

/// Push every component schema id `ty` references. The match is exhaustive (no `_ =>` arm) so a new
/// [`Type`] variant fails to compile here until its reachability is stated (T-03).
fn collect_named_refs<'a>(ty: &'a Type, out: &mut Vec<&'a str>) {
    match ty {
        Type::Named(ref_id) => out.push(ref_id.as_str()),
        Type::Object(fields) => {
            for field in fields {
                collect_named_refs(&field.schema, out);
            }
        }
        Type::Array(items) => collect_named_refs(items, out),
        Type::Map { key, value } => {
            collect_named_refs(key, out);
            collect_named_refs(value, out);
        }
        Type::Union(variants) => {
            for variant in variants {
                collect_named_refs(variant, out);
            }
        }
        Type::Primitive(_) | Type::WellKnown(_) | Type::Enum(_) | Type::Any {} => {}
    }
}

/// Push every known component schema id referenced by a local `$ref` anywhere inside an imported
/// `OpenAPI` fragment. Ids are looked up in `bodies` so the borrow outlives the decoded name and an
/// unknown reference is skipped rather than invented.
fn collect_json_component_refs<'a>(
    value: &serde_json::Value,
    bodies: &BTreeMap<&'a str, &'a Type>,
    out: &mut Vec<&'a str>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some((id, _)) = object
                .get("$ref")
                .and_then(serde_json::Value::as_str)
                .and_then(super::split_local_component_ref)
                .and_then(|(name, _)| bodies.get_key_value(name.as_str()))
            {
                out.push(id);
            }
            for child in object.values() {
                collect_json_component_refs(child, bodies, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_component_refs(item, bodies, out);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaDirections;
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{Field, Prim, Type};

    fn field() -> Field {
        Field {
            json_name: "value".to_string(),
            serializer_may_omit: false,
            deserializer_accepts_absent: true,
            deserializer_accepts_null: true,
            serializer_may_emit_null: true,
            validator_requires_presence: false,
            validator_rejects_null: false,
            schema: Type::Primitive(Prim::String),
            description: None,
            example: None,
            meta: FieldMeta::default(),
        }
    }

    #[test]
    fn response_nullability_does_not_change_presence() {
        let field = field();
        let response = SchemaDirections {
            request: false,
            response: true,
        };
        assert!(response.field_is_required(&field));
        assert!(!response.model_field_is_optional(&field));
        assert!(response.field_is_nullable(&field));
    }

    #[test]
    fn output_omission_and_null_are_independent() {
        let mut field = field();
        field.serializer_may_omit = true;
        field.serializer_may_emit_null = false;
        let response = SchemaDirections {
            request: false,
            response: true,
        };
        assert!(!response.field_is_required(&field));
        assert!(response.model_field_is_optional(&field));
        assert!(!response.field_is_nullable(&field));
    }

    #[test]
    fn inbound_validation_can_reject_null_without_changing_output() {
        let mut field = field();
        field.validator_requires_presence = true;
        field.validator_rejects_null = true;
        assert!(SchemaDirections::REQUEST.field_is_required(&field));
        assert!(!SchemaDirections::REQUEST.field_is_nullable(&field));
        let response = SchemaDirections {
            request: false,
            response: true,
        };
        assert!(response.field_is_nullable(&field));
    }
}
