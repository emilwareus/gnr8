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
use crate::analyze::facts::{Constraints, Extension, LiteralValue};
use crate::graph::{ApiGraph, Response, Schema, SchemaRef, SecurityScheme, Type};
use crate::lower::model::{OpenApiDoc, SchemaObject};
use crate::sdk::docs::{write_sdk_docs, SdkDocs};
use crate::sdk::emit_common::quoted_string_literal;
use crate::sdk::go::{
    GoExecuteCompatibility, GoQuerySetterArgumentPolicy, GoRequestBuilderAliases,
    GoRequestBuilderScope, GoSdkOptions, QueryTimeFormat, RequiredPointerConstructorPolicy,
};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::model::SdkModel;
use crate::sdk::model_style::PyModelStyle;
use crate::sdk::profile::SdkProfile;
use crate::sdk::surface::SdkTypeAliases;
use crate::sdk::typescript::{
    TsBarrelExports, TsCompatibility, TsModelPropertyPolicy, TsNullablePolicy, TsResponsePolicy,
    TsSdkOptions,
};
use crate::CoreError;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    route_package_patterns: Vec<String>,
    schema_package_patterns: Vec<String>,
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
        let patterns: Vec<String> = patterns.into_iter().map(Into::into).collect();
        self.route_package_patterns.clone_from(&patterns);
        self.schema_package_patterns = patterns;
        self
    }

    /// Scope Go route recognition and handler analysis to the given `go/packages` patterns.
    #[must_use]
    pub fn route_packages<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.route_package_patterns = patterns.into_iter().map(Into::into).collect();
        self
    }

    /// Scope Go schema extraction to the given `go/packages` patterns.
    #[must_use]
    pub fn schema_packages<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.schema_package_patterns = patterns.into_iter().map(Into::into).collect();
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
        let cache_key = go_gin_cache_key(
            &resolved,
            &self.route_package_patterns,
            &self.schema_package_patterns,
            cx,
        );
        if let Some(cached) = load_go_gin_cache(cx, &cache_key) {
            return Ok(cached);
        }
        let input_arg = resolved.to_string_lossy();
        let graph = crate::analyze::build_go_graph_with_package_scopes(
            &input_arg,
            &self.route_package_patterns,
            &self.schema_package_patterns,
        )?;
        save_go_gin_cache(cx, &cache_key, &graph);
        Ok(graph)
    }

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        single_input_cache_root(&cx.project_root, &self.inputs)
    }
}

fn single_input_cache_root(
    project_root: &Path,
    inputs: &[String],
) -> Option<Vec<std::path::PathBuf>> {
    let [single] = inputs else {
        return None;
    };
    Some(vec![project_root.join(single)])
}

fn go_gin_cache_key(
    input: &Path,
    route_package_patterns: &[String],
    schema_package_patterns: &[String],
    cx: &Cx,
) -> String {
    let mut files = Vec::new();
    collect_cache_input_files(input, &mut files);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gnr8-go-gin-source-cache-v3\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\n");
    hasher.update(b"routes\n");
    for pattern in route_package_patterns {
        hasher.update(pattern.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"schemas\n");
    for pattern in schema_package_patterns {
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

/// An OpenAPI/Swagger artifact source.
///
/// Accepts JSON or YAML Swagger 2.0, OpenAPI 3.0, and OpenAPI 3.1 documents, then normalizes paths,
/// operations, parameters, request/response schemas, and named components into the shared
/// [`ApiGraph`]. Output generation remains owned by normal targets such as [`OpenApi31`],
/// [`TsSdk`], and [`GoSdk`].
#[derive(Debug, Default, Clone)]
pub struct OpenApi {
    input: String,
}

impl OpenApi {
    /// An OpenAPI source with no input yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the project-relative OpenAPI/Swagger JSON or YAML input file.
    #[must_use]
    pub fn input(mut self, input: impl Into<String>) -> Self {
        self.input = input.into();
        self
    }
}

impl Source for OpenApi {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        if self.input.is_empty() {
            return Err(CoreError::Config {
                message: "OpenApi source has no input — call .input(\"openapi.yaml\")".to_string(),
            });
        }
        crate::sdk::openapi_source::load_openapi(&cx.project_root, &self.input)
    }

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        if self.input.is_empty() {
            None
        } else {
            Some(vec![cx.project_root.join(&self.input)])
        }
    }
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

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        single_input_cache_root(&cx.project_root, &self.inputs)
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

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        single_input_cache_root(&cx.project_root, &self.inputs)
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

    fn cache_input_roots(&self, cx: &Cx) -> Option<Vec<std::path::PathBuf>> {
        single_input_cache_root(&cx.project_root, &self.inputs)
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
        validate_base_path(&self.base_path)?;
        ir.base_path.clone_from(&self.base_path);
        Ok(())
    }
}

fn validate_base_path(base_path: &str) -> Result<(), CoreError> {
    if base_path.is_empty() || base_path == "/" {
        return Ok(());
    }
    if !base_path.starts_with('/') {
        return Err(CoreError::Config {
            message: format!("base path {base_path:?} must be empty, '/', or start with '/'"),
        });
    }
    if base_path.chars().any(|ch| matches!(ch, '?' | '#' | '\\'))
        || base_path.split('/').any(|part| part == "..")
    {
        return Err(CoreError::Config {
            message: format!(
                "base path {base_path:?} must be a clean path prefix without query, fragment, backslash, or '..'"
            ),
        });
    }
    Ok(())
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
        if !(200..300).contains(&self.status) {
            return Err(CoreError::Config {
                message: format!(
                    "success response status {} is not a 2xx status",
                    self.status
                ),
            });
        }

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
            .retain(|response| !(200..300).contains(&response.status));
        op.responses.push(Response {
            status: self.status,
            body: Some(SchemaRef { ref_id: schema_id }),
            body_kind: "json".to_string(),
            content_type: None,
            content_types: vec!["application/json".to_string()],
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

/// Graph-level API fact overrides for source patterns that need explicit correction.
///
/// These overrides mutate the neutral IR before targets render, so OpenAPI and every SDK target read
/// the same corrected API facts.
#[derive(Debug, Clone, Default)]
pub struct ApiOverrides {
    field_presence: Vec<FieldPresenceOverride>,
    query_params: Vec<QueryParamOverride>,
    request_bodies: Vec<RequestBodyOverride>,
    responses: Vec<ResponseOverride>,
    default_responses: Vec<DefaultResponseOverride>,
}

#[derive(Debug, Clone)]
struct FieldPresenceOverride {
    schema: String,
    field: String,
    required: bool,
}

#[derive(Debug, Clone)]
struct QueryParamOverride {
    matcher: OperationMatcher,
    param: QueryParam,
}

#[derive(Debug, Clone)]
struct RequestBodyOverride {
    matcher: OperationMatcher,
    required: Option<bool>,
}

#[derive(Debug, Clone)]
struct ResponseOverride {
    matcher: OperationMatcher,
    status: u16,
    body_kind: String,
    content_type: Option<String>,
    content_types: Vec<String>,
    schema_ref: Option<String>,
}

#[derive(Debug, Clone)]
struct DefaultResponseOverride {
    status: u16,
    body_kind: String,
    content_type: Option<String>,
    content_types: Vec<String>,
    schema_ref: Option<String>,
}

/// Query parameter override builder.
#[derive(Debug, Clone)]
pub struct QueryParam {
    name: String,
    schema: Type,
    required: bool,
    default: Option<LiteralValue>,
}

impl QueryParam {
    /// Create a string, optional query parameter override.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: Type::Primitive(crate::graph::Prim::String),
            required: false,
            default: None,
        }
    }

    /// Set the query parameter type to string.
    #[must_use]
    pub fn string(mut self) -> Self {
        self.schema = Type::Primitive(crate::graph::Prim::String);
        self
    }

    /// Set the query parameter type to integer.
    #[must_use]
    pub fn integer(mut self) -> Self {
        self.schema = Type::Primitive(crate::graph::Prim::Int {
            bits: 64,
            signed: true,
        });
        self
    }

    /// Set the query parameter type to RFC-3339 date-time.
    #[must_use]
    pub fn date_time(mut self) -> Self {
        self.schema = Type::WellKnown(crate::graph::WellKnown::DateTime);
        self
    }

    /// Set the query parameter type to an RFC-3339 full-date (`OpenAPI` `format: date`).
    #[must_use]
    pub fn date(mut self) -> Self {
        self.schema = Type::WellKnown(crate::graph::WellKnown::Date);
        self
    }

    /// Mark the query parameter required.
    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Mark the query parameter optional.
    #[must_use]
    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Set a numeric default.
    #[must_use]
    pub fn default_number(mut self, value: impl Into<String>) -> Self {
        self.default = Some(LiteralValue::Number(value.into()));
        self
    }

    /// Set a string default.
    #[must_use]
    pub fn default_string(mut self, value: impl Into<String>) -> Self {
        self.default = Some(LiteralValue::String(value.into()));
        self
    }
}

impl ApiOverrides {
    /// Create an empty override set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Force one schema field into the OpenAPI/schema required set.
    #[must_use]
    pub fn force_required(mut self, schema: impl Into<String>, field: impl Into<String>) -> Self {
        self.field_presence.push(FieldPresenceOverride {
            schema: schema.into(),
            field: field.into(),
            required: true,
        });
        self
    }

    /// Force one schema field out of the OpenAPI/schema required set.
    #[must_use]
    pub fn force_optional(mut self, schema: impl Into<String>, field: impl Into<String>) -> Self {
        self.field_presence.push(FieldPresenceOverride {
            schema: schema.into(),
            field: field.into(),
            required: false,
        });
        self
    }

    /// Add or replace a query parameter on an operation matched by method and graph path.
    #[must_use]
    pub fn query_param(
        mut self,
        method: impl Into<String>,
        path: impl Into<String>,
        param: QueryParam,
    ) -> Self {
        self.query_params.push(QueryParamOverride {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            param,
        });
        self
    }

    /// Target a request body on an operation matched by method and graph path.
    #[must_use]
    pub fn request_body(mut self, method: impl Into<String>, path: impl Into<String>) -> Self {
        self.request_bodies.push(RequestBodyOverride {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            required: None,
        });
        self
    }

    /// Mark the most recently configured request body optional.
    #[must_use]
    pub fn optional(mut self) -> Self {
        if let Some(body) = self.request_bodies.last_mut() {
            body.required = Some(false);
        }
        self
    }

    /// Mark one response as binary/file content.
    #[must_use]
    pub fn binary_response(
        mut self,
        method: impl Into<String>,
        path: impl Into<String>,
        status: u16,
    ) -> Self {
        self.responses.push(ResponseOverride {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            status,
            body_kind: "binary".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            content_types: vec!["application/octet-stream".to_string()],
            schema_ref: None,
        });
        self
    }

    /// Set or replace one JSON response body on an operation matched by method and graph path.
    #[must_use]
    pub fn json_response(
        mut self,
        method: impl Into<String>,
        path: impl Into<String>,
        status: u16,
        schema: impl Into<String>,
    ) -> Self {
        self.responses.push(ResponseOverride {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            status,
            body_kind: "json".to_string(),
            content_type: None,
            content_types: vec!["application/json".to_string()],
            schema_ref: Some(schema.into()),
        });
        self
    }

    /// Attach a JSON error response model to every operation that does not already declare `status`.
    #[must_use]
    pub fn default_error_response(mut self, status: u16, schema: impl Into<String>) -> Self {
        self.default_responses.push(DefaultResponseOverride {
            status,
            body_kind: "json".to_string(),
            content_type: None,
            content_types: vec!["application/json".to_string()],
            schema_ref: Some(schema.into()),
        });
        self
    }

    /// Mark one response as server-sent events.
    #[must_use]
    pub fn sse_response(mut self, method: impl Into<String>, path: impl Into<String>) -> Self {
        self.responses.push(ResponseOverride {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            status: 200,
            body_kind: "sse".to_string(),
            content_type: Some("text/event-stream".to_string()),
            content_types: vec!["text/event-stream".to_string()],
            schema_ref: None,
        });
        self
    }

    /// Attach an existing schema as the event envelope for the most recently configured SSE response.
    #[must_use]
    pub fn event_schema(mut self, schema: impl Into<String>) -> Self {
        if let Some(response) = self.responses.last_mut() {
            if response.body_kind == "sse" {
                response.schema_ref = Some(schema.into());
            }
        }
        self
    }
}

impl Transform for ApiOverrides {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        for override_ in &self.field_presence {
            apply_field_presence_override(
                ir,
                &override_.schema,
                &override_.field,
                override_.required,
            )?;
        }
        for override_ in &self.query_params {
            apply_query_param_override(ir, &override_.matcher, &override_.param)?;
        }
        for override_ in &self.request_bodies {
            apply_request_body_override(ir, &override_.matcher, override_.required)?;
        }
        for override_ in &self.responses {
            apply_response_override(ir, override_)?;
        }
        for override_ in &self.default_responses {
            apply_default_response_override(ir, override_)?;
        }
        Ok(())
    }
}

fn apply_field_presence_override(
    ir: &mut ApiGraph,
    schema_match: &str,
    field_name: &str,
    required: bool,
) -> Result<(), CoreError> {
    let matches: Vec<usize> = ir
        .schemas
        .iter()
        .enumerate()
        .filter_map(|(index, schema)| {
            (schema.id == schema_match || schema.name == schema_match).then_some(index)
        })
        .collect();
    let schema_index = match matches.as_slice() {
        [single] => *single,
        [] => {
            return Err(CoreError::Config {
                message: format!(
                    "field presence override schema {schema_match:?} does not match any graph schema id or name"
                ),
            });
        }
        many => {
            return Err(CoreError::Config {
                message: format!(
                    "field presence override schema {schema_match:?} matches {} schemas; use the full schema id",
                    many.len()
                ),
            });
        }
    };

    let schema = &mut ir.schemas[schema_index];
    let Type::Object(fields) = &mut schema.body else {
        return Err(CoreError::Config {
            message: format!(
                "field presence override schema {schema_match:?} is not an object schema"
            ),
        });
    };

    let field = fields
        .iter_mut()
        .find(|field| field.json_name == field_name)
        .ok_or_else(|| CoreError::Config {
            message: format!(
                "field presence override did not find field {field_name:?} on schema {schema_match:?}"
            ),
        })?;
    field.required = required;
    Ok(())
}

fn apply_query_param_override(
    ir: &mut ApiGraph,
    matcher: &OperationMatcher,
    param: &QueryParam,
) -> Result<(), CoreError> {
    let op_index = find_operation_index(ir, matcher, "query parameter override")?;
    let op_method = ir.operations[op_index].method.clone();
    let op_path = ir.operations[op_index].path.clone();
    let op = &mut ir.operations[op_index];
    op.params
        .retain(|existing| !(existing.location == "query" && existing.name == param.name));
    op.params.push(crate::graph::Param {
        name: param.name.clone(),
        location: "query".to_string(),
        required: param.required,
        schema: param.schema.clone(),
        default: param.default.clone(),
        provenance: op.provenance.clone(),
    });
    op.params.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.location.cmp(&b.location))
    });
    remove_untyped_query_diagnostics(ir, &op_method, &op_path, &param.name);
    Ok(())
}

fn apply_request_body_override(
    ir: &mut ApiGraph,
    matcher: &OperationMatcher,
    required: Option<bool>,
) -> Result<(), CoreError> {
    let op_index = find_operation_index(ir, matcher, "request body override")?;
    let op = &mut ir.operations[op_index];
    if op.request_body.is_none() {
        return Err(CoreError::Config {
            message: format!(
                "request body override matched operation '{}' with no request body",
                op.id
            ),
        });
    }
    if let Some(required) = required {
        op.request_body_required = required;
    }
    Ok(())
}

fn apply_response_override(
    ir: &mut ApiGraph,
    override_: &ResponseOverride,
) -> Result<(), CoreError> {
    let op_index = find_operation_index(ir, &override_.matcher, "response override")?;
    let body = response_override_body(ir, override_.schema_ref.as_deref())?;
    let op_span = ir.operations[op_index].provenance.clone();
    let op = &mut ir.operations[op_index];
    op.responses
        .retain(|response| response.status != override_.status);
    op.responses.push(Response {
        status: override_.status,
        body,
        body_kind: override_.body_kind.clone(),
        content_type: override_.content_type.clone(),
        content_types: response_override_content_types(
            override_.content_type.as_deref(),
            &override_.content_types,
        ),
    });
    op.responses.sort_by_key(|response| response.status);
    if override_.body_kind == "binary"
        && override_
            .content_types
            .iter()
            .any(|content_type| content_type == "application/octet-stream")
    {
        remove_binary_octet_stream_default_diagnostics(ir, &op_span);
    }
    Ok(())
}

fn apply_default_response_override(
    ir: &mut ApiGraph,
    override_: &DefaultResponseOverride,
) -> Result<(), CoreError> {
    if (200..300).contains(&override_.status) {
        return Err(CoreError::Config {
            message: format!(
                "default error response status {} is a 2xx status",
                override_.status
            ),
        });
    }
    let body_ref = override_
        .schema_ref
        .as_deref()
        .map(|schema| resolve_schema_ref(ir, schema))
        .transpose()?;
    let content_types = response_override_content_types(
        override_.content_type.as_deref(),
        &override_.content_types,
    );
    for op in &mut ir.operations {
        if op
            .responses
            .iter()
            .any(|response| response.status == override_.status)
        {
            continue;
        }
        op.responses.push(Response {
            status: override_.status,
            body: body_ref.as_ref().map(|ref_id| SchemaRef {
                ref_id: ref_id.clone(),
            }),
            body_kind: override_.body_kind.clone(),
            content_type: override_.content_type.clone(),
            content_types: content_types.clone(),
        });
        op.responses.sort_by_key(|response| response.status);
    }
    Ok(())
}

fn response_override_body(
    ir: &ApiGraph,
    schema_ref: Option<&str>,
) -> Result<Option<SchemaRef>, CoreError> {
    schema_ref
        .map(|schema| {
            Ok(SchemaRef {
                ref_id: resolve_schema_ref(ir, schema)?,
            })
        })
        .transpose()
}

fn response_override_content_types(
    content_type: Option<&str>,
    content_types: &[String],
) -> Vec<String> {
    if content_types.is_empty() {
        content_type.map(str::to_string).into_iter().collect()
    } else {
        content_types.to_vec()
    }
}

fn resolve_schema_ref(ir: &ApiGraph, schema: &str) -> Result<String, CoreError> {
    if let Some(candidate) = ir.schemas.iter().find(|candidate| candidate.id == schema) {
        return Ok(candidate.id.clone());
    }

    let matches: Vec<&Schema> = ir
        .schemas
        .iter()
        .filter(|candidate| candidate.name == schema)
        .collect();
    match matches.as_slice() {
        [single] => Ok(single.id.clone()),
        [] => Err(CoreError::Config {
            message: format!("response override schema '{schema}' did not match any schema"),
        }),
        many => Err(CoreError::Config {
            message: format!(
                "response override schema '{schema}' matches {} schemas; use the full schema id",
                many.len()
            ),
        }),
    }
}

fn find_operation_index(
    ir: &ApiGraph,
    matcher: &OperationMatcher,
    label: &str,
) -> Result<usize, CoreError> {
    let matched_indices: Vec<usize> = ir
        .operations
        .iter()
        .enumerate()
        .filter_map(|(index, op)| {
            let is_match = match matcher {
                OperationMatcher::Id(id) => op.id == *id,
                OperationMatcher::Route { method, path } => {
                    op.method == *method && op.path == *path
                }
            };
            is_match.then_some(index)
        })
        .collect();
    match matched_indices.as_slice() {
        [single] => Ok(*single),
        [] => Err(CoreError::Config {
            message: format!("{label} did not match any operation: {matcher:?}"),
        }),
        many => Err(CoreError::Config {
            message: format!("{label} matched {} operations: {matcher:?}", many.len()),
        }),
    }
}

fn remove_untyped_query_diagnostics(ir: &mut ApiGraph, method: &str, path: &str, param_name: &str) {
    let prefix = format!("untyped query param '{param_name}' on {method} {path}:");
    ir.diagnostics
        .retain(|diagnostic| !diagnostic.message.starts_with(&prefix));
}

fn remove_binary_octet_stream_default_diagnostics(
    ir: &mut ApiGraph,
    op_span: &crate::graph::SourceSpan,
) {
    ir.diagnostics.retain(|diagnostic| {
        let is_same_operation = diagnostic.file == op_span.file
            && diagnostic.line >= op_span.start_line
            && diagnostic.line <= op_span.end_line;
        let is_resolved_binary_default = diagnostic
            .message
            .contains("unsupported binary response pattern")
            && diagnostic
                .message
                .contains("defaulting to application/octet-stream");
        !(is_same_operation && is_resolved_binary_default)
    });
}

/// Enum ordering policy for generated OpenAPI/SDK surfaces.
#[derive(Debug, Clone)]
pub enum EnumOrder {
    /// Lexical ordering (the default graph normalization behavior).
    Lexical,
    /// Restore source declaration order when the source sidecar provided it.
    Source,
    /// Apply explicit overrides. Targets are schema id/name or `Schema.field` for inline enum fields.
    Explicit(Vec<(String, Vec<String>)>),
}

/// Apply enum ordering controls to the graph before targets render it.
#[derive(Debug, Clone)]
pub struct SetEnumOrder {
    order: EnumOrder,
}

impl SetEnumOrder {
    /// Create an enum-order transform.
    #[must_use]
    pub fn new(order: EnumOrder) -> Self {
        Self { order }
    }

    /// Restore source declaration order for named enums where available.
    #[must_use]
    pub fn source() -> Self {
        Self::new(EnumOrder::Source)
    }

    /// Sort every enum lexically.
    #[must_use]
    pub fn lexical() -> Self {
        Self::new(EnumOrder::Lexical)
    }

    /// Apply one explicit override.
    #[must_use]
    pub fn explicit<I, S>(target: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(EnumOrder::Explicit(vec![(
            target.into(),
            values.into_iter().map(Into::into).collect(),
        )]))
    }
}

impl Transform for SetEnumOrder {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        match &self.order {
            EnumOrder::Lexical => {
                for schema in &mut ir.schemas {
                    sort_enums_in_type(&mut schema.body);
                }
            }
            EnumOrder::Source => {
                for schema in &mut ir.schemas {
                    if let Type::Enum(members) = &mut schema.body {
                        if !schema.enum_source_order.is_empty() {
                            ensure_same_enum_members(
                                &schema.name,
                                members,
                                &schema.enum_source_order,
                            )?;
                            members.clone_from(&schema.enum_source_order);
                        }
                    }
                }
            }
            EnumOrder::Explicit(overrides) => {
                for (target, values) in overrides {
                    apply_explicit_enum_order(ir, target, values)?;
                }
            }
        }
        Ok(())
    }
}

fn sort_enums_in_type(ty: &mut Type) {
    match ty {
        Type::Enum(members) => members.sort(),
        Type::Object(fields) => {
            for field in fields {
                sort_enums_in_type(&mut field.schema);
            }
        }
        Type::Array(inner) => sort_enums_in_type(inner),
        Type::Map { key, value } => {
            sort_enums_in_type(key);
            sort_enums_in_type(value);
        }
        Type::Union(variants) => {
            for variant in variants {
                sort_enums_in_type(variant);
            }
        }
        Type::Primitive(_) | Type::WellKnown(_) | Type::Named(_) | Type::Any {} => {}
    }
}

fn apply_explicit_enum_order(
    ir: &mut ApiGraph,
    target: &str,
    values: &[String],
) -> Result<(), CoreError> {
    if let Some((schema_name, field_name)) = target.split_once('.') {
        let schema = ir
            .schemas
            .iter_mut()
            .find(|schema| schema.id == schema_name || schema.name == schema_name)
            .ok_or_else(|| CoreError::Config {
                message: format!("enum order override references unknown schema {schema_name:?}"),
            })?;
        let Type::Object(fields) = &mut schema.body else {
            return Err(CoreError::Config {
                message: format!("enum order override target {schema_name:?} is not an object"),
            });
        };
        let field = fields
            .iter_mut()
            .find(|field| field.json_name == field_name)
            .ok_or_else(|| CoreError::Config {
                message: format!("enum order override references unknown field {target:?}"),
            })?;
        let Type::Enum(members) = &mut field.schema else {
            return Err(CoreError::Config {
                message: format!("enum order override target {target:?} is not an inline enum"),
            });
        };
        ensure_same_enum_members(target, members, values)?;
        *members = values.to_vec();
        return Ok(());
    }

    let schema = ir
        .schemas
        .iter_mut()
        .find(|schema| schema.id == target || schema.name == target)
        .ok_or_else(|| CoreError::Config {
            message: format!("enum order override references unknown schema {target:?}"),
        })?;
    let Type::Enum(members) = &mut schema.body else {
        return Err(CoreError::Config {
            message: format!("enum order override target {target:?} is not a named enum"),
        });
    };
    ensure_same_enum_members(target, members, values)?;
    *members = values.to_vec();
    Ok(())
}

fn ensure_same_enum_members(
    target: &str,
    existing: &[String],
    requested: &[String],
) -> Result<(), CoreError> {
    let mut existing = existing.to_vec();
    let mut requested = requested.to_vec();
    existing.sort();
    requested.sort();
    if existing == requested {
        return Ok(());
    }
    Err(CoreError::Config {
        message: format!(
            "enum order override for {target:?} must contain exactly the existing enum members"
        ),
    })
}

/// Push a security scheme onto [`ApiGraph::security`] — the single source of truth for the generated
/// `security` requirement + `components.securitySchemes` (replaces the `[[security.schemes]]` knob,
/// CLAUDE.md rule 4).
#[derive(Debug, Clone)]
pub struct ApplySecurity {
    scheme: SecurityScheme,
    selectors: Vec<OperationSelector>,
}

/// Reusable operation selector for transforms that need to match routes by path, method, middleware,
/// or boolean composition.
#[derive(Debug, Clone)]
pub enum OperationSelector {
    /// Match operations whose graph path, or base-path-joined path, starts with this prefix.
    PathPrefix(String),
    /// Match operations whose HTTP method is one of these uppercase method names.
    Methods(Vec<String>),
    /// Match operations carrying this source middleware symbol.
    Middleware(String),
    /// Match if any nested selector matches.
    Any(Vec<OperationSelector>),
    /// Match only if all nested selectors match.
    All(Vec<OperationSelector>),
}

impl OperationSelector {
    /// Match operations whose graph path, or base-path-joined path, starts with `prefix`.
    #[must_use]
    pub fn path_prefix(prefix: impl Into<String>) -> Self {
        Self::PathPrefix(prefix.into())
    }

    /// Match operations whose HTTP method is in `methods`.
    #[must_use]
    pub fn methods<I, S>(methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut methods: Vec<String> = methods
            .into_iter()
            .map(Into::into)
            .map(|method| method.to_ascii_uppercase())
            .collect();
        methods.sort();
        methods.dedup();
        Self::Methods(methods)
    }

    /// Match operations carrying a source middleware symbol.
    #[must_use]
    pub fn middleware(symbol: impl Into<String>) -> Self {
        Self::Middleware(symbol.into())
    }

    /// Match if any nested selector matches.
    #[must_use]
    pub fn any<I>(selectors: I) -> Self
    where
        I: IntoIterator<Item = OperationSelector>,
    {
        Self::Any(selectors.into_iter().collect())
    }

    /// Match only if all nested selectors match.
    #[must_use]
    pub fn all<I>(selectors: I) -> Self
    where
        I: IntoIterator<Item = OperationSelector>,
    {
        Self::All(selectors.into_iter().collect())
    }
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
                global: true,
            },
            selectors: Vec::new(),
        }
    }

    /// Apply this scheme only to operations matched by `selector`.
    #[must_use]
    pub fn when(mut self, selector: OperationSelector) -> Self {
        self.scheme.global = false;
        self.selectors.push(selector);
        self
    }

    /// Apply this scheme only to operations whose graph path, or base-path-joined path, starts with
    /// `prefix`.
    #[must_use]
    pub fn when_path_prefix(self, prefix: impl Into<String>) -> Self {
        self.when(OperationSelector::path_prefix(prefix))
    }

    /// Apply this scheme only to operations whose HTTP method is in `methods`.
    #[must_use]
    pub fn when_methods<I, S>(self, methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.when(OperationSelector::methods(methods))
    }

    /// Apply this scheme only to operations that carry a source middleware symbol.
    #[must_use]
    pub fn when_middleware(self, symbol: impl Into<String>) -> Self {
        self.when(OperationSelector::middleware(symbol))
    }
}

impl Transform for ApplySecurity {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.security.push(self.scheme.clone());
        if self.selectors.is_empty() {
            return Ok(());
        }
        let base_path = ir.base_path.clone();
        let mut matched = 0_usize;
        for op in &mut ir.operations {
            if self
                .selectors
                .iter()
                .all(|selector| operation_selector_matches(selector, op, &base_path))
            {
                matched += 1;
                op.security.push(self.scheme.id.clone());
                op.security.sort();
                op.security.dedup();
            }
        }
        if matched == 0 {
            return Err(CoreError::Config {
                message: format!(
                    "security scheme '{}' did not match any operations",
                    self.scheme.id
                ),
            });
        }
        Ok(())
    }
}

fn operation_selector_matches(
    selector: &OperationSelector,
    op: &crate::graph::Operation,
    base_path: &str,
) -> bool {
    match selector {
        OperationSelector::PathPrefix(prefix) => {
            op.path.starts_with(prefix)
                || joined_operation_path(base_path, &op.path).starts_with(prefix)
        }
        OperationSelector::Methods(methods) => methods.iter().any(|method| method == &op.method),
        OperationSelector::Middleware(symbol) => op
            .middleware
            .iter()
            .any(|middleware| middleware_symbol_matches(middleware, symbol)),
        OperationSelector::Any(selectors) => selectors
            .iter()
            .any(|selector| operation_selector_matches(selector, op, base_path)),
        OperationSelector::All(selectors) => selectors
            .iter()
            .all(|selector| operation_selector_matches(selector, op, base_path)),
    }
}

fn middleware_symbol_matches(actual: &str, expected: &str) -> bool {
    actual == expected
        || actual
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| suffix == expected)
}

fn joined_operation_path(base_path: &str, path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    match (base.is_empty() || base == "/", path.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{path}"),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}/{path}"),
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

/// Route-scoped SDK operation aliases for preserving an existing public SDK surface.
///
/// These aliases are user-supplied code-as-config metadata. They do not parse another generator's
/// output; they match the neutral graph route and set the operation group/tag and generated operation
/// name that SDK targets already consume.
#[derive(Debug, Clone, Default)]
pub struct SdkOperationAliases {
    aliases: Vec<SdkOperationAlias>,
}

#[derive(Debug, Clone)]
struct SdkOperationAlias {
    matcher: OperationMatcher,
    tag: Option<String>,
    name: Option<String>,
}

impl SdkOperationAliases {
    /// Create an empty operation alias set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start configuring an operation alias matched by method and graph path.
    #[must_use]
    pub fn operation(mut self, method: impl Into<String>, path: impl Into<String>) -> Self {
        self.aliases.push(SdkOperationAlias {
            matcher: OperationMatcher::Route {
                method: method.into().to_ascii_uppercase(),
                path: path.into(),
            },
            tag: None,
            name: None,
        });
        self
    }

    /// Set the SDK group/tag for the most recently configured operation alias.
    #[must_use]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        if let Some(alias) = self.aliases.last_mut() {
            alias.tag = Some(tag.into());
        }
        self
    }

    /// Set the SDK operation name for the most recently configured operation alias.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        if let Some(alias) = self.aliases.last_mut() {
            alias.name = Some(name.into());
        }
        self
    }
}

impl Transform for SdkOperationAliases {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        for alias in &self.aliases {
            if alias.tag.is_none() && alias.name.is_none() {
                return Err(CoreError::Config {
                    message: format!(
                        "SDK operation alias has no tag or name: {:?}",
                        alias.matcher
                    ),
                });
            }
            let op_index = find_operation_index(ir, &alias.matcher, "SDK operation alias")?;
            let op = &mut ir.operations[op_index];
            if let Some(tag) = &alias.tag {
                op.group = Some(tag.clone());
            }
            if let Some(name) = &alias.name {
                op.id.clone_from(name);
                op.handler.clone_from(name);
            }
        }
        ensure_unique_operation_ids(ir)?;
        Ok(())
    }
}

fn ensure_unique_operation_ids(ir: &ApiGraph) -> Result<(), CoreError> {
    for (index, op) in ir.operations.iter().enumerate() {
        if ir
            .operations
            .iter()
            .skip(index + 1)
            .any(|other| other.id == op.id)
        {
            return Err(CoreError::Config {
                message: format!(
                    "SDK operation alias produced duplicate operation id {:?}",
                    op.id
                ),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------------------

/// Typed OpenAPI component aliases. Aliases are added as component schemas whose body is a `$ref` to
/// the canonical component.
#[derive(Debug, Clone, Default)]
pub struct OpenApiSchemaAliases {
    aliases: Vec<(String, String)>,
}

impl OpenApiSchemaAliases {
    /// Create an empty alias set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one component alias (`alias` points at `canonical`).
    #[must_use]
    pub fn alias(mut self, canonical: impl Into<String>, alias: impl Into<String>) -> Self {
        self.aliases.push((canonical.into(), alias.into()));
        self
    }
}

/// Typed OpenAPI schema patch. Field patches mutate properties on the named object schema.
#[derive(Debug, Clone)]
pub struct OpenApiSchemaPatch {
    schema: String,
    field_patches: Vec<OpenApiFieldPatch>,
}

impl OpenApiSchemaPatch {
    /// Patch an existing named component schema.
    #[must_use]
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            field_patches: Vec::new(),
        }
    }

    /// Add a field patch for a property on this object schema.
    #[must_use]
    pub fn field(mut self, patch: OpenApiFieldPatch) -> Self {
        self.field_patches.push(patch);
        self
    }
}

/// Typed OpenAPI field patch builder for constraints/defaults/extensions.
#[derive(Debug, Clone)]
pub struct OpenApiFieldPatch {
    field: String,
    constraints: Constraints,
    default: Option<LiteralValue>,
    extensions: Vec<Extension>,
}

impl OpenApiFieldPatch {
    /// Patch an existing object property.
    #[must_use]
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            constraints: Constraints::default(),
            default: None,
            extensions: Vec::new(),
        }
    }

    /// Set `minLength`.
    #[must_use]
    pub fn min_length(mut self, value: u64) -> Self {
        self.constraints.min_length = Some(value);
        self
    }

    /// Set `maxLength`.
    #[must_use]
    pub fn max_length(mut self, value: u64) -> Self {
        self.constraints.max_length = Some(value);
        self
    }

    /// Set inclusive numeric `minimum`.
    #[must_use]
    pub fn minimum(mut self, value: impl Into<String>) -> Self {
        self.constraints.minimum = Some(value.into());
        self
    }

    /// Set inclusive numeric `maximum`.
    #[must_use]
    pub fn maximum(mut self, value: impl Into<String>) -> Self {
        self.constraints.maximum = Some(value.into());
        self
    }

    /// Set a field-level enum.
    #[must_use]
    pub fn enum_values<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.constraints.enum_values = values.into_iter().map(Into::into).collect();
        self.constraints.enum_values.sort();
        self
    }

    /// Set a string default.
    #[must_use]
    pub fn default_string(mut self, value: impl Into<String>) -> Self {
        self.default = Some(LiteralValue::String(value.into()));
        self
    }

    /// Set a numeric default.
    #[must_use]
    pub fn default_number(mut self, value: impl Into<String>) -> Self {
        self.default = Some(LiteralValue::Number(value.into()));
        self
    }

    /// Set a boolean default.
    #[must_use]
    pub fn default_bool(mut self, value: bool) -> Self {
        self.default = Some(LiteralValue::Bool(value));
        self
    }

    /// Add or replace a string vendor extension.
    #[must_use]
    pub fn extension_string(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extensions.push(Extension {
            name: name.into(),
            value: LiteralValue::String(value.into()),
        });
        self
    }
}

fn apply_openapi_customizations(
    doc: &mut OpenApiDoc,
    aliases: &OpenApiSchemaAliases,
    patches: &[OpenApiSchemaPatch],
) -> Result<(), CoreError> {
    for (canonical, alias) in &aliases.aliases {
        if !doc
            .components
            .schemas
            .iter()
            .any(|(name, _)| name == canonical)
        {
            return Err(CoreError::Config {
                message: format!("OpenAPI schema alias references unknown schema {canonical:?}"),
            });
        }
        if doc.components.schemas.iter().any(|(name, _)| name == alias) {
            return Err(CoreError::Config {
                message: format!("OpenAPI schema alias {alias:?} collides with an existing schema"),
            });
        }
        doc.components
            .schemas
            .push((alias.clone(), SchemaObject::reference(canonical.clone())));
    }
    doc.components.schemas.sort_by(|a, b| a.0.cmp(&b.0));

    for patch in patches {
        let Some((_, schema)) = doc
            .components
            .schemas
            .iter_mut()
            .find(|(name, _)| name == &patch.schema)
        else {
            return Err(CoreError::Config {
                message: format!(
                    "OpenAPI schema patch references unknown schema {:?}",
                    patch.schema
                ),
            });
        };
        for field_patch in &patch.field_patches {
            apply_openapi_field_patch(&patch.schema, schema, field_patch)?;
        }
    }
    Ok(())
}

fn apply_openapi_field_patch(
    schema_name: &str,
    schema: &mut SchemaObject,
    patch: &OpenApiFieldPatch,
) -> Result<(), CoreError> {
    let Some((_, prop)) = schema
        .properties
        .iter_mut()
        .find(|(field, _)| field == &patch.field)
    else {
        return Err(CoreError::Config {
            message: format!(
                "OpenAPI schema patch references unknown field {schema_name}.{}",
                patch.field
            ),
        });
    };

    prop.min_length = patch.constraints.min_length;
    prop.max_length = patch.constraints.max_length;
    prop.minimum.clone_from(&patch.constraints.minimum);
    prop.maximum.clone_from(&patch.constraints.maximum);
    prop.exclusive_minimum
        .clone_from(&patch.constraints.exclusive_minimum);
    prop.exclusive_maximum
        .clone_from(&patch.constraints.exclusive_maximum);
    prop.pattern.clone_from(&patch.constraints.pattern);
    if !patch.constraints.enum_values.is_empty() {
        prop.enum_values.clone_from(&patch.constraints.enum_values);
    }
    prop.default_value.clone_from(&patch.default);
    for extension in &patch.extensions {
        if let Some(existing) = prop
            .extensions
            .iter_mut()
            .find(|existing| existing.name == extension.name)
        {
            *existing = extension.clone();
        } else {
            prop.extensions.push(extension.clone());
        }
    }
    prop.extensions.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(())
}

/// The OpenAPI 3.1 target: lowers the frozen IR to an OpenAPI document and writes it at [`OpenApi31::to`].
///
/// Reads `ir.title` / `ir.base_path` / `ir.security` (the metadata transforms set) and calls the
/// existing [`crate::lower::to_openapi`] — NOT a re-implementation. The graph's [`SecurityScheme`]s
/// are passed straight through (`to_openapi` takes `&[SecurityScheme]` directly).
#[derive(Debug, Clone)]
pub struct OpenApi31 {
    path: String,
    schema_aliases: OpenApiSchemaAliases,
    schema_patches: Vec<OpenApiSchemaPatch>,
}

impl OpenApi31 {
    /// An OpenAPI 3.1 target with no output path yet (set with [`OpenApi31::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: String::new(),
            schema_aliases: OpenApiSchemaAliases::default(),
            schema_patches: Vec::new(),
        }
    }

    /// Set the output path for the OpenAPI document (e.g. `"generated/openapi.yaml"`).
    #[must_use]
    pub fn to(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Add typed component aliases.
    #[must_use]
    pub fn schema_aliases(mut self, aliases: OpenApiSchemaAliases) -> Self {
        self.schema_aliases.aliases.extend(aliases.aliases);
        self
    }

    /// Add a typed schema patch.
    #[must_use]
    pub fn schema_patch(mut self, patch: OpenApiSchemaPatch) -> Self {
        self.schema_patches.push(patch);
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
        let mut doc = crate::lower::build_openapi_doc(ir, &ir.title, &ir.base_path, &ir.security)?;
        apply_openapi_customizations(&mut doc, &self.schema_aliases, &self.schema_patches)?;
        out.write(self.path.clone(), crate::lower::write_openapi_yaml(&doc));
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
    schema_aliases: OpenApiSchemaAliases,
    schema_patches: Vec<OpenApiSchemaPatch>,
}

impl OpenApi31Json {
    /// An OpenAPI 3.1 JSON target with no output path yet (set with [`OpenApi31Json::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: String::new(),
            schema_aliases: OpenApiSchemaAliases::default(),
            schema_patches: Vec::new(),
        }
    }

    /// Set the output path for the OpenAPI JSON document (e.g. `"generated/openapi.json"`).
    #[must_use]
    pub fn to(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Add typed component aliases.
    #[must_use]
    pub fn schema_aliases(mut self, aliases: OpenApiSchemaAliases) -> Self {
        self.schema_aliases.aliases.extend(aliases.aliases);
        self
    }

    /// Add a typed schema patch.
    #[must_use]
    pub fn schema_patch(mut self, patch: OpenApiSchemaPatch) -> Self {
        self.schema_patches.push(patch);
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
        let mut doc = crate::lower::build_openapi_doc(ir, &ir.title, &ir.base_path, &ir.security)?;
        apply_openapi_customizations(&mut doc, &self.schema_aliases, &self.schema_patches)?;
        out.write(self.path.clone(), crate::lower::write_openapi_json(&doc)?);
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
        let (source_root, files) = self.static_source_files(cx)?;
        let to_dir = validate_static_dir("output dir", &self.to_dir)?;
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

    fn cache_input_files(&self, cx: &Cx) -> Result<Vec<std::path::PathBuf>, CoreError> {
        let (source_root, files) = self.static_source_files(cx)?;
        Ok(files.into_iter().map(|rel| source_root.join(rel)).collect())
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

impl StaticFiles {
    fn static_source_files(&self, cx: &Cx) -> Result<(std::path::PathBuf, Vec<String>), CoreError> {
        let from_dir = validate_static_dir("source dir", &self.from_dir)?;
        validate_static_dir("output dir", &self.to_dir)?;

        let source_root = cx.project_root.join(from_dir);
        let mut files = Vec::new();
        for include in &self.includes {
            collect_static_include(&source_root, include, &mut files)?;
        }
        files.sort();
        files.dedup();
        Ok((source_root, files))
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
    go_version: String,
    dir: String,
    layout: SdkFileLayout,
    aliases: SdkTypeAliases,
    profile: SdkProfile,
    docs: SdkDocs,
    package_metadata: bool,
    error_model: Option<String>,
    required_pointer_constructor_policy: Option<RequiredPointerConstructorPolicy>,
    query_time_format: Option<QueryTimeFormat>,
    request_builder_scope: Option<GoRequestBuilderScope>,
    request_builder_aliases: Option<GoRequestBuilderAliases>,
    query_setter_argument_policy: Option<GoQuerySetterArgumentPolicy>,
    execute_compatibility: Option<GoExecuteCompatibility>,
}

impl GoSdk {
    /// A Go SDK target with no module/output yet (set with [`GoSdk::module`] + [`GoSdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            go_version: "1.23".to_string(),
            dir: String::new(),
            layout: SdkFileLayout::compact(),
            aliases: SdkTypeAliases::default(),
            profile: SdkProfile::default(),
            docs: SdkDocs::default(),
            package_metadata: true,
            error_model: None,
            required_pointer_constructor_policy: None,
            query_time_format: None,
            request_builder_scope: None,
            request_builder_aliases: None,
            query_setter_argument_policy: None,
            execute_compatibility: None,
        }
    }

    /// Set the Go module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The package
    /// name is derived from this — the single source of truth (CLAUDE.md rule 3).
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the Go module path for the generated SDK.
    ///
    /// Alias for [`GoSdk::module`] for call sites that prefer `module_path(...)`.
    #[must_use]
    pub fn module_path(self, module: impl Into<String>) -> Self {
        self.module(module)
    }

    /// Set the Go language version for the generated `go.mod`.
    #[must_use]
    pub fn go_version(mut self, version: impl Into<String>) -> Self {
        self.go_version = version.into();
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

    /// Set the SDK profile. The minimal profile preserves the historical Go SDK output.
    #[must_use]
    pub fn profile(mut self, profile: SdkProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Decode non-2xx response bodies into the named error model when `GenericOpenAPIError::Model`
    /// is called and no explicit model was attached.
    #[must_use]
    pub fn error_model(mut self, model: impl Into<String>) -> Self {
        self.error_model = Some(model.into());
        self
    }

    /// Set how required pointer fields are represented in OpenAPI Generator-compatible constructors.
    #[must_use]
    pub const fn required_pointer_constructor_policy(
        mut self,
        policy: RequiredPointerConstructorPolicy,
    ) -> Self {
        self.required_pointer_constructor_policy = Some(policy);
        self
    }

    /// Set how `time.Time` query values are serialized in compatibility helpers.
    #[must_use]
    pub const fn query_time_format(mut self, format: QueryTimeFormat) -> Self {
        self.query_time_format = Some(format);
        self
    }

    /// Set whether request builders emit operation-local or legacy graph-wide setters.
    #[must_use]
    pub const fn request_builder_scope(mut self, scope: GoRequestBuilderScope) -> Self {
        self.request_builder_scope = Some(scope);
        self
    }

    /// Add request-builder body/query alias setters for OpenAPI Generator-compatible output.
    #[must_use]
    pub fn request_builder_aliases(mut self, aliases: GoRequestBuilderAliases) -> Self {
        self.request_builder_aliases = Some(aliases);
        self
    }

    /// Set how OpenAPI Generator-compatible query setter arguments are typed.
    #[must_use]
    pub fn query_setter_argument_policy(mut self, policy: GoQuerySetterArgumentPolicy) -> Self {
        self.query_setter_argument_policy = Some(policy);
        self
    }

    /// Configure legacy `Execute` wrappers for selected compatibility request builders.
    #[must_use]
    pub fn execute_compatibility(mut self, compatibility: GoExecuteCompatibility) -> Self {
        self.execute_compatibility = Some(compatibility);
        self
    }

    /// Configure generated SDK documentation output.
    #[must_use]
    pub fn docs(mut self, docs: impl Into<SdkDocs>) -> Self {
        self.docs = docs.into();
        self
    }

    /// Disable generated SDK README/reference docs.
    #[must_use]
    pub fn without_docs(self) -> Self {
        self.docs(false)
    }

    /// Enable or disable package metadata files such as `go.mod`.
    #[must_use]
    pub const fn package_metadata(mut self, enabled: bool) -> Self {
        self.package_metadata = enabled;
        self
    }

    /// Emit source files only, without docs or package metadata.
    #[must_use]
    pub fn source_only(self) -> Self {
        self.docs(false).package_metadata(false)
    }

    /// Expose `alias` as an additional type name for a schema id or generated schema name.
    #[must_use]
    pub fn type_alias(self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        let aliases = self.aliases.clone().type_alias(schema, alias);
        self.aliases(aliases)
    }

    fn effective_options(&self) -> GoSdkOptions {
        let mut options = GoSdkOptions::for_profile(&self.profile);
        if let Some(model) = &self.error_model {
            options.error_model = Some(model.clone());
        }
        if let Some(policy) = self.required_pointer_constructor_policy {
            options.required_pointer_constructor_policy = policy;
        }
        if let Some(format) = self.query_time_format {
            options.query_time_format = format;
        }
        if let Some(scope) = self.request_builder_scope {
            options.request_builder_scope = scope;
        }
        if let Some(aliases) = &self.request_builder_aliases {
            options.request_builder_aliases = aliases.clone();
        }
        if let Some(policy) = &self.query_setter_argument_policy {
            options.query_setter_argument_policy = policy.clone();
        }
        if let Some(compatibility) = &self.execute_compatibility {
            options.execute_compatibility = compatibility.clone();
        }
        options
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
        if self.go_version.trim().is_empty() || self.go_version.chars().any(char::is_whitespace) {
            return Err(CoreError::Config {
                message: "GoSdk go_version must be a non-empty Go version without whitespace"
                    .to_string(),
            });
        }
        // Derive the package from the module path (the single source of truth) and generate via the
        // existing deterministic SDK generator — never a re-implementation (CLAUDE.md rules 2 & 3).
        let package = sdk_package(&self.module)?;
        let model = SdkModel::build(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
            &self.profile,
        )?;
        let files = crate::gosdk::generate_files_with_profile_options(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
            &self.profile,
            self.effective_options(),
        )?;
        write_sdk_files(out, &self.dir, files)?;
        write_sdk_docs(out, &self.dir, "Go", &package, ir, &model, &self.docs)?;
        if self.package_metadata {
            out.write(
                format!("{}/go.mod", self.dir.trim_end_matches('/')),
                format!("module {}\n\ngo {}\n", self.module, self.go_version),
            );
        }
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
    profile: SdkProfile,
    docs: SdkDocs,
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
            profile: SdkProfile::default(),
            docs: SdkDocs::default(),
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

    /// Set the SDK profile. The minimal profile preserves the historical Python SDK output.
    #[must_use]
    pub fn profile(mut self, profile: SdkProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Configure generated SDK documentation output.
    #[must_use]
    pub fn docs(mut self, docs: impl Into<SdkDocs>) -> Self {
        self.docs = docs.into();
        self
    }

    /// Disable generated SDK README/reference docs.
    #[must_use]
    pub fn without_docs(self) -> Self {
        self.docs(false)
    }

    /// Emit source files only, without generated docs.
    #[must_use]
    pub fn source_only(self) -> Self {
        self.docs(false)
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
        let model = SdkModel::build(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
            &self.profile,
        )?;
        let files = crate::pysdk::generate_files_with_options(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            self.model_style,
            &self.aliases,
        )?;
        write_sdk_files(out, &self.dir, files)?;
        write_sdk_docs(out, &self.dir, "Python", &package, ir, &model, &self.docs)?;
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
    profile: SdkProfile,
    docs: SdkDocs,
    package_metadata: Option<bool>,
    model_property_policy: Option<TsModelPropertyPolicy>,
    nullable_policy: Option<TsNullablePolicy>,
    response_policy: Option<TsResponsePolicy>,
    request_body_param_name: Option<String>,
    init_override_function: Option<bool>,
    barrel_exports: Option<TsBarrelExports>,
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
            profile: SdkProfile::default(),
            docs: SdkDocs::default(),
            package_metadata: None,
            model_property_policy: None,
            nullable_policy: None,
            response_policy: None,
            request_body_param_name: None,
            init_override_function: None,
            barrel_exports: None,
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

    /// Set the SDK profile.
    #[must_use]
    pub fn profile(mut self, profile: SdkProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Configure generated SDK documentation output.
    #[must_use]
    pub fn docs(mut self, docs: impl Into<SdkDocs>) -> Self {
        self.docs = docs.into();
        self
    }

    /// Disable generated SDK README/reference docs.
    #[must_use]
    pub fn without_docs(self) -> Self {
        self.docs(false)
    }

    /// Enable or disable package metadata files such as `package.json`.
    #[must_use]
    pub const fn package_metadata(mut self, enabled: bool) -> Self {
        self.package_metadata = Some(enabled);
        self
    }

    /// Emit source files only, without docs or package metadata.
    #[must_use]
    pub fn source_only(self) -> Self {
        self.docs(false).package_metadata(false)
    }

    /// Set a TypeScript compatibility profile.
    #[must_use]
    pub fn compatibility(self, compatibility: TsCompatibility) -> Self {
        match compatibility {
            TsCompatibility::OpenApiGenerator => {
                self.profile(SdkProfile::openapi_generator_compat())
            }
        }
    }

    /// Set how model interface properties use `?:`.
    #[must_use]
    pub const fn model_property_policy(mut self, policy: TsModelPropertyPolicy) -> Self {
        self.model_property_policy = Some(policy);
        self
    }

    /// Set how model interface properties include `| null`.
    #[must_use]
    pub const fn nullable_policy(mut self, policy: TsNullablePolicy) -> Self {
        self.nullable_policy = Some(policy);
        self
    }

    /// Set how operation methods return response bodies.
    #[must_use]
    pub const fn response_policy(mut self, policy: TsResponsePolicy) -> Self {
        self.response_policy = Some(policy);
        self
    }

    /// Set the request object property name used for JSON request bodies.
    #[must_use]
    pub fn request_body_param_name(mut self, name: impl Into<String>) -> Self {
        self.request_body_param_name = Some(name.into());
        self
    }

    /// Enable or disable OpenAPI Generator-compatible `InitOverrideFunction` support.
    #[must_use]
    pub const fn init_override_function(mut self, enabled: bool) -> Self {
        self.init_override_function = Some(enabled);
        self
    }

    /// Set how the TypeScript fetch profile emits the root barrel.
    #[must_use]
    pub const fn barrel_exports(mut self, exports: TsBarrelExports) -> Self {
        self.barrel_exports = Some(exports);
        self
    }

    /// Expose `alias` as an additional type name for a schema id or generated schema name.
    #[must_use]
    pub fn type_alias(self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        let aliases = self.aliases.clone().type_alias(schema, alias);
        self.aliases(aliases)
    }

    fn effective_options(&self) -> TsSdkOptions {
        let mut options = TsSdkOptions::for_profile(&self.profile);
        if let Some(policy) = self.model_property_policy {
            options.model_properties = policy;
        }
        if let Some(policy) = self.nullable_policy {
            options.nullable = policy;
        }
        if let Some(policy) = self.response_policy {
            options.response = policy;
        }
        if let Some(name) = &self.request_body_param_name {
            options.request_body_param_name.clone_from(name);
        }
        if let Some(enabled) = self.init_override_function {
            options.init_override_function = enabled;
        }
        if let Some(exports) = self.barrel_exports {
            options.barrel_exports = exports;
        }
        options
    }

    fn effective_package_metadata(&self) -> bool {
        self.package_metadata.unwrap_or_else(|| {
            self.profile.is_typescript_fetch_compat() || self.profile.is_typescript_axios_compat()
        })
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
        let model = SdkModel::build(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
            &self.profile,
        )?;
        let options = self.effective_options();
        let mut files = crate::tssdk::generate_files_with_profile_options(
            ir,
            &package,
            &ir.base_path,
            &self.layout,
            &self.aliases,
            &self.profile,
            &options,
        )?;
        if self.effective_package_metadata() {
            if !files.iter().any(|file| file.name == "package.json") {
                files.push(super::bundle::SdkFile {
                    name: "package.json".to_string(),
                    contents: ts_package_json(&package, self.profile.is_typescript_axios_compat()),
                });
                files.sort_by(|a, b| a.name.cmp(&b.name));
            }
        } else {
            files.retain(|file| file.name != "package.json");
        }
        write_sdk_files(out, &self.dir, files)?;
        write_sdk_docs(
            out,
            &self.dir,
            "TypeScript",
            &package,
            ir,
            &model,
            &self.docs,
        )?;
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

/// Run a formatter or normalizer against generated artifacts before the host writes them.
#[derive(Debug, Clone)]
pub struct FormatCommand {
    program: String,
    args: Vec<String>,
}

impl FormatCommand {
    /// Create a command postprocessor.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Set command arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

impl PostProcess for FormatCommand {
    fn run(&self, out: &mut Artifacts, cx: &Cx) -> Result<(), CoreError> {
        let temp = unique_postprocess_dir(&cx.project_root)?;
        std::fs::create_dir_all(&temp).map_err(|err| CoreError::Io {
            message: format!(
                "failed to create post-write temp dir {}: {err}",
                temp.display()
            ),
        })?;
        let result = self.run_in_temp(out, &temp);
        let cleanup = std::fs::remove_dir_all(&temp);
        match (result, cleanup) {
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(CoreError::Io {
                message: format!(
                    "failed to remove post-write temp dir {}: {err}",
                    temp.display()
                ),
            }),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn cache_key_fragment(&self, _cx: &Cx) -> Result<Vec<u8>, CoreError> {
        let mut fragment = format!("FormatCommand\0{}\0", self.program).into_bytes();
        for arg in &self.args {
            fragment.extend(arg.as_bytes());
            fragment.push(0);
        }
        if let Some(path) = resolve_command_path(&self.program) {
            fragment.extend(path.to_string_lossy().as_bytes());
            fragment.push(0);
            if let Ok(metadata) = std::fs::metadata(&path) {
                fragment.extend(metadata.len().to_string().as_bytes());
                fragment.push(0);
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        fragment.extend(duration.as_nanos().to_string().as_bytes());
                    }
                }
            }
        }
        Ok(fragment)
    }
}

impl FormatCommand {
    fn run_in_temp(&self, out: &mut Artifacts, temp: &Path) -> Result<(), CoreError> {
        let artifact_paths: BTreeSet<String> = out
            .files()
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect();
        for artifact in out.files() {
            let path = temp_artifact_path(temp, &artifact.path)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| CoreError::Io {
                    message: format!("failed to create {}: {err}", parent.display()),
                })?;
            }
            std::fs::write(&path, &artifact.text).map_err(|err| CoreError::Io {
                message: format!("failed to write {}: {err}", path.display()),
            })?;
        }

        let output = Command::new(&self.program)
            .args(&self.args)
            .current_dir(temp)
            .output()
            .map_err(|err| CoreError::Config {
                message: format!("failed to run post-write command '{}': {err}", self.program),
            })?;
        if !output.status.success() {
            return Err(CoreError::Config {
                message: format!(
                    "post-write command '{}' exited with status {:?}:\n{}",
                    self.program,
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        let temp_paths = collect_temp_files(temp)?;
        for path in &temp_paths {
            if !artifact_paths.contains(path) {
                return Err(CoreError::Config {
                    message: format!(
                        "post-write command '{}' created undeclared artifact '{}'",
                        self.program, path
                    ),
                });
            }
        }
        for artifact_path in artifact_paths {
            let path = temp_artifact_path(temp, &artifact_path)?;
            let text = std::fs::read_to_string(&path).map_err(|err| CoreError::Config {
                message: format!(
                    "post-write command '{}' removed or invalidated {}: {err}",
                    self.program,
                    path.display()
                ),
            })?;
            out.write(artifact_path, text);
        }
        Ok(())
    }
}

fn unique_postprocess_dir(project_root: &Path) -> Result<PathBuf, CoreError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| CoreError::Io {
            message: format!("system clock before Unix epoch: {err}"),
        })?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!(
        "gnr8-post-write-{}-{nanos}",
        project_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project")
    )))
}

fn temp_artifact_path(root: &Path, rel: &str) -> Result<PathBuf, CoreError> {
    let path = Path::new(rel);
    if rel.is_empty() || path.is_absolute() {
        return Err(CoreError::Io {
            message: format!("unsafe generated artifact path '{rel}'"),
        });
    }
    let mut out = root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(CoreError::Io {
                    message: format!("unsafe generated artifact path '{rel}'"),
                });
            }
        }
    }
    Ok(out)
}

fn collect_temp_files(root: &Path) -> Result<BTreeSet<String>, CoreError> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|err| CoreError::Io {
            message: format!(
                "failed to read post-write temp dir {}: {err}",
                dir.display()
            ),
        })? {
            let entry = entry.map_err(|err| CoreError::Io {
                message: format!(
                    "failed to read post-write temp dir {}: {err}",
                    dir.display()
                ),
            })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|err| CoreError::Io {
                message: format!(
                    "failed to inspect post-write temp file {}: {err}",
                    path.display()
                ),
            })?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|err| CoreError::Io {
                        message: format!("failed to relativize post-write temp file: {err}"),
                    })?
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel);
            }
        }
    }
    Ok(out)
}

fn resolve_command_path(program: &str) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return program_path.is_file().then(|| program_path.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

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

fn ts_package_json(package: &str, axios: bool) -> String {
    let dependencies = if axios {
        "\n  \"dependencies\": {\n    \"axios\": \"^1.0.0\"\n  },"
    } else {
        ""
    };
    format!(
        "{{
  \"name\": {},
  \"version\": \"0.1.0\",
  \"type\": \"module\",{}
  \"main\": \"./index.js\",
  \"module\": \"./index.js\",
  \"types\": \"./index.d.ts\",
  \"exports\": {{
    \".\": {{
      \"types\": \"./index.d.ts\",
      \"import\": \"./index.js\"
    }}
  }}
}}
",
        quoted_string_literal(package),
        dependencies
    )
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

fn validate_static_dir(kind: &str, path: &str) -> Result<String, CoreError> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(CoreError::Config {
            message: format!(
                "StaticFiles target has no {kind} — call .from(\"path\")/.to(\"path\")"
            ),
        });
    }
    super::bundle::safe_frame_name(trimmed).map_err(|err| CoreError::Config {
        message: format!("invalid StaticFiles {kind} {path:?}: {err}"),
    })?;
    Ok(trimmed.to_string())
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
        sdk_package, ApiOverrides, ApplySecurity, Cx, EnumOrder, FastApi, Flask, FormatCommand,
        GoGin, GoSdk, GroupOperations, Header, NestJs, OpenApi31, OpenApi31Json, OpenApiFieldPatch,
        OpenApiSchemaAliases, OpenApiSchemaPatch, OperationSelector, PostProcess, PySdk,
        QueryParam, SdkOperationAliases, SetBasePath, SetEnumOrder, SetOperationSuccessResponse,
        SetSchemaFieldType, SetTitle, Source, StaticFiles, Target, Transform, TsSdk,
    };
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{
        ApiGraph, Diagnostic, Field, Operation, Prim, Response, Schema, SchemaRef, SourceSpan, Type,
    };
    use crate::sdk::docs::SdkDocs;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::typescript::TsCompatibility;
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
    fn language_sources_report_cache_input_roots_for_doctor_probe() {
        let root = temp_project("source-roots");
        let cx = Cx::new(&root);

        assert_eq!(
            FastApi::new().inputs(["api"]).cache_input_roots(&cx),
            Some(vec![root.join("api")])
        );
        assert_eq!(
            Flask::new().inputs(["flask_app"]).cache_input_roots(&cx),
            Some(vec![root.join("flask_app")])
        );
        assert_eq!(
            NestJs::new().inputs(["src"]).cache_input_roots(&cx),
            Some(vec![root.join("src")])
        );
    }

    #[test]
    fn apply_security_can_scope_schemes_to_routes_and_methods() {
        let mut ir = ApiGraph {
            base_path: "/v1".to_string(),
            operations: vec![
                Operation {
                    id: "activeSchool".to_string(),
                    method: "GET".to_string(),
                    path: "/schools/active/profile".to_string(),
                    handler: "activeSchool".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "createItem".to_string(),
                    method: "POST".to_string(),
                    path: "/items".to_string(),
                    handler: "createItem".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "activeWrite".to_string(),
                    method: "PATCH".to_string(),
                    path: "/schools/active/items".to_string(),
                    handler: "activeWrite".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        ApplySecurity::api_key("ActiveSchoolAuth", "X-Plint-School-Id")
            .when_path_prefix("/v1/schools/active/")
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("CSRFAuth", "X-CSRF-Token")
            .when_methods(["POST", "PUT", "PATCH", "DELETE"])
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.security.len(), 2);
        assert!(ir.security.iter().all(|scheme| !scheme.global));
        assert_eq!(ir.operations[0].security, vec!["ActiveSchoolAuth"]);
        assert_eq!(ir.operations[1].security, vec!["CSRFAuth"]);
        assert_eq!(
            ir.operations[2].security,
            vec!["ActiveSchoolAuth", "CSRFAuth"]
        );

        let mut out = Artifacts::new();
        OpenApi31::new()
            .to("openapi.yaml")
            .generate(&ir, &mut out, &cx())
            .unwrap();
        let yaml = out.files()[0].text.as_str();
        assert!(!yaml.starts_with("security:"), "{yaml}");
        assert!(yaml.contains("ActiveSchoolAuth: []"), "{yaml}");
        assert!(yaml.contains("CSRFAuth: []"), "{yaml}");
        assert!(
            yaml.contains("        - ActiveSchoolAuth: []\n          CSRFAuth: []"),
            "{yaml}"
        );
    }

    #[test]
    fn apply_security_can_scope_schemes_to_source_middleware() {
        let mut ir = ApiGraph {
            operations: vec![
                Operation {
                    id: "openActiveFile".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/schools/active/files/{fileId}/open".to_string(),
                    handler: "openActiveFile".to_string(),
                    group: Some("files".to_string()),
                    middleware: vec!["RequireActiveSchool".to_string()],
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "createActiveFile".to_string(),
                    method: "POST".to_string(),
                    path: "/v1/schools/active/files".to_string(),
                    handler: "createActiveFile".to_string(),
                    group: Some("files".to_string()),
                    middleware: vec!["RequireActiveSchool".to_string(), "RequireCSRF".to_string()],
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "exportAdmin".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/admin/export/{exportId}".to_string(),
                    handler: "exportAdmin".to_string(),
                    group: Some("admin".to_string()),
                    middleware: vec!["Auth.RequireActor".to_string()],
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        ApplySecurity::api_key("ActiveSchoolAuth", "X-School-Id")
            .when_middleware("RequireActiveSchool")
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("CSRFAuth", "X-CSRF-Token")
            .when_middleware("RequireCSRF")
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("ActorAuth", "Authorization")
            .when_middleware("RequireActor")
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].security, vec!["ActiveSchoolAuth"]);
        assert_eq!(
            ir.operations[1].security,
            vec!["ActiveSchoolAuth", "CSRFAuth"]
        );
        assert_eq!(ir.operations[2].security, vec!["ActorAuth"]);
    }

    #[test]
    fn apply_security_accepts_reusable_composed_operation_selectors() {
        let active_school = OperationSelector::any([
            OperationSelector::path_prefix("/v1/schools/active/"),
            OperationSelector::path_prefix("/v1/import-jobs/"),
        ]);
        let mutating = OperationSelector::methods(["POST", "PUT", "PATCH", "DELETE"]);

        let mut ir = ApiGraph {
            operations: vec![
                Operation {
                    id: "readActive".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/schools/active/files".to_string(),
                    handler: "readActive".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "createActive".to_string(),
                    method: "POST".to_string(),
                    path: "/v1/schools/active/files".to_string(),
                    handler: "createActive".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "deleteGovernance".to_string(),
                    method: "DELETE".to_string(),
                    path: "/v1/governance/legal-holds/book/1".to_string(),
                    handler: "deleteGovernance".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "readGovernance".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/governance/legal-holds/book/1".to_string(),
                    handler: "readGovernance".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        ApplySecurity::api_key("ActiveSchoolAuth", "X-Plint-School-Id")
            .when(active_school.clone())
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("CSRFAuth", "X-CSRF-Token")
            .when(OperationSelector::all([
                OperationSelector::any([
                    active_school,
                    OperationSelector::path_prefix("/v1/governance/"),
                ]),
                mutating,
            ]))
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].security, vec!["ActiveSchoolAuth"]);
        assert_eq!(
            ir.operations[1].security,
            vec!["ActiveSchoolAuth", "CSRFAuth"]
        );
        assert_eq!(ir.operations[2].security, vec!["CSRFAuth"]);
        assert!(ir.operations[3].security.is_empty());
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn api_overrides_can_patch_query_body_binary_and_sse_facts() {
        let mut ir = ApiGraph {
            schemas: vec![
                Schema {
                    id: "app.MarkReadRequest".to_string(),
                    name: "MarkReadRequest".to_string(),
                    body: Type::Object(vec![]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
                Schema {
                    id: "app.SyncStreamEnvelope".to_string(),
                    name: "SyncStreamEnvelope".to_string(),
                    body: Type::Object(vec![]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
            ],
            operations: vec![
                Operation {
                    id: "markRead".to_string(),
                    method: "PATCH".to_string(),
                    path: "/conversations/{conversationId}/read".to_string(),
                    handler: "markRead".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: Some(SchemaRef {
                        ref_id: "app.MarkReadRequest".to_string(),
                    }),
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "download".to_string(),
                    method: "GET".to_string(),
                    path: "/files/{fileId}/download".to_string(),
                    handler: "download".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "stream".to_string(),
                    method: "GET".to_string(),
                    path: "/sync/stream".to_string(),
                    handler: "stream".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        ApiOverrides::new()
            .query_param(
                "PATCH",
                "/conversations/{conversationId}/read",
                QueryParam::new("limit")
                    .integer()
                    .optional()
                    .default_number("5"),
            )
            .request_body("PATCH", "/conversations/{conversationId}/read")
            .optional()
            .binary_response("GET", "/files/{fileId}/download", 200)
            .sse_response("GET", "/sync/stream")
            .event_schema("SyncStreamEnvelope")
            .apply(&mut ir, &cx())
            .unwrap();

        assert!(!ir.operations[0].request_body_required);
        assert_eq!(ir.operations[0].params[0].name, "limit");
        assert_eq!(ir.operations[1].responses[0].body_kind, "binary");
        assert_eq!(ir.operations[2].responses[0].body_kind, "sse");

        let mut out = Artifacts::new();
        OpenApi31::new()
            .to("openapi.yaml")
            .generate(&ir, &mut out, &cx())
            .unwrap();
        let yaml = out.files()[0].text.as_str();
        assert!(yaml.contains("required: false"), "{yaml}");
        assert!(yaml.contains("default: 5"), "{yaml}");
        assert!(yaml.contains("format: binary"), "{yaml}");
        assert!(yaml.contains("text/event-stream"), "{yaml}");
        assert!(
            yaml.contains("'#/components/schemas/SyncStreamEnvelope'"),
            "{yaml}"
        );
    }

    #[test]
    fn query_param_date_lowers_to_openapi_date_and_cleans_untyped_diagnostic() {
        let mut ir = ApiGraph {
            operations: vec![Operation {
                id: "listSchedule".to_string(),
                method: "GET".to_string(),
                path: "/schedule/week".to_string(),
                handler: "listSchedule".to_string(),
                group: None,
                middleware: Vec::new(),
                params: vec![],
                request_body: None,
                request_body_required: true,
                request_body_content_type: None,
                responses: vec![],
                security: Vec::new(),
                security_overrides_global: false,
                provenance: span(),
            }],
            diagnostics: vec![Diagnostic {
                severity: "WARN".to_string(),
                message:
                    "untyped query param 'startDate' on GET /schedule/week: defaulting to string"
                        .to_string(),
                file: "handlers.go".to_string(),
                line: 12,
            }],
            ..ApiGraph::default()
        };

        ApiOverrides::new()
            .query_param(
                "GET",
                "/schedule/week",
                QueryParam::new("startDate").date().required(),
            )
            .apply(&mut ir, &cx())
            .unwrap();

        assert!(ir.diagnostics.is_empty());
        let mut out = Artifacts::new();
        OpenApi31::new()
            .to("openapi.yaml")
            .generate(&ir, &mut out, &cx())
            .unwrap();
        let yaml = out.files()[0].text.as_str();
        assert!(yaml.contains("name: startDate"), "{yaml}");
        assert!(yaml.contains("format: date"), "{yaml}");
        assert!(!yaml.contains("format: date-time"), "{yaml}");
    }

    #[test]
    fn binary_response_override_cleans_resolved_octet_stream_diagnostic_only_for_that_operation() {
        let mut ir = ApiGraph {
            operations: vec![Operation {
                id: "downloadFile".to_string(),
                method: "GET".to_string(),
                path: "/files/{fileId}/download".to_string(),
                handler: "downloadFile".to_string(),
                group: None,
                middleware: Vec::new(),
                params: vec![],
                request_body: None,
                request_body_required: true,
                request_body_content_type: None,
                responses: vec![],
                security: Vec::new(),
                security_overrides_global: false,
                provenance: SourceSpan {
                    file: "handlers.go".to_string(),
                    start_line: 10,
                    end_line: 20,
                },
            }],
            diagnostics: vec![
                Diagnostic {
                    severity: "WARN".to_string(),
                    message: "unsupported binary response pattern on GET /files/{fileId}/download: defaulting to application/octet-stream".to_string(),
                    file: "handlers.go".to_string(),
                    line: 12,
                },
                Diagnostic {
                    severity: "WARN".to_string(),
                    message: "unsupported binary response pattern on GET /other: defaulting to application/octet-stream".to_string(),
                    file: "handlers.go".to_string(),
                    line: 30,
                },
            ],
            ..ApiGraph::default()
        };

        ApiOverrides::new()
            .binary_response("GET", "/files/{fileId}/download", 200)
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.diagnostics.len(), 1);
        assert!(ir.diagnostics[0].message.contains("/other"));
    }

    #[test]
    fn sdk_operation_aliases_patch_group_and_sdk_operation_name() {
        let mut ir = ApiGraph {
            operations: vec![
                Operation {
                    id: "download".to_string(),
                    method: "GET".to_string(),
                    path: "/v1/files/{fileId}/download".to_string(),
                    handler: "download".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "search".to_string(),
                    method: "POST".to_string(),
                    path: "/v1/coursework/search".to_string(),
                    handler: "search".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        SdkOperationAliases::new()
            .operation("GET", "/v1/files/{fileId}/download")
            .tag("files")
            .name("downloadSchoolFile")
            .operation("POST", "/v1/coursework/search")
            .tag("coursework")
            .name("searchCoursework")
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].group.as_deref(), Some("files"));
        assert_eq!(ir.operations[0].id, "downloadSchoolFile");
        assert_eq!(ir.operations[0].handler, "downloadSchoolFile");
        assert_eq!(ir.operations[1].group.as_deref(), Some("coursework"));
        assert_eq!(ir.operations[1].id, "searchCoursework");
        assert_eq!(ir.operations[1].handler, "searchCoursework");
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn api_overrides_can_patch_json_and_default_error_responses() {
        let mut ir = ApiGraph {
            schemas: vec![
                Schema {
                    id: "app.Book".to_string(),
                    name: "Book".to_string(),
                    body: Type::Object(vec![]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
                Schema {
                    id: "app.ErrorResponse".to_string(),
                    name: "ErrorResponse".to_string(),
                    body: Type::Object(vec![Field {
                        json_name: "message".to_string(),
                        required: true,
                        optional: false,
                        nullable: false,
                        schema: Type::Primitive(Prim::String),
                        description: None,
                        example: None,
                        meta: FieldMeta::default(),
                    }]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
            ],
            operations: vec![
                Operation {
                    id: "getBook".to_string(),
                    method: "GET".to_string(),
                    path: "/books/current".to_string(),
                    handler: "getBook".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![Response {
                        status: 200,
                        body: Some(SchemaRef {
                            ref_id: "app.Book".to_string(),
                        }),
                        body_kind: "json".to_string(),
                        content_type: None,
                        content_types: vec!["application/json".to_string()],
                    }],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "createBook".to_string(),
                    method: "POST".to_string(),
                    path: "/books".to_string(),
                    handler: "createBook".to_string(),
                    group: None,
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![Response {
                        status: 400,
                        body: None,
                        body_kind: "empty".to_string(),
                        content_type: None,
                        content_types: Vec::new(),
                    }],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        ApiOverrides::new()
            .json_response("GET", "/books/current", 404, "ErrorResponse")
            .default_error_response(400, "ErrorResponse")
            .apply(&mut ir, &cx())
            .unwrap();

        let get_book = &ir.operations[0];
        assert!(
            get_book
                .responses
                .iter()
                .any(|response| response.status == 400
                    && response
                        .body
                        .as_ref()
                        .is_some_and(|body| body.ref_id == "app.ErrorResponse")
                    && response.content_types.len() == 1
                    && response.content_types[0] == "application/json"),
            "{get_book:?}"
        );
        assert!(
            get_book
                .responses
                .iter()
                .any(|response| response.status == 404
                    && response
                        .body
                        .as_ref()
                        .is_some_and(|body| body.ref_id == "app.ErrorResponse")
                    && response.content_types.len() == 1
                    && response.content_types[0] == "application/json"),
            "{get_book:?}"
        );
        assert!(
            ir.operations[1]
                .responses
                .iter()
                .any(|response| response.status == 400 && response.body.is_none()),
            "default response override must not replace explicit operation responses"
        );

        let mut out = Artifacts::new();
        GoSdk::new()
            .module("example.com/bookclient")
            .to("sdk")
            .generate(&ir, &mut out, &cx())
            .unwrap();
        let operations = out
            .files()
            .iter()
            .find(|artifact| artifact.path == "sdk/operations.go")
            .unwrap()
            .text
            .as_str();
        assert!(
            operations.contains("var apiErr ErrorResponse"),
            "Go SDK should decode non-2xx graph responses into ErrorResponse:\n{operations}"
        );
    }

    #[test]
    fn api_overrides_rejects_2xx_default_error_response() {
        let mut ir = ApiGraph::default();
        let err = ApiOverrides::new()
            .default_error_response(200, "ErrorResponse")
            .apply(&mut ir, &cx())
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("default error response status 200 is a 2xx status"),
            "{err}"
        );
    }

    #[test]
    fn api_overrides_rejects_ambiguous_response_schema_name() {
        let mut ir = ApiGraph {
            schemas: vec![
                Schema {
                    id: "public.ErrorResponse".to_string(),
                    name: "ErrorResponse".to_string(),
                    body: Type::Object(vec![]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
                Schema {
                    id: "admin.ErrorResponse".to_string(),
                    name: "ErrorResponse".to_string(),
                    body: Type::Object(vec![]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
            ],
            operations: vec![Operation {
                id: "getBook".to_string(),
                method: "GET".to_string(),
                path: "/books/current".to_string(),
                handler: "getBook".to_string(),
                group: None,
                middleware: Vec::new(),
                params: vec![],
                request_body: None,
                request_body_required: true,
                request_body_content_type: None,
                responses: Vec::new(),
                security: Vec::new(),
                security_overrides_global: false,
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        let err = ApiOverrides::new()
            .json_response("GET", "/books/current", 400, "ErrorResponse")
            .apply(&mut ir, &cx())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("response override schema 'ErrorResponse' matches 2 schemas"),
            "{err}"
        );

        ApiOverrides::new()
            .json_response("GET", "/books/current", 400, "admin.ErrorResponse")
            .apply(&mut ir, &cx())
            .unwrap();
        assert!(
            ir.operations[0].responses.iter().any(|response| response
                .body
                .as_ref()
                .is_some_and(|body| body.ref_id == "admin.ErrorResponse")),
            "{ir:?}"
        );
    }

    #[test]
    fn group_operations_overrides_matches_and_preserves_source_groups() {
        let mut ir = ApiGraph {
            operations: vec![
                Operation {
                    id: "login".to_string(),
                    method: "POST".to_string(),
                    path: "/auth/login".to_string(),
                    handler: "login".to_string(),
                    group: Some("auth".to_string()),
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
                Operation {
                    id: "download".to_string(),
                    method: "GET".to_string(),
                    path: "/files/{fileId}".to_string(),
                    handler: "download".to_string(),
                    group: Some("files".to_string()),
                    middleware: Vec::new(),
                    params: vec![],
                    request_body: None,
                    request_body_required: true,
                    request_body_content_type: None,
                    responses: vec![],
                    security: Vec::new(),
                    security_overrides_global: false,
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        GroupOperations::new()
            .by_operation("login", "session")
            .by_path_prefix("/missing", "unused")
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].group.as_deref(), Some("session"));
        assert_eq!(ir.operations[1].group.as_deref(), Some("files"));
    }

    #[test]
    fn set_base_path_rejects_relative_or_url_like_paths() {
        let mut ir = ApiGraph::default();
        let err = SetBasePath::new("books").apply(&mut ir, &cx()).unwrap_err();
        assert!(err.to_string().contains("must be empty"), "{err}");

        let err = SetBasePath::new("/books?draft=true")
            .apply(&mut ir, &cx())
            .unwrap_err();
        assert!(err.to_string().contains("clean path prefix"), "{err}");
    }

    #[test]
    fn transform_sets_operation_success_response_by_route() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.CreateBookResponse".to_string(),
                name: "CreateBookResponse".to_string(),
                body: Type::Object(vec![]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            operations: vec![Operation {
                id: "createBook".to_string(),
                method: "POST".to_string(),
                path: "/books".to_string(),
                handler: "createBook".to_string(),
                group: None,
                middleware: Vec::new(),
                params: vec![],
                request_body: None,
                request_body_required: true,
                request_body_content_type: None,
                responses: vec![
                    crate::graph::Response {
                        status: 200,
                        body: None,
                        body_kind: "empty".to_string(),
                        content_type: None,
                        content_types: Vec::new(),
                    },
                    crate::graph::Response {
                        status: 404,
                        body: None,
                        body_kind: "empty".to_string(),
                        content_type: None,
                        content_types: Vec::new(),
                    },
                ],
                security: Vec::new(),
                security_overrides_global: false,
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        SetOperationSuccessResponse::for_route("post", "/books", "CreateBookResponse")
            .status(201)
            .apply(&mut ir, &cx())
            .unwrap();

        assert_eq!(ir.operations[0].responses.len(), 2);
        assert_eq!(
            ir.operations[0]
                .responses
                .iter()
                .map(|response| response.status)
                .collect::<Vec<_>>(),
            vec![201, 404]
        );
        assert_eq!(
            ir.operations[0].responses[0]
                .body
                .as_ref()
                .map(|body| body.ref_id.as_str()),
            Some("app.CreateBookResponse")
        );
    }

    #[test]
    fn transform_rejects_non_success_status_override() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.CreateBookResponse".to_string(),
                name: "CreateBookResponse".to_string(),
                body: Type::Object(vec![]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            operations: vec![Operation {
                id: "createBook".to_string(),
                method: "POST".to_string(),
                path: "/books".to_string(),
                handler: "createBook".to_string(),
                group: None,
                middleware: Vec::new(),
                params: vec![],
                request_body: None,
                request_body_required: true,
                request_body_content_type: None,
                responses: vec![],
                security: Vec::new(),
                security_overrides_global: false,
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        let err = SetOperationSuccessResponse::for_operation("createBook", "CreateBookResponse")
            .status(404)
            .apply(&mut ir, &cx())
            .unwrap_err();

        assert!(err.to_string().contains("is not a 2xx status"), "{err}");
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
                    meta: FieldMeta::default(),
                }]),
                enum_source_order: Vec::new(),
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
    fn api_overrides_force_field_presence_for_openapi_and_typescript() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.User".to_string(),
                name: "User".to_string(),
                body: Type::Object(vec![
                    Field {
                        json_name: "settings".to_string(),
                        required: false,
                        optional: true,
                        nullable: false,
                        schema: Type::Primitive(Prim::String),
                        description: None,
                        example: None,
                        meta: FieldMeta::default(),
                    },
                    Field {
                        json_name: "stable".to_string(),
                        required: true,
                        optional: false,
                        nullable: false,
                        schema: Type::Primitive(Prim::String),
                        description: None,
                        example: None,
                        meta: FieldMeta::default(),
                    },
                ]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        ApiOverrides::new()
            .force_required("User", "settings")
            .force_optional("User", "stable")
            .apply(&mut ir, &cx())
            .unwrap();

        let mut openapi_out = Artifacts::new();
        OpenApi31::new()
            .to("openapi.yaml")
            .generate(&ir, &mut openapi_out, &cx())
            .unwrap();
        let yaml = openapi_out.files()[0].text.as_str();
        assert!(yaml.contains("required: [settings]"), "{yaml}");

        let mut ts_out = Artifacts::new();
        TsSdk::new()
            .module("example.com/user/sdk")
            .to("sdk")
            .profile(SdkProfile::openapi_generator_compat())
            .generate(&ir, &mut ts_out, &cx())
            .unwrap();
        let models = ts_out
            .files()
            .iter()
            .find(|artifact| artifact.path == "sdk/models.ts")
            .unwrap()
            .text
            .as_str();
        assert!(models.contains("settings: string;"), "{models}");
        assert!(models.contains("stable?: string;"), "{models}");
    }

    #[test]
    fn api_overrides_unknown_field_is_a_config_error() {
        let mut ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.User".to_string(),
                name: "User".to_string(),
                body: Type::Object(vec![]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        let err = ApiOverrides::new()
            .force_required("User", "missing")
            .apply(&mut ir, &cx())
            .unwrap_err();

        assert!(err.to_string().contains("did not find field"), "{err}");
    }

    #[test]
    fn api_overrides_unknown_schema_is_a_config_error() {
        let mut ir = ApiGraph::default();

        let err = ApiOverrides::new()
            .force_required("User", "id")
            .apply(&mut ir, &cx())
            .unwrap_err();

        assert!(
            err.to_string().contains("does not match any graph schema"),
            "{err}"
        );
    }

    #[test]
    fn ts_sdk_compatibility_alias_uses_openapi_generator_profile() {
        let ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.User".to_string(),
                name: "User".to_string(),
                body: Type::Object(vec![Field {
                    json_name: "id".to_string(),
                    required: true,
                    optional: false,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                }]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            ..ApiGraph::default()
        };
        let mut out = Artifacts::new();

        TsSdk::new()
            .module("example.com/user/sdk")
            .to("sdk")
            .compatibility(TsCompatibility::OpenApiGenerator)
            .generate(&ir, &mut out, &cx())
            .unwrap();

        assert!(
            out.files()
                .iter()
                .any(|artifact| artifact.path == "sdk/api.ts"),
            "compatibility alias should emit OpenAPI Generator API surface"
        );
    }

    #[test]
    fn enum_order_transform_supports_source_and_explicit_inline_overrides() {
        let mut ir = ApiGraph {
            schemas: vec![
                Schema {
                    id: "app.Direction".to_string(),
                    name: "Direction".to_string(),
                    body: Type::Enum(vec!["gte".to_string(), "lte".to_string()]),
                    enum_source_order: vec!["lte".to_string(), "gte".to_string()],
                    provenance: span(),
                },
                Schema {
                    id: "app.Filter".to_string(),
                    name: "Filter".to_string(),
                    body: Type::Object(vec![Field {
                        json_name: "sort".to_string(),
                        required: false,
                        optional: true,
                        nullable: false,
                        schema: Type::Enum(vec!["asc".to_string(), "desc".to_string()]),
                        description: None,
                        example: None,
                        meta: FieldMeta::default(),
                    }]),
                    enum_source_order: Vec::new(),
                    provenance: span(),
                },
            ],
            ..ApiGraph::default()
        };

        SetEnumOrder::source().apply(&mut ir, &cx()).unwrap();
        let Type::Enum(direction) = &ir.schemas[0].body else {
            panic!("expected named enum");
        };
        assert_eq!(direction, &vec!["lte".to_string(), "gte".to_string()]);

        SetEnumOrder::new(EnumOrder::Explicit(vec![(
            "Filter.sort".to_string(),
            vec!["desc".to_string(), "asc".to_string()],
        )]))
        .apply(&mut ir, &cx())
        .unwrap();
        let Type::Object(fields) = &ir.schemas[1].body else {
            panic!("expected object");
        };
        let Type::Enum(sort) = &fields[0].schema else {
            panic!("expected inline enum");
        };
        assert_eq!(sort, &vec!["desc".to_string(), "asc".to_string()]);
    }

    #[test]
    fn openapi_helpers_patch_typed_doc_before_yaml_and_json_serialization() {
        let ir = ApiGraph {
            schemas: vec![Schema {
                id: "app.CreateBookInput".to_string(),
                name: "CreateBookInput".to_string(),
                body: Type::Object(vec![Field {
                    json_name: "title".to_string(),
                    required: true,
                    optional: false,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                }]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            ..ApiGraph::default()
        };

        let aliases = OpenApiSchemaAliases::new().alias("CreateBookInput", "CreateBookRequest");
        let patch = OpenApiSchemaPatch::new("CreateBookInput").field(
            OpenApiFieldPatch::new("title")
                .min_length(3)
                .default_string("Untitled")
                .extension_string("x-gnr8-render", "input"),
        );

        let mut yaml_out = Artifacts::new();
        OpenApi31::new()
            .to("openapi.yaml")
            .schema_aliases(aliases.clone())
            .schema_patch(patch.clone())
            .generate(&ir, &mut yaml_out, &cx())
            .unwrap();
        let yaml = yaml_out
            .files()
            .iter()
            .find(|artifact| artifact.path == "openapi.yaml")
            .unwrap()
            .text
            .as_str();
        assert!(yaml.contains("CreateBookRequest:"), "{yaml}");
        assert!(
            yaml.contains("$ref: '#/components/schemas/CreateBookInput'"),
            "{yaml}"
        );
        assert!(yaml.contains("minLength: 3"), "{yaml}");
        assert!(yaml.contains("default: Untitled"), "{yaml}");
        assert!(yaml.contains("x-gnr8-render: input"), "{yaml}");

        let mut json_out = Artifacts::new();
        OpenApi31Json::new()
            .to("openapi.json")
            .schema_aliases(aliases)
            .schema_patch(patch)
            .generate(&ir, &mut json_out, &cx())
            .unwrap();
        let json = json_out
            .files()
            .iter()
            .find(|artifact| artifact.path == "openapi.json")
            .unwrap()
            .text
            .as_str();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            value["components"]["schemas"]["CreateBookRequest"]["$ref"],
            "#/components/schemas/CreateBookInput"
        );
        let title = &value["components"]["schemas"]["CreateBookInput"]["properties"]["title"];
        assert_eq!(title["minLength"], 3);
        assert_eq!(title["default"], "Untitled");
        assert_eq!(title["x-gnr8-render"], "input");
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
    fn static_files_target_reports_cache_inputs() {
        let root = temp_project("cache-inputs");
        std::fs::create_dir_all(root.join("static/runtime")).unwrap();
        std::fs::write(root.join("static/runtime/__init__.py"), "ROOT\n").unwrap();
        std::fs::write(root.join("static/README.md"), "README\n").unwrap();

        let files = StaticFiles::new()
            .from("static")
            .to("pkg")
            .include(["runtime/**", "README.md"])
            .cache_input_files(&Cx::new(&root))
            .unwrap();
        let rels: Vec<_> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert_eq!(rels, vec!["static/README.md", "static/runtime/__init__.py"]);
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
    fn static_files_target_rejects_unsafe_source_and_output_dirs() {
        let mut out = Artifacts::new();
        let err = StaticFiles::new()
            .from("../static")
            .to("pkg")
            .include(["README.md"])
            .generate(&ApiGraph::default(), &mut out, &cx())
            .unwrap_err();
        assert!(
            err.to_string().contains("invalid StaticFiles source dir"),
            "{err}"
        );

        let err = StaticFiles::new()
            .from("static")
            .to("/pkg")
            .include(["README.md"])
            .generate(&ApiGraph::default(), &mut out, &cx())
            .unwrap_err();
        assert!(
            err.to_string().contains("invalid StaticFiles output dir"),
            "{err}"
        );
    }

    #[test]
    fn gosdk_target_emits_go_mod_under_output_dir() {
        let ir = ApiGraph::default();
        let target = GoSdk::new()
            .module_path("example.com/bookstore/sdk")
            .go_version("1.26.4")
            .to("generated/sdk-go");

        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();

        let go_mod = out
            .files()
            .iter()
            .find(|file| file.path == "generated/sdk-go/go.mod")
            .expect("GoSdk must emit go.mod so pruned SDK dirs remain buildable");
        assert_eq!(
            go_mod.text,
            "module example.com/bookstore/sdk\n\ngo 1.26.4\n"
        );
    }

    #[test]
    fn gosdk_source_only_omits_docs_and_package_metadata() {
        let ir = ApiGraph::default();
        let target = GoSdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-go")
            .source_only();

        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();

        for path in [
            "generated/sdk-go/go.mod",
            "generated/sdk-go/README.md",
            "generated/sdk-go/reference.md",
        ] {
            assert!(
                !out.files().iter().any(|file| file.path == path),
                "source_only should not emit {path}"
            );
        }
    }

    #[test]
    fn sdk_docs_openapi_generator_compat_emits_per_symbol_markdown() {
        let mut ir = ApiGraph {
            title: "Bookstore".to_string(),
            base_path: "/api".to_string(),
            ..ApiGraph::default()
        };
        ir.schemas.push(Schema {
            id: "Book".to_string(),
            name: "Book".to_string(),
            body: Type::Object(vec![Field {
                json_name: "title".to_string(),
                required: true,
                optional: false,
                nullable: false,
                schema: Type::Primitive(Prim::String),
                description: None,
                example: None,
                meta: FieldMeta::default(),
            }]),
            enum_source_order: Vec::new(),
            provenance: span(),
        });
        ir.schemas.push(Schema {
            id: "CreateBookRequest".to_string(),
            name: "CreateBookRequest".to_string(),
            body: Type::Object(Vec::new()),
            enum_source_order: Vec::new(),
            provenance: span(),
        });
        ir.operations.push(Operation {
            id: "createBook".to_string(),
            method: "POST".to_string(),
            path: "/books".to_string(),
            handler: "createBook".to_string(),
            group: Some("books".to_string()),
            middleware: Vec::new(),
            params: Vec::new(),
            request_body: Some(SchemaRef {
                ref_id: "CreateBookRequest".to_string(),
            }),
            request_body_required: true,
            request_body_content_type: None,
            responses: vec![Response {
                status: 201,
                body: Some(SchemaRef {
                    ref_id: "Book".to_string(),
                }),
                body_kind: "json".to_string(),
                content_type: None,
                content_types: Vec::new(),
            }],
            security: Vec::new(),
            security_overrides_global: false,
            provenance: span(),
        });

        let mut out = Artifacts::new();
        TsSdk::new()
            .module("@example/bookstore")
            .to("generated/sdk-ts")
            .docs(SdkDocs::both())
            .generate(&ir, &mut out, &cx())
            .unwrap();

        let paths = out
            .files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "generated/sdk-ts/README.md",
            "generated/sdk-ts/reference.md",
            "generated/sdk-ts/docs/README.md",
            "generated/sdk-ts/docs/BooksApi.md",
            "generated/sdk-ts/docs/Book.md",
            "generated/sdk-ts/docs/CreateBookRequest.md",
        ] {
            assert!(
                paths.contains(&expected),
                "expected {expected} in generated paths: {paths:?}"
            );
        }
        let readme = out
            .files()
            .iter()
            .find(|file| file.path == "generated/sdk-ts/README.md")
            .unwrap();
        assert!(readme.text.contains("[SDK reference](reference.md)"));
        assert!(readme
            .text
            .contains("[OpenAPI Generator-compatible docs](docs/README.md)"));
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
    fn pysdk_source_only_omits_docs() {
        let ir = ApiGraph::default();
        let target = PySdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-py")
            .source_only();

        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();

        for path in [
            "generated/sdk-py/README.md",
            "generated/sdk-py/reference.md",
        ] {
            assert!(
                !out.files().iter().any(|file| file.path == path),
                "source_only should not emit {path}"
            );
        }
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
    fn tssdk_fetch_compat_emits_package_metadata_and_source_only_can_disable_it() {
        let ir = ApiGraph::default();

        let mut compat_out = Artifacts::new();
        TsSdk::new()
            .module("@example/bookstore-sdk")
            .to("generated/sdk-ts")
            .profile(SdkProfile::typescript_fetch_compat())
            .generate(&ir, &mut compat_out, &cx())
            .unwrap();
        let package_json = compat_out
            .files()
            .iter()
            .find(|file| file.path == "generated/sdk-ts/package.json")
            .expect("typescript-fetch compat should emit package metadata by default");
        assert!(package_json.text.contains("\"types\": \"./index.d.ts\""));

        let mut axios_out = Artifacts::new();
        TsSdk::new()
            .module("@example/bookstore-sdk")
            .to("generated/sdk-ts")
            .profile(SdkProfile::typescript_axios_compat())
            .generate(&ir, &mut axios_out, &cx())
            .unwrap();
        let axios_package_json = axios_out
            .files()
            .iter()
            .find(|file| file.path == "generated/sdk-ts/package.json")
            .expect("typescript-axios compat should emit package metadata by default");
        assert!(axios_package_json.text.contains("\"axios\""));

        let mut source_only_out = Artifacts::new();
        TsSdk::new()
            .module("@example/bookstore-sdk")
            .to("generated/sdk-ts")
            .profile(SdkProfile::typescript_fetch_compat())
            .source_only()
            .generate(&ir, &mut source_only_out, &cx())
            .unwrap();

        for path in [
            "generated/sdk-ts/package.json",
            "generated/sdk-ts/README.md",
            "generated/sdk-ts/reference.md",
        ] {
            assert!(
                !source_only_out.files().iter().any(|file| file.path == path),
                "source_only should not emit {path}"
            );
        }
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
    fn go_gin_supports_separate_route_and_schema_packages() {
        let project = temp_project("go-gin-scopes");
        std::fs::create_dir_all(project.join("ginstub")).unwrap();
        std::fs::create_dir_all(project.join("internal/api")).unwrap();
        std::fs::create_dir_all(project.join("internal/dto")).unwrap();
        std::fs::write(
            project.join("go.mod"),
            r"module example.com/scoped

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
",
        )
        .unwrap();
        std::fs::write(
            project.join("ginstub/go.mod"),
            "module github.com/gin-gonic/gin\n\ngo 1.22\n",
        )
        .unwrap();
        std::fs::write(
            project.join("ginstub/gin.go"),
            r"package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBindJSON(any) error { return nil }
func (c *Context) JSON(int, any) {}
",
        )
        .unwrap();
        std::fs::write(
            project.join("internal/dto/models.go"),
            r#"package dto

type CreateRequest struct {
	Name string `json:"name"`
}

type CreateResponse struct {
	ID string `json:"id"`
}
"#,
        )
        .unwrap();
        std::fs::write(
            project.join("internal/api/handlers.go"),
            r#"package api

import (
	"example.com/scoped/internal/dto"
	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }

func (s Server) Register() {
	s.R.POST("/items", s.create)
}

func (s Server) create(c *gin.Context) {
	var input dto.CreateRequest
	_ = c.ShouldBindJSON(&input)
	c.JSON(200, dto.CreateResponse{})
}
"#,
        )
        .unwrap();

        let graph = GoGin::new()
            .inputs(["."])
            .route_packages(["./internal/api"])
            .schema_packages(["./internal/dto"])
            .load(&Cx::new(project))
            .unwrap();

        assert_eq!(graph.operations.len(), 1);
        let op = &graph.operations[0];
        assert_eq!(op.path, "/items");
        assert_eq!(
            op.request_body.as_ref().map(|body| body.ref_id.as_str()),
            Some("internal/dto.CreateRequest")
        );
        assert!(graph
            .schemas
            .iter()
            .any(|schema| schema.id == "internal/dto.CreateResponse"));
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

    #[test]
    fn format_command_rewrites_artifacts_before_host_ownership() {
        let mut out = Artifacts::new();
        out.write("generated/openapi.json", "{\"openapi\":\"3.1.0\"}\n");
        FormatCommand::new("sh")
            .args([
                "-c",
                "printf '{\"openapi\":\"3.1.0\",\"formatted\":true}\\n' > generated/openapi.json",
            ])
            .run(&mut out, &cx())
            .unwrap();

        let artifact = out
            .files()
            .iter()
            .find(|artifact| artifact.path == "generated/openapi.json")
            .unwrap();
        assert_eq!(
            artifact.text,
            "{\"openapi\":\"3.1.0\",\"formatted\":true}\n"
        );
    }

    #[test]
    fn format_command_rejects_undeclared_artifacts() {
        let mut out = Artifacts::new();
        out.write("generated/openapi.json", "{}\n");
        let err = FormatCommand::new("sh")
            .args([
                "-c",
                "mkdir -p generated && printf x > generated/extra.json",
            ])
            .run(&mut out, &cx())
            .unwrap_err();
        assert!(err.to_string().contains("undeclared artifact"), "{err}");
    }
}
