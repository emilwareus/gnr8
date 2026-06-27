//! The built-in pipeline stages — thin wrappers over the existing deterministic core functions.
//!
//! Every stage here reproduces a knob that used to live in `.gnr8/config.toml`, now expressed as a
//! composable Rust value. CRITICAL (CLAUDE.md rules 2 & 3): these NEVER re-implement extraction,
//! lowering, or SDK emission, and they NEVER add a second source for a fact or a fallback path. A
//! source calls [`crate::analyze::build_graph`]; a target reads the graph metadata a transform set
//! and calls the existing [`crate::lower::to_openapi`] / [`crate::gosdk::generate`]; a transform
//! mutates the one graph. One deterministic path per fact.

// User-facing prose dense with proper nouns (Gin, OpenAPI, SDK, apiKey, ...); allow doc_markdown
// module-wide (mirrors the rest of the framework surface).
#![allow(clippy::doc_markdown)]

use super::{
    collect_cache_input_files, hash_files, Artifacts, Cx, PostProcess, Source, Target, Transform,
};
use crate::graph::{ApiGraph, Response, SchemaRef, SecurityScheme, Type};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::model_style::PyModelStyle;
use crate::sdk::surface::SdkTypeAliases;
use crate::CoreError;
use std::path::Path;

// ---------------------------------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------------------------------

/// The Go + Gin source: wraps [`crate::analyze::build_graph`] (the goextract subprocess driver).
///
/// `inputs` are project-relative source directories; for now exactly ONE is supported (multi-input
/// fan-in is a documented later stage), and a different count is a clear typed error rather than a
/// silent first-wins. The single input is resolved against [`Cx::project_root`] so a relative `"."`
/// analyzes the project root, not the process cwd.
#[derive(Debug, Default, Clone)]
pub struct GoGin {
    inputs: Vec<String>,
    package_patterns: Vec<String>,
}

impl GoGin {
    /// A Go + Gin source with no inputs yet (configure with [`GoGin::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }

    /// Scope Go package loading to the given `go/packages` patterns, resolved from the input module
    /// root. Empty means the historical whole-module `"./..."` load.
    #[must_use]
    pub fn packages<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.package_patterns = patterns.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for GoGin {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        // Exactly one input dir for now (mirrors the lifecycle single-input PoC restriction): reject
        // zero or many with a clear typed error rather than silently analyzing the first (D-02).
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "GoGin source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "GoGin source lists {} inputs, but multi-input analysis is not yet supported \
                         — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        // Resolve the input against the project root so a relative input analyzes the PROJECT, not the
        // process cwd (an absolute input is left as-is by `Path::join`). This matches the lifecycle's
        // input-resolution and keeps span provenance relative to the same root.
        let resolved = cx.project_root.join(input);
        let cache_key = go_gin_cache_key(&resolved, &self.package_patterns, cx);
        if let Some(cached) = load_go_gin_cache(cx, &cache_key) {
            return Ok(cached);
        }
        let input_arg = resolved.to_string_lossy();
        let graph =
            crate::analyze::build_go_graph_with_patterns(&input_arg, &self.package_patterns)?;
        save_go_gin_cache(cx, &cache_key, &graph);
        Ok(graph)
    }

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        let [single] = self.inputs.as_slice() else {
            return None;
        };
        Some(vec![cx.project_root.join(single)])
    }
}

fn go_gin_cache_key(input: &Path, package_patterns: &[String], cx: &Cx) -> String {
    let mut files = Vec::new();
    collect_cache_input_files(input, &mut files);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gnr8-go-gin-source-cache-v1\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\n");
    for pattern in package_patterns {
        hasher.update(pattern.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(hash_files(&files, &cx.project_root).as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn load_go_gin_cache(cx: &Cx, key: &str) -> Option<ApiGraph> {
    let bytes = std::fs::read(go_gin_cache_path(cx, key)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_go_gin_cache(cx: &Cx, key: &str, graph: &ApiGraph) {
    let path = go_gin_cache_path(cx, key);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(graph) else {
        return;
    };
    let _ = std::fs::write(path, bytes);
}

fn go_gin_cache_path(cx: &Cx, key: &str) -> std::path::PathBuf {
    cx.project_root
        .join(crate::lifecycle::WORKSPACE_DIR)
        .join("cache")
        .join("sources")
        .join("go-gin")
        .join(format!("{key}.json"))
}

/// The FastAPI (Python) source: wraps [`crate::analyze::build_graph`] (the pyextract subprocess
/// driver), exactly like [`GoGin`] wraps goextract.
///
/// `inputs` are project-relative source directories; for now exactly ONE is supported, and a
/// different count is a clear typed error rather than a silent first-wins. The single input is
/// resolved against [`Cx::project_root`]. This Source does NOT pick the language — it calls the SAME
/// [`crate::analyze::build_graph`], which detects Python by scanning the target (CLAUDE.md rule 3):
/// one deterministic path per fact, never a per-Source extraction fork.
#[derive(Debug, Default, Clone)]
pub struct FastApi {
    inputs: Vec<String>,
}

impl FastApi {
    /// A FastAPI source with no inputs yet (configure with [`FastApi::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for FastApi {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        // Exactly one input dir for now: reject zero or many with a clear typed error rather than
        // silently analyzing the first (mirrors GoGin).
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "FastApi source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "FastApi source lists {} inputs, but multi-input analysis is not yet \
                         supported — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        // Resolve against the project root so a relative input analyzes the PROJECT, not the process
        // cwd. The SAME build_graph the Go source calls — language dispatch is by target detection.
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph_for_lang(
            &resolved.to_string_lossy(),
            crate::analyze::Lang::Python,
        )
    }
}

/// The Flask (Python) source: wraps [`crate::analyze::build_graph`] (the pyextract subprocess
/// driver), a verbatim twin of [`FastApi`]/[`GoGin`] differing only in the error proper noun.
///
/// `inputs` are project-relative source directories; exactly ONE is supported for now. Like every
/// other source it calls the SAME [`crate::analyze::build_graph`] — language is detected from the
/// target, never from which Source was used (CLAUDE.md rule 3).
#[derive(Debug, Default, Clone)]
pub struct Flask {
    inputs: Vec<String>,
}

impl Flask {
    /// A Flask source with no inputs yet (configure with [`Flask::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for Flask {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "Flask source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "Flask source lists {} inputs, but multi-input analysis is not yet \
                         supported — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph_for_lang(
            &resolved.to_string_lossy(),
            crate::analyze::Lang::Python,
        )
    }
}

/// The NestJS (TypeScript) source: wraps [`crate::analyze::build_graph`] (the tsextract subprocess
/// driver), a verbatim twin of [`FastApi`]/[`Flask`]/[`GoGin`] differing only in the error proper
/// noun.
///
/// `inputs` are project-relative source directories; exactly ONE is supported for now. Like every
/// other source it calls the SAME [`crate::analyze::build_graph`] — language is detected from the
/// TARGET (the `*.ts` tree), never from which Source was used (CLAUDE.md rule 3/4): there is no
/// per-Source extraction fork.
#[derive(Debug, Default, Clone)]
pub struct NestJs {
    inputs: Vec<String>,
}

impl NestJs {
    /// A NestJS source with no inputs yet (configure with [`NestJs::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for NestJs {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "NestJs source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "NestJs source lists {} inputs, but multi-input analysis is not yet \
                         supported — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph_for_lang(
            &resolved.to_string_lossy(),
            crate::analyze::Lang::TypeScript,
        )
    }
}

// ---------------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------------

/// Set [`ApiGraph::base_path`] — the API base/mount path joined to every group-relative operation
/// path (replaces the `base_path` TOML knob).
#[derive(Debug, Clone)]
pub struct SetBasePath {
    base_path: String,
}

impl SetBasePath {
    /// Build the transform with the given base path (e.g. `"/books"`).
    #[must_use]
    pub fn new(base_path: impl Into<String>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }
}

impl Transform for SetBasePath {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.base_path.clone_from(&self.base_path);
        Ok(())
    }
}

/// Set [`ApiGraph::title`] — the OpenAPI document title (`info.title`) (replaces the `title` knob).
#[derive(Debug, Clone)]
pub struct SetTitle {
    title: String,
}

impl SetTitle {
    /// Build the transform with the given title (e.g. `"Bookstore API"`).
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl Transform for SetTitle {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.title.clone_from(&self.title);
        Ok(())
    }
}

/// Set or replace the typed success response for one operation.
///
/// This is a graph-level correction hook for source frameworks where a handler's response type is not
/// statically recoverable. Because it mutates the neutral IR, every downstream target sees the same
/// response fact: OpenAPI, Go, Python, and TypeScript stay in agreement.
#[derive(Debug, Clone)]
pub struct SetOperationSuccessResponse {
    matcher: OperationMatcher,
    schema: String,
    status: u16,
}

#[derive(Debug, Clone)]
enum OperationMatcher {
    Id(String),
    Route { method: String, path: String },
}

impl SetOperationSuccessResponse {
    /// Match an operation by generated operation id.
    #[must_use]
    pub fn for_operation(operation_id: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            matcher: OperationMatcher::Id(operation_id.into()),
            schema: schema.into(),
            status: 200,
        }
    }

    /// Match an operation by method and graph path.
    #[must_use]
    pub fn for_route(
        method: impl Into<String>,
        path: impl Into<String>,
        schema: impl Into<String>,
    ) -> Self {
        Self {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            schema: schema.into(),
            status: 200,
        }
    }

    /// Override the success status code to set. Defaults to 200.
    #[must_use]
    pub const fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}

impl Transform for SetOperationSuccessResponse {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let schema_matches: Vec<_> = ir
            .schemas
            .iter()
            .filter(|schema| schema.id == self.schema || schema.name == self.schema)
            .map(|schema| schema.id.clone())
            .collect();
        let schema_id = match schema_matches.as_slice() {
            [single] => single.clone(),
            [] => {
                return Err(CoreError::Config {
                    message: format!(
                        "success response schema {:?} does not match any graph schema id or name",
                        self.schema
                    ),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "success response schema {:?} matches {} schemas; use the full schema id",
                        self.schema,
                        many.len()
                    ),
                });
            }
        };

        let matches: Vec<usize> = ir
            .operations
            .iter()
            .enumerate()
            .filter_map(|(index, op)| {
                let is_match = match &self.matcher {
                    OperationMatcher::Id(id) => op.id == *id,
                    OperationMatcher::Route { method, path } => {
                        op.method == *method && op.path == *path
                    }
                };
                is_match.then_some(index)
            })
            .collect();
        let op_index = match matches.as_slice() {
            [single] => *single,
            [] => {
                return Err(CoreError::Config {
                    message: format!(
                        "success response override did not match any operation: {:?}",
                        self.matcher
                    ),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "success response override matched {} operations: {:?}",
                        many.len(),
                        self.matcher
                    ),
                });
            }
        };

        let op = &mut ir.operations[op_index];
        op.responses
            .retain(|response| response.status != self.status);
        op.responses.push(Response {
            status: self.status,
            body: Some(SchemaRef { ref_id: schema_id }),
        });
        op.responses.sort_by_key(|response| response.status);
        Ok(())
    }
}

/// Override the type of one field in one object schema.
///
/// This is a graph-level correction hook for schema shapes that are intentionally dynamic in source
/// code and cannot be recovered precisely by static extraction. Because the override happens in the
/// neutral IR, OpenAPI and every SDK target agree on the corrected field shape.
#[derive(Debug, Clone)]
pub struct SetSchemaFieldType {
    schema: String,
    field: String,
    ty: Type,
}

impl SetSchemaFieldType {
    /// Match a schema by id or bare generated name, then replace `field`'s type.
    #[must_use]
    pub fn new(schema: impl Into<String>, field: impl Into<String>, ty: Type) -> Self {
        Self {
            schema: schema.into(),
            field: field.into(),
            ty,
        }
    }

    /// Set the field to a homogeneous array of free-form object/value payloads.
    #[must_use]
    pub fn array_of_free_form_objects(schema: impl Into<String>, field: impl Into<String>) -> Self {
        Self::new(schema, field, Type::Array(Box::new(Type::Any {})))
    }
}

impl Transform for SetSchemaFieldType {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let matches: Vec<usize> = ir
            .schemas
            .iter()
            .enumerate()
            .filter_map(|(index, schema)| {
                (schema.id == self.schema || schema.name == self.schema).then_some(index)
            })
            .collect();
        let schema_index = match matches.as_slice() {
            [single] => *single,
            [] => {
                return Err(CoreError::Config {
                    message: format!(
                        "field type override schema {:?} does not match any graph schema id or name",
                        self.schema
                    ),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "field type override schema {:?} matches {} schemas; use the full schema id",
                        self.schema,
                        many.len()
                    ),
                });
            }
        };

        let schema = &mut ir.schemas[schema_index];
        let Type::Object(fields) = &mut schema.body else {
            return Err(CoreError::Config {
                message: format!(
                    "field type override schema {:?} is not an object schema",
                    self.schema
                ),
            });
        };

        let field = fields
            .iter_mut()
            .find(|field| field.json_name == self.field)
            .ok_or_else(|| CoreError::Config {
                message: format!(
                    "field type override did not find field {:?} on schema {:?}",
                    self.field, self.schema
                ),
            })?;
        field.schema = self.ty.clone();
        Ok(())
    }
}

/// Push a security scheme onto [`ApiGraph::security`] — the single source of truth for the generated
/// `security` requirement + `components.securitySchemes` (replaces the `[[security.schemes]]` knob,
/// CLAUDE.md rule 4).
#[derive(Debug, Clone)]
pub struct ApplySecurity {
    scheme: SecurityScheme,
}

impl ApplySecurity {
    /// An `apiKey`-in-`header` scheme: `id` is the OpenAPI scheme id (e.g. `"ApiKeyAuth"`),
    /// `header_name` is the credential header (e.g. `"X-API-Key"`).
    #[must_use]
    pub fn api_key(id: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            scheme: SecurityScheme {
                id: id.into(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: header_name.into(),
            },
        }
    }
}

impl Transform for ApplySecurity {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.security.push(self.scheme.clone());
        Ok(())
    }
}

/// Rename an operation by id: remap `from`'s `operation.id` to `to` (replaces a `[naming.operations]`
/// entry). Reuses the existing [`crate::lifecycle::apply_naming`] logic so the rename semantics (and
/// the `$ref`-rewrite guarantees) stay identical to the host path.
#[derive(Debug, Clone)]
pub struct RenameOperation {
    from: String,
    to: String,
}

impl RenameOperation {
    /// Remap the operation whose id is `from` to `to`.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl Transform for RenameOperation {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let mut naming = crate::lifecycle::NamingOverrides::default();
        naming.operations.insert(self.from.clone(), self.to.clone());
        crate::lifecycle::apply_naming(ir, &naming)
    }
}

/// Rename a type (schema) by id-or-bare-name: remap `from` to `to`, rewriting every `$ref` that
/// pointed at it (replaces a `[naming.types]` entry). Reuses [`crate::lifecycle::apply_naming`] so a
/// rename that would collide/collapse/chain is rejected exactly as on the host path.
#[derive(Debug, Clone)]
pub struct RenameType {
    from: String,
    to: String,
}

impl RenameType {
    /// Remap the schema matched by `from` (its id OR bare name) to `to`.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl Transform for RenameType {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let mut naming = crate::lifecycle::NamingOverrides::default();
        naming.types.insert(self.from.clone(), self.to.clone());
        crate::lifecycle::apply_naming(ir, &naming)
    }
}

/// Assign SDK operation groups from configurable rules.
///
/// Groups are generation metadata used by SDK layout templates and future grouped client surfaces.
/// Rules run in the order they are configured; the first match for an operation wins.
#[derive(Debug, Clone, Default)]
pub struct GroupOperations {
    rules: Vec<GroupRule>,
}

#[derive(Debug, Clone)]
enum GroupRule {
    PathPrefix { prefix: String, group: String },
    Operation { id: String, group: String },
}

impl GroupOperations {
    /// No grouping rules.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Group operations whose path starts with `prefix`.
    #[must_use]
    pub fn by_path_prefix(mut self, prefix: impl Into<String>, group: impl Into<String>) -> Self {
        self.rules.push(GroupRule::PathPrefix {
            prefix: prefix.into(),
            group: group.into(),
        });
        self
    }

    /// Group one operation by exact operation id.
    #[must_use]
    pub fn by_operation(mut self, id: impl Into<String>, group: impl Into<String>) -> Self {
        self.rules.push(GroupRule::Operation {
            id: id.into(),
            group: group.into(),
        });
        self
    }
}

impl Transform for GroupOperations {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        for op in &mut ir.operations {
            op.group = None;
            for rule in &self.rules {
                let matched = match rule {
                    GroupRule::PathPrefix { prefix, group } => {
                        if op.path.starts_with(prefix) {
                            op.group = Some(group.clone());
                            true
                        } else {
                            false
                        }
                    }
                    GroupRule::Operation { id, group } => {
                        if op.id == *id {
                            op.group = Some(group.clone());
                            true
                        } else {
                            false
                        }
                    }
                };
                if matched {
                    break;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------------------

/// The OpenAPI 3.1 target: lowers the frozen IR to an OpenAPI document and writes it at [`OpenApi31::to`].
///
/// Reads `ir.title` / `ir.base_path` / `ir.security` (the metadata transforms set) and calls the
/// existing [`crate::lower::to_openapi`] — NOT a re-implementation. The graph's [`SecurityScheme`]s
/// are passed straight through (`to_openapi` takes `&[SecurityScheme]` directly).
#[derive(Debug, Clone)]
pub struct OpenApi31 {
    path: String,
}

impl OpenApi31 {
    /// An OpenAPI 3.1 target with no output path yet (set with [`OpenApi31::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: String::new(),
        }
    }

    /// Set the output path for the OpenAPI document (e.g. `"generated/openapi.yaml"`).
    #[must_use]
    pub fn to(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}

impl Default for OpenApi31 {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for OpenApi31 {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.path.is_empty() {
            return Err(CoreError::Config {
                message: "OpenApi31 target has no output path — call .to(\"openapi.yaml\")"
                    .to_string(),
            });
        }
        // Pass the graph's security schemes straight to the existing lowering (the single source of
        // truth — an `ApplySecurity` transform set them); never a re-implementation (CLAUDE.md rule 3).
        let doc = crate::lower::to_openapi(ir, &ir.title, &ir.base_path, &ir.security)?;
        out.write(self.path.clone(), doc);
        Ok(())
    }

    /// The OpenAPI artifact path is a loop-safety anchor (a re-run must not ingest the document it
    /// wrote — although it is YAML not Go, declaring it keeps the pipeline's exclusion complete).
    fn output_anchors(&self) -> Vec<String> {
        if self.path.is_empty() {
            Vec::new()
        } else {
            vec![self.path.clone()]
        }
    }
}

/// The OpenAPI 3.1 JSON target: lowers the frozen IR to OpenAPI and writes pretty JSON.
#[derive(Debug, Clone)]
pub struct OpenApi31Json {
    path: String,
}

impl OpenApi31Json {
    /// An OpenAPI 3.1 JSON target with no output path yet (set with [`OpenApi31Json::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: String::new(),
        }
    }

    /// Set the output path for the OpenAPI JSON document (e.g. `"generated/openapi.json"`).
    #[must_use]
    pub fn to(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}

impl Default for OpenApi31Json {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for OpenApi31Json {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.path.is_empty() {
            return Err(CoreError::Config {
                message: "OpenApi31Json target has no output path — call .to(\"openapi.json\")"
                    .to_string(),
            });
        }
        let doc = crate::lower::to_openapi_json(ir, &ir.title, &ir.base_path, &ir.security)?;
        out.write(self.path.clone(), doc);
        Ok(())
    }

    fn output_anchors(&self) -> Vec<String> {
        if self.path.is_empty() {
            Vec::new()
        } else {
            vec![self.path.clone()]
        }
    }
}

/// A static text-file target for SDK/runtime files that should be produced alongside generated code.
///
/// Include entries are exact relative file paths, or directory prefixes ending in `/**`.
/// Files are read from `from` and written under `to` with the same relative path. This keeps
/// hand-authored support modules, package metadata, examples, or docs inside the same deterministic
/// lifecycle as generated SDK files without baking any project-specific paths into gnr8.
#[derive(Debug, Clone, Default)]
pub struct StaticFiles {
    from_dir: String,
    to_dir: String,
    includes: Vec<String>,
}

impl StaticFiles {
    /// A static file target with no source/destination yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the project-relative source directory to read static files from.
    #[must_use]
    pub fn from(mut self, dir: impl Into<String>) -> Self {
        self.from_dir = dir.into();
        self
    }

    /// Set the project-relative destination directory to write static files under.
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.to_dir = dir.into();
        self
    }

    /// Set exact file includes and/or recursive directory includes ending in `/**`.
    #[must_use]
    pub fn include<I, S>(mut self, includes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.includes = includes.into_iter().map(Into::into).collect();
        self
    }
}

impl Target for StaticFiles {
    fn generate(&self, _ir: &ApiGraph, out: &mut Artifacts, cx: &Cx) -> Result<(), CoreError> {
        if self.from_dir.is_empty() {
            return Err(CoreError::Config {
                message: "StaticFiles target has no source dir — call .from(\"path\")".to_string(),
            });
        }
        if self.to_dir.is_empty() {
            return Err(CoreError::Config {
                message: "StaticFiles target has no output dir — call .to(\"path\")".to_string(),
            });
        }

        let source_root = cx.project_root.join(self.from_dir.trim_end_matches('/'));
        let mut files = Vec::new();
        for include in &self.includes {
            collect_static_include(&source_root, include, &mut files)?;
        }
        files.sort();
        files.dedup();

        let to_dir = self.to_dir.trim_end_matches('/');
        for rel in files {
            let source_path = source_root.join(&rel);
            let text = std::fs::read_to_string(&source_path).map_err(|err| CoreError::Io {
                message: format!(
                    "failed to read static file {}: {err}",
                    source_path.display()
                ),
            })?;
            out.write(format!("{to_dir}/{rel}"), text);
        }
        Ok(())
    }

    fn output_anchors(&self) -> Vec<String> {
        if self.to_dir.is_empty() {
            return Vec::new();
        }

        let to_dir = self.to_dir.trim_end_matches('/');
        let mut anchors: Vec<String> = self
            .includes
            .iter()
            .map(|include| {
                let rel = include.trim_end_matches("/**").trim_end_matches('/');
                format!("{to_dir}/{rel}")
            })
            .collect();
        anchors.sort();
        anchors.dedup();
        anchors.retain(|anchor| !anchor.ends_with('/'));
        anchors
    }
}

/// The Go SDK target: generates the multi-file Go SDK bundle and writes each file under [`GoSdk::to`].
///
/// Derives the SDK's Go package name from [`GoSdk::module`] (the last path segment, sanitized — the
/// same single-source-of-truth derivation the config used), calls the existing
/// [`crate::gosdk::generate`] to produce the bundle, splits it into files via
/// [`crate::gosdk::split_bundle`], and writes each at `<dir>/<name>`.
#[derive(Debug, Clone)]
pub struct GoSdk {
    module: String,
    dir: String,
    layout: SdkFileLayout,
    aliases: SdkTypeAliases,
}

impl GoSdk {
    /// A Go SDK target with no module/output yet (set with [`GoSdk::module`] + [`GoSdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            dir: String::new(),
            layout: SdkFileLayout::compact(),
            aliases: SdkTypeAliases::default(),
        }
    }

    /// Set the Go module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The package
    /// name is derived from this — the single source of truth (CLAUDE.md rule 3).
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the output directory for the generated SDK files (e.g. `"generated/sdk"`).
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }

    /// Set the generated file layout.
    #[must_use]
    pub fn layout(mut self, layout: SdkFileLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Use the split layout for larger SDKs.
    #[must_use]
    pub fn split_files(self) -> Self {
        self.layout(SdkFileLayout::split().root_operations().root_models())
    }

    /// Add compatibility type aliases to the generated SDK surface.
    #[must_use]
    pub fn aliases(mut self, aliases: SdkTypeAliases) -> Self {
        self.aliases = aliases;
        self
    }

    /// Expose `alias` as an additional type name for a schema id or generated schema name.
    #[must_use]
    pub fn type_alias(self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        let aliases = self.aliases.clone().type_alias(schema, alias);
        self.aliases(aliases)
    }
}

impl Default for GoSdk {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for GoSdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no module — call .module(\"example.com/acme/sdk\")"
                    .to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        // Derive the package from the module path (the single source of truth) and generate via the
        // existing deterministic SDK generator — never a re-implementation (CLAUDE.md rules 2 & 3).
        let package = sdk_package(&self.module)?;
        let files = crate::gosdk::generate_files_with_layout(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
        )?;
        write_sdk_files(out, &self.dir, files)?;
        out.write(
            format!("{}/go.mod", self.dir.trim_end_matches('/')),
            format!("module {}\n\ngo 1.23\n", self.module),
        );
        Ok(())
    }

    /// The SDK output directory is the critical loop-safety anchor: the generated `*.go` files form a
    /// Go package inside the analyzed module, so without excluding this dir the source would re-ingest
    /// them and duplicate every schema (the contamination `crate::lifecycle::exclude_output_paths`
    /// prevents on the host path).
    fn output_anchors(&self) -> Vec<String> {
        if self.dir.is_empty() {
            Vec::new()
        } else {
            vec![self.dir.trim_end_matches('/').to_string()]
        }
    }
}

/// The Python SDK target: generates the multi-file Python SDK bundle and writes each file under
/// [`PySdk::to`].
///
/// The structural twin of [`GoSdk`] (minus the `gofmt` step Python has no analog for). Derives the
/// SDK's Python package name from [`PySdk::module`] via the SAME [`sdk_package`] single-source-of-truth
/// derivation `GoSdk` uses (CLAUDE.md rule 3 — no second derivation), takes the URL prefix from
/// `ir.base_path` (the value `SetBasePath` set and the OpenAPI lowering reads — never re-derived),
/// calls the existing [`crate::pysdk::generate`] to produce the bundle, splits it into files via
/// [`crate::pysdk::split_bundle`], and writes each at `<dir>/<name>`.
#[derive(Debug, Clone)]
pub struct PySdk {
    module: String,
    dir: String,
    layout: SdkFileLayout,
    model_style: PyModelStyle,
    aliases: SdkTypeAliases,
}

impl PySdk {
    /// A Python SDK target with no module/output yet (set with [`PySdk::module`] + [`PySdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            dir: String::new(),
            layout: SdkFileLayout::compact(),
            model_style: PyModelStyle::default(),
            aliases: SdkTypeAliases::default(),
        }
    }

    /// Set the module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The Python
    /// package name is derived from this — the single source of truth (CLAUDE.md rule 3), the same
    /// derivation `GoSdk` uses.
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the output directory for the generated SDK files (e.g. `"generated/sdk-py"`).
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }

    /// Set the generated file layout.
    #[must_use]
    pub fn layout(mut self, layout: SdkFileLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Use the split layout for larger SDKs.
    #[must_use]
    pub fn split_files(self) -> Self {
        self.layout(SdkFileLayout::split().model_dir("models"))
    }

    /// Use Pydantic v2 `BaseModel` models. This is the default.
    #[must_use]
    pub fn pydantic(mut self) -> Self {
        self.model_style = PyModelStyle::Pydantic;
        self
    }

    /// Use stdlib dataclass models instead of Pydantic.
    #[must_use]
    pub fn dataclasses(mut self) -> Self {
        self.model_style = PyModelStyle::Dataclass;
        self
    }

    /// Add compatibility type aliases to the generated SDK surface.
    #[must_use]
    pub fn aliases(mut self, aliases: SdkTypeAliases) -> Self {
        self.aliases = aliases;
        self
    }

    /// Expose `alias` as an additional type name for a schema id or generated schema name.
    #[must_use]
    pub fn type_alias(self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        let aliases = self.aliases.clone().type_alias(schema, alias);
        self.aliases(aliases)
    }
}

impl Default for PySdk {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for PySdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "PySdk target has no module — call .module(\"example.com/acme/sdk\")"
                    .to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "PySdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        // Derive the package from the module path via the SAME single source of truth GoSdk uses, and
        // generate via the existing deterministic Python SDK generator — never a re-derivation, never
        // a fallback (CLAUDE.md rules 2 & 3). `ir.base_path` is the same single source of truth the
        // OpenAPI lowering reads (rule 3/4 — never re-derived).
        let package = sdk_package(&self.module)?;
        let files = crate::pysdk::generate_files_with_options(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            self.model_style,
            &self.aliases,
        )?;
        write_sdk_files(out, &self.dir, files)?;
        Ok(())
    }

    /// The SDK output directory is the critical loop-safety anchor: the generated `*.py` files form a
    /// Python package inside the analyzed source tree, so without excluding this dir the source would
    /// re-ingest them and duplicate every schema (the contamination
    /// `crate::lifecycle::exclude_output_paths` prevents on the host path, T-03-02-02).
    fn output_anchors(&self) -> Vec<String> {
        if self.dir.is_empty() {
            Vec::new()
        } else {
            vec![self.dir.trim_end_matches('/').to_string()]
        }
    }
}

/// The TypeScript SDK target: generates the multi-file TypeScript SDK bundle and writes each file
/// under [`TsSdk::to`].
///
/// The structural twin of [`PySdk`]/[`GoSdk`]. Derives the SDK's package name from [`TsSdk::module`]
/// via the SAME [`sdk_package`] single-source-of-truth derivation `PySdk`/`GoSdk` use (CLAUDE.md
/// rule 3 — no second derivation, no TS-specific sanitizer), takes the URL prefix from `ir.base_path`
/// (the value `SetBasePath` set and the OpenAPI lowering reads — never re-derived), calls the existing
/// [`crate::tssdk::generate`] to produce the bundle, splits it into files via
/// [`crate::tssdk::split_bundle`], and writes each at `<dir>/<name>`.
#[derive(Debug, Clone)]
pub struct TsSdk {
    module: String,
    dir: String,
    layout: SdkFileLayout,
    aliases: SdkTypeAliases,
}

impl TsSdk {
    /// A TypeScript SDK target with no module/output yet (set with [`TsSdk::module`] + [`TsSdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            dir: String::new(),
            layout: SdkFileLayout::compact(),
            aliases: SdkTypeAliases::default(),
        }
    }

    /// Set the module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The package
    /// name is derived from this — the single source of truth (CLAUDE.md rule 3), the same derivation
    /// `PySdk`/`GoSdk` use.
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the output directory for the generated SDK files (e.g. `"generated/sdk-ts"`).
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }

    /// Set the generated file layout.
    #[must_use]
    pub fn layout(mut self, layout: SdkFileLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Use the split layout for larger SDKs.
    #[must_use]
    pub fn split_files(self) -> Self {
        self.layout(SdkFileLayout::split().model_dir("models"))
    }

    /// Add compatibility type aliases to the generated SDK surface.
    #[must_use]
    pub fn aliases(mut self, aliases: SdkTypeAliases) -> Self {
        self.aliases = aliases;
        self
    }

    /// Expose `alias` as an additional type name for a schema id or generated schema name.
    #[must_use]
    pub fn type_alias(self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        let aliases = self.aliases.clone().type_alias(schema, alias);
        self.aliases(aliases)
    }
}

impl Default for TsSdk {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for TsSdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "TsSdk target has no module — call .module(\"example.com/acme/sdk\")"
                    .to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "TsSdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        // Derive the package from the module path via the SAME single source of truth GoSdk/PySdk use,
        // and generate via the existing deterministic TypeScript SDK generator — never a re-derivation,
        // never a fallback (CLAUDE.md rules 2 & 3). `ir.base_path` is the same single source of truth
        // the OpenAPI lowering reads (rule 3/4 — never re-derived).
        let package = sdk_package(&self.module)?;
        let files = crate::tssdk::generate_files_with_layout(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
        )?;
        write_sdk_files(out, &self.dir, files)?;
        Ok(())
    }

    /// The SDK output directory is the critical loop-safety anchor: the generated `*.ts` files form a
    /// TypeScript package inside the analyzed source tree, so without excluding this dir the source
    /// would re-ingest them and duplicate every schema (the contamination
    /// `crate::lifecycle::exclude_output_paths` prevents on the host path, T-05-02-03).
    fn output_anchors(&self) -> Vec<String> {
        if self.dir.is_empty() {
            Vec::new()
        } else {
            vec![self.dir.trim_end_matches('/').to_string()]
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// PostProcess
// ---------------------------------------------------------------------------------------------------

/// The "Code generated by gnr8" banner line prepended to every generated `.go` file.
const GENERATED_HEADER: &str = "// Code generated by gnr8. DO NOT EDIT.";

/// A post-processor that prepends a "Code generated by gnr8. DO NOT EDIT." line to every `.go`
/// artifact (non-`.go` files are skipped). A small, useful built-in demonstrating the post-process
/// seam; the line is idempotent (a file that already starts with it is left unchanged).
#[derive(Debug, Default, Clone)]
pub struct Header;

impl Header {
    /// The generated-code banner post-processor.
    #[must_use]
    pub fn generated() -> Self {
        Self
    }
}

impl PostProcess for Header {
    fn run(&self, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        // Collect the rewrites first (we can't mutate while iterating `files()`), then re-write each
        // through `Artifacts::write` so the set stays sorted (a rewrite of an existing path replaces
        // it in place). Only `.go` files get the header; the prepend is idempotent.
        let rewrites: Vec<(String, String)> = out
            .files()
            .iter()
            .filter(|a| is_go_file(&a.path))
            .filter(|a| !a.text.starts_with(GENERATED_HEADER))
            .map(|a| (a.path.clone(), format!("{GENERATED_HEADER}\n{}", a.text)))
            .collect();
        for (path, text) in rewrites {
            out.write(path, text);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------------

/// Whether a project-relative artifact `path` is a Go source file (its extension is `go`,
/// case-insensitively) — used to scope the generated-code header to `.go` files only.
fn is_go_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
}

/// Derive the generated SDK's Go package name from a module path — the LAST path segment, sanitized
/// to a valid Go package identifier.
///
/// A single deterministic transform the `GoSdk` target owns: keep ASCII letters/digits lower-cased,
/// drop every separator, trim a leading digit run so the identifier starts with a letter. NOT a
/// fallback — exactly one path; the only branch is input validation.
///
/// # Errors
///
/// Returns [`CoreError::Config`] if `module`'s last segment yields no valid Go identifier (no ASCII
/// letter to anchor it).
fn sdk_package(module: &str) -> Result<String, CoreError> {
    let last = module.rsplit('/').next().unwrap_or("");
    let kept: String = last
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let pkg = kept.trim_start_matches(|c: char| c.is_ascii_digit());
    if pkg.is_empty() {
        return Err(CoreError::Config {
            message: format!(
                "GoSdk module {module:?} has no last path segment that forms a valid Go package \
                 identifier (need at least one ASCII letter, e.g. \"example.com/acme/sdk\")"
            ),
        });
    }
    Ok(pkg.to_string())
}

fn write_sdk_files(
    out: &mut Artifacts,
    dir: &str,
    files: Vec<super::bundle::SdkFile>,
) -> Result<(), CoreError> {
    let dir = dir.trim_end_matches('/');
    for file in files {
        // File names are program-controlled, but reject anything that can traverse out of `dir`.
        super::bundle::safe_frame_name(&file.name)?;
        out.write(format!("{dir}/{}", file.name), file.contents);
    }
    Ok(())
}

fn collect_static_include(
    source_root: &Path,
    include: &str,
    out: &mut Vec<String>,
) -> Result<(), CoreError> {
    if let Some(prefix) = include.strip_suffix("/**") {
        validate_static_rel(prefix)?;
        collect_static_dir(source_root, Path::new(prefix), out)
    } else {
        validate_static_rel(include)?;
        out.push(include.replace('\\', "/"));
        Ok(())
    }
}

fn collect_static_dir(
    source_root: &Path,
    rel_dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), CoreError> {
    let dir = source_root.join(rel_dir);
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| CoreError::Io {
        message: format!("failed to read static dir {}: {err}", dir.display()),
    })? {
        let entry = entry.map_err(|err| CoreError::Io {
            message: format!(
                "failed to read static dir entry in {}: {err}",
                dir.display()
            ),
        })?;
        entries.push(entry.path());
    }
    entries.sort();

    for path in entries {
        let rel = path
            .strip_prefix(source_root)
            .map_err(|err| CoreError::Config {
                message: format!(
                    "static file {} is not under source root {}: {err}",
                    path.display(),
                    source_root.display()
                ),
            })?;
        let rel_str = rel_to_slash_string(rel)?;
        let meta = std::fs::symlink_metadata(&path).map_err(|err| CoreError::Io {
            message: format!("failed to inspect static file {}: {err}", path.display()),
        })?;
        if meta.is_dir() {
            collect_static_dir(source_root, rel, out)?;
        } else if meta.is_file() {
            validate_static_rel(&rel_str)?;
            out.push(rel_str);
        }
    }
    Ok(())
}

fn validate_static_rel(path: &str) -> Result<(), CoreError> {
    super::bundle::safe_frame_name(path).map_err(|err| CoreError::Config {
        message: format!("invalid StaticFiles include {path:?}: {err}"),
    })
}

fn rel_to_slash_string(path: &Path) -> Result<String, CoreError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(CoreError::Config {
                message: format!("invalid static file path {}", path.display()),
            });
        };
        let Some(part) = part.to_str() else {
            return Err(CoreError::Config {
                message: format!("static file path is not UTF-8: {}", path.display()),
            });
        };
        parts.push(part);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        sdk_package, ApplySecurity, Cx, FastApi, Flask, GoSdk, Header, NestJs, OpenApi31,
        OpenApi31Json, PostProcess, PySdk, SetBasePath, SetOperationSuccessResponse,
        SetSchemaFieldType, SetTitle, Source, StaticFiles, Target, Transform, TsSdk,
    };
    use crate::graph::{ApiGraph, Field, Operation, Prim, Schema, SourceSpan, Type};
    use crate::sdk::Artifacts;

    fn cx() -> Cx {
        Cx::new(std::env::temp_dir())
    }

    fn span() -> SourceSpan {
        SourceSpan {
            file: "handlers.go".to_string(),
            start_line: 10,
            end_line: 20,
        }
    }

    fn temp_project(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("gnr8-static-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn transforms_set_graph_metadata() {
        let mut ir = ApiGraph::default();
        SetBasePath::new("/books").apply(&mut ir, &cx()).unwrap();
        SetTitle::new("Bookstore API")
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("ApiKeyAuth", "X-API-Key")
            .apply(&mut ir, &cx())
            .unwrap();
        assert_eq!(ir.base_path, "/books");
        assert_eq!(ir.title, "Bookstore API");
        assert_eq!(ir.security.len(), 1);
        let s = &ir.security[0];
        assert_eq!(s.id, "ApiKeyAuth");
        assert_eq!(s.kind, "apiKey");
        assert_eq!(s.location, "header");
        assert_eq!(s.name, "X-API-Key");
    }

    #[test]
    fn transform_sets_operation_success_response_by_route() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.CreateBookResponse".to_string(),
                name: "CreateBookResponse".to_string(),
                body: Type::Object(vec![]),
                provenance: span(),
            }],
            operations: vec![Operation {
                id: "createBook".to_string(),
                method: "POST".to_string(),
                path: "/books".to_string(),
                handler: "createBook".to_string(),
                group: None,
                params: vec![],
                request_body: None,
                responses: vec![],
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        SetOperationSuccessResponse::for_route("post", "/books", "CreateBookResponse")
            .status(201)
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].responses.len(), 1);
        assert_eq!(ir.operations[0].responses[0].status, 201);
        assert_eq!(
            ir.operations[0].responses[0]
                .body
                .as_ref()
                .map(|body| body.ref_id.as_str()),
            Some("app.CreateBookResponse")
        );
    }

    #[test]
    fn transform_sets_schema_field_type() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.DocumentBody".to_string(),
                name: "DocumentBody".to_string(),
                body: Type::Object(vec![Field {
                    json_name: "blocks".to_string(),
                    required: true,
                    optional: false,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                }]),
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        SetSchemaFieldType::array_of_free_form_objects("DocumentBody", "blocks")
            .apply(&mut ir, &cx())
            .unwrap();

        let Type::Object(fields) = &ir.schemas[0].body else {
            panic!("expected object schema");
        };
        assert!(matches!(
            fields[0].schema,
            Type::Array(ref inner) if matches!(**inner, Type::Any {})
        ));
    }

    #[test]
    fn sdk_package_derives_last_segment() {
        assert_eq!(sdk_package("example.com/bookstore/sdk").unwrap(), "sdk");
        assert_eq!(sdk_package("example.com/acme/gnr8sdk").unwrap(), "gnr8sdk");
        assert!(sdk_package("example.com/123").is_err());
    }

    #[test]
    fn targets_error_when_unconfigured() {
        let ir = ApiGraph::default();
        let mut out = Artifacts::new();
        assert!(matches!(
            OpenApi31::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            OpenApi31Json::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            GoSdk::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            GoSdk::new()
                .module("x.com/sdk")
                .generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            PySdk::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            PySdk::new()
                .module("x.com/sdk")
                .generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            StaticFiles::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            StaticFiles::new()
                .from("static")
                .generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
    }

    #[test]
    fn static_files_target_copies_exact_files_and_recursive_dirs() {
        let root = temp_project("copies");
        std::fs::create_dir_all(root.join("static/runtime/nested")).unwrap();
        std::fs::write(root.join("static/runtime/__init__.py"), "ROOT\n").unwrap();
        std::fs::write(root.join("static/runtime/nested/tool.py"), "TOOL\n").unwrap();
        std::fs::write(root.join("static/README.md"), "README\n").unwrap();

        let mut out = Artifacts::new();
        StaticFiles::new()
            .from("static")
            .to("pkg")
            .include(["runtime/**", "README.md"])
            .generate(&ApiGraph::default(), &mut out, &Cx::new(&root))
            .unwrap();

        let files: Vec<_> = out
            .files()
            .iter()
            .map(|file| (file.path.as_str(), file.text.as_str()))
            .collect();
        assert_eq!(
            files,
            vec![
                ("pkg/README.md", "README\n"),
                ("pkg/runtime/__init__.py", "ROOT\n"),
                ("pkg/runtime/nested/tool.py", "TOOL\n"),
            ]
        );
        assert_eq!(
            StaticFiles::new()
                .from("static")
                .to("pkg")
                .include(["runtime/**", "README.md"])
                .output_anchors(),
            vec!["pkg/README.md".to_string(), "pkg/runtime".to_string()]
        );
    }

    #[test]
    fn static_files_target_rejects_unsafe_includes() {
        let mut out = Artifacts::new();
        let err = StaticFiles::new()
            .from("static")
            .to("pkg")
            .include(["../secret.py"])
            .generate(&ApiGraph::default(), &mut out, &cx())
            .unwrap_err();
        assert!(err.to_string().contains("invalid StaticFiles include"));
    }

    #[test]
    fn gosdk_target_emits_go_mod_under_output_dir() {
        let ir = ApiGraph::default();
        let target = GoSdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-go");

        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();

        let go_mod = out
            .files()
            .iter()
            .find(|file| file.path == "generated/sdk-go/go.mod")
            .expect("GoSdk must emit go.mod so pruned SDK dirs remain buildable");
        assert_eq!(go_mod.text, "module example.com/bookstore/sdk\n\ngo 1.23\n");
    }

    #[test]
    fn pysdk_target_writes_under_the_output_dir_and_is_deterministic() {
        let ir = ApiGraph::default();
        let target = PySdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-py/");

        // A configured run writes one Artifact per generated Python file, all anchored under the
        // (slash-trimmed) output dir.
        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();
        assert!(
            !out.files().is_empty(),
            "a configured PySdk run must emit at least one Artifact"
        );
        for artifact in out.files() {
            assert!(
                artifact.path.starts_with("generated/sdk-py/"),
                "every Artifact path must be under the output dir, got {:?}",
                artifact.path
            );
        }

        // The trimmed output dir is the loop-safety anchor (so the pipeline never re-ingests the
        // generated *.py); an unconfigured target anchors nothing.
        assert_eq!(
            target.output_anchors(),
            vec!["generated/sdk-py".to_string()]
        );
        assert!(PySdk::new().output_anchors().is_empty());

        // Two fresh runs over the same IR yield byte-identical Artifacts (T-03-02-05).
        let mut out2 = Artifacts::new();
        target.generate(&ir, &mut out2, &cx()).unwrap();
        let first: Vec<(&str, &str)> = out
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        let second: Vec<(&str, &str)> = out2
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        assert_eq!(first, second, "two PySdk runs must be byte-identical");
    }

    #[test]
    fn tssdk_target_errors_when_unconfigured() {
        // An unconfigured TsSdk (no module / no dir) is a typed Config error, not a panic — exactly
        // like PySdk/GoSdk; only the proper noun differs.
        let ir = ApiGraph::default();
        let mut out = Artifacts::new();
        assert!(
            matches!(
                TsSdk::new().generate(&ir, &mut out, &cx()),
                Err(crate::CoreError::Config { .. })
            ),
            "TsSdk with no module must be a Config error"
        );
        assert!(
            matches!(
                TsSdk::new()
                    .module("x.com/sdk")
                    .generate(&ir, &mut out, &cx()),
                Err(crate::CoreError::Config { .. })
            ),
            "TsSdk with a module but no output dir must be a Config error"
        );
    }

    #[test]
    fn tssdk_target_writes_under_the_output_dir_and_is_deterministic() {
        let ir = ApiGraph::default();
        let target = TsSdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-ts/");

        // A configured run writes one Artifact per generated TypeScript file, all anchored under the
        // (slash-trimmed) output dir.
        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();
        assert!(
            !out.files().is_empty(),
            "a configured TsSdk run must emit at least one Artifact"
        );
        for artifact in out.files() {
            assert!(
                artifact.path.starts_with("generated/sdk-ts/"),
                "every Artifact path must be under the output dir, got {:?}",
                artifact.path
            );
        }

        // The trimmed output dir is the loop-safety anchor (so the pipeline never re-ingests the
        // generated *.ts); an unconfigured target anchors nothing.
        assert_eq!(
            target.output_anchors(),
            vec!["generated/sdk-ts".to_string()]
        );
        assert!(TsSdk::new().output_anchors().is_empty());

        // Two fresh runs over the same IR yield byte-identical Artifacts (T-05-02-03 determinism).
        let mut out2 = Artifacts::new();
        target.generate(&ir, &mut out2, &cx()).unwrap();
        let first: Vec<(&str, &str)> = out
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        let second: Vec<(&str, &str)> = out2
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        assert_eq!(first, second, "two TsSdk runs must be byte-identical");
    }

    #[test]
    fn python_sources_error_when_unconfigured() {
        // Both Python sources reject zero inputs and many inputs with a typed Config error, exactly
        // like GoGin — the single-input guard is identical; only the proper noun differs.
        let cx = cx();
        assert!(
            matches!(
                FastApi::new().load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "FastApi with no inputs must be a Config error"
        );
        assert!(
            matches!(
                FastApi::new().inputs(["a", "b"]).load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "FastApi with many inputs must be a Config error"
        );
        assert!(
            matches!(Flask::new().load(&cx), Err(crate::CoreError::Config { .. })),
            "Flask with no inputs must be a Config error"
        );
        assert!(
            matches!(
                Flask::new().inputs(["a", "b"]).load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "Flask with many inputs must be a Config error"
        );
    }

    #[test]
    fn nestjs_source_errors_when_unconfigured() {
        // The TypeScript source rejects zero inputs and many inputs with a typed Config error,
        // exactly like the Python/Go sources — the single-input guard is identical; only the proper
        // noun differs. It calls the SAME build_graph (language detected from the target, rule 3/4).
        let cx = cx();
        assert!(
            matches!(
                NestJs::new().load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "NestJs with no inputs must be a Config error"
        );
        assert!(
            matches!(
                NestJs::new().inputs(["a", "b"]).load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "NestJs with many inputs must be a Config error"
        );
    }

    #[test]
    fn header_prepends_to_go_files_only_and_is_idempotent() {
        let mut out = Artifacts::new();
        out.write("openapi.yaml", "openapi: 3.1.0\n");
        out.write("sdk/client.go", "package sdk\n");
        Header::generated().run(&mut out, &cx()).unwrap();
        let go = out
            .files()
            .iter()
            .find(|f| f.path == "sdk/client.go")
            .unwrap();
        assert!(
            go.text
                .starts_with("// Code generated by gnr8. DO NOT EDIT.\n"),
            "go file gets the header: {:?}",
            go.text
        );
        let yaml = out
            .files()
            .iter()
            .find(|f| f.path == "openapi.yaml")
            .unwrap();
        assert!(
            !yaml.text.contains("Code generated"),
            "non-go file is untouched"
        );
        // Idempotent: running twice does not double the header.
        Header::generated().run(&mut out, &cx()).unwrap();
        let go2 = out
            .files()
            .iter()
            .find(|f| f.path == "sdk/client.go")
            .unwrap();
        assert_eq!(go2.text.matches("Code generated").count(), 1);
    }
}
