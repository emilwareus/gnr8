//! The internal API graph — the source of truth from which `OpenAPI` and the Go SDK are lowered.
//!
//! The graph is deliberately **router-agnostic** (D-03): it stores HTTP route facts (method, path
//! template, params, request type, response type + status), NOT framework internals. Gin is the only
//! recognized router in this proof-of-concept, but no Gin-specific field belongs here — that keeps
//! `chi`/`echo`/`net-http` addable later without reshaping the graph.
//!
//! Determinism (GRAPH-02 / D-08): every collection in the graph is a sorted [`Vec`] (operations by
//! `(path, method)`, schemas by id, params by name, responses by status, fields by json name). The
//! graph never serializes a [`std::collections::HashMap`], so two `build_graph` runs over unchanged
//! source produce byte-identical output. Operation ids are the handler function symbol (e.g.
//! `createGoal`) — purely code-derived, with no annotation override (CLAUDE.md rules 1 & 3). Schema
//! ids are the package-qualified type name the helper already emits.
//!
//! Provenance (D-07): every operation, param, and schema carries a [`SourceSpan`] (file + line range,
//! the file path normalized relative to the analyzed module so the graph is portable across machines).

use crate::analyze::facts::{
    FieldFact, GoFacts, ParamFact, ResponseFact, RouteFact, SchemaFact, TypeRef,
};

/// The router-agnostic API graph extracted from one analyzed Go module (D-07).
///
/// All collections are sorted by a stable key so serialization is deterministic (GRAPH-02).
#[derive(Debug, Default, serde::Serialize)]
pub struct ApiGraph {
    /// The module path of the analyzed target (e.g. `github.com/acme/svc`).
    pub module: String,
    /// HTTP operations, sorted by `(path, method)`.
    pub operations: Vec<Operation>,
    /// Named schemas (objects + enums), sorted by id.
    pub schemas: Vec<Schema>,
    /// Analysis diagnostics (lossy/unsupported patterns), sorted by `(file, line)`.
    pub diagnostics: Vec<Diagnostic>,
}

/// One HTTP operation: a method + path template plus its inferred params/body/responses (D-07).
///
/// Router-agnostic — there is deliberately no Gin handle here; only the recognized HTTP facts.
/// Every field is derived PURELY from Go code (CLAUDE.md rules 1 & 3); there is no annotation
/// carry-through (no summary, tags, router-path override, or security here — security comes from
/// the user's gnr8 config at lowering time, rule 4).
#[derive(Debug, serde::Serialize)]
pub struct Operation {
    /// Stable operation id, derived deterministically from the handler symbol (D-08).
    pub id: String,
    /// HTTP method, uppercase (e.g. `"POST"`).
    pub method: String,
    /// Code-derived, group-relative, normalized path template (`/`, `/list`, `/{uuid}`).
    ///
    /// The dynamic group prefix (`"/" + basePath` in the fixture) is NOT folded here; the absolute
    /// `/goal/...` path is a lowering concern supplied by the Rust layer (never scraped, rule 1).
    pub path: String,
    /// The handler function symbol name (e.g. `"createGoal"`).
    pub handler: String,
    /// Path and query parameters, sorted by name.
    pub params: Vec<Param>,
    /// The request body schema reference, if a typed body was inferred.
    pub request_body: Option<SchemaRef>,
    /// Responses, sorted by status.
    pub responses: Vec<Response>,
    /// Source provenance for the route registration (D-07).
    pub provenance: SourceSpan,
}

/// One path or query parameter of an operation, derived purely from code. Path params are required;
/// query params default to type `string` and not required. There is no enum or description — those
/// were annotation-only and have been removed (CLAUDE.md rules 1 & 3).
#[derive(Debug, serde::Serialize)]
pub struct Param {
    /// The parameter name (e.g. `"uuid"`, `"cursor"`).
    pub name: String,
    /// Where the parameter is read from: `"path"` or `"query"`.
    pub location: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// The parameter's primitive schema.
    pub schema: SchemaType,
    /// Source provenance for the parameter access (D-07).
    pub provenance: SourceSpan,
}

/// One response of an operation keyed by HTTP status (from `c.JSON(status, x)`).
#[derive(Debug, serde::Serialize)]
pub struct Response {
    /// The HTTP status code (e.g. `201`).
    pub status: u16,
    /// The response body schema reference, if a typed body was inferred.
    pub body: Option<SchemaRef>,
}

/// One named schema: an object struct or a string enum.
#[derive(Debug, serde::Serialize)]
pub struct Schema {
    /// Stable, package-qualified id (e.g. `"internal/common/dto.CreateGoalInput"`).
    pub id: String,
    /// The Go type name (e.g. `"CreateGoalInput"`).
    pub name: String,
    /// `"object"` for structs, `"enum"` for string-enum newtypes.
    pub kind: String,
    /// Object fields, sorted by json name; empty for enums.
    pub fields: Vec<Field>,
    /// Sorted enum string values; empty for objects.
    pub enum_values: Vec<String>,
    /// Source provenance for the type declaration (D-07).
    pub provenance: SourceSpan,
}

/// One field of an object schema.
#[derive(Debug, serde::Serialize)]
pub struct Field {
    /// The effective JSON field name (from the `json:"..."` tag).
    pub json_name: String,
    /// Whether the field is required (`binding:"required"`).
    pub required: bool,
    /// Whether the field is optional (pointer or `,omitempty`).
    pub optional: bool,
    /// The field's primitive/ref schema.
    pub schema: SchemaType,
    /// Optional description from a `description:"..."` tag.
    pub description: Option<String>,
    /// Optional example from an `example:"..."` tag.
    pub example: Option<String>,
}

/// A router-/OpenAPI-agnostic description of a Go type (mirrors the helper's `SchemaType`).
#[derive(Debug, serde::Serialize)]
pub struct SchemaType {
    /// One of `string|integer|number|boolean|array|object|ref`.
    pub kind: String,
    /// Format hint (e.g. `"uuid"`, `"date-time"`, `"int64"`), if any.
    pub format: Option<String>,
    /// Element schema for `array` kinds.
    pub items: Option<Box<SchemaType>>,
    /// Referenced schema id for `ref` kinds.
    pub ref_id: Option<String>,
    /// `true` for free-form maps (`object` with additional properties).
    pub additional_properties: Option<bool>,
}

/// A reference to a schema by its stable id.
#[derive(Debug, serde::Serialize)]
pub struct SchemaRef {
    /// The referenced schema id.
    pub ref_id: String,
}

/// One diagnostic (lossy/unsupported pattern) with a source location (D-10).
#[derive(Debug, serde::Serialize)]
pub struct Diagnostic {
    /// Severity, `"WARN"` or `"ERROR"`.
    pub severity: String,
    /// The human-readable message (rule + identity).
    pub message: String,
    /// The source file the diagnostic applies to (module-relative).
    pub file: String,
    /// The 1-based line number.
    pub line: u32,
}

/// File + line-range provenance attached to every graph node (D-07).
///
/// Graph-owned (not the crate-private `facts::SourceSpan`) so the public graph surface is
/// self-contained and the analyzed-module prefix has been stripped from `file` for portability.
#[derive(Debug, serde::Serialize)]
pub struct SourceSpan {
    /// The source file path, relative to the analyzed module.
    pub file: String,
    /// The 1-based start line.
    pub start_line: u32,
    /// The 1-based end line.
    pub end_line: u32,
}

impl ApiGraph {
    /// Build the router-agnostic graph from the helper's [`GoFacts`].
    ///
    /// Maps routes → [`Operation`]s (operation id = handler symbol, D-08), request/response
    /// type refs → [`SchemaRef`]s, and schema facts → [`Schema`]s, with provenance on every node
    /// (D-07). `module_root` is the analyzed directory; every span/diagnostic file path is normalized
    /// relative to it so the serialized graph is portable and byte-stable across machines (GRAPH-02).
    ///
    /// Every collection is sorted by a stable key before it is stored, so two runs over unchanged
    /// source serialize byte-identically.
    ///
    /// `pub(crate)` because it consumes the crate-private [`GoFacts`] DTO; the public entry point is
    /// [`crate::analyze::build_graph`], which runs the helper and calls this.
    #[must_use]
    pub(crate) fn from_facts(facts: GoFacts, module_root: &str) -> Self {
        let root = normalize_root(module_root);

        let mut operations: Vec<Operation> = facts
            .routes
            .into_iter()
            .map(|route| Operation::from_fact(route, &root))
            .collect();
        operations.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.method.cmp(&b.method)));

        let mut schemas: Vec<Schema> = facts
            .schemas
            .into_iter()
            .map(|schema| Schema::from_fact(schema, &root))
            .collect();
        schemas.sort_by(|a, b| a.id.cmp(&b.id));

        let mut diagnostics: Vec<Diagnostic> = facts
            .diagnostics
            .into_iter()
            .map(|diag| Diagnostic {
                severity: diag.severity,
                message: diag.message,
                file: relativize(&diag.file, &root),
                line: diag.line,
            })
            .collect();
        diagnostics.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.message.cmp(&b.message))
        });

        Self {
            module: facts.module,
            operations,
            schemas,
            diagnostics,
        }
    }
}

impl Operation {
    /// Lower one [`RouteFact`] into an [`Operation`], carrying the code-derived id and sorting children.
    fn from_fact(route: RouteFact, root: &str) -> Self {
        // Stable operation id (D-08): the handler-symbol-derived id the helper already emits — purely
        // code-derived, deterministic, with no annotation override path (CLAUDE.md rules 1 & 3).
        let id = route.operation_id;

        let mut params: Vec<Param> = route
            .params
            .into_iter()
            .map(|param| Param::from_fact(param, root))
            .collect();
        params.sort_by(|a, b| a.name.cmp(&b.name));

        let mut responses: Vec<Response> = route
            .responses
            .into_iter()
            .map(Response::from_fact)
            .collect();
        responses.sort_by_key(|r| r.status);

        Self {
            id,
            method: route.method,
            path: route.path,
            handler: route.handler,
            params,
            request_body: route.request_body.map(SchemaRef::from_fact),
            responses,
            provenance: relativize_span(&route.span, root),
        }
    }
}

impl Param {
    fn from_fact(param: ParamFact, root: &str) -> Self {
        Self {
            name: param.name,
            location: param.location,
            required: param.required,
            schema: SchemaType::from_fact(param.schema),
            provenance: relativize_span(&param.span, root),
        }
    }
}

impl Response {
    fn from_fact(response: ResponseFact) -> Self {
        Self {
            status: response.status,
            body: response.body.map(SchemaRef::from_fact),
        }
    }
}

impl Schema {
    fn from_fact(schema: SchemaFact, root: &str) -> Self {
        let mut fields: Vec<Field> = schema.fields.into_iter().map(Field::from_fact).collect();
        fields.sort_by(|a, b| a.json_name.cmp(&b.json_name));
        let mut enum_values = schema.enum_values;
        enum_values.sort();
        Self {
            id: schema.id,
            name: schema.name,
            kind: schema.kind,
            fields,
            enum_values,
            provenance: relativize_span(&schema.span, root),
        }
    }
}

impl Field {
    fn from_fact(field: FieldFact) -> Self {
        Self {
            json_name: field.json_name,
            required: field.required,
            optional: field.optional,
            schema: SchemaType::from_fact(field.schema),
            description: field.description,
            example: field.example,
        }
    }
}

impl SchemaType {
    fn from_fact(schema: crate::analyze::facts::SchemaType) -> Self {
        Self {
            kind: schema.kind,
            format: schema.format,
            items: schema.items.map(|item| Box::new(Self::from_fact(*item))),
            ref_id: schema.ref_id,
            additional_properties: schema.additional_properties,
        }
    }
}

impl SchemaRef {
    fn from_fact(type_ref: TypeRef) -> Self {
        Self {
            ref_id: type_ref.ref_id,
        }
    }
}

/// Normalize the analyzed module root to a trailing-slash-free string for prefix stripping.
fn normalize_root(module_root: &str) -> String {
    module_root.trim_end_matches('/').to_string()
}

/// Make `file` portable + byte-stable (GRAPH-02): strip the analyzed-module prefix so an absolute
/// path like `<root>/internal/goal/ports/http.go` becomes `internal/goal/ports/http.go`. Paths that
/// are not under the root (or are already relative) are returned unchanged.
///
/// The prefix is stripped only on a path-separator boundary, so a sibling directory whose name
/// shares the root as a string prefix (e.g. `root = "/a/svc"`, `file = "/a/svc-utils/x.go"`) is left
/// absolute rather than mis-stripped to `-utils/x.go`. An exact match of the root maps to the empty
/// relative path.
fn relativize(file: &str, root: &str) -> String {
    if root.is_empty() {
        return file.to_string();
    }
    match file.strip_prefix(root) {
        Some("") => String::new(),
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/').to_string(),
        _ => file.to_string(), // not actually under root/ — leave it absolute.
    }
}

/// Map a crate-private `facts::SourceSpan` to the graph-owned [`SourceSpan`], relativizing the file
/// path against the analyzed module root (provenance portability + byte-stability).
fn relativize_span(span: &crate::analyze::facts::SourceSpan, root: &str) -> SourceSpan {
    SourceSpan {
        file: relativize(&span.file, root),
        start_line: span.start_line,
        end_line: span.end_line,
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::ApiGraph;
    use crate::analyze::facts::GoFacts;

    /// A facts document mirroring real goextract output: two routes whose operation ids are derived
    /// from the handler symbol (no annotation source), two unsorted schemas, one diagnostic, and
    /// absolute span paths under a synthetic module root.
    const SAMPLE: &[u8] = br#"{
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
              "span": { "file": "/root/handlers.go", "start_line": 94, "end_line": 94 }
            }
          ],
          "request_body": { "ref_id": "internal/common/dto.UpdateGoalInput" },
          "responses": [
            { "status": 400, "body": { "ref_id": "internal/common/dto.HttpError" } },
            { "status": 200, "body": { "ref_id": "internal/common/dto.CommandMessage" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 57, "end_line": 57 }
        },
        {
          "method": "POST",
          "path": "/",
          "handler": "createGoal",
          "operation_id": "createGoal",
          "params": [],
          "request_body": { "ref_id": "internal/common/dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "internal/common/dto.CommandMessageWithUUID" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 55, "end_line": 55 }
        }
      ],
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
              "description": null,
              "example": null
            }
          ],
          "enum_values": [],
          "span": { "file": "/root/goal.go", "start_line": 28, "end_line": 28 }
        },
        {
          "id": "internal/common/dto.TargetDirection",
          "name": "TargetDirection",
          "kind": "enum",
          "fields": [],
          "enum_values": ["lte", "gte"],
          "span": { "file": "/root/common.go", "start_line": 39, "end_line": 39 }
        }
      ],
      "diagnostics": [
        {
          "severity": "WARN",
          "message": "float64 -> float32 narrowing: field CreateGoalInput.TargetValue (*float64) loses precision",
          "file": "/root/goal.go",
          "line": 32
        }
      ]
    }"#;

    fn sample_facts() -> GoFacts {
        serde_json::from_slice(SAMPLE).unwrap()
    }

    #[test]
    fn operations_sorted_by_path_then_method() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let order: Vec<(&str, &str)> = graph
            .operations
            .iter()
            .map(|op| (op.path.as_str(), op.method.as_str()))
            .collect();
        // "/" sorts before "/{uuid}".
        assert_eq!(order, vec![("/", "POST"), ("/{uuid}", "PUT")]);
    }

    #[test]
    fn operation_id_is_the_handler_symbol() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let put = graph
            .operations
            .iter()
            .find(|op| op.method == "PUT")
            .unwrap();
        // The id is the handler symbol — there is no annotation override path.
        assert_eq!(put.id, "updateGoal");
        let post = graph
            .operations
            .iter()
            .find(|op| op.method == "POST")
            .unwrap();
        assert_eq!(post.id, "createGoal");
    }

    #[test]
    fn schemas_sorted_by_id_and_enum_values_sorted() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let ids: Vec<&str> = graph.schemas.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "internal/common/dto.CreateGoalInput",
                "internal/common/dto.TargetDirection",
            ]
        );
        let target = graph.schemas.iter().find(|s| s.kind == "enum").unwrap();
        // Input was ["lte","gte"]; stored sorted.
        assert_eq!(target.enum_values, vec!["gte", "lte"]);
    }

    #[test]
    fn responses_sorted_by_status() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let put = graph
            .operations
            .iter()
            .find(|op| op.method == "PUT")
            .unwrap();
        let statuses: Vec<u16> = put.responses.iter().map(|r| r.status).collect();
        assert_eq!(statuses, vec![200, 400]);
    }

    #[test]
    fn every_node_carries_relativized_provenance() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        // Operation spans are stripped of the module root → portable, byte-stable.
        for op in &graph.operations {
            assert_eq!(op.provenance.file, "http.go");
            assert!(!op.provenance.file.starts_with('/'));
            for param in &op.params {
                assert_eq!(param.provenance.file, "handlers.go");
            }
        }
        for schema in &graph.schemas {
            assert!(!schema.provenance.file.starts_with('/'));
        }
        // Diagnostic file paths are relativized too.
        assert_eq!(graph.diagnostics[0].file, "goal.go");
    }

    #[test]
    fn serialization_is_byte_identical_across_two_runs() {
        let a = ApiGraph::from_facts(sample_facts(), "/root");
        let b = ApiGraph::from_facts(sample_facts(), "/root");
        let ja = serde_json::to_string(&a).unwrap();
        let jb = serde_json::to_string(&b).unwrap();
        assert_eq!(ja, jb, "two from_facts runs must serialize identically");
    }
}
