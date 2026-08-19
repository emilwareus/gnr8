//! Which HTTP positions a component schema is reached from, and what each artifact reads once it
//! knows.
//!
//! "May this key be absent?" is one question with two code-derived answers on the graph, and which
//! one applies depends on which side of an exchange the payload is on:
//!
//! - `field.required` — what the server's own validation rejects an INBOUND payload for lacking.
//! - `field.optional` — what the serializer may leave out of a payload it WRITES.
//!
//! Neither is a preference over the other and neither is a fallback for the other: the position a
//! schema occupies decides which one speaks, so there is one source per fact per position (CLAUDE.md
//! rule 3). [`schema_directions`] computes those positions once, by walking the graph from every
//! operation; [`SchemaDirections::field_is_required`] and [`SchemaDirections::model_field_is_optional`]
//! are what the `OpenAPI` document and the generated SDK models respectively read off the result.

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

    /// Whether `field`'s key must be present in every payload this schema describes — the `OpenAPI`
    /// `required` array.
    ///
    /// On a request the answer is `field.required`: what the server's own validation rejects a payload
    /// for lacking. On a response no validation rule applies (a handler does not validate what it
    /// marshals), and the answer is `!field.optional`: what the serializer unconditionally writes.
    pub(crate) fn field_is_required(self, field: &Field) -> bool {
        match (self.request, self.response) {
            // Reached from both directions: ONE component has to describe both payloads, so publish
            // only what holds in every position it occupies. Anything else states as mandatory
            // something one of the two sides may legitimately omit.
            (true, true) => field.required && !field.optional,
            // Response-only: what the serializer always writes.
            (false, true) => !field.optional,
            // Request-only — and a schema no operation reaches, which has no position to be read
            // from, so the source's own statement about the field stands.
            (_, false) => field.required,
        }
    }

    /// Whether a generated SDK model may leave `field`'s key out — Go's `,omitempty`, TypeScript's
    /// `?:`, Python's `= None`.
    ///
    /// **This is not the negation of [`Self::field_is_required`], and it is not meant to be.** That
    /// method answers what the document may *claim* about a payload, where the weakest true statement
    /// is always safe. A model does not claim, it *behaves*: on the way out its optionality is a
    /// permission granted to the caller, and on the way in it is tolerance of what the server sent.
    /// So a model is safe in a position when
    ///
    /// - it does not permit omitting a key the server rejects the request for lacking
    ///   (`field.required` ⟹ not optional), and
    /// - it does not demand a key the server may leave out of what it writes
    ///   (`field.optional` ⟹ optional).
    ///
    /// Those hold together everywhere except on a field that is both validated and omittable, and the
    /// rule below follows from one sentence: **a validation rule states what the server rejects an
    /// INBOUND payload for lacking, so a model reads it exactly where the model is inbound and only
    /// inbound; everywhere else the presence axis — what the serializer does with the value, which is
    /// true of the type wherever it appears — stands.**
    ///
    /// The both-directions arm is the deliberate part. It keeps the response answer, so a field that
    /// is validated *and* omittable on a type used both ways stays optional: the alternative makes a
    /// legal response the SDK cannot decode (the over-required response model of #59), where this way
    /// the residual failure is a request the caller can see rejected and fix by passing the value. A
    /// model does not get to crash on data the caller does not control. Publishing the document's
    /// intersection here instead would also mark optional every field of a both-ways type that carries
    /// no validation rule at all — which is most of them — for no gain in either direction.
    pub(crate) fn model_field_is_optional(self, field: &Field) -> bool {
        match (self.request, self.response) {
            // Inbound and only inbound: the server's validation is the whole of what an omission is
            // accepted or rejected against. `,omitempty` governs marshalling, and the server never
            // marshals a request DTO, so the presence axis says nothing here.
            (true, false) => !field.required,
            // Reached from a response, from both, or from no operation at all: the model is (or may
            // be) the decode side, where the key's absence is the server's choice and not the
            // caller's, and demanding it breaks a payload the server is entitled to send.
            (true, true) | (false, _) => field.optional,
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
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4); scope the allow to the
    // test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::SchemaDirections;
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{Field, Prim, Type};

    /// A field carrying nothing but the two presence axes, so a case states what it means and nothing
    /// else.
    fn presence_field(required: bool, optional: bool) -> Field {
        Field {
            json_name: "value".to_string(),
            required,
            optional,
            nullable: false,
            schema: Type::Primitive(Prim::String),
            description: None,
            example: None,
            meta: FieldMeta::default(),
        }
    }

    /// The four positions, and every axis pair a source can state about a field in one of them.
    fn every_case() -> Vec<(SchemaDirections, Field)> {
        let mut cases = Vec::new();
        for request in [false, true] {
            for response in [false, true] {
                for required in [false, true] {
                    for optional in [false, true] {
                        cases.push((
                            SchemaDirections { request, response },
                            presence_field(required, optional),
                        ));
                    }
                }
            }
        }
        cases
    }

    #[test]
    fn a_model_never_permits_omitting_a_key_the_server_validates_for() {
        for (directions, field) in every_case() {
            // The one exception is the deliberate one, and it is exactly this: a field both validated
            // and omittable, on a type used in BOTH directions. No marking is safe there, and
            // `model_field_is_optional` keeps the response answer rather than breaking a legal decode.
            let no_safe_answer =
                directions.request && directions.response && field.required && field.optional;
            if directions.request && field.required && !no_safe_answer {
                assert!(
                    !directions.model_field_is_optional(&field),
                    "{directions:?} + required={} optional={} must not let a caller omit the key",
                    field.required,
                    field.optional
                );
            }
        }
    }

    #[test]
    fn a_model_always_tolerates_a_key_the_serializer_may_drop() {
        for (directions, field) in every_case() {
            // No exception on this side: a response model that demanded a key the server is entitled
            // to leave out could not decode a payload the server is entitled to send.
            if directions.response && field.optional {
                assert!(
                    directions.model_field_is_optional(&field),
                    "{directions:?} + required={} optional={} must tolerate an absent key",
                    field.required,
                    field.optional
                );
            }
        }
    }

    #[test]
    fn the_document_and_a_model_answer_presence_differently_only_where_they_must() {
        // The two answers are the same question asked of two artifacts, so where they diverge is worth
        // pinning: a later "simplification" of one into the other has to fail here rather than in a
        // user's generated code. Note what is NOT in the list: no request-only row survives, so for
        // every schema an operation only ever sends, the `required` array and the model now say the
        // same thing about the same key.
        let divergent: Vec<(bool, bool, bool, bool)> = every_case()
            .into_iter()
            .filter(|(directions, field)| {
                directions.field_is_required(field) == directions.model_field_is_optional(field)
            })
            .map(|(directions, field)| {
                (
                    directions.request,
                    directions.response,
                    field.required,
                    field.optional,
                )
            })
            .collect();
        assert_eq!(
            divergent,
            vec![
                // No operation reaches it, so the document keeps the source's own `required` while the
                // model keeps the presence axis — the two facts, and the two answers, are unrelated.
                (false, false, false, false),
                (false, false, true, true),
                // Reached from both: the document publishes only what holds in every position, the
                // model keeps the answer that cannot break a decode. Here the document under-claims a
                // key the model always writes, which costs nothing on either side of the wire.
                (true, true, false, false),
            ],
            "the two answers may agree or diverge, but only in the documented cases"
        );
    }
}
