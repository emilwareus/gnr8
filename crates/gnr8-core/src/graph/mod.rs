//! The internal API graph — the source of truth from which `OpenAPI` and SDKs are lowered.
//!
//! The graph is deliberately **language-neutral** (D-03): it stores HTTP route facts (method, path
//! template, params, request type, response type + status) plus named schemas described by the closed
//! [`Type`] vocabulary, NOT framework or language internals. No source-language or framework field
//! belongs here — that keeps additional routers and languages addable later without reshaping the graph.
//!
//! Determinism (GRAPH-02 / D-08): every collection in the graph is a sorted [`Vec`] (operations by
//! `(path, method)`, schemas by id, params by name, responses by status, object fields by name, enum
//! members lexically). The graph never serializes an unordered hash map — only sorted vectors — so two
//! `build_graph` runs over unchanged source produce byte-identical output. Operation ids are the handler
//! function symbol (e.g. `createGoal`) — purely code-derived, with no annotation override (CLAUDE.md
//! rules 1 & 3). Schema ids are the package-qualified type name the sidecar already emits.
//!
//! Provenance (D-07): every operation, param, and schema carries a [`SourceSpan`] (file + line range,
//! the file path normalized relative to the analyzed module so the graph is portable across machines).

use crate::analyze::facts::{
    DiagnosticFact, FieldFact, GoFacts, ParamFact, ResponseFact, RouteFact, SchemaFact, TypeRef,
};

// Re-export the neutral type vocabulary so the IR and the facts DTO share ONE definition (the IR
// mirrors the wire contract byte-for-byte; a single source of truth prevents drift). Consumers of the
// graph match these variants exhaustively.
pub use crate::analyze::facts::{FieldFact as Field, Prim, Type, WellKnown};

/// The language-neutral API graph extracted from one analyzed module (D-07).
///
/// All collections are sorted by a stable key so serialization is deterministic (GRAPH-02).
///
/// ## Generation metadata (set by transforms, read by targets)
///
/// `base_path`, `title`, and `security` are **not** extracted from the source — they are facts the
/// typed source cannot express (the mount prefix is often a runtime value; the title is author
/// metadata; auth lives in middleware, not handler signatures, CLAUDE.md rule 4). They live on the
/// graph as plain metadata that a [`crate::sdk::Transform`] sets and a [`crate::sdk::Target`] reads,
/// then passes to the existing [`crate::lower::to_openapi`] / [`crate::gosdk::generate`] functions.
/// They default to a root-mounted, untitled, unsecured API so a bare `build_graph` graph still lowers.
#[derive(Debug, serde::Serialize)]
pub struct ApiGraph {
    /// The module/package path of the analyzed target (e.g. `github.com/acme/svc`).
    pub module: String,
    /// HTTP operations, sorted by `(path, method)`.
    pub operations: Vec<Operation>,
    /// Named schemas (objects + enums), sorted by id.
    pub schemas: Vec<Schema>,
    /// Analysis diagnostics (lossy/unsupported patterns), sorted by `(file, line)`.
    pub diagnostics: Vec<Diagnostic>,
    /// The API base/mount path joined to every group-relative operation path — set by a transform
    /// (`SetBasePath`), read by targets. Defaults to `"/"` (a root-mounted service).
    pub base_path: String,
    /// The `OpenAPI` document title (`info.title`) — set by a transform (`SetTitle`), read by targets.
    /// Defaults to `"API"`.
    pub title: String,
    /// The API security schemes — set by a transform (`ApplySecurity`), read by targets. The single
    /// source of truth for the generated `security` requirement + `components.securitySchemes`
    /// (CLAUDE.md rule 4). Defaults to empty (no security).
    pub security: Vec<SecurityScheme>,
}

/// The default API base path (`"/"`, a root-mounted service) used when no transform sets one.
fn default_base_path() -> String {
    "/".to_string()
}

/// The default `OpenAPI` title (`"API"`) used when no transform sets one.
fn default_title() -> String {
    "API".to_string()
}

impl Default for ApiGraph {
    /// An empty graph with the metadata defaults (`base_path = "/"`, `title = "API"`, no security).
    fn default() -> Self {
        Self {
            module: String::new(),
            operations: Vec::new(),
            schemas: Vec::new(),
            diagnostics: Vec::new(),
            base_path: default_base_path(),
            title: default_title(),
            security: Vec::new(),
        }
    }
}

/// One declared security scheme — graph-owned generation metadata (CLAUDE.md rule 4).
///
/// Security cannot be derived from typed source (auth lives in middleware), so it is supplied by the
/// user configuring our engine — an `ApplySecurity` transform pushes one of these onto
/// [`ApiGraph::security`], and the `OpenAPI` target reads them. This is the public, framework-facing
/// home for the scheme shape (re-exported via [`crate::sdk::prelude`]); the lowering layer maps it
/// into the emitted `components.securitySchemes` entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SecurityScheme {
    /// The `OpenAPI` scheme id (the key under `components.securitySchemes`, e.g. `"ApiKeyAuth"`).
    pub id: String,
    /// The scheme kind (e.g. `"apiKey"`).
    pub kind: String,
    /// Where the credential is read from (e.g. `"header"`).
    pub location: String,
    /// The credential name (for an `apiKey`/`header` scheme, the header name, e.g. `"X-API-Key"`).
    pub name: String,
}

/// One HTTP operation: a method + path template plus its inferred params/body/responses (D-07).
///
/// Language-neutral — there is deliberately no framework handle here; only the recognized HTTP facts.
/// Every field is derived PURELY from source code (CLAUDE.md rules 1 & 3); there is no annotation
/// carry-through (no summary, tags, router-path override, or security here — security comes from the
/// user's gnr8 config at lowering time, rule 4).
#[derive(Debug, serde::Serialize)]
pub struct Operation {
    /// Stable operation id, derived deterministically from the handler symbol (D-08).
    pub id: String,
    /// HTTP method, uppercase (e.g. `"POST"`).
    pub method: String,
    /// Code-derived, group-relative, normalized path template (`/`, `/list`, `/{uuid}`).
    ///
    /// The dynamic mount prefix (`"/" + basePath`) is NOT folded here; the absolute path is a
    /// lowering concern supplied by the host layer (never scraped, rule 1).
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
/// query params default to a string type and not required. There is no enum or description — those
/// were annotation-only and have been removed (CLAUDE.md rules 1 & 3).
#[derive(Debug, serde::Serialize)]
pub struct Param {
    /// The parameter name (e.g. `"uuid"`, `"cursor"`).
    pub name: String,
    /// Where the parameter is read from: `"path"` or `"query"`.
    pub location: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// The parameter's type.
    pub schema: Type,
    /// Source provenance for the parameter access (D-07).
    pub provenance: SourceSpan,
}

/// One response of an operation keyed by HTTP status.
#[derive(Debug, serde::Serialize)]
pub struct Response {
    /// The HTTP status code (e.g. `201`).
    pub status: u16,
    /// The response body schema reference, if a typed body was inferred.
    pub body: Option<SchemaRef>,
}

/// One named schema. Its shape is carried by the neutral [`Type`] vocabulary: a struct/class becomes
/// [`Type::Object`], a string-enum becomes [`Type::Enum`]. There is no separate string discriminator —
/// the [`Type`] variant *is* the discriminant, so a new kind of named type is a compile error in every
/// consumer rather than a silently-mishandled magic string.
#[derive(Debug, serde::Serialize)]
pub struct Schema {
    /// Stable, package-qualified id (e.g. `"internal/common/dto.CreateGoalInput"`).
    pub id: String,
    /// The declared type's name (e.g. `"CreateGoalInput"`).
    pub name: String,
    /// The schema body — typically [`Type::Object`] or [`Type::Enum`]. Object fields are sorted by
    /// name and enum members lexically (determinism).
    pub body: Type,
    /// Source provenance for the type declaration (D-07).
    pub provenance: SourceSpan,
}

/// A reference to a schema by its stable id.
#[derive(Debug, serde::Serialize)]
pub struct SchemaRef {
    /// The referenced schema id.
    pub ref_id: String,
}

/// One diagnostic (lossy/unsupported pattern) with a source location (D-10).
///
/// Derives `Deserialize` as well as `Serialize` so it survives the host↔child JSON boundary inside
/// an [`crate::runner::ArtifactBundle`] (the host deserializes the child's emitted bundle).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Build the language-neutral graph from a sidecar's [`GoFacts`].
    ///
    /// Maps routes → [`Operation`]s (operation id = handler symbol, D-08), request/response type refs
    /// → [`SchemaRef`]s, and schema facts → [`Schema`]s, with provenance on every node (D-07).
    /// `module_root` is the analyzed directory; every span/diagnostic file path is normalized relative
    /// to it so the serialized graph is portable and byte-stable across machines (GRAPH-02).
    ///
    /// Every collection is sorted by a stable key before it is stored (including the object fields and
    /// enum members nested inside a schema body), so two runs over unchanged source serialize
    /// byte-identically.
    ///
    /// `pub(crate)` because it consumes the crate-private [`GoFacts`] DTO; the public entry point is
    /// [`crate::analyze::build_graph`], which runs the sidecar and calls this.
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
            .map(|diag: DiagnosticFact| Diagnostic {
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
            // Generation metadata is not extracted from source — it starts at the defaults and is set
            // by transforms (SetBasePath / SetTitle / ApplySecurity) before targets read it.
            base_path: default_base_path(),
            title: default_title(),
            security: Vec::new(),
        }
    }
}

impl Operation {
    /// Lower one [`RouteFact`] into an [`Operation`], carrying the code-derived id and sorting children.
    fn from_fact(route: RouteFact, root: &str) -> Self {
        // Stable operation id (D-08): the handler-symbol-derived id the sidecar already emits — purely
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
            schema: normalize_type(param.schema),
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
        Self {
            id: schema.id,
            name: schema.name,
            body: normalize_type(schema.body),
            provenance: relativize_span(&schema.span, root),
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

/// Recursively normalize a neutral [`Type`] for the IR: sort an object's fields by name and an enum's
/// members lexically, and recurse through every type-bearing variant, so the serialized graph is
/// byte-stable (GRAPH-02). The match is exhaustive — no `_ =>` arm — so a future [`Type`] variant
/// fails to compile here until it is handled explicitly.
fn normalize_type(ty: Type) -> Type {
    match ty {
        Type::Primitive(prim) => Type::Primitive(prim),
        Type::WellKnown(well_known) => Type::WellKnown(well_known),
        Type::Array(inner) => Type::Array(Box::new(normalize_type(*inner))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(normalize_type(*key)),
            value: Box::new(normalize_type(*value)),
        },
        Type::Named(id) => Type::Named(id),
        Type::Object(fields) => Type::Object(normalize_fields(fields)),
        Type::Enum(mut members) => {
            members.sort();
            Type::Enum(members)
        }
        Type::Union(variants) => Type::Union(variants.into_iter().map(normalize_type).collect()),
        Type::Any {} => Type::Any {},
    }
}

/// Normalize a list of object fields: sort by name (determinism) and recurse into each field's type.
fn normalize_fields(fields: Vec<FieldFact>) -> Vec<FieldFact> {
    let mut fields: Vec<FieldFact> = fields
        .into_iter()
        .map(|f| FieldFact {
            json_name: f.json_name,
            required: f.required,
            optional: f.optional,
            nullable: f.nullable,
            schema: normalize_type(f.schema),
            description: f.description,
            example: f.example,
        })
        .collect();
    fields.sort_by(|a, b| a.json_name.cmp(&b.json_name));
    fields
}

/// Normalize the analyzed module root to a trailing-slash-free string for prefix stripping.
fn normalize_root(module_root: &str) -> String {
    module_root.trim_end_matches('/').to_string()
}

/// Make `file` portable + byte-stable (GRAPH-02): strip the analyzed-module prefix so an absolute
/// path like `<root>/internal/goal/ports/http.go` becomes `internal/goal/ports/http.go`. Paths that
/// are not under the root (or are already relative) are returned unchanged.
///
/// The prefix is stripped only on a path-separator boundary, so a sibling directory whose name shares
/// the root as a string prefix (e.g. `root = "/a/svc"`, `file = "/a/svc-utils/x.go"`) is left absolute
/// rather than mis-stripped to `-utils/x.go`. An exact match of the root maps to the empty relative path.
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{ApiGraph, Type};
    use crate::analyze::facts::GoFacts;

    /// A facts document mirroring real sidecar output: two routes whose operation ids are derived from
    /// the handler symbol (no annotation source), two unsorted schemas (one object with an unsorted
    /// field list, one enum with unsorted members), one diagnostic, and absolute span paths under a
    /// synthetic module root.
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
              "schema": { "type": "well_known", "of": "uuid" },
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
          "body": {
            "type": "object",
            "of": [
              {
                "json_name": "zeta",
                "required": false,
                "optional": true,
                "nullable": true,
                "schema": { "type": "primitive", "of": { "prim": "string" } },
                "description": null,
                "example": null
              },
              {
                "json_name": "name",
                "required": true,
                "optional": false,
                "nullable": false,
                "schema": { "type": "primitive", "of": { "prim": "string" } },
                "description": null,
                "example": null
              }
            ]
          },
          "span": { "file": "/root/goal.go", "start_line": 28, "end_line": 28 }
        },
        {
          "id": "internal/common/dto.TargetDirection",
          "name": "TargetDirection",
          "body": { "type": "enum", "of": ["lte", "gte"] },
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
        assert_eq!(put.id, "updateGoal");
        let post = graph
            .operations
            .iter()
            .find(|op| op.method == "POST")
            .unwrap();
        assert_eq!(post.id, "createGoal");
    }

    #[test]
    fn schemas_sorted_by_id_and_enum_members_sorted() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let ids: Vec<&str> = graph.schemas.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "internal/common/dto.CreateGoalInput",
                "internal/common/dto.TargetDirection",
            ]
        );
        // The enum body's members come back sorted: input was ["lte","gte"], stored ["gte","lte"].
        let target = graph
            .schemas
            .iter()
            .find(|s| s.id.ends_with("TargetDirection"))
            .unwrap();
        match &target.body {
            Type::Enum(members) => assert_eq!(members, &vec!["gte", "lte"]),
            other => panic!("expected enum body, got {other:?}"),
        }
    }

    #[test]
    fn object_fields_sorted_by_name() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let create = graph
            .schemas
            .iter()
            .find(|s| s.id.ends_with("CreateGoalInput"))
            .unwrap();
        match &create.body {
            Type::Object(fields) => {
                let names: Vec<&str> = fields.iter().map(|f| f.json_name.as_str()).collect();
                // Input was [zeta, name]; stored sorted [name, zeta].
                assert_eq!(names, vec!["name", "zeta"]);
            }
            other => panic!("expected object body, got {other:?}"),
        }
    }

    #[test]
    fn field_nullable_axis_is_carried_distinctly_from_optional() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        let create = graph
            .schemas
            .iter()
            .find(|s| s.id.ends_with("CreateGoalInput"))
            .unwrap();
        let fields = match &create.body {
            Type::Object(fields) => fields,
            other => panic!("expected object body, got {other:?}"),
        };
        // `name`: neither optional nor nullable.
        let name = fields.iter().find(|f| f.json_name == "name").unwrap();
        assert!(!name.optional);
        assert!(!name.nullable);
        // `zeta`: both optional and nullable — the two axes are carried independently.
        let zeta = fields.iter().find(|f| f.json_name == "zeta").unwrap();
        assert!(zeta.optional);
        assert!(zeta.nullable);
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
        assert_eq!(graph.diagnostics[0].file, "goal.go");
    }

    #[test]
    fn generation_metadata_starts_at_defaults() {
        let graph = ApiGraph::from_facts(sample_facts(), "/root");
        assert_eq!(graph.base_path, "/");
        assert_eq!(graph.title, "API");
        assert!(graph.security.is_empty());
        let empty = ApiGraph::default();
        assert_eq!(empty.base_path, "/");
        assert_eq!(empty.title, "API");
        assert!(empty.security.is_empty());
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
