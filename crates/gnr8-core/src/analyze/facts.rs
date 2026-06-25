//! Serde mirror of the `goextract` JSON facts document — the Rust side of the
//! Rust↔Go contract (CONTEXT D-02).
//!
//! Every struct uses `#[serde(deny_unknown_fields)]` so malformed or
//! forward-incompatible JSON from the helper is rejected rather than silently
//! trusted (Security V5 / threat T-02-05). The field names mirror the Go `json`
//! tags in `goextract/internal/facts/facts.go` exactly.
//!
//! 02-01 owns the routes-free part of the schema. [`RouteFact`] and its children
//! ([`ParamFact`], [`ResponseFact`]) are defined now so the type exists for 02-02,
//! even though `goextract` currently emits an empty `routes` array.

// These DTOs are the deserialize target for 02-03's `build_graph`. Until that
// consumer lands they are constructed only by the round-trip unit tests, so allow
// dead_code this wave to keep the clippy `-D warnings` gate green without hiding a
// real unused-code signal (the fields are all part of the stable JSON contract).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// The top-level facts document for one analyzed Go module.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoFacts {
    /// The module path of the analyzed target (e.g. `github.com/acme/svc`).
    pub(crate) module: String,
    /// HTTP routes. Empty until 02-02 implements route/handler recognition.
    pub(crate) routes: Vec<RouteFact>,
    /// Extracted DTO/object and enum schemas, sorted by id by the helper.
    pub(crate) schemas: Vec<SchemaFact>,
    /// Analysis diagnostics (lossy/unsupported patterns), sorted by the helper.
    pub(crate) diagnostics: Vec<DiagnosticFact>,
}

/// One HTTP route, derived PURELY from Go code by the helper (no annotation source).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteFact {
    /// HTTP method, uppercase (e.g. `"POST"`).
    pub(crate) method: String,
    /// Code-derived, group-relative, normalized path template (`/`, `/list`,
    /// `/{uuid}`). The dynamic `"/" + basePath` group prefix is not folded here.
    pub(crate) path: String,
    /// The handler function symbol name (e.g. `"createGoal"`).
    pub(crate) handler: String,
    /// Operation id, derived deterministically from the handler symbol in code.
    pub(crate) operation_id: String,
    /// Path and query parameters.
    pub(crate) params: Vec<ParamFact>,
    /// The request body schema reference, if a typed body was inferred.
    pub(crate) request_body: Option<TypeRef>,
    /// Responses keyed by HTTP status.
    pub(crate) responses: Vec<ResponseFact>,
    /// Source provenance for the route registration.
    pub(crate) span: SourceSpan,
}

/// One path or query parameter of a route, derived purely from code. Path params
/// are required; query params default to type `string` and not required. There is
/// no description or enum — those were annotation-only and are gone.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParamFact {
    /// The parameter name (e.g. `"uuid"`, `"cursor"`).
    pub(crate) name: String,
    /// Where the parameter is read from: `"path"` or `"query"`.
    pub(crate) location: String,
    /// Whether the parameter is required.
    pub(crate) required: bool,
    /// The parameter's primitive schema.
    pub(crate) schema: SchemaType,
    /// Source provenance for the parameter access.
    pub(crate) span: SourceSpan,
}

/// One response of a route keyed by HTTP status (from `c.JSON(status, x)`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseFact {
    /// The HTTP status code (e.g. `201`).
    pub(crate) status: u16,
    /// The response body schema reference, if a typed body was inferred.
    pub(crate) body: Option<TypeRef>,
}

/// One extracted named type: an object struct or a string enum.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaFact {
    /// Stable, package-qualified id (e.g. `"internal/common/dto.CreateGoalInput"`).
    pub(crate) id: String,
    /// The Go type name (e.g. `"CreateGoalInput"`).
    pub(crate) name: String,
    /// `"object"` for structs, `"enum"` for string-enum newtypes.
    pub(crate) kind: String,
    /// Object fields, sorted by json name; empty for enums.
    pub(crate) fields: Vec<FieldFact>,
    /// Sorted enum string values; empty for objects.
    pub(crate) enum_values: Vec<String>,
    /// Source provenance for the type declaration.
    pub(crate) span: SourceSpan,
}

/// One field of an object schema.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldFact {
    /// The effective JSON field name (from the `json:"..."` tag).
    pub(crate) json_name: String,
    /// Whether the field is required (`binding:"required"`).
    pub(crate) required: bool,
    /// Whether the field is optional (pointer or `,omitempty`).
    pub(crate) optional: bool,
    /// The field's primitive/ref schema.
    pub(crate) schema: SchemaType,
    /// Optional description from a `description:"..."` tag.
    pub(crate) description: Option<String>,
    /// Optional example from an `example:"..."` tag.
    pub(crate) example: Option<String>,
}

/// A router-/OpenAPI-agnostic description of a Go type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaType {
    /// One of `string|integer|number|boolean|array|object|ref`.
    pub(crate) kind: String,
    /// Format hint (e.g. `"uuid"`, `"date-time"`, `"int64"`), if any.
    pub(crate) format: Option<String>,
    /// Element schema for `array` kinds.
    pub(crate) items: Option<Box<SchemaType>>,
    /// Referenced schema id for `ref` kinds.
    pub(crate) ref_id: Option<String>,
    /// `true` for free-form maps (`object` with additional properties).
    pub(crate) additional_properties: Option<bool>,
}

/// A reference to a schema by its stable id.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypeRef {
    /// The referenced schema id.
    pub(crate) ref_id: String,
}

/// One diagnostic (lossy/unsupported pattern) with a source location (D-10).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticFact {
    /// Severity, `"WARN"` or `"ERROR"`.
    pub(crate) severity: String,
    /// The human-readable message (rule + identity).
    pub(crate) message: String,
    /// The source file the diagnostic applies to.
    pub(crate) file: String,
    /// The 1-based line number.
    pub(crate) line: u32,
}

/// File + line range provenance attached to nodes (D-07).
///
/// Also derives `Serialize` because it flows into the graph serialization in 02-03.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSpan {
    /// The source file path.
    pub(crate) file: String,
    /// The 1-based start line.
    pub(crate) start_line: u32,
    /// The 1-based end line.
    pub(crate) end_line: u32,
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5);
    // scope the allow to the test module so the workspace-wide RUST-04 deny stays
    // intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    /// A minimal facts document mirroring real goextract output (one object schema,
    /// one enum, one diagnostic, empty routes).
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [],
      "schemas": [
        {
          "id": "internal/common/dto.CreateGoalInput",
          "name": "CreateGoalInput",
          "kind": "object",
          "fields": [
            {
              "json_name": "name",
              "required": true,
              "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": "Short name",
              "example": null
            },
            {
              "json_name": "workflowChainIds",
              "required": false,
              "optional": true,
              "schema": {
                "kind": "array",
                "format": null,
                "items": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
                "ref_id": null,
                "additional_properties": null
              },
              "description": null,
              "example": null
            }
          ],
          "enum_values": [],
          "span": { "file": "goal.go", "start_line": 28, "end_line": 28 }
        },
        {
          "id": "internal/common/dto.TargetDirection",
          "name": "TargetDirection",
          "kind": "enum",
          "fields": [],
          "enum_values": ["gte", "lte"],
          "span": { "file": "common.go", "start_line": 39, "end_line": 39 }
        }
      ],
      "diagnostics": [
        {
          "severity": "WARN",
          "message": "float64 -> float32 narrowing: field CreateGoalInput.TargetValue (*float64) loses precision",
          "file": "goal.go",
          "line": 32
        }
      ]
    }"#;

    mod go_facts {
        use super::SAMPLE;
        use crate::analyze::facts::GoFacts;

        #[test]
        fn deserializes_sample_facts_without_error() {
            let facts: GoFacts = serde_json::from_slice(SAMPLE).unwrap();
            assert_eq!(facts.module, "github.com/acme/svc");
            assert!(facts.routes.is_empty());
            assert_eq!(facts.schemas.len(), 2);
            assert_eq!(facts.diagnostics.len(), 1);

            let create = &facts.schemas[0];
            assert_eq!(create.name, "CreateGoalInput");
            assert_eq!(create.kind, "object");
            let name = &create.fields[0];
            assert_eq!(name.json_name, "name");
            assert!(name.required);
            assert_eq!(name.schema.kind, "string");

            let chain = &create.fields[1];
            assert_eq!(chain.schema.kind, "array");
            let items = chain.schema.items.as_ref().unwrap();
            assert_eq!(items.kind, "string");
            assert_eq!(items.format.as_deref(), Some("uuid"));

            let enum_schema = &facts.schemas[1];
            assert_eq!(enum_schema.kind, "enum");
            assert_eq!(enum_schema.enum_values, vec!["gte", "lte"]);
        }

        #[test]
        fn rejects_unknown_fields() {
            // An extra top-level key must fail under deny_unknown_fields.
            let bad = br#"{
              "module": "x",
              "routes": [],
              "schemas": [],
              "diagnostics": [],
              "unexpected": true
            }"#;
            let result: Result<GoFacts, _> = serde_json::from_slice(bad);
            assert!(
                result.is_err(),
                "deny_unknown_fields must reject an unexpected top-level key"
            );
        }
    }

    /// Round-trip a fully-populated route fact (the code-first shape) so the serde
    /// mirror stays in lockstep with `goextract`'s purely code-derived output: a
    /// handler-derived `operation_id`, a path param, and responses by numeric
    /// status. There is no `router_path`/`summary`/`tags`/`secured`/
    /// `security_schemes` and no param `description`/`enum_values` — those were
    /// annotation facts and have been removed (CLAUDE.md rules 1, 3 & 4).
    mod route_facts {
        use crate::analyze::facts::GoFacts;

        // Mirrors a real `go run . ../fixtures/goalservice` route entry for
        // `PUT /{uuid}`: operation_id derived from the handler symbol, a code-read
        // path param, a bound request body, and 200/400/404 from c.JSON.
        const ROUTE: &[u8] = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "PUT",
              "path": "/{uuid}",
              "handler": "updateGoal",
              "operation_id": "updateGoal",
              "params": [
                {
                  "name": "uuid",
                  "location": "path",
                  "required": true,
                  "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
                  "span": { "file": "handlers.go", "start_line": 94, "end_line": 94 }
                }
              ],
              "request_body": { "ref_id": "internal/common/dto.UpdateGoalInput" },
              "responses": [
                { "status": 200, "body": { "ref_id": "internal/common/dto.CommandMessage" } },
                { "status": 400, "body": { "ref_id": "internal/common/dto.HttpError" } },
                { "status": 404, "body": { "ref_id": "internal/common/dto.HttpError" } }
              ],
              "span": { "file": "http.go", "start_line": 57, "end_line": 57 }
            }
          ],
          "schemas": [],
          "diagnostics": []
        }"#;

        #[test]
        fn deserializes_populated_route_with_code_first_fields() {
            let facts: GoFacts = serde_json::from_slice(ROUTE).unwrap();
            assert_eq!(facts.routes.len(), 1);
            let r = &facts.routes[0];

            assert_eq!(r.method, "PUT");
            assert_eq!(r.path, "/{uuid}");
            // operation_id is the handler symbol — no annotation override.
            assert_eq!(r.operation_id, "updateGoal");
            assert_eq!(r.handler, "updateGoal");

            let body = r.request_body.as_ref().unwrap();
            assert!(body.ref_id.ends_with("dto.UpdateGoalInput"));

            let statuses: Vec<u16> = r.responses.iter().map(|x| x.status).collect();
            assert_eq!(statuses, vec![200, 400, 404]);

            let uuid = &r.params[0];
            assert_eq!(uuid.name, "uuid");
            assert_eq!(uuid.location, "path");
            assert!(uuid.required);
            assert_eq!(uuid.schema.format.as_deref(), Some("uuid"));
        }

        #[test]
        fn rejects_unknown_route_field() {
            // A new Go field that the Rust mirror has not adopted must fail-fast.
            let bad = br#"{
              "module": "x",
              "routes": [
                {
                  "method": "GET", "path": "/", "handler": "h",
                  "operation_id": "h", "params": [], "request_body": null,
                  "responses": [], "span": { "file": "f", "start_line": 1, "end_line": 1 },
                  "unexpected_route_field": true
                }
              ],
              "schemas": [], "diagnostics": []
            }"#;
            let result: Result<GoFacts, _> = serde_json::from_slice(bad);
            assert!(
                result.is_err(),
                "deny_unknown_fields must reject an unexpected route key"
            );
        }

        #[test]
        fn rejects_removed_annotation_route_field() {
            // The removed annotation fields (e.g. `security_schemes`) must now be
            // rejected by deny_unknown_fields — proving they are gone for good.
            let bad = br#"{
              "module": "x",
              "routes": [
                {
                  "method": "GET", "path": "/", "handler": "h",
                  "operation_id": "h", "security_schemes": [], "params": [],
                  "request_body": null, "responses": [],
                  "span": { "file": "f", "start_line": 1, "end_line": 1 }
                }
              ],
              "schemas": [], "diagnostics": []
            }"#;
            let result: Result<GoFacts, _> = serde_json::from_slice(bad);
            assert!(
                result.is_err(),
                "a removed annotation field (security_schemes) must be rejected"
            );
        }
    }
}
