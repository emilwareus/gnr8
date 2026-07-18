//! Exact semantic compatibility checking for Swagger 2 and OpenAPI 3 documents.
//!
//! Compatibility uses a dedicated canonical contract rather than [`crate::graph::ApiGraph`]. The
//! graph intentionally models only facts that gnr8 can currently generate, while a compatibility
//! oracle must retain every compared fact or it could report a false match.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::openapi_source::{detect_version, parse_json_or_yaml, SpecVersion};
use crate::CoreError;

/// The exact-comparison policy supported by OpenAPI compatibility checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenApiCompatibilityPolicy {
    /// Require consumer-visible semantic equality after Swagger/OpenAPI normalization.
    Exact,
}

/// One deterministic, machine-readable contract difference.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OpenApiDifference {
    /// Stable dotted identity for this kind of difference.
    pub code: String,
    /// Precise canonical contract location expressed as a JSON pointer.
    pub location: String,
    /// HTTP operation (`METHOD /path`) when the difference belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Parameter, schema, media-type, or security-scheme name when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Response status when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Baseline value; absent when the value was added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<Value>,
    /// Candidate value; absent when the value was removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<Value>,
}

/// Result of comparing two Swagger/OpenAPI documents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OpenApiCompatibilityReport {
    /// Whether no semantic differences were found.
    pub compatible: bool,
    /// Detected version of the baseline document.
    pub old_version: String,
    /// Detected version of the candidate document.
    pub new_version: String,
    /// Differences sorted by location, code, operation, name, and status.
    pub differences: Vec<OpenApiDifference>,
}

/// Compare two Swagger 2/OpenAPI 3 files under the requested policy.
///
/// # Errors
///
/// Returns a typed error when either file cannot be read, parsed, or normalized without ambiguity.
pub fn compare_openapi_files(
    old: impl AsRef<Path>,
    new: impl AsRef<Path>,
    policy: OpenApiCompatibilityPolicy,
) -> Result<OpenApiCompatibilityReport, CoreError> {
    let old = ParsedDocument::load(old.as_ref())?;
    let new = ParsedDocument::load(new.as_ref())?;
    compare_documents(old, new, policy)
}

fn compare_documents(
    old: ParsedDocument,
    new: ParsedDocument,
    policy: OpenApiCompatibilityPolicy,
) -> Result<OpenApiCompatibilityReport, CoreError> {
    match policy {
        OpenApiCompatibilityPolicy::Exact => {
            let old_version = spec_version_label(old.version).to_string();
            let new_version = spec_version_label(new.version).to_string();
            let old = Canonicalizer::new(old).canonicalize()?;
            let new = Canonicalizer::new(new).canonicalize()?;
            let mut differences = diff_documents(&old, &new);
            differences.sort_by(|a, b| {
                a.location
                    .cmp(&b.location)
                    .then_with(|| a.code.cmp(&b.code))
                    .then_with(|| a.operation.cmp(&b.operation))
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.status.cmp(&b.status))
            });
            Ok(OpenApiCompatibilityReport {
                compatible: differences.is_empty(),
                old_version,
                new_version,
                differences,
            })
        }
    }
}

fn spec_version_label(version: SpecVersion) -> &'static str {
    match version {
        SpecVersion::Swagger2 => "swagger-2.0",
        SpecVersion::OpenApi30 => "openapi-3.0",
        SpecVersion::OpenApi31 => "openapi-3.1",
    }
}

struct ParsedDocument {
    path: PathBuf,
    root: Value,
    version: SpecVersion,
}

impl ParsedDocument {
    fn load(path: &Path) -> Result<Self, CoreError> {
        let text = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            message: format!(
                "failed to read OpenAPI compatibility input '{}': {source}",
                path.display()
            ),
        })?;
        let root = parse_json_or_yaml(&text, path)?;
        let version = detect_version(&root, path)?;
        Ok(Self {
            path: path.to_path_buf(),
            root,
            version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct CanonicalDocument {
    metadata: BTreeMap<String, Value>,
    servers: Vec<Value>,
    security: SecurityRequirements,
    security_schemes: BTreeMap<String, Value>,
    path_items: BTreeMap<String, Value>,
    operations: BTreeMap<String, CanonicalOperation>,
    webhook_items: BTreeMap<String, Value>,
    webhooks: BTreeMap<String, CanonicalOperation>,
    schemas: BTreeMap<String, Value>,
    components: BTreeMap<String, Value>,
    external_documents: BTreeMap<String, Value>,
}

type SecurityRequirements = Vec<BTreeMap<String, Vec<String>>>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct CanonicalOperation {
    operation_id: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    tags: Vec<String>,
    deprecated: bool,
    external_docs: Option<Value>,
    servers: Vec<Value>,
    parameters: BTreeMap<String, CanonicalParameter>,
    request_body: Option<CanonicalRequestBody>,
    responses: BTreeMap<String, CanonicalResponse>,
    security: SecurityRequirements,
    callbacks: Option<Value>,
    extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "OpenAPI parameters expose independent required, deprecated, explode, allowReserved, and allowEmptyValue flags"
)]
struct CanonicalParameter {
    name: String,
    location: String,
    description: Option<String>,
    required: bool,
    deprecated: bool,
    style: String,
    explode: bool,
    allow_reserved: bool,
    allow_empty_value: bool,
    schema: Value,
    examples: Option<Value>,
    extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct CanonicalRequestBody {
    description: Option<String>,
    required: bool,
    content: BTreeMap<String, CanonicalMedia>,
    extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct CanonicalResponse {
    description: Option<String>,
    headers: BTreeMap<String, Value>,
    content: BTreeMap<String, CanonicalMedia>,
    links: Option<Value>,
    extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct CanonicalMedia {
    schema: Option<Value>,
    example: Option<Value>,
    examples: Option<Value>,
    encoding: Option<Value>,
    extensions: BTreeMap<String, Value>,
}

struct Canonicalizer {
    document: ParsedDocument,
    external_documents: BTreeMap<PathBuf, Value>,
}

impl Canonicalizer {
    fn new(document: ParsedDocument) -> Self {
        Self {
            document,
            external_documents: BTreeMap::new(),
        }
    }

    fn canonicalize(mut self) -> Result<CanonicalDocument, CoreError> {
        let metadata = self.metadata();
        let servers = self.servers();
        let security = canonical_security(self.document.root.get("security"));
        let security_schemes = self.security_schemes();
        let schemas = self.schemas();
        let components = self.reusable_components()?;
        let path_items = self.path_item_metadata("paths", true)?;
        let operations = self.operations()?;
        let webhook_items = self.path_item_metadata("webhooks", false)?;
        let webhooks = self.webhooks()?;
        let external_documents = self.canonical_external_documents()?;
        Ok(CanonicalDocument {
            metadata,
            servers,
            security,
            security_schemes,
            path_items,
            operations,
            webhook_items,
            webhooks,
            schemas,
            components,
            external_documents,
        })
    }

    fn metadata(&self) -> BTreeMap<String, Value> {
        let mut metadata = BTreeMap::new();
        if let Some(info) = self.document.root.get("info").and_then(Value::as_object) {
            for key in [
                "title",
                "version",
                "summary",
                "description",
                "termsOfService",
                "contact",
                "license",
            ] {
                if let Some(value) = info.get(key) {
                    metadata.insert(key.to_string(), normalize_generic(value));
                }
            }
        }
        for key in ["externalDocs", "tags"] {
            if let Some(value) = self.document.root.get(key) {
                metadata.insert(key.to_string(), normalize_generic(value));
            }
        }
        if let Some(dialect) = self.document.root.get("jsonSchemaDialect") {
            metadata.insert("jsonSchemaDialect".to_string(), normalize_generic(dialect));
        }
        for (key, value) in self
            .document
            .root
            .as_object()
            .map(all_extensions)
            .unwrap_or_default()
        {
            metadata.insert(key, value);
        }
        if let Some(info) = self.document.root.get("info").and_then(Value::as_object) {
            for (key, value) in all_extensions(info) {
                metadata.insert(format!("info.{key}"), value);
            }
        }
        for container in ["paths", "webhooks"] {
            if let Some(object) = self.document.root.get(container).and_then(Value::as_object) {
                for (key, value) in all_extensions(object) {
                    metadata.insert(format!("{container}.{key}"), value);
                }
            }
        }
        metadata
    }

    fn servers(&self) -> Vec<Value> {
        let mut servers = match self.document.version {
            SpecVersion::Swagger2 => {
                let Some(host) = self.document.root.get("host").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let schemes = self
                    .document
                    .root
                    .get("schemes")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .filter(|items| !items.is_empty());
                schemes.map_or_else(
                    || vec![canonical_server_url(&format!("//{host}"))],
                    |schemes| {
                        schemes
                            .into_iter()
                            .map(|scheme| canonical_server_url(&format!("{scheme}://{host}")))
                            .collect()
                    },
                )
            }
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => {
                canonical_servers(self.document.root.get("servers"))
            }
        };
        servers.sort_by_key(value_sort_key);
        servers.dedup();
        servers
    }

    fn security_schemes(&self) -> BTreeMap<String, Value> {
        let raw = match self.document.version {
            SpecVersion::Swagger2 => self
                .document
                .root
                .get("securityDefinitions")
                .and_then(Value::as_object),
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => self
                .document
                .root
                .pointer("/components/securitySchemes")
                .and_then(Value::as_object),
        };
        raw.into_iter()
            .flatten()
            .map(|(name, scheme)| {
                let normalized = if self.document.version == SpecVersion::Swagger2 {
                    normalize_swagger_security_scheme(scheme)
                } else {
                    normalize_generic(scheme)
                };
                (name.clone(), normalized)
            })
            .collect()
    }

    fn schemas(&self) -> BTreeMap<String, Value> {
        let raw = match self.document.version {
            SpecVersion::Swagger2 => self
                .document
                .root
                .get("definitions")
                .and_then(Value::as_object),
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => self
                .document
                .root
                .pointer("/components/schemas")
                .and_then(Value::as_object),
        };
        raw.into_iter()
            .flatten()
            .map(|(name, schema)| (name.clone(), normalize_schema(schema)))
            .collect()
    }

    fn reusable_components(&mut self) -> Result<BTreeMap<String, Value>, CoreError> {
        let mut components = BTreeMap::new();
        match self.document.version {
            SpecVersion::Swagger2 => {
                let consumes = string_array(self.document.root.get("consumes"));
                let produces = string_array(self.document.root.get("produces"));
                let parameters = self
                    .document
                    .root
                    .get("parameters")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for (name, raw) in parameters {
                    let resolved = self.resolve_object(&raw, 0)?;
                    let Some(parameter) = resolved.as_object() else {
                        continue;
                    };
                    if parameter.get("in").and_then(Value::as_str) == Some("body") {
                        if let Some(body) = Self::swagger_body_component(parameter, &consumes) {
                            insert_component(
                                &mut components,
                                "requestBodies",
                                &name,
                                to_value(&body),
                            );
                        }
                    } else if let Some((_, parameter)) = self.canonical_parameter(parameter) {
                        insert_component(
                            &mut components,
                            "parameters",
                            &name,
                            to_value(&parameter),
                        );
                    } else {
                        insert_component(
                            &mut components,
                            "parameters",
                            &name,
                            normalize_generic(&resolved),
                        );
                    }
                }
                let responses = self
                    .document
                    .root
                    .get("responses")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for (name, raw) in responses {
                    let resolved = self.resolve_object(&raw, 0)?;
                    let Some(response) = resolved.as_object() else {
                        continue;
                    };
                    let response = self.canonical_response(response, &produces)?;
                    insert_component(&mut components, "responses", &name, to_value(&response));
                }
            }
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => {
                let raw_components = self
                    .document
                    .root
                    .get("components")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                self.add_openapi_components(&mut components, &raw_components)?;
            }
        }
        Ok(components)
    }

    fn add_openapi_components(
        &mut self,
        components: &mut BTreeMap<String, Value>,
        raw_components: &Map<String, Value>,
    ) -> Result<(), CoreError> {
        for (kind, values) in raw_components {
            if matches!(kind.as_str(), "schemas" | "securitySchemes") {
                continue;
            }
            if kind.starts_with("x-") {
                components.insert(kind.clone(), normalize_generic(values));
                continue;
            }
            let Some(values) = values.as_object() else {
                components.insert(kind.clone(), normalize_generic(values));
                continue;
            };
            for (name, raw) in values {
                let resolved = self.resolve_object(raw, 0)?;
                let normalized = self.canonical_openapi_component(kind, &resolved)?;
                insert_component(components, kind, name, normalized);
            }
        }
        Ok(())
    }

    fn canonical_openapi_component(
        &mut self,
        kind: &str,
        value: &Value,
    ) -> Result<Value, CoreError> {
        let Some(object) = value.as_object() else {
            return Ok(normalize_generic(value));
        };
        match kind {
            "parameters" => Ok(self.canonical_parameter(object).map_or_else(
                || normalize_generic(value),
                |(_, parameter)| to_value(&parameter),
            )),
            "responses" => Ok(to_value(&self.canonical_response(object, &[])?)),
            "requestBodies" => Ok(to_value(&Self::canonical_openapi_request_body(object))),
            "headers" => Ok(self.canonical_header(object)),
            _ => Ok(normalize_generic(value)),
        }
    }

    fn swagger_body_component(
        parameter: &Map<String, Value>,
        consumes: &[String],
    ) -> Option<CanonicalRequestBody> {
        let schema = parameter.get("schema").map(normalize_schema)?;
        let media_types = if consumes.is_empty() {
            vec!["application/json".to_string()]
        } else {
            consumes.to_vec()
        };
        let content = media_types
            .into_iter()
            .map(|media_type| {
                (
                    media_type,
                    CanonicalMedia {
                        schema: Some(schema.clone()),
                        example: None,
                        examples: None,
                        encoding: None,
                        extensions: BTreeMap::new(),
                    },
                )
            })
            .collect();
        Some(CanonicalRequestBody {
            description: optional_string(parameter.get("description")),
            required: parameter
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content,
            extensions: extensions(parameter),
        })
    }

    fn operations(&mut self) -> Result<BTreeMap<String, CanonicalOperation>, CoreError> {
        let Some(paths) = self
            .document
            .root
            .get("paths")
            .and_then(Value::as_object)
            .cloned()
        else {
            return Ok(BTreeMap::new());
        };
        let base_path = if self.document.version == SpecVersion::Swagger2 {
            self.document
                .root
                .get("basePath")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        self.canonical_operations(paths, &base_path, true)
    }

    fn path_item_metadata(
        &mut self,
        container: &str,
        include_server_path: bool,
    ) -> Result<BTreeMap<String, Value>, CoreError> {
        if container == "webhooks" && self.document.version != SpecVersion::OpenApi31 {
            return Ok(BTreeMap::new());
        }
        let items = self
            .document
            .root
            .get(container)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let base_path = if include_server_path && self.document.version == SpecVersion::Swagger2 {
            self.document
                .root
                .get("basePath")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        let empty_operation = Map::new();
        let mut metadata = BTreeMap::new();
        for (path, raw_item) in items {
            if path.starts_with("x-") {
                continue;
            }
            let item = self.resolve_object(&raw_item, 0)?;
            let Some(item) = item.as_object() else {
                continue;
            };
            let (servers, server_path) = self.operation_servers(item, &empty_operation);
            let canonical_path = if include_server_path {
                let operation_base = join_contract_path(&base_path, &server_path);
                join_contract_path(&operation_base, &path)
            } else {
                path
            };
            let mut canonical = Map::new();
            for field in ["summary", "description"] {
                if let Some(value) = item.get(field) {
                    canonical.insert(field.to_string(), normalize_generic(value));
                }
            }
            if item.contains_key("servers") {
                canonical.insert("servers".to_string(), to_value(&servers));
            }
            for (key, value) in extensions(item) {
                canonical.insert(key, value);
            }
            metadata.insert(canonical_path, Value::Object(canonical));
        }
        Ok(metadata)
    }

    fn webhooks(&mut self) -> Result<BTreeMap<String, CanonicalOperation>, CoreError> {
        if self.document.version != SpecVersion::OpenApi31 {
            return Ok(BTreeMap::new());
        }
        let webhooks = self
            .document
            .root
            .get("webhooks")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        self.canonical_operations(webhooks, "", false)
    }

    fn canonical_operations(
        &mut self,
        paths: Map<String, Value>,
        base_path: &str,
        include_server_path: bool,
    ) -> Result<BTreeMap<String, CanonicalOperation>, CoreError> {
        let root_consumes = string_array(self.document.root.get("consumes"));
        let root_produces = string_array(self.document.root.get("produces"));
        let root_security = canonical_security(self.document.root.get("security"));
        let mut operations = BTreeMap::new();
        for (path, raw_item) in paths {
            let item = self.resolve_object(&raw_item, 0)?;
            let Some(item) = item.as_object() else {
                continue;
            };
            let path_parameters = item
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (method, operation) in item {
                if !is_http_method(method) {
                    continue;
                }
                let Some(operation) = operation.as_object() else {
                    continue;
                };
                let (servers, server_path) = self.operation_servers(item, operation);
                let normalized_path = if include_server_path {
                    let operation_base = join_contract_path(base_path, &server_path);
                    join_contract_path(&operation_base, &path)
                } else {
                    path.clone()
                };
                let operation_label = format!("{} {normalized_path}", method.to_ascii_uppercase());
                let consumes = string_array(operation.get("consumes"));
                let consumes = if consumes.is_empty() {
                    &root_consumes
                } else {
                    &consumes
                };
                let produces = string_array(operation.get("produces"));
                let produces = if produces.is_empty() {
                    &root_produces
                } else {
                    &produces
                };
                let parameters = self.parameters(&path_parameters, operation)?;
                let request_body = self.request_body(operation, consumes)?;
                let responses = self.responses(operation, produces)?;
                let security = if operation.contains_key("security") {
                    canonical_security(operation.get("security"))
                } else {
                    root_security.clone()
                };
                let mut tags = string_array(operation.get("tags"));
                tags.sort();
                tags.dedup();
                operations.insert(
                    operation_label,
                    CanonicalOperation {
                        operation_id: optional_string(operation.get("operationId")),
                        summary: optional_string(operation.get("summary")),
                        description: optional_string(operation.get("description")),
                        tags,
                        deprecated: operation
                            .get("deprecated")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        external_docs: operation.get("externalDocs").map(normalize_generic),
                        servers,
                        parameters,
                        request_body,
                        responses,
                        security,
                        callbacks: operation.get("callbacks").map(normalize_generic),
                        extensions: extensions(operation),
                    },
                );
            }
        }
        Ok(operations)
    }

    fn parameters(
        &mut self,
        path_parameters: &[Value],
        operation: &Map<String, Value>,
    ) -> Result<BTreeMap<String, CanonicalParameter>, CoreError> {
        let mut parameters = BTreeMap::new();
        let operation_parameters = operation
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for raw in path_parameters.iter().chain(&operation_parameters) {
            let resolved = self.resolve_object(raw, 0)?;
            let Some(parameter) = resolved.as_object() else {
                continue;
            };
            if let Some((key, parameter)) = self.canonical_parameter(parameter) {
                parameters.insert(key, parameter);
            }
        }
        Ok(parameters)
    }

    fn canonical_parameter(
        &self,
        parameter: &Map<String, Value>,
    ) -> Option<(String, CanonicalParameter)> {
        let location = parameter.get("in").and_then(Value::as_str)?;
        if matches!(location, "body" | "formData") {
            return None;
        }
        let raw_name = parameter.get("name").and_then(Value::as_str)?;
        let name = if location == "header" {
            raw_name.to_ascii_lowercase()
        } else {
            raw_name.to_string()
        };
        let schema = parameter_schema(self.document.version, parameter);
        let (style, mut explode) =
            parameter_serialization(self.document.version, parameter, location);
        if !schema_is_collection(&schema) {
            explode = false;
        }
        let key = format!("{location}/{name}");
        Some((
            key,
            CanonicalParameter {
                name,
                location: location.to_string(),
                description: optional_string(parameter.get("description")),
                required: location == "path"
                    || parameter
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                deprecated: parameter
                    .get("deprecated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                style,
                explode,
                allow_reserved: parameter
                    .get("allowReserved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                allow_empty_value: parameter
                    .get("allowEmptyValue")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                schema,
                examples: parameter
                    .get("examples")
                    .or_else(|| parameter.get("example"))
                    .map(normalize_generic),
                extensions: extensions(parameter),
            },
        ))
    }

    fn request_body(
        &mut self,
        operation: &Map<String, Value>,
        consumes: &[String],
    ) -> Result<Option<CanonicalRequestBody>, CoreError> {
        if self.document.version != SpecVersion::Swagger2 {
            return self.openapi_request_body(operation);
        }

        let parameters = operation
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut body_schema = None;
        let mut body_description = None;
        let mut required = false;
        let mut form_properties = Map::new();
        let mut form_required = Vec::new();
        for raw in parameters {
            let resolved = self.resolve_object(&raw, 0)?;
            let Some(parameter) = resolved.as_object() else {
                continue;
            };
            match parameter.get("in").and_then(Value::as_str) {
                Some("body") => {
                    body_schema = parameter.get("schema").map(normalize_schema);
                    body_description = optional_string(parameter.get("description"));
                    required |= parameter
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                }
                Some("formData") => {
                    let Some(name) = parameter.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    form_properties.insert(
                        name.to_string(),
                        parameter_schema(SpecVersion::Swagger2, parameter),
                    );
                    if parameter
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        form_required.push(Value::String(name.to_string()));
                        required = true;
                    }
                }
                _ => {}
            }
        }
        if body_schema.is_none() && !form_properties.is_empty() {
            form_required.sort_by_key(value_sort_key);
            let mut schema = Map::new();
            schema.insert("type".to_string(), Value::String("object".to_string()));
            schema.insert("properties".to_string(), Value::Object(form_properties));
            if !form_required.is_empty() {
                schema.insert("required".to_string(), Value::Array(form_required));
            }
            body_schema = Some(Value::Object(schema));
        }
        let Some(schema) = body_schema else {
            return Ok(None);
        };
        let media_types = if consumes.is_empty() {
            vec!["application/json".to_string()]
        } else {
            consumes.to_vec()
        };
        let content = media_types
            .into_iter()
            .map(|media_type| {
                (
                    media_type,
                    CanonicalMedia {
                        schema: Some(schema.clone()),
                        example: None,
                        examples: None,
                        encoding: None,
                        extensions: BTreeMap::new(),
                    },
                )
            })
            .collect();
        Ok(Some(CanonicalRequestBody {
            description: body_description,
            required,
            content,
            extensions: BTreeMap::new(),
        }))
    }

    fn openapi_request_body(
        &mut self,
        operation: &Map<String, Value>,
    ) -> Result<Option<CanonicalRequestBody>, CoreError> {
        let Some(raw) = operation.get("requestBody") else {
            return Ok(None);
        };
        let body = self.resolve_object(raw, 0)?;
        let Some(body) = body.as_object() else {
            return Ok(None);
        };
        Ok(Some(Self::canonical_openapi_request_body(body)))
    }

    fn canonical_openapi_request_body(body: &Map<String, Value>) -> CanonicalRequestBody {
        CanonicalRequestBody {
            description: optional_string(body.get("description")),
            required: body
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content: canonical_content(body.get("content")),
            extensions: extensions(body),
        }
    }

    fn responses(
        &mut self,
        operation: &Map<String, Value>,
        produces: &[String],
    ) -> Result<BTreeMap<String, CanonicalResponse>, CoreError> {
        let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
            return Ok(BTreeMap::new());
        };
        let mut out = BTreeMap::new();
        for (status, raw) in responses {
            let resolved = self.resolve_object(raw, 0)?;
            let Some(response) = resolved.as_object() else {
                continue;
            };
            out.insert(
                status.to_ascii_uppercase(),
                self.canonical_response(response, produces)?,
            );
        }
        Ok(out)
    }

    fn canonical_response(
        &mut self,
        response: &Map<String, Value>,
        produces: &[String],
    ) -> Result<CanonicalResponse, CoreError> {
        let content = if self.document.version == SpecVersion::Swagger2 {
            if let Some(schema) = response.get("schema").map(normalize_schema) {
                let media_types = if produces.is_empty() {
                    vec!["application/json".to_string()]
                } else {
                    produces.to_vec()
                };
                media_types
                    .into_iter()
                    .map(|media_type| {
                        let example = response
                            .get("examples")
                            .and_then(Value::as_object)
                            .and_then(|examples| examples.get(&media_type))
                            .map(normalize_generic);
                        (
                            media_type,
                            CanonicalMedia {
                                schema: Some(schema.clone()),
                                example,
                                examples: None,
                                encoding: None,
                                extensions: BTreeMap::new(),
                            },
                        )
                    })
                    .collect()
            } else {
                BTreeMap::new()
            }
        } else {
            canonical_content(response.get("content"))
        };
        Ok(CanonicalResponse {
            description: optional_string(response.get("description")),
            headers: self.response_headers(response)?,
            content,
            links: response.get("links").map(normalize_generic),
            extensions: extensions(response),
        })
    }

    fn operation_servers(
        &self,
        path_item: &Map<String, Value>,
        operation: &Map<String, Value>,
    ) -> (Vec<Value>, String) {
        match self.document.version {
            SpecVersion::Swagger2 => {
                let Some(host) = self.document.root.get("host").and_then(Value::as_str) else {
                    return (Vec::new(), String::new());
                };
                let operation_schemes = string_array(operation.get("schemes"));
                let schemes = if operation_schemes.is_empty() {
                    let root_schemes = string_array(self.document.root.get("schemes"));
                    (!root_schemes.is_empty()).then_some(root_schemes)
                } else {
                    Some(operation_schemes)
                };
                let mut servers = schemes.map_or_else(
                    || vec![canonical_server_url(&format!("//{host}"))],
                    |schemes| {
                        schemes
                            .into_iter()
                            .map(|scheme| canonical_server_url(&format!("{scheme}://{host}")))
                            .collect::<Vec<_>>()
                    },
                );
                servers.sort_by_key(value_sort_key);
                servers.dedup();
                (servers, String::new())
            }
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => {
                let raw = if operation.contains_key("servers") {
                    operation.get("servers")
                } else if path_item.contains_key("servers") {
                    path_item.get("servers")
                } else {
                    self.document.root.get("servers")
                };
                canonical_servers_with_common_path(raw)
            }
        }
    }

    fn response_headers(
        &mut self,
        response: &Map<String, Value>,
    ) -> Result<BTreeMap<String, Value>, CoreError> {
        let headers = response
            .get("headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut out = BTreeMap::new();
        for (name, raw) in headers {
            let resolved = self.resolve_object(&raw, 0)?;
            let Some(header) = resolved.as_object() else {
                continue;
            };
            out.insert(name.to_ascii_lowercase(), self.canonical_header(header));
        }
        Ok(out)
    }

    fn canonical_header(&self, header: &Map<String, Value>) -> Value {
        let schema = parameter_schema(self.document.version, header);
        let (style, mut explode) = parameter_serialization(self.document.version, header, "header");
        if !schema_is_collection(&schema) {
            explode = false;
        }
        let mut canonical = Map::new();
        if let Some(description) = optional_string(header.get("description")) {
            canonical.insert("description".to_string(), Value::String(description));
        }
        canonical.insert("schema".to_string(), schema);
        canonical.insert("style".to_string(), Value::String(style));
        canonical.insert("explode".to_string(), Value::Bool(explode));
        for field in ["required", "deprecated"] {
            canonical.insert(
                field.to_string(),
                Value::Bool(header.get(field).and_then(Value::as_bool).unwrap_or(false)),
            );
        }
        if let Some(examples) = header.get("examples").or_else(|| header.get("example")) {
            canonical.insert("examples".to_string(), normalize_generic(examples));
        }
        for (key, value) in extensions(header) {
            canonical.insert(key, value);
        }
        Value::Object(canonical)
    }

    fn canonical_external_documents(&mut self) -> Result<BTreeMap<String, Value>, CoreError> {
        let root_parent = self
            .document
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut queue = vec![(self.document.path.clone(), self.document.root.clone())];
        let mut seen = BTreeSet::new();
        let mut canonical = BTreeMap::new();

        while let Some((owner_path, document)) = queue.pop() {
            let mut references = Vec::new();
            collect_references(&document, &mut references);
            for reference in references {
                let (path, _) = split_reference(&reference);
                if path.is_empty() {
                    continue;
                }
                let parent = owner_path.parent().unwrap_or_else(|| Path::new("."));
                let external_path = lexical_normalize(&parent.join(path));
                if !seen.insert(external_path.clone()) {
                    continue;
                }
                let external = if let Some(cached) = self.external_documents.get(&external_path) {
                    cached.clone()
                } else {
                    let text = std::fs::read_to_string(&external_path).map_err(|source| {
                        CoreError::Io {
                            message: format!(
                                "failed to read OpenAPI compatibility reference '{}': {source}",
                                external_path.display()
                            ),
                        }
                    })?;
                    let parsed = parse_json_or_yaml(&text, &external_path)?;
                    self.external_documents
                        .insert(external_path.clone(), parsed.clone());
                    parsed
                };
                let key = external_path
                    .strip_prefix(&root_parent)
                    .unwrap_or(&external_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                canonical.insert(key, normalize_generic(&external));
                queue.push((external_path, external));
            }
        }
        Ok(canonical)
    }

    fn resolve_object(&mut self, value: &Value, depth: usize) -> Result<Value, CoreError> {
        let owner_path = self.document.path.clone();
        self.resolve_object_from(value, depth, &owner_path)
    }

    fn resolve_object_from(
        &mut self,
        value: &Value,
        depth: usize,
        owner_path: &Path,
    ) -> Result<Value, CoreError> {
        if depth > 32 {
            return Err(CoreError::Config {
                message: format!(
                    "OpenAPI compatibility reference traversal exceeded 32 levels in '{}'",
                    self.document.path.display()
                ),
            });
        }
        let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
            return Ok(value.clone());
        };
        let (path, fragment) = split_reference(reference);
        let referenced_path = if path.is_empty() {
            owner_path.to_path_buf()
        } else {
            let parent = owner_path.parent().unwrap_or_else(|| Path::new("."));
            lexical_normalize(&parent.join(path))
        };
        let referenced_document = if referenced_path == self.document.path {
            self.document.root.clone()
        } else {
            if !self.external_documents.contains_key(&referenced_path) {
                let text =
                    std::fs::read_to_string(&referenced_path).map_err(|source| CoreError::Io {
                        message: format!(
                            "failed to read OpenAPI compatibility reference '{}': {source}",
                            referenced_path.display()
                        ),
                    })?;
                let parsed = parse_json_or_yaml(&text, &referenced_path)?;
                self.external_documents
                    .insert(referenced_path.clone(), parsed);
            }
            self.external_documents
                .get(&referenced_path)
                .cloned()
                .ok_or_else(|| CoreError::Config {
                    message: format!(
                        "OpenAPI compatibility reference cache lost '{}'",
                        referenced_path.display()
                    ),
                })?
        };
        let referenced = referenced_document
            .pointer(fragment)
            .cloned()
            .ok_or_else(|| CoreError::Config {
                message: format!(
                    "OpenAPI compatibility input '{}' contains unresolved reference {reference:?}",
                    self.document.path.display()
                ),
            })?;
        let mut resolved = self.resolve_object_from(&referenced, depth + 1, &referenced_path)?;
        if let (Some(resolved), Some(original)) = (resolved.as_object_mut(), value.as_object()) {
            for (key, sibling) in original {
                if key != "$ref" {
                    resolved.insert(key.clone(), sibling.clone());
                }
            }
        }
        Ok(resolved)
    }
}

fn insert_component(
    components: &mut BTreeMap<String, Value>,
    kind: &str,
    name: &str,
    value: Value,
) {
    let values = components
        .entry(kind.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !values.is_object() {
        *values = Value::Object(Map::new());
    }
    if let Some(values) = values.as_object_mut() {
        values.insert(name.to_string(), value);
    }
}

fn split_reference(reference: &str) -> (&str, &str) {
    let (path, fragment) = reference.split_once('#').unwrap_or((reference, ""));
    let fragment = if fragment.is_empty() { "" } else { fragment };
    (path, fragment)
}

fn is_http_method(method: &str) -> bool {
    matches!(
        method.to_ascii_lowercase().as_str(),
        "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
    )
}

fn join_contract_path(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    match (base.is_empty(), path.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{path}"),
        (false, true) => format!("{base}/"),
        (false, false) => format!("{base}/{path}"),
    }
}

fn normalize_server_url(url: &str) -> String {
    if url == "/" {
        "/".to_string()
    } else {
        url.trim_end_matches('/').to_string()
    }
}

fn canonical_server_url(url: &str) -> Value {
    Value::Object(Map::from_iter([(
        "url".to_string(),
        Value::String(normalize_server_url(url)),
    )]))
}

fn canonical_servers(value: Option<&Value>) -> Vec<Value> {
    canonical_servers_with_common_path(value).0
}

fn canonical_servers_with_common_path(value: Option<&Value>) -> (Vec<Value>, String) {
    let mut servers = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|server| {
            let server = server.as_object()?;
            let url = server.get("url")?.as_str()?;
            let mut canonical = Map::new();
            canonical.insert("url".to_string(), Value::String(normalize_server_url(url)));
            for key in ["description", "variables"] {
                if let Some(value) = server.get(key) {
                    let value = if key == "variables" {
                        normalize_server_variables(value)
                    } else {
                        normalize_generic(value)
                    };
                    canonical.insert(key.to_string(), value);
                }
            }
            for (key, value) in extensions(server) {
                canonical.insert(key, value);
            }
            Some(Value::Object(canonical))
        })
        .collect::<Vec<_>>();

    let common_path = servers
        .iter()
        .filter_map(|server| server.get("url").and_then(Value::as_str))
        .map(split_server_base_path)
        .map(|(_, path)| path)
        .collect::<BTreeSet<_>>();
    let common_path = if !servers.is_empty() && common_path.len() == 1 {
        common_path.into_iter().next().unwrap_or_default()
    } else {
        String::new()
    };
    if !common_path.is_empty() {
        for server in &mut servers {
            let Some(url) = server.get("url").and_then(Value::as_str) else {
                continue;
            };
            let (base, _) = split_server_base_path(url);
            if let Some(url) = server.get_mut("url") {
                *url = Value::String(base);
            }
        }
    }

    // An omitted OpenAPI servers array and a bare `/` both select the document origin.
    servers.retain(|server| {
        server.as_object().is_none_or(|server| {
            server.len() != 1 || server.get("url").and_then(Value::as_str) != Some("/")
        })
    });
    servers.sort_by_key(value_sort_key);
    servers.dedup();
    (servers, common_path)
}

fn split_server_base_path(url: &str) -> (String, String) {
    let url = normalize_server_url(url);
    let path_start = if let Some(scheme) = url.find("://") {
        url[scheme + 3..]
            .find('/')
            .map(|offset| scheme + 3 + offset)
    } else if let Some(authority) = url.strip_prefix("//") {
        authority.find('/').map(|offset| 2 + offset)
    } else if url.starts_with('/') {
        Some(0)
    } else {
        None
    };
    let Some(path_start) = path_start else {
        return (url, String::new());
    };
    let path = normalize_server_url(&url[path_start..]);
    if path == "/" {
        return (url, String::new());
    }
    let base = if path_start == 0 {
        "/".to_string()
    } else {
        url[..path_start].to_string()
    };
    (base, path)
}

fn normalize_server_variables(value: &Value) -> Value {
    let Some(variables) = value.as_object() else {
        return normalize_generic(value);
    };
    Value::Object(
        variables
            .iter()
            .map(|(name, variable)| {
                let mut variable = variable
                    .as_object()
                    .map(|object| {
                        object
                            .iter()
                            .map(|(key, value)| (key.clone(), normalize_generic(value)))
                            .collect::<Map<_, _>>()
                    })
                    .unwrap_or_default();
                if let Some(values) = variable.get_mut("enum").and_then(Value::as_array_mut) {
                    values.sort_by_key(value_sort_key);
                    values.dedup();
                }
                (name.clone(), Value::Object(variable))
            })
            .collect(),
    )
}

fn collect_references(value: &Value, references: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                references.push(reference.to_string());
            }
            for child in object.values() {
                collect_references(child, references);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_references(child, references);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn canonical_security(value: Option<&Value>) -> SecurityRequirements {
    let mut requirements = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|requirement| {
            requirement
                .iter()
                .map(|(scheme, scopes)| {
                    let mut scopes = scopes
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    scopes.sort();
                    scopes.dedup();
                    (scheme.clone(), scopes)
                })
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    requirements.sort_by_key(|requirement| {
        value_sort_key(&serde_json::to_value(requirement).unwrap_or(Value::Null))
    });
    requirements
}

fn parameter_serialization(
    version: SpecVersion,
    parameter: &Map<String, Value>,
    location: &str,
) -> (String, bool) {
    if version != SpecVersion::Swagger2 {
        let style = parameter
            .get("style")
            .and_then(Value::as_str)
            .unwrap_or(match location {
                "path" | "header" => "simple",
                _ => "form",
            })
            .to_string();
        let explode = parameter
            .get("explode")
            .and_then(Value::as_bool)
            .unwrap_or(style == "form");
        return (style, explode);
    }
    let collection = parameter
        .get("collectionFormat")
        .and_then(Value::as_str)
        .unwrap_or("csv");
    match (location, collection) {
        ("query" | "formData", "multi") => ("form".to_string(), true),
        ("query", "ssv") => ("spaceDelimited".to_string(), false),
        // OpenAPI 3 has no standard tab-delimited style. Preserve a distinct canonical value so
        // Swagger `tsv` can never compare equal to comma-delimited `form` serialization.
        ("query", "tsv") => ("tabDelimited".to_string(), false),
        ("query", "pipes") => ("pipeDelimited".to_string(), false),
        ("path" | "header", _) => ("simple".to_string(), false),
        _ => ("form".to_string(), false),
    }
}

fn parameter_schema(version: SpecVersion, parameter: &Map<String, Value>) -> Value {
    if let Some(schema) = parameter.get("schema") {
        return normalize_schema(schema);
    }
    if version != SpecVersion::Swagger2 {
        if let Some(content) = parameter.get("content") {
            let mut object = Map::new();
            object.insert("content".to_string(), normalize_generic(content));
            return Value::Object(object);
        }
    }
    let keys = [
        "type",
        "format",
        "items",
        "enum",
        "default",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
        "nullable",
        "x-nullable",
    ];
    let mut schema = Map::new();
    for key in keys {
        if let Some(value) = parameter.get(key) {
            schema.insert(key.to_string(), value.clone());
        }
    }
    normalize_schema(&Value::Object(schema))
}

fn schema_is_collection(schema: &Value) -> bool {
    matches!(
        schema.get("type").and_then(Value::as_str),
        Some("array" | "object")
    ) || schema.get("properties").is_some()
        || schema.get("additionalProperties").is_some()
}

fn canonical_content(value: Option<&Value>) -> BTreeMap<String, CanonicalMedia> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(media_type, media)| {
            let media = media.as_object();
            (
                media_type.to_ascii_lowercase(),
                CanonicalMedia {
                    schema: media
                        .and_then(|item| item.get("schema"))
                        .map(normalize_schema),
                    example: media
                        .and_then(|item| item.get("example"))
                        .map(normalize_generic),
                    examples: media
                        .and_then(|item| item.get("examples"))
                        .map(normalize_generic),
                    encoding: media
                        .and_then(|item| item.get("encoding"))
                        .map(normalize_generic),
                    extensions: media.map(extensions).unwrap_or_default(),
                },
            )
        })
        .collect()
}

fn normalize_swagger_security_scheme(value: &Value) -> Value {
    let Some(scheme) = value.as_object() else {
        return normalize_generic(value);
    };
    match scheme.get("type").and_then(Value::as_str) {
        Some("basic") => {
            let mut out = Map::new();
            out.insert("type".to_string(), Value::String("http".to_string()));
            out.insert("scheme".to_string(), Value::String("basic".to_string()));
            add_security_scheme_common(&mut out, scheme);
            Value::Object(out)
        }
        Some("oauth2") => {
            let flow = scheme.get("flow").and_then(Value::as_str).unwrap_or("");
            let (flow_name, authorization_key, token_key) = match flow {
                "implicit" => ("implicit", Some("authorizationUrl"), None),
                "password" => ("password", None, Some("tokenUrl")),
                "application" => ("clientCredentials", None, Some("tokenUrl")),
                "accessCode" => (
                    "authorizationCode",
                    Some("authorizationUrl"),
                    Some("tokenUrl"),
                ),
                _ => (flow, None, None),
            };
            let mut details = Map::new();
            if let Some(key) = authorization_key {
                if let Some(url) = scheme.get("authorizationUrl") {
                    details.insert(key.to_string(), url.clone());
                }
            }
            if let Some(key) = token_key {
                if let Some(url) = scheme.get("tokenUrl") {
                    details.insert(key.to_string(), url.clone());
                }
            }
            details.insert(
                "scopes".to_string(),
                normalize_generic(scheme.get("scopes").unwrap_or(&Value::Object(Map::new()))),
            );
            let mut flows = Map::new();
            flows.insert(flow_name.to_string(), Value::Object(details));
            let mut out = Map::new();
            out.insert("type".to_string(), Value::String("oauth2".to_string()));
            out.insert("flows".to_string(), Value::Object(flows));
            add_security_scheme_common(&mut out, scheme);
            Value::Object(out)
        }
        _ => normalize_generic(value),
    }
}

fn add_security_scheme_common(out: &mut Map<String, Value>, scheme: &Map<String, Value>) {
    if let Some(description) = scheme.get("description") {
        out.insert("description".to_string(), normalize_generic(description));
    }
    for (key, value) in extensions(scheme) {
        out.insert(key, value);
    }
}

fn normalize_schema(value: &Value) -> Value {
    let Value::Object(object) = value else {
        return normalize_generic(value);
    };
    let (skip_minimum, exclusive_minimum) =
        canonical_exclusive_bound(object, "minimum", "exclusiveMinimum");
    let (skip_maximum, exclusive_maximum) =
        canonical_exclusive_bound(object, "maximum", "exclusiveMaximum");
    let mut nullable = object
        .get("nullable")
        .or_else(|| object.get("x-nullable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut out = Map::new();
    for (key, value) in object {
        if matches!(key.as_str(), "nullable" | "x-nullable") {
            continue;
        }
        if (key == "minimum" && skip_minimum) || (key == "maximum" && skip_maximum) {
            continue;
        }
        if key == "exclusiveMinimum" {
            if let Some(value) = &exclusive_minimum {
                out.insert(key.clone(), value.clone());
            }
            continue;
        }
        if key == "exclusiveMaximum" {
            if let Some(value) = &exclusive_maximum {
                out.insert(key.clone(), value.clone());
            }
            continue;
        }
        if key == "additionalProperties"
            && (value.as_bool() == Some(true) || value.as_object().is_some_and(Map::is_empty))
        {
            continue;
        }
        if key == "$ref" {
            if let Some(reference) = value.as_str() {
                out.insert(key.clone(), Value::String(normalize_reference(reference)));
            }
            continue;
        }
        if key == "type" {
            if let Some(types) = value.as_array() {
                let mut retained = Vec::new();
                for item in types {
                    if item.as_str() == Some("null") {
                        nullable = true;
                    } else {
                        retained.push(item.clone());
                    }
                }
                retained.sort_by_key(value_sort_key);
                if retained.len() == 1 {
                    if let Some(item) = retained.into_iter().next() {
                        out.insert(key.clone(), item);
                    }
                } else if !retained.is_empty() {
                    out.insert(key.clone(), Value::Array(retained));
                }
                continue;
            }
            if value.as_str() == Some("file") {
                out.insert(key.clone(), Value::String("string".to_string()));
                out.insert("format".to_string(), Value::String("binary".to_string()));
                continue;
            }
        }
        let normalized = normalize_schema_keyword(key, value);
        out.insert(key.clone(), normalized);
    }
    if nullable {
        out.insert("x-gnr8-nullable".to_string(), Value::Bool(true));
    }
    Value::Object(out)
}

fn normalize_schema_keyword(key: &str, value: &Value) -> Value {
    match key {
        "properties" => Value::Object(
            value
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, schema)| (name.clone(), normalize_schema(schema)))
                .collect(),
        ),
        "items" | "additionalProperties" | "not" => normalize_schema(value),
        "discriminator" => value.as_str().map_or_else(
            || normalize_generic(value),
            |property| {
                Value::Object(Map::from_iter([(
                    "propertyName".to_string(),
                    Value::String(property.to_string()),
                )]))
            },
        ),
        "allOf" | "anyOf" | "oneOf" => {
            let mut items = value
                .as_array()
                .into_iter()
                .flatten()
                .map(normalize_schema)
                .collect::<Vec<_>>();
            items.sort_by_key(value_sort_key);
            Value::Array(items)
        }
        "required" | "enum" => {
            let mut items = value.as_array().cloned().unwrap_or_default();
            items.sort_by_key(value_sort_key);
            items.dedup();
            Value::Array(items)
        }
        _ => normalize_generic(value),
    }
}

fn canonical_exclusive_bound(
    schema: &Map<String, Value>,
    inclusive: &str,
    exclusive: &str,
) -> (bool, Option<Value>) {
    match schema.get(exclusive) {
        Some(Value::Bool(true)) => schema
            .get(inclusive)
            .map_or((false, Some(Value::Bool(true))), |bound| {
                (true, Some(bound.clone()))
            }),
        Some(Value::Bool(false)) | None => (false, None),
        Some(bound) => (false, Some(bound.clone())),
    }
}

fn normalize_reference(reference: &str) -> String {
    let (path, fragment) = split_reference(reference);
    let fragment = fragment
        .replace("/definitions/", "/components/schemas/")
        .replace("/securityDefinitions/", "/components/securitySchemes/")
        .replace("/parameters/", "/components/parameters/")
        .replace("/responses/", "/components/responses/");
    format!("{path}#{fragment}")
}

fn normalize_generic(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if key == "$ref" {
                        value.as_str().map_or_else(
                            || normalize_generic(value),
                            |reference| Value::String(normalize_reference(reference)),
                        )
                    } else {
                        normalize_generic(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalize_generic).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

fn extensions(object: &Map<String, Value>) -> BTreeMap<String, Value> {
    object
        .iter()
        .filter(|(key, _)| key.starts_with("x-") && key.as_str() != "x-nullable")
        .map(|(key, value)| (key.clone(), normalize_generic(value)))
        .collect()
}

fn all_extensions(object: &Map<String, Value>) -> BTreeMap<String, Value> {
    object
        .iter()
        .filter(|(key, _)| key.starts_with("x-"))
        .map(|(key, value)| (key.clone(), normalize_generic(value)))
        .collect()
}

fn value_sort_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn diff_documents(old: &CanonicalDocument, new: &CanonicalDocument) -> Vec<OpenApiDifference> {
    let mut differences = Vec::new();
    diff_map_values(
        &old.metadata,
        &new.metadata,
        "/metadata",
        "metadata",
        None,
        None,
        None,
        &mut differences,
    );
    diff_json_value(
        &serde_json::to_value(&old.servers).unwrap_or(Value::Null),
        &serde_json::to_value(&new.servers).unwrap_or(Value::Null),
        "/servers",
        "server.changed",
        None,
        None,
        None,
        &mut differences,
    );
    diff_json_value(
        &serde_json::to_value(&old.security).unwrap_or(Value::Null),
        &serde_json::to_value(&new.security).unwrap_or(Value::Null),
        "/security",
        "security.requirement.changed",
        None,
        None,
        None,
        &mut differences,
    );
    diff_map_values(
        &old.security_schemes,
        &new.security_schemes,
        "/components/securitySchemes",
        "security.scheme",
        None,
        None,
        None,
        &mut differences,
    );
    diff_map_values(
        &old.schemas,
        &new.schemas,
        "/components/schemas",
        "schema",
        None,
        None,
        None,
        &mut differences,
    );
    diff_components(&old.components, &new.components, &mut differences);
    diff_map_values(
        &old.external_documents,
        &new.external_documents,
        "/externalDocuments",
        "external.document",
        None,
        None,
        None,
        &mut differences,
    );
    diff_map_values(
        &old.path_items,
        &new.path_items,
        "/paths",
        "path.item",
        None,
        None,
        None,
        &mut differences,
    );
    diff_operations(&old.operations, &new.operations, &mut differences);
    diff_map_values(
        &old.webhook_items,
        &new.webhook_items,
        "/webhooks",
        "webhook.item",
        None,
        None,
        None,
        &mut differences,
    );
    diff_operation_collection(
        &old.webhooks,
        &new.webhooks,
        "/webhooks",
        "webhook",
        &mut differences,
    );
    differences
}

fn diff_operations(
    old: &BTreeMap<String, CanonicalOperation>,
    new: &BTreeMap<String, CanonicalOperation>,
    differences: &mut Vec<OpenApiDifference>,
) {
    diff_operation_collection(old, new, "/paths", "operation", differences);
}

fn diff_components(
    old: &BTreeMap<String, Value>,
    new: &BTreeMap<String, Value>,
    differences: &mut Vec<OpenApiDifference>,
) {
    for kind in union_keys(old, new) {
        let location = format!("/components/{}", escape_pointer(kind));
        let code = format!("component.{}", dotted(kind));
        match (old.get(kind), new.get(kind)) {
            (Some(old), Some(new)) => diff_json_value(
                old,
                new,
                &location,
                &format!("{code}.changed"),
                None,
                Some(kind),
                None,
                differences,
            ),
            (Some(old), None) => differences.push(difference(
                &format!("{code}.missing"),
                &location,
                None,
                Some(kind),
                None,
                Some(old.clone()),
                None,
            )),
            (None, Some(new)) => differences.push(difference(
                &format!("{code}.added"),
                &location,
                None,
                Some(kind),
                None,
                None,
                Some(new.clone()),
            )),
            (None, None) => {}
        }
    }
}

fn diff_operation_collection(
    old: &BTreeMap<String, CanonicalOperation>,
    new: &BTreeMap<String, CanonicalOperation>,
    base: &str,
    code_prefix: &str,
    differences: &mut Vec<OpenApiDifference>,
) {
    for operation in union_keys(old, new) {
        let location = operation_pointer(base, operation);
        match (old.get(operation), new.get(operation)) {
            (Some(old), Some(new)) => diff_operation(operation, &location, old, new, differences),
            (Some(old), None) => differences.push(difference(
                &format!("{code_prefix}.missing"),
                &location,
                Some(operation),
                None,
                None,
                Some(to_value(old)),
                None,
            )),
            (None, Some(new)) => differences.push(difference(
                &format!("{code_prefix}.added"),
                &location,
                Some(operation),
                None,
                None,
                None,
                Some(to_value(new)),
            )),
            (None, None) => {}
        }
    }
}

fn diff_operation(
    operation: &str,
    location: &str,
    old: &CanonicalOperation,
    new: &CanonicalOperation,
    differences: &mut Vec<OpenApiDifference>,
) {
    for (field, old, new) in [
        (
            "operationId",
            to_value(&old.operation_id),
            to_value(&new.operation_id),
        ),
        ("summary", to_value(&old.summary), to_value(&new.summary)),
        (
            "description",
            to_value(&old.description),
            to_value(&new.description),
        ),
        ("tags", to_value(&old.tags), to_value(&new.tags)),
        (
            "deprecated",
            to_value(&old.deprecated),
            to_value(&new.deprecated),
        ),
        (
            "externalDocs",
            to_value(&old.external_docs),
            to_value(&new.external_docs),
        ),
        ("servers", to_value(&old.servers), to_value(&new.servers)),
        ("security", to_value(&old.security), to_value(&new.security)),
        (
            "callbacks",
            to_value(&old.callbacks),
            to_value(&new.callbacks),
        ),
        (
            "extensions",
            to_value(&old.extensions),
            to_value(&new.extensions),
        ),
    ] {
        diff_json_value(
            &old,
            &new,
            &format!("{location}/{}", escape_pointer(field)),
            &format!("operation.{}.changed", dotted(field)),
            Some(operation),
            None,
            None,
            differences,
        );
    }
    diff_parameters(
        operation,
        location,
        &old.parameters,
        &new.parameters,
        differences,
    );
    diff_request_body(
        operation,
        location,
        old.request_body.as_ref(),
        new.request_body.as_ref(),
        differences,
    );
    diff_responses(
        operation,
        location,
        &old.responses,
        &new.responses,
        differences,
    );
}

fn diff_parameters(
    operation: &str,
    operation_location: &str,
    old: &BTreeMap<String, CanonicalParameter>,
    new: &BTreeMap<String, CanonicalParameter>,
    differences: &mut Vec<OpenApiDifference>,
) {
    for key in union_keys(old, new) {
        let location = format!("{operation_location}/parameters/{}", escape_pointer(key));
        let name = key.split_once('/').map_or(key, |(_, name)| name);
        match (old.get(key), new.get(key)) {
            (Some(old), Some(new)) => diff_json_value(
                &to_value(old),
                &to_value(new),
                &location,
                "parameter.changed",
                Some(operation),
                Some(name),
                None,
                differences,
            ),
            (Some(old), None) => differences.push(difference(
                "parameter.missing",
                &location,
                Some(operation),
                Some(name),
                None,
                Some(to_value(old)),
                None,
            )),
            (None, Some(new)) => differences.push(difference(
                "parameter.added",
                &location,
                Some(operation),
                Some(name),
                None,
                None,
                Some(to_value(new)),
            )),
            (None, None) => {}
        }
    }
}

fn diff_request_body(
    operation: &str,
    operation_location: &str,
    old: Option<&CanonicalRequestBody>,
    new: Option<&CanonicalRequestBody>,
    differences: &mut Vec<OpenApiDifference>,
) {
    let location = format!("{operation_location}/requestBody");
    match (old, new) {
        (Some(old), Some(new)) => diff_json_value(
            &to_value(old),
            &to_value(new),
            &location,
            "request.body.changed",
            Some(operation),
            None,
            None,
            differences,
        ),
        (Some(old), None) => differences.push(difference(
            "request.body.missing",
            &location,
            Some(operation),
            None,
            None,
            Some(to_value(old)),
            None,
        )),
        (None, Some(new)) => differences.push(difference(
            "request.body.added",
            &location,
            Some(operation),
            None,
            None,
            None,
            Some(to_value(new)),
        )),
        (None, None) => {}
    }
}

fn diff_responses(
    operation: &str,
    operation_location: &str,
    old: &BTreeMap<String, CanonicalResponse>,
    new: &BTreeMap<String, CanonicalResponse>,
    differences: &mut Vec<OpenApiDifference>,
) {
    for status in union_keys(old, new) {
        let location = format!("{operation_location}/responses/{}", escape_pointer(status));
        match (old.get(status), new.get(status)) {
            (Some(old), Some(new)) => diff_json_value(
                &to_value(old),
                &to_value(new),
                &location,
                "response.changed",
                Some(operation),
                None,
                Some(status),
                differences,
            ),
            (Some(old), None) => differences.push(difference(
                "response.missing",
                &location,
                Some(operation),
                None,
                Some(status),
                Some(to_value(old)),
                None,
            )),
            (None, Some(new)) => differences.push(difference(
                "response.added",
                &location,
                Some(operation),
                None,
                Some(status),
                None,
                Some(to_value(new)),
            )),
            (None, None) => {}
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "difference context is explicit so every recursive report retains its stable subject fields"
)]
fn diff_map_values(
    old: &BTreeMap<String, Value>,
    new: &BTreeMap<String, Value>,
    base: &str,
    code_prefix: &str,
    operation: Option<&str>,
    name: Option<&str>,
    status: Option<&str>,
    differences: &mut Vec<OpenApiDifference>,
) {
    for key in union_keys(old, new) {
        let location = format!("{base}/{}", escape_pointer(key));
        let subject_name = name.or(Some(key));
        match (old.get(key), new.get(key)) {
            (Some(old), Some(new)) => diff_json_value(
                old,
                new,
                &location,
                &format!("{code_prefix}.changed"),
                operation,
                subject_name,
                status,
                differences,
            ),
            (Some(old), None) => differences.push(difference(
                &format!("{code_prefix}.missing"),
                &location,
                operation,
                subject_name,
                status,
                Some(old.clone()),
                None,
            )),
            (None, Some(new)) => differences.push(difference(
                &format!("{code_prefix}.added"),
                &location,
                operation,
                subject_name,
                status,
                None,
                Some(new.clone()),
            )),
            (None, None) => {}
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "recursive JSON differences carry explicit operation/name/status context"
)]
fn diff_json_value(
    old: &Value,
    new: &Value,
    location: &str,
    code: &str,
    operation: Option<&str>,
    name: Option<&str>,
    status: Option<&str>,
    differences: &mut Vec<OpenApiDifference>,
) {
    if old == new {
        return;
    }
    match (old.as_object(), new.as_object()) {
        (Some(old), Some(new)) => {
            let old: BTreeMap<String, Value> = old
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let new: BTreeMap<String, Value> = new
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let prefix = code.trim_end_matches(".changed");
            for key in union_keys(&old, &new) {
                let child_location = format!("{location}/{}", escape_pointer(key));
                let child_code = format!("{prefix}.{}", dotted(key));
                match (old.get(key), new.get(key)) {
                    (Some(old), Some(new)) => diff_json_value(
                        old,
                        new,
                        &child_location,
                        &format!("{child_code}.changed"),
                        operation,
                        name,
                        status,
                        differences,
                    ),
                    (Some(old), None) => differences.push(difference(
                        &format!("{child_code}.missing"),
                        &child_location,
                        operation,
                        name,
                        status,
                        Some(old.clone()),
                        None,
                    )),
                    (None, Some(new)) => differences.push(difference(
                        &format!("{child_code}.added"),
                        &child_location,
                        operation,
                        name,
                        status,
                        None,
                        Some(new.clone()),
                    )),
                    (None, None) => {}
                }
            }
        }
        _ => differences.push(difference(
            code,
            location,
            operation,
            name,
            status,
            Some(old.clone()),
            Some(new.clone()),
        )),
    }
}

fn difference(
    code: &str,
    location: &str,
    operation: Option<&str>,
    name: Option<&str>,
    status: Option<&str>,
    old: Option<Value>,
    new: Option<Value>,
) -> OpenApiDifference {
    OpenApiDifference {
        code: code.to_string(),
        location: location.to_string(),
        operation: operation.map(str::to_string),
        name: name.map(str::to_string),
        status: status.map(str::to_string),
        old,
        new,
    }
}

fn union_keys<'a, T>(
    old: &'a BTreeMap<String, T>,
    new: &'a BTreeMap<String, T>,
) -> impl Iterator<Item = &'a str> {
    let keys: BTreeSet<&str> = old.keys().chain(new.keys()).map(String::as_str).collect();
    keys.into_iter()
}

fn operation_pointer(base: &str, operation: &str) -> String {
    let (method, path) = operation.split_once(' ').unwrap_or((operation, ""));
    format!(
        "{base}/{}/{method}",
        escape_pointer(path),
        method = method.to_ascii_lowercase()
    )
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn dotted(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('.');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn to_value(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use std::path::Path;

    use super::{
        compare_documents, compare_openapi_files, OpenApiCompatibilityPolicy, ParsedDocument,
    };
    use crate::sdk::openapi_source::{detect_version, parse_json_or_yaml};

    fn parsed(name: &str, text: &str) -> ParsedDocument {
        let path = Path::new(name);
        let root = parse_json_or_yaml(text, path).expect("parse fixture");
        let version = detect_version(&root, path).expect("detect fixture version");
        ParsedDocument {
            path: path.to_path_buf(),
            root,
            version,
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gnr8-openapi-compat-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        dir
    }

    #[test]
    fn swagger_and_openapi_equivalents_compare_equal() {
        let swagger = parsed(
            "old.json",
            r##"{
  "swagger": "2.0",
  "basePath": "/api",
  "info": {"title": "Books", "version": "1.0", "description": "API"},
  "produces": ["application/json"],
  "paths": {
    "/books": {
      "get": {
        "operationId": "listBooks",
        "parameters": [{"name":"status","in":"query","type":"array","items":{"type":"string"},"collectionFormat":"multi"}],
        "responses": {"200":{"description":"ok","schema":{"type":"array","items":{"$ref":"#/definitions/Book"}}}}
      }
    }
  },
  "definitions": {"Book":{"type":"object","required":["id"],"properties":{"id":{"type":"string","format":"uuid"}}}}
}"##,
        );
        let openapi = parsed(
            "new.yaml",
            r"openapi: 3.1.0
info:
  title: Books
  version: '1.0'
  description: API
paths:
  /api/books:
    get:
      operationId: listBooks
      parameters:
        - name: status
          in: query
          required: false
          style: form
          explode: true
          schema:
            type: array
            items:
              type: string
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Book'
components:
  schemas:
    Book:
      type: object
      required: [id]
      properties:
        id:
          type: string
          format: uuid
",
        );
        let report = compare_documents(swagger, openapi, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn swagger_and_openapi_schema_dialect_forms_compare_equal() {
        let swagger = parsed(
            "old.json",
            r#"{"swagger":"2.0","info":{"title":"Schemas","version":"1"},"paths":{},"definitions":{"Score":{"type":"number","minimum":1,"exclusiveMinimum":true},"Pet":{"type":"object","discriminator":"kind","properties":{"kind":{"type":"string"}}},"Metadata":{"type":"object","additionalProperties":true}}}"#,
        );
        let openapi = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Schemas","version":"1"},"paths":{},"components":{"schemas":{"Score":{"type":"number","exclusiveMinimum":1},"Pet":{"type":"object","discriminator":{"propertyName":"kind"},"properties":{"kind":{"type":"string"}}},"Metadata":{"type":"object"}}}}"#,
        );

        let report = compare_documents(swagger, openapi, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn swagger_and_openapi_reusable_components_compare_equal() {
        let swagger = parsed(
            "old.json",
            r#"{"swagger":"2.0","info":{"title":"Components","version":"1"},"produces":["application/json"],"paths":{},"parameters":{"Trace":{"name":"X-Trace","in":"header","type":"string"}},"responses":{"Error":{"description":"error","schema":{"type":"string"}}}}"#,
        );
        let openapi = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Components","version":"1"},"paths":{},"components":{"parameters":{"Trace":{"name":"X-Trace","in":"header","schema":{"type":"string"}}},"responses":{"Error":{"description":"error","content":{"application/json":{"schema":{"type":"string"}}}}}}}"#,
        );

        let report = compare_documents(swagger, openapi, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn unused_reusable_component_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"Components","version":"1"},"paths":{},"components":{"responses":{"Error":{"description":"old"}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Components","version":"1"},"paths":{},"components":{"responses":{"Error":{"description":"new"}}}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.differences.iter().any(|difference| {
            difference.code == "component.responses.error.description.changed"
                && difference.location == "/components/responses/Error/description"
        }));
    }

    #[test]
    fn swagger_base_path_matches_openapi_server_path() {
        let swagger = parsed(
            "old.json",
            r#"{"swagger":"2.0","info":{"title":"Books","version":"1"},"host":"api.example.test","schemes":["https"],"basePath":"/v1","paths":{"/books":{"get":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let openapi = parsed(
            "new.json",
            r#"{"openapi":"3.0.3","info":{"title":"Books","version":"1"},"servers":[{"url":"https://api.example.test/v1"}],"paths":{"/books":{"get":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );

        let report = compare_documents(swagger, openapi, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn changed_parameter_type_has_stable_precise_difference() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.0.3","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"limit","in":"query","schema":{"type":"integer"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"limit","in":"query","schema":{"type":"string"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        let difference = report.differences.first().expect("one difference");
        assert_eq!(difference.code, "parameter.schema.type.changed");
        assert_eq!(
            difference.location,
            "/paths/~1x/get/parameters/query~1limit/schema/type"
        );
    }

    #[test]
    fn swagger_tsv_and_csv_array_serialization_do_not_compare_equal() {
        let tsv = parsed(
            "old.json",
            r#"{"swagger":"2.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"ids","in":"query","type":"array","items":{"type":"string"},"collectionFormat":"tsv"}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let csv = parsed(
            "new.json",
            r#"{"swagger":"2.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"ids","in":"query","type":"array","items":{"type":"string"},"collectionFormat":"csv"}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let report = compare_documents(tsv, csv, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(report
            .differences
            .iter()
            .any(|difference| difference.code == "parameter.style.changed"));
    }

    #[test]
    fn security_alternatives_and_and_groups_are_not_conflated() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"security":[{"ApiKey":[],"Bearer":[]}],"paths":{}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"security":[{"ApiKey":[]},{"Bearer":[]}],"paths":{}}"#,
        );
        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert_eq!(report.differences[0].code, "security.requirement.changed");
    }

    #[test]
    fn operation_server_override_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://api.example.test"}],"paths":{"/x":{"get":{"servers":[{"url":"https://edge.example.test"}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://api.example.test"}],"paths":{"/x":{"get":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(report
            .differences
            .iter()
            .any(|difference| difference.code == "operation.servers.changed"));
    }

    #[test]
    fn server_variable_default_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://{region}.example.test","variables":{"region":{"default":"eu","enum":["eu","us"]}}}],"paths":{}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://{region}.example.test","variables":{"region":{"default":"us","enum":["eu","us"]}}}],"paths":{}}"#,
        );
        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(!report.compatible);
        assert!(report
            .differences
            .iter()
            .any(|difference| difference.code == "server.changed"));

        let reordered = parsed(
            "reordered.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://{region}.example.test","variables":{"region":{"default":"eu","enum":["us","eu"]}}}],"paths":{}}"#,
        );
        let baseline = parsed(
            "baseline.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"servers":[{"url":"https://{region}.example.test","variables":{"region":{"default":"eu","enum":["eu","us"]}}}],"paths":{}}"#,
        );
        let report = compare_documents(baseline, reordered, OpenApiCompatibilityPolicy::Exact)
            .expect("compare reordered server enum");
        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn openapi_webhook_response_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"Hooks","version":"1"},"paths":{},"webhooks":{"event":{"post":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Hooks","version":"1"},"paths":{},"webhooks":{"event":{"post":{"responses":{"200":{"description":"changed"}}}}}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(!report.compatible);
        assert!(report.differences.iter().any(|difference| {
            difference.code == "response.missing"
                && difference.location == "/webhooks/event/post/responses/204"
        }));
    }

    #[test]
    fn openapi_path_item_metadata_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"Paths","version":"1"},"paths":{"/events":{"summary":"Old summary","get":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Paths","version":"1"},"paths":{"/events":{"summary":"New summary","get":{"responses":{"204":{"description":"ok"}}}}}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.differences.iter().any(|difference| {
            difference.code == "path.item.summary.changed"
                && difference.location == "/paths/~1events/summary"
        }));
    }

    #[test]
    fn paths_object_extension_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"Paths","version":"1"},"paths":{"x-display":"old"}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"Paths","version":"1"},"paths":{"x-display":"new"}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report.differences.iter().any(|difference| {
            difference.code == "metadata.changed"
                && difference.location == "/metadata/paths.x-display"
        }));
    }

    #[test]
    fn header_parameter_names_compare_case_insensitively() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"X-Trace-Id","in":"header","schema":{"type":"string"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"x-trace-id","in":"header","schema":{"type":"string"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn equivalent_swagger_and_openapi_response_headers_compare_equal() {
        let swagger = parsed(
            "old.json",
            r#"{"swagger":"2.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"responses":{"200":{"description":"ok","headers":{"X-Request-Id":{"description":"request id","type":"string","format":"uuid"}}}}}}}}"#,
        );
        let openapi = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"responses":{"200":{"description":"ok","headers":{"X-Request-Id":{"description":"request id","schema":{"type":"string","format":"uuid"}}}}}}}}}"#,
        );
        let report = compare_documents(swagger, openapi, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");
        assert!(report.compatible, "differences: {:?}", report.differences);
    }

    #[test]
    fn parameter_allow_empty_value_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"filter","in":"query","allowEmptyValue":false,"schema":{"type":"string"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"name":"filter","in":"query","allowEmptyValue":true,"schema":{"type":"string"}}],"responses":{"204":{"description":"ok"}}}}}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report
            .differences
            .iter()
            .any(|difference| difference.code == "parameter.allow_empty_value.changed"));
    }

    #[test]
    fn response_header_explode_change_is_reported() {
        let old = parsed(
            "old.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"responses":{"200":{"description":"ok","headers":{"X-Values":{"explode":false,"schema":{"type":"array","items":{"type":"string"}}}}}}}}}}"#,
        );
        let new = parsed(
            "new.json",
            r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"responses":{"200":{"description":"ok","headers":{"X-Values":{"explode":true,"schema":{"type":"array","items":{"type":"string"}}}}}}}}}}"#,
        );

        let report = compare_documents(old, new, OpenApiCompatibilityPolicy::Exact)
            .expect("compare documents");

        assert!(report
            .differences
            .iter()
            .any(|difference| { difference.code == "response.headers.x-values.explode.changed" }));
    }

    #[test]
    fn changed_external_schema_content_cannot_compare_equal() {
        let old = temp_dir("external-old");
        let new = temp_dir("external-new");
        let root = r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"responses":{"200":{"description":"ok","content":{"application/json":{"schema":{"$ref":"schemas.json#/Book"}}}}}}}}}"#;
        std::fs::write(old.join("openapi.json"), root).expect("write old root");
        std::fs::write(new.join("openapi.json"), root).expect("write new root");
        std::fs::write(old.join("schemas.json"), r#"{"Book":{"type":"string"}}"#)
            .expect("write old schemas");
        std::fs::write(new.join("schemas.json"), r#"{"Book":{"type":"integer"}}"#)
            .expect("write new schemas");

        let report = compare_openapi_files(
            old.join("openapi.json"),
            new.join("openapi.json"),
            OpenApiCompatibilityPolicy::Exact,
        )
        .expect("compare documents");
        assert!(!report.compatible);

        std::fs::remove_dir_all(old).expect("remove old fixtures");
        std::fs::remove_dir_all(new).expect("remove new fixtures");
    }

    #[test]
    fn nested_external_references_resolve_from_the_referring_document() {
        let old = temp_dir("nested-external-old");
        let new = temp_dir("nested-external-new");
        let root = r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{"/x":{"get":{"parameters":[{"$ref":"shared/parameters.json#/BookId"}],"responses":{"204":{"description":"ok"}}}}}}"#;
        let parameters = r#"{"BookId":{"$ref":"types.json#/BookId"}}"#;
        let types = r#"{"BookId":{"name":"bookId","in":"query","required":true,"schema":{"type":"string","format":"uuid"}}}"#;
        for dir in [&old, &new] {
            std::fs::create_dir_all(dir.join("shared")).expect("create shared fixture directory");
            std::fs::write(dir.join("openapi.json"), root).expect("write root document");
            std::fs::write(dir.join("shared/parameters.json"), parameters)
                .expect("write parameter aliases");
            std::fs::write(dir.join("shared/types.json"), types)
                .expect("write parameter definitions");
        }

        let report = compare_openapi_files(
            old.join("openapi.json"),
            new.join("openapi.json"),
            OpenApiCompatibilityPolicy::Exact,
        )
        .expect("compare documents");
        assert!(report.compatible, "differences: {:?}", report.differences);

        std::fs::remove_dir_all(old).expect("remove old fixtures");
        std::fs::remove_dir_all(new).expect("remove new fixtures");
    }
}
