//! OpenAPI/Swagger artifact source.
//!
//! This module is intentionally source-side only: it parses an existing OpenAPI/Swagger document and
//! normalizes it into the shared [`crate::graph::ApiGraph`]. The existing OpenAPI and SDK targets then
//! consume that graph exactly like code-first sources do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::analyze::facts::{
    Constraints, FieldFact, FieldMeta, LiteralValue, Prim, Type, WellKnown,
};
use crate::graph::{
    ApiGraph, Diagnostic, Operation, Param, Response, Schema, SchemaRef, SecurityScheme, SourceSpan,
};
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecVersion {
    Swagger2,
    OpenApi30,
    OpenApi31,
}

#[derive(Debug, Clone)]
struct ImportedType {
    ty: Type,
    nullable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportedResponseKind {
    Json,
    Binary,
}

impl ImportedType {
    fn new(ty: Type) -> Self {
        Self {
            ty,
            nullable: false,
        }
    }
}

struct Importer {
    project_root: PathBuf,
    root_file: PathBuf,
    root: Value,
    version: SpecVersion,
    raw_schemas: BTreeMap<String, Value>,
    schema_names: BTreeMap<String, String>,
    used_schema_names: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
    external_docs: BTreeMap<PathBuf, Value>,
    used_operation_ids: BTreeSet<String>,
}

/// Load an OpenAPI/Swagger document from `input`, resolved against `project_root`.
///
/// # Errors
///
/// Returns [`CoreError::Config`] for parse/shape errors and [`CoreError::Io`] for file reads.
pub(crate) fn load_openapi(project_root: &Path, input: &str) -> Result<ApiGraph, CoreError> {
    let path = project_root.join(input);
    let text = read_text(&path)?;
    import_openapi_document(project_root, path, &text)
}

fn import_openapi_document(
    project_root: &Path,
    root_file: PathBuf,
    text: &str,
) -> Result<ApiGraph, CoreError> {
    let root = parse_json_or_yaml(text, &root_file)?;
    let version = detect_version(&root, &root_file)?;
    let mut importer = Importer::new(project_root.to_path_buf(), root_file, root, version);
    importer.import()
}

pub(super) fn validate_openapi_artifact(text: &str, path: &Path) -> Result<(), CoreError> {
    let root = parse_json_or_yaml(text, path)?;
    detect_version(&root, path)?;
    validate_operation_ids(&root, path)?;
    validate_schema_names(&root, path)?;
    validate_local_refs(&root, path)
}

fn read_text(path: &Path) -> Result<String, CoreError> {
    std::fs::read_to_string(path).map_err(|err| CoreError::Io {
        message: format!("failed to read OpenAPI source '{}': {err}", path.display()),
    })
}

fn parse_json_or_yaml(text: &str, path: &Path) -> Result<Value, CoreError> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => Ok(value),
        Err(json_err) => noyalib::from_str::<Value>(text).map_err(|yaml_err| CoreError::Config {
            message: format!(
                "failed to parse OpenAPI source '{}': JSON error: {json_err}; YAML error: {yaml_err}",
                path.display()
            ),
        }),
    }
}

fn detect_version(root: &Value, path: &Path) -> Result<SpecVersion, CoreError> {
    if root.get("swagger").and_then(Value::as_str) == Some("2.0") {
        return Ok(SpecVersion::Swagger2);
    }
    let Some(openapi) = root.get("openapi").and_then(Value::as_str) else {
        return Err(CoreError::Config {
            message: format!(
                "OpenAPI source '{}' must contain swagger: \"2.0\" or openapi: \"3.x.x\"",
                path.display()
            ),
        });
    };
    if openapi.starts_with("3.0.") || openapi == "3.0" {
        Ok(SpecVersion::OpenApi30)
    } else if openapi.starts_with("3.1.") || openapi == "3.1" {
        Ok(SpecVersion::OpenApi31)
    } else {
        Err(CoreError::Config {
            message: format!(
                "OpenAPI source '{}' uses unsupported openapi version '{openapi}'",
                path.display()
            ),
        })
    }
}

fn validate_operation_ids(root: &Value, path: &Path) -> Result<(), CoreError> {
    let Some(paths) = root.get("paths").and_then(Value::as_object) else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    for (route, item) in paths {
        let Some(methods) = item.as_object() else {
            continue;
        };
        for (method, operation) in methods {
            if !is_openapi_method(method) {
                continue;
            }
            let Some(id) = operation.get("operationId").and_then(Value::as_str) else {
                return Err(CoreError::Config {
                    message: format!(
                        "OpenAPI artifact '{}' operation {} {} is missing operationId",
                        path.display(),
                        method.to_ascii_uppercase(),
                        route
                    ),
                });
            };
            if id.trim().is_empty() || id.chars().any(char::is_whitespace) {
                return Err(CoreError::Config {
                    message: format!(
                        "OpenAPI artifact '{}' operation {} {} has unstable operationId {:?}",
                        path.display(),
                        method.to_ascii_uppercase(),
                        route,
                        id
                    ),
                });
            }
            if !seen.insert(id.to_string()) {
                return Err(CoreError::Config {
                    message: format!(
                        "OpenAPI artifact '{}' contains duplicate operationId {:?}",
                        path.display(),
                        id
                    ),
                });
            }
        }
    }
    Ok(())
}

fn is_openapi_method(method: &str) -> bool {
    matches!(
        method,
        "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
    )
}

fn validate_schema_names(root: &Value, path: &Path) -> Result<(), CoreError> {
    let Some(schemas) = root
        .pointer("/components/schemas")
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    for name in schemas.keys() {
        if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
            return Err(CoreError::Config {
                message: format!(
                    "OpenAPI artifact '{}' contains unstable schema name {:?}",
                    path.display(),
                    name
                ),
            });
        }
    }
    Ok(())
}

fn validate_local_refs(root: &Value, path: &Path) -> Result<(), CoreError> {
    let mut refs = Vec::new();
    collect_ref_values(root, &mut refs);
    for ref_value in refs {
        let Some(fragment) = ref_value.strip_prefix('#') else {
            continue;
        };
        if root.pointer(fragment).is_none() {
            return Err(CoreError::Config {
                message: format!(
                    "OpenAPI artifact '{}' contains unresolved local ref {ref_value:?}",
                    path.display()
                ),
            });
        }
    }
    Ok(())
}

fn collect_ref_values<'a>(value: &'a Value, refs: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => {
            if let Some(ref_value) = map.get("$ref").and_then(Value::as_str) {
                refs.push(ref_value);
            }
            for child in map.values() {
                collect_ref_values(child, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_ref_values(item, refs);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

impl Importer {
    fn new(project_root: PathBuf, root_file: PathBuf, root: Value, version: SpecVersion) -> Self {
        Self {
            project_root,
            root_file,
            root,
            version,
            raw_schemas: BTreeMap::new(),
            schema_names: BTreeMap::new(),
            used_schema_names: BTreeSet::new(),
            diagnostics: Vec::new(),
            external_docs: BTreeMap::new(),
            used_operation_ids: BTreeSet::new(),
        }
    }

    fn import(&mut self) -> Result<ApiGraph, CoreError> {
        self.validate_representable_security()?;
        self.validate_representable_responses()?;
        self.collect_root_schemas();
        let security = self.import_security_schemes();
        let mut operations = self.import_operations();
        let mut schemas = self.import_schemas();

        operations.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.method.cmp(&b.method)));
        schemas.sort_by(|a, b| a.id.cmp(&b.id));
        self.diagnostics
            .sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.line.cmp(&b.line)));

        Ok(ApiGraph {
            module: self
                .root
                .get("info")
                .and_then(|info| info.get("title"))
                .and_then(Value::as_str)
                .map_or_else(|| "openapi".to_string(), module_slug),
            operations,
            schemas,
            diagnostics: std::mem::take(&mut self.diagnostics),
            base_path: self.base_path(),
            title: self
                .root
                .get("info")
                .and_then(|info| info.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("API")
                .to_string(),
            security,
            runtime: crate::graph::RuntimePolicy::default(),
            operation_runtime: Vec::new(),
            pagination: Vec::new(),
        })
    }

    fn validate_representable_security(&self) -> Result<(), CoreError> {
        let root_security = self.root.get("security").and_then(Value::as_array);
        let raw_schemes = self.raw_security_schemes();
        if root_security.is_some_and(|requirements| requirements.len() > 1) {
            return Err(CoreError::Config {
                message: "OpenAPI source uses alternative top-level security requirements; gnr8 currently supports one AND requirement object, not OR alternatives".to_string(),
            });
        }
        if let Some(requirement) = root_security
            .and_then(|requirements| requirements.first())
            .and_then(Value::as_object)
        {
            for id in requirement.keys() {
                Self::validate_security_scheme_ref(&raw_schemes, id, "top-level security")?;
            }
        }
        let has_inherited_security =
            !security_requirement_scheme_ids(self.root.get("security")).is_empty();
        let Some(paths) = self.root.get("paths").and_then(Value::as_object) else {
            return Ok(());
        };
        for (path, item) in paths {
            let Some(path_item) = item.as_object() else {
                continue;
            };
            for (method, operation) in path_item {
                if !is_http_method(method) || !is_lowerable_method(method) {
                    continue;
                }
                let Some(operation_object) = operation.as_object() else {
                    continue;
                };
                let Some(security) = operation_object.get("security") else {
                    continue;
                };
                let Some(requirements) = security.as_array() else {
                    continue;
                };
                let op_label = operation_object
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || format!("{} {}", method.to_ascii_uppercase(), path),
                        str::to_string,
                    );
                if requirements.is_empty() && has_inherited_security {
                    return Err(CoreError::Config {
                        message: format!(
                            "operation '{op_label}' disables inherited security with security: []; gnr8 graph cannot represent per-operation security removal"
                        ),
                    });
                }
                if requirements.len() > 1 {
                    return Err(CoreError::Config {
                        message: format!(
                            "operation '{op_label}' uses alternative security requirements; gnr8 currently supports one AND requirement object, not OR alternatives"
                        ),
                    });
                }
                if has_inherited_security
                    && requirements
                        .first()
                        .and_then(Value::as_object)
                        .is_some_and(serde_json::Map::is_empty)
                {
                    return Err(CoreError::Config {
                        message: format!(
                            "operation '{op_label}' disables inherited security with an empty security requirement; gnr8 graph cannot represent per-operation security removal"
                        ),
                    });
                }
                if let Some(requirement) = requirements.first().and_then(Value::as_object) {
                    for id in requirement.keys() {
                        Self::validate_security_scheme_ref(
                            &raw_schemes,
                            id,
                            &format!("operation '{op_label}' security"),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn raw_security_schemes(&self) -> BTreeMap<String, Value> {
        match self.version {
            SpecVersion::Swagger2 => self
                .root
                .get("securityDefinitions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => self
                .root
                .get("components")
                .and_then(|components| components.get("securitySchemes"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        }
        .into_iter()
        .collect()
    }

    fn validate_security_scheme_ref(
        schemes: &BTreeMap<String, Value>,
        id: &str,
        context: &str,
    ) -> Result<(), CoreError> {
        let Some(scheme) = schemes.get(id) else {
            return Err(CoreError::Config {
                message: format!(
                    "{context} references missing security scheme '{id}' in OpenAPI source"
                ),
            });
        };
        let kind = scheme.get("type").and_then(Value::as_str).unwrap_or("");
        let location = scheme.get("in").and_then(Value::as_str).unwrap_or("");
        let name = scheme.get("name").and_then(Value::as_str);
        let http_scheme = scheme.get("scheme").and_then(Value::as_str);
        let supported = match kind {
            "apiKey" => matches!(location, "header" | "query") && name.is_some(),
            "http" => matches!(http_scheme, Some("bearer" | "basic")),
            _ => false,
        };
        if supported {
            return Ok(());
        }
        Err(CoreError::Config {
            message: format!(
                "{context} references unsupported security scheme '{id}'; gnr8 imports apiKey/header, apiKey/query, http/bearer, and http/basic schemes only"
            ),
        })
    }

    fn validate_representable_responses(&self) -> Result<(), CoreError> {
        let Some(paths) = self.root.get("paths").and_then(Value::as_object) else {
            return Ok(());
        };
        for (path, item) in paths {
            let Some(path_item) = item.as_object() else {
                continue;
            };
            for (method, operation) in path_item {
                if !is_http_method(method) || !is_lowerable_method(method) {
                    continue;
                }
                let Some(operation_object) = operation.as_object() else {
                    continue;
                };
                let op_label = operation_object
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || format!("{} {}", method.to_ascii_uppercase(), path),
                        str::to_string,
                    );
                let Some(responses) = operation_object.get("responses").and_then(Value::as_object)
                else {
                    continue;
                };
                for status in responses.keys() {
                    if status.parse::<u16>().is_err() {
                        return Err(CoreError::Config {
                            message: format!(
                                "operation '{op_label}' uses non-numeric response key '{status}'; gnr8 graph cannot represent default responses from OpenAPI sources"
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn import_security_schemes(&mut self) -> Vec<SecurityScheme> {
        let raw_schemes = self.raw_security_schemes();
        let global_ids = security_requirement_scheme_ids(self.root.get("security"));
        let mut schemes = Vec::new();
        for (id, scheme) in raw_schemes {
            let kind = scheme.get("type").and_then(Value::as_str).unwrap_or("");
            let location = scheme.get("in").and_then(Value::as_str).unwrap_or("");
            match kind {
                "apiKey" => {
                    let Some(name) = scheme.get("name").and_then(Value::as_str) else {
                        self.warn(format!(
                            "security scheme '{id}' has no apiKey name and was not imported"
                        ));
                        continue;
                    };
                    if !matches!(location, "header" | "query") {
                        self.warn(format!(
                            "security scheme '{id}' uses unsupported apiKey/{location}; only apiKey/header and apiKey/query are imported"
                        ));
                        continue;
                    }
                    schemes.push(SecurityScheme {
                        id: id.clone(),
                        kind: "apiKey".to_string(),
                        location: location.to_string(),
                        name: name.to_string(),
                        global: global_ids.contains(&id),
                    });
                }
                "http" => {
                    let http_scheme = scheme.get("scheme").and_then(Value::as_str).unwrap_or("");
                    if !matches!(http_scheme, "bearer" | "basic") {
                        self.warn(format!(
                            "security scheme '{id}' uses unsupported http/{http_scheme}; only http/bearer and http/basic are imported"
                        ));
                        continue;
                    }
                    schemes.push(SecurityScheme {
                        id: id.clone(),
                        kind: "http".to_string(),
                        location: String::new(),
                        name: http_scheme.to_string(),
                        global: global_ids.contains(&id),
                    });
                }
                _ => {
                    self.warn(format!(
                        "security scheme '{id}' uses unsupported type {kind}; only apiKey and http are imported"
                    ));
                }
            }
        }
        schemes.sort_by(|a, b| a.id.cmp(&b.id));
        schemes
    }

    fn collect_root_schemas(&mut self) {
        let raw: Vec<(String, Value)> = match self.version {
            SpecVersion::Swagger2 => self
                .root
                .get("definitions")
                .and_then(Value::as_object)
                .map(|schemas| {
                    schemas
                        .iter()
                        .map(|(id, schema)| (id.clone(), schema.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => self
                .root
                .get("components")
                .and_then(|components| components.get("schemas"))
                .and_then(Value::as_object)
                .map(|schemas| {
                    schemas
                        .iter()
                        .map(|(id, schema)| (id.clone(), schema.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        };
        for (id, schema) in raw {
            self.raw_schemas.entry(id).or_insert(schema);
        }
    }

    fn base_path(&self) -> String {
        if self.version == SpecVersion::Swagger2 {
            return self
                .root
                .get("basePath")
                .and_then(Value::as_str)
                .map_or_else(|| "/".to_string(), normalize_path);
        }
        self.root
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|servers| servers.first())
            .and_then(|server| server.get("url"))
            .and_then(Value::as_str)
            .map_or_else(|| "/".to_string(), server_url_path)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keeps path/operation normalization state in one auditable pass"
    )]
    fn import_operations(&mut self) -> Vec<Operation> {
        let path_items: Vec<(String, Value)> = self
            .root
            .get("paths")
            .and_then(Value::as_object)
            .map(|paths| {
                paths
                    .iter()
                    .map(|(path, item)| (path.clone(), item.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let mut operations = Vec::new();
        for (path, item) in path_items {
            let Some(item_object) = item.as_object() else {
                self.warn(format!("path item '{path}' is not an object"));
                continue;
            };
            let path_parameters = item_object
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (method, operation) in item_object {
                if !is_http_method(method) {
                    continue;
                }
                if !is_lowerable_method(method) {
                    self.warn(format!(
                        "operation {} {} uses HTTP method '{}', which gnr8 cannot lower yet",
                        method.to_uppercase(),
                        path,
                        method.to_uppercase()
                    ));
                    continue;
                }
                let Some(operation_object) = operation.as_object() else {
                    self.warn(format!(
                        "operation {} {path} is not an object",
                        method.to_uppercase()
                    ));
                    continue;
                };
                let operation_id = self.operation_id(method, &path, operation);
                let group = operation_object
                    .get("tags")
                    .and_then(Value::as_array)
                    .and_then(|tags| tags.first())
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let mut params = Vec::new();
                let mut form_fields = Vec::new();
                let mut request_body = None;
                let mut request_body_required = false;
                let mut request_body_content_type = None;

                let mut all_parameters = path_parameters.clone();
                if let Some(operation_parameters) =
                    operation_object.get("parameters").and_then(Value::as_array)
                {
                    all_parameters.extend(operation_parameters.iter().cloned());
                }

                for parameter in all_parameters {
                    let parameter = self.resolve_parameter(parameter);
                    let Some(parameter_object) = parameter.as_object() else {
                        self.warn(format!(
                            "parameter on {} {path} is not an object",
                            method.to_uppercase()
                        ));
                        continue;
                    };
                    match parameter_object.get("in").and_then(Value::as_str) {
                        Some("path" | "query") => {
                            if let Some(param) = self.import_param(&parameter) {
                                params.push(param);
                            }
                        }
                        Some("body") if self.version == SpecVersion::Swagger2 => {
                            if let Some(schema) = parameter_object.get("schema") {
                                request_body = Some(
                                    self.schema_ref_for(schema, &format!("{operation_id}Request")),
                                );
                                request_body_required = parameter_object
                                    .get("required")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                            }
                        }
                        Some("formData") if self.version == SpecVersion::Swagger2 => {
                            if let Some(field) = self.field_from_parameter(&parameter) {
                                request_body_required |= field.required;
                                form_fields.push(field);
                            }
                        }
                        Some(location) => self.warn(format!(
                            "parameter location '{location}' on {} {path} is not imported",
                            method.to_uppercase()
                        )),
                        None => self.warn(format!(
                            "parameter on {} {path} has no 'in' location",
                            method.to_uppercase()
                        )),
                    }
                }

                if request_body.is_none() {
                    request_body = match self.version {
                        SpecVersion::Swagger2 => {
                            if form_fields.is_empty() {
                                None
                            } else {
                                request_body_content_type = Some(
                                    if form_fields
                                        .iter()
                                        .any(|field| type_contains_bytes(&field.schema))
                                    {
                                        "multipart/form-data".to_string()
                                    } else {
                                        "application/x-www-form-urlencoded".to_string()
                                    },
                                );
                                Some(self.insert_synthetic_schema(
                                    &format!("{operation_id}FormRequest"),
                                    Type::Object(form_fields),
                                ))
                            }
                        }
                        SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => self
                            .request_body_schema_ref(operation, &operation_id)
                            .map(|(schema_ref, media_type)| {
                                request_body_required = operation
                                    .get("requestBody")
                                    .and_then(|body| body.get("required"))
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                if media_type != "application/json" {
                                    request_body_content_type = Some(media_type);
                                }
                                schema_ref
                            }),
                    };
                }

                params.sort_by(|a, b| {
                    a.name
                        .cmp(&b.name)
                        .then_with(|| a.location.cmp(&b.location))
                });
                let mut responses = self.import_responses(operation, &operation_id);
                responses.sort_by_key(|response| response.status);
                let security = self.operation_security(operation_object, &operation_id);
                let security_overrides_global = operation_object.contains_key("security");

                operations.push(Operation {
                    id: operation_id.clone(),
                    method: method.to_uppercase(),
                    path: normalize_path(&path),
                    handler: operation_id,
                    group,
                    middleware: Vec::new(),
                    params,
                    request_body,
                    request_body_required,
                    request_body_content_type,
                    responses,
                    security,
                    security_overrides_global,
                    provenance: self.span(),
                });
            }
        }
        operations
    }

    fn operation_security(
        &mut self,
        operation_object: &serde_json::Map<String, Value>,
        operation_id: &str,
    ) -> Vec<String> {
        let Some(security) = operation_object.get("security") else {
            return Vec::new();
        };
        let Some(requirements) = security.as_array() else {
            self.warn(format!(
                "security on operation '{operation_id}' is not an array and was not imported"
            ));
            return Vec::new();
        };
        if requirements.is_empty() {
            if self
                .root
                .get("security")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
            {
                self.warn(format!(
                    "operation '{operation_id}' disables inherited security; gnr8 graph cannot represent per-operation security removal"
                ));
            }
            return Vec::new();
        }
        if requirements.len() > 1 {
            self.warn(format!(
                "operation '{operation_id}' has alternative security requirements; importing merged scheme ids"
            ));
        }
        let mut ids = Vec::new();
        for requirement in requirements {
            let Some(object) = requirement.as_object() else {
                self.warn(format!(
                    "security requirement on operation '{operation_id}' is not an object"
                ));
                continue;
            };
            ids.extend(object.keys().cloned());
        }
        ids.sort();
        ids.dedup();
        ids
    }

    fn operation_id(&mut self, method: &str, path: &str, operation: &Value) -> String {
        let base = operation
            .get("operationId")
            .and_then(Value::as_str)
            .map_or_else(|| generated_operation_id(method, path), sanitize_identifier);
        unique_name(base, &mut self.used_operation_ids)
    }

    fn resolve_parameter(&mut self, parameter: Value) -> Value {
        let Some(ref_value) = parameter.get("$ref").and_then(Value::as_str) else {
            return parameter;
        };
        if let Some((_, schema)) = self.resolve_ref_schema(ref_value) {
            return schema;
        }
        self.warn(format!(
            "parameter reference '{ref_value}' could not be resolved"
        ));
        parameter
    }

    fn import_param(&mut self, parameter: &Value) -> Option<Param> {
        let name = parameter.get("name").and_then(Value::as_str)?.to_string();
        let location = parameter.get("in").and_then(Value::as_str)?.to_string();
        let required = parameter
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(location == "path");
        let schema = parameter.get("schema").unwrap_or(parameter);
        let default = schema.get("default").and_then(literal_value);
        let imported = self.type_from_schema(schema);
        Some(Param {
            name,
            location,
            required,
            schema: imported.ty,
            default,
            provenance: self.span(),
        })
    }

    fn field_from_parameter(&mut self, parameter: &Value) -> Option<FieldFact> {
        let name = parameter.get("name").and_then(Value::as_str)?.to_string();
        let required = parameter
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let imported = self.type_from_schema(parameter);
        Some(FieldFact {
            json_name: name,
            required,
            optional: !required,
            nullable: imported.nullable,
            schema: imported.ty,
            description: parameter
                .get("description")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            example: parameter
                .get("example")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            meta: field_meta_from_schema(parameter),
        })
    }

    fn request_body_schema_ref(
        &mut self,
        operation: &Value,
        operation_id: &str,
    ) -> Option<(SchemaRef, String)> {
        let request_body = operation.get("requestBody")?;
        if request_body.get("$ref").is_some() {
            self.warn(format!(
                "requestBody $ref on operation '{operation_id}' is not imported yet"
            ));
            return None;
        }
        let Some(content) = request_body.get("content").and_then(Value::as_object) else {
            self.warn(format!(
                "requestBody on operation '{operation_id}' has no content object"
            ));
            return None;
        };
        let Some((media_type, media)) = choose_content(content) else {
            self.warn(format!(
                "requestBody on operation '{operation_id}' has no supported media type"
            ));
            return None;
        };
        if !is_supported_request_media(media_type) {
            self.warn(format!(
                "requestBody media type '{media_type}' on operation '{operation_id}' is imported as a schema only"
            ));
        }
        let Some(schema) = media.get("schema") else {
            self.warn(format!(
                "requestBody media type '{media_type}' on operation '{operation_id}' has no schema"
            ));
            return None;
        };
        Some((
            self.schema_ref_for(schema, &format!("{operation_id}Request")),
            media_type.to_string(),
        ))
    }

    fn import_responses(&mut self, operation: &Value, operation_id: &str) -> Vec<Response> {
        let mut responses = Vec::new();
        let Some(response_map) = operation.get("responses").and_then(Value::as_object) else {
            return responses;
        };
        for (status, response) in response_map {
            let Ok(status_code) = status.parse::<u16>() else {
                continue;
            };
            if matches!(
                self.version,
                SpecVersion::OpenApi30 | SpecVersion::OpenApi31
            ) {
                if let Some(media) = response
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(|content| content.get("text/event-stream"))
                {
                    responses.push(Response {
                        status: status_code,
                        body: media.get("schema").map(|schema| {
                            self.schema_ref_for(
                                schema,
                                &format!("{operation_id}{status_code}Event"),
                            )
                        }),
                        body_kind: "sse".to_string(),
                        content_type: Some("text/event-stream".to_string()),
                        content_types: vec!["text/event-stream".to_string()],
                    });
                    continue;
                }
            }
            let selected: Option<(String, &Value)> = match self.version {
                SpecVersion::Swagger2 => response
                    .get("schema")
                    .map(|schema| (self.swagger_response_media_type(operation), schema)),
                SpecVersion::OpenApi30 | SpecVersion::OpenApi31 => response
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(choose_content)
                    .and_then(|(media_type, media)| {
                        media
                            .get("schema")
                            .map(|schema| (media_type.to_string(), schema))
                    }),
            };
            let Some((media_type, schema)) = selected else {
                responses.push(Response {
                    status: status_code,
                    body: None,
                    body_kind: "empty".to_string(),
                    content_type: None,
                    content_types: Vec::new(),
                });
                continue;
            };
            if self.response_schema_is_binary(schema) {
                let content_type =
                    if self.version == SpecVersion::Swagger2 && media_type == "application/json" {
                        "application/octet-stream".to_string()
                    } else {
                        media_type
                    };
                let content_types = self.response_content_types_for_kind(
                    response,
                    &content_type,
                    ImportedResponseKind::Binary,
                );
                responses.push(Response {
                    status: status_code,
                    body: None,
                    body_kind: "binary".to_string(),
                    content_type: Some(content_type.clone()),
                    content_types,
                });
                continue;
            }
            let content_types = self.response_content_types_for_kind(
                response,
                &media_type,
                ImportedResponseKind::Json,
            );
            responses.push(Response {
                status: status_code,
                body: Some(
                    self.schema_ref_for(schema, &format!("{operation_id}{status_code}Response")),
                ),
                body_kind: "json".to_string(),
                content_type: (media_type != "application/json").then_some(media_type.clone()),
                content_types,
            });
        }
        responses
    }

    fn response_content_types_for_kind(
        &mut self,
        response: &Value,
        selected_media_type: &str,
        kind: ImportedResponseKind,
    ) -> Vec<String> {
        let mut content_types = vec![selected_media_type.to_string()];
        let Some(content) = response.get("content").and_then(Value::as_object) else {
            return content_types;
        };
        for (media_type, media) in content {
            if media_type == selected_media_type {
                continue;
            }
            let Some(schema) = media.get("schema") else {
                continue;
            };
            let is_binary = self.response_schema_is_binary(schema);
            if (kind == ImportedResponseKind::Binary && is_binary)
                || (kind == ImportedResponseKind::Json && !is_binary)
            {
                content_types.push(media_type.clone());
            }
        }
        content_types
    }

    fn swagger_response_media_type(&self, operation: &Value) -> String {
        operation
            .get("produces")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(Value::as_str)
            .or_else(|| {
                self.root
                    .get("produces")
                    .and_then(Value::as_array)
                    .and_then(|values| values.first())
                    .and_then(Value::as_str)
            })
            .unwrap_or("application/json")
            .to_string()
    }

    fn response_schema_is_binary(&mut self, schema: &Value) -> bool {
        if is_binary_response_schema(schema) {
            return true;
        }
        let Some(ref_value) = schema.get("$ref").and_then(Value::as_str) else {
            return false;
        };
        self.resolve_ref_schema(ref_value)
            .is_some_and(|(_, raw)| is_binary_response_schema(&raw))
    }

    fn schema_ref_for(&mut self, schema: &Value, suggested: &str) -> SchemaRef {
        if let Some(ref_value) = schema.get("$ref").and_then(Value::as_str) {
            if let Some((id, _)) = self.resolve_ref_schema(ref_value) {
                return SchemaRef { ref_id: id };
            }
            self.warn(format!(
                "schema reference '{ref_value}' could not be resolved"
            ));
        }
        self.insert_raw_synthetic_schema(suggested, schema.clone())
    }

    fn insert_raw_synthetic_schema(&mut self, suggested: &str, schema: Value) -> SchemaRef {
        let id = unique_synthetic_id(suggested, &self.raw_schemas);
        self.raw_schemas.insert(id.clone(), schema);
        SchemaRef { ref_id: id }
    }

    fn insert_synthetic_schema(&mut self, suggested: &str, ty: Type) -> SchemaRef {
        let id = unique_synthetic_id(suggested, &self.raw_schemas);
        let name = self.schema_name(&id);
        let schema = Schema {
            id: id.clone(),
            name: name.clone(),
            body: ty,
            enum_source_order: Vec::new(),
            provenance: self.span(),
        };
        self.schema_names.insert(id.clone(), name);
        self.inject_imported_schema(&schema);
        SchemaRef { ref_id: id }
    }

    fn inject_imported_schema(&mut self, schema: &Schema) {
        let schema_value = serde_json::json!({
            "x-gnr8-imported-schema": {
                "id": &schema.id,
                "name": &schema.name,
                "body": &schema.body
            }
        });
        let id = schema_value
            .get("x-gnr8-imported-schema")
            .and_then(|data| data.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !id.is_empty() {
            self.raw_schemas.insert(id, schema_value);
        }
    }

    fn import_schemas(&mut self) -> Vec<Schema> {
        let mut imported_ids = BTreeSet::new();
        let mut schemas = Vec::new();
        loop {
            let next_id = self
                .raw_schemas
                .keys()
                .find(|id| !imported_ids.contains(*id))
                .cloned();
            let Some(id) = next_id else {
                break;
            };
            imported_ids.insert(id.clone());
            let Some(raw) = self.raw_schemas.get(&id).cloned() else {
                continue;
            };
            if let Some(schema) = self.import_prebuilt_schema(&raw) {
                schemas.push(schema);
                continue;
            }
            let imported = self.type_from_schema(&raw);
            let enum_source_order = match &imported.ty {
                Type::Enum(values) => values.clone(),
                _ => Vec::new(),
            };
            schemas.push(Schema {
                id: id.clone(),
                name: self.schema_name(&id),
                body: imported.ty,
                enum_source_order,
                provenance: self.span(),
            });
        }
        schemas
    }

    fn import_prebuilt_schema(&mut self, raw: &Value) -> Option<Schema> {
        let data = raw.get("x-gnr8-imported-schema")?;
        let id = data.get("id").and_then(Value::as_str)?.to_string();
        let name = data.get("name").and_then(Value::as_str)?.to_string();
        let body = serde_json::from_value::<Type>(data.get("body")?.clone()).ok()?;
        Some(Schema {
            id,
            name,
            body,
            enum_source_order: Vec::new(),
            provenance: self.span(),
        })
    }

    fn schema_name(&mut self, id: &str) -> String {
        if let Some(existing) = self.schema_names.get(id) {
            return existing.clone();
        }
        let name = unique_name(type_name(id), &mut self.used_schema_names);
        self.schema_names.insert(id.to_string(), name.clone());
        name
    }

    #[expect(
        clippy::too_many_lines,
        reason = "central schema-type dispatch keeps OpenAPI normalization exhaustive"
    )]
    fn type_from_schema(&mut self, schema: &Value) -> ImportedType {
        if let Some(ref_value) = schema.get("$ref").and_then(Value::as_str) {
            if let Some((id, _)) = self.resolve_ref_schema(ref_value) {
                return ImportedType::new(Type::Named(id));
            }
            self.warn(format!(
                "schema reference '{ref_value}' could not be resolved"
            ));
            return ImportedType::new(Type::Any {});
        }

        if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
            let nullable = schema
                .get("nullable")
                .and_then(Value::as_bool)
                .or_else(|| schema.get("x-nullable").and_then(Value::as_bool))
                .unwrap_or(false);
            return ImportedType {
                ty: self.type_from_all_of(all_of),
                nullable,
            };
        }

        if let Some(one_of) = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
        {
            let mut nullable = schema
                .get("nullable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut variants = Vec::new();
            for variant in one_of.clone() {
                let imported = self.type_from_schema(&variant);
                nullable |= imported.nullable;
                variants.push(imported.ty);
            }
            return ImportedType {
                ty: Type::Union(variants),
                nullable,
            };
        }

        let (schema_type, nullable_from_type_array) = schema_type(schema);
        let nullable = schema
            .get("nullable")
            .and_then(Value::as_bool)
            .or_else(|| schema.get("x-nullable").and_then(Value::as_bool))
            .unwrap_or(false)
            || nullable_from_type_array;

        if let Some(enum_values) = string_enum_values(schema) {
            return ImportedType {
                ty: Type::Enum(enum_values.values),
                nullable: nullable || enum_values.nullable,
            };
        }

        match schema_type.as_deref() {
            Some("array") => {
                let item_type = schema.get("items").map_or_else(
                    || ImportedType::new(Type::Any {}),
                    |items| self.type_from_schema(items),
                );
                ImportedType {
                    ty: Type::Array(Box::new(item_type.ty)),
                    nullable,
                }
            }
            Some("object") => ImportedType {
                ty: self.object_type_from_schema(schema),
                nullable,
            },
            Some("string" | "file") => ImportedType {
                ty: string_type(schema),
                nullable,
            },
            Some("integer") => ImportedType {
                ty: Type::Primitive(Prim::Int {
                    bits: integer_bits(schema),
                    signed: true,
                }),
                nullable,
            },
            Some("number") => ImportedType {
                ty: Type::Primitive(Prim::Float {
                    bits: number_bits(schema),
                }),
                nullable,
            },
            Some("boolean") => ImportedType {
                ty: Type::Primitive(Prim::Bool),
                nullable,
            },
            Some(other) => {
                self.warn(format!(
                    "schema type '{other}' is not supported; importing as any"
                ));
                ImportedType {
                    ty: Type::Any {},
                    nullable,
                }
            }
            None => {
                if schema.get("properties").is_some()
                    || schema.get("additionalProperties").is_some()
                {
                    ImportedType {
                        ty: self.object_type_from_schema(schema),
                        nullable,
                    }
                } else {
                    ImportedType {
                        ty: Type::Any {},
                        nullable,
                    }
                }
            }
        }
    }

    fn type_from_all_of(&mut self, all_of: &[Value]) -> Type {
        let mut fields = BTreeMap::new();
        let mut merged_any = false;
        for member in all_of {
            match self.object_fields_from_composed_schema(member) {
                Some(member_fields) => {
                    for field in member_fields {
                        fields.insert(field.json_name.clone(), field);
                    }
                }
                None => merged_any = true,
            }
        }
        if merged_any && fields.is_empty() {
            self.warn("allOf composition had no object members; importing as any".to_string());
            return Type::Any {};
        }
        Type::Object(fields.into_values().collect())
    }

    fn object_fields_from_composed_schema(&mut self, schema: &Value) -> Option<Vec<FieldFact>> {
        if let Some(ref_value) = schema.get("$ref").and_then(Value::as_str) {
            let Some((_, resolved)) = self.resolve_ref_schema(ref_value) else {
                self.warn(format!(
                    "allOf reference '{ref_value}' could not be resolved"
                ));
                return None;
            };
            return self.object_fields_from_composed_schema(&resolved);
        }
        match self.type_from_schema(schema).ty {
            Type::Object(fields) => Some(fields),
            other => {
                self.warn(format!(
                    "allOf member '{other:?}' is not an object; member was skipped"
                ));
                None
            }
        }
    }

    fn object_type_from_schema(&mut self, schema: &Value) -> Type {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            let required = required_set(schema);
            let mut fields = Vec::new();
            for (name, property_schema) in properties {
                let imported = self.type_from_schema(property_schema);
                let is_required = required.contains(name);
                fields.push(FieldFact {
                    json_name: name.clone(),
                    required: is_required,
                    optional: !is_required,
                    nullable: imported.nullable,
                    schema: imported.ty,
                    description: property_schema
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    example: property_schema
                        .get("example")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    meta: field_meta_from_schema(property_schema),
                });
            }
            fields.sort_by(|a, b| a.json_name.cmp(&b.json_name));
            return Type::Object(fields);
        }
        match schema.get("additionalProperties") {
            Some(Value::Object(_)) => {
                let imported = self.type_from_schema(&schema["additionalProperties"]);
                Type::Map {
                    key: Box::new(Type::Primitive(Prim::String)),
                    value: Box::new(imported.ty),
                }
            }
            Some(Value::Bool(true)) => Type::Map {
                key: Box::new(Type::Primitive(Prim::String)),
                value: Box::new(Type::Any {}),
            },
            _ => Type::Object(Vec::new()),
        }
    }

    fn resolve_ref_schema(&mut self, ref_value: &str) -> Option<(String, Value)> {
        let (file_part, pointer) = split_ref(ref_value);
        let id = schema_id_from_pointer(pointer).unwrap_or_else(|| sanitize_identifier(ref_value));
        if file_part.is_empty() {
            if let Some(schema) = self.raw_schemas.get(&id).cloned() {
                return Some((id, schema));
            }
            let resolved = resolve_pointer(&self.root, pointer)?;
            self.raw_schemas.insert(id.clone(), resolved.clone());
            return Some((id, resolved));
        }

        let external_doc = self.external_doc(file_part)?;
        let resolved = resolve_pointer(&external_doc, pointer)?;
        self.raw_schemas
            .entry(id.clone())
            .or_insert_with(|| resolved.clone());
        Some((id, resolved))
    }

    fn external_doc(&mut self, file_part: &str) -> Option<Value> {
        let base_dir = self
            .root_file
            .parent()
            .unwrap_or(self.project_root.as_path());
        let path = base_dir.join(file_part);
        if let Some(doc) = self.external_docs.get(&path) {
            return Some(doc.clone());
        }
        let text = match read_text(&path) {
            Ok(text) => text,
            Err(err) => {
                self.warn(format!(
                    "failed to read external OpenAPI ref '{}': {err}",
                    path.display()
                ));
                return None;
            }
        };
        match parse_json_or_yaml(&text, &path) {
            Ok(doc) => {
                self.external_docs.insert(path, doc.clone());
                Some(doc)
            }
            Err(err) => {
                self.warn(format!("failed to parse external OpenAPI ref: {err}"));
                None
            }
        }
    }

    fn span(&self) -> SourceSpan {
        SourceSpan {
            file: self.display_file(),
            start_line: 1,
            end_line: 1,
        }
    }

    fn warn(&mut self, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: "WARN".to_string(),
            message,
            file: self.display_file(),
            line: 1,
        });
    }

    fn display_file(&self) -> String {
        self.root_file.strip_prefix(&self.project_root).map_or_else(
            |_| self.root_file.to_string_lossy().to_string(),
            |path| path.to_string_lossy().to_string(),
        )
    }
}

fn split_ref(ref_value: &str) -> (&str, &str) {
    match ref_value.split_once('#') {
        Some((file, pointer)) => {
            let pointer = if pointer.is_empty() { "/" } else { pointer };
            (file, pointer)
        }
        None => ("", ref_value),
    }
}

fn schema_id_from_pointer(pointer: &str) -> Option<String> {
    pointer
        .trim_start_matches('#')
        .trim_start_matches('/')
        .split('/')
        .next_back()
        .filter(|segment| !segment.is_empty())
        .map(decode_pointer_segment)
}

fn resolve_pointer(doc: &Value, pointer: &str) -> Option<Value> {
    let pointer = pointer.trim_start_matches('#');
    if pointer.is_empty() || pointer == "/" {
        return Some(doc.clone());
    }
    let mut current = doc;
    for raw in pointer.trim_start_matches('/').split('/') {
        let key = decode_pointer_segment(raw);
        current = current.get(&key)?;
    }
    Some(current.clone())
}

fn decode_pointer_segment(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}

fn schema_type(schema: &Value) -> (Option<String>, bool) {
    match schema.get("type") {
        Some(Value::String(value)) => (Some(value.clone()), false),
        Some(Value::Array(values)) => {
            let mut nullable = false;
            let mut ty = None;
            for value in values {
                if value.as_str() == Some("null") {
                    nullable = true;
                } else if ty.is_none() {
                    ty = value.as_str().map(ToString::to_string);
                }
            }
            (ty, nullable)
        }
        _ => {
            if schema.get("format").and_then(Value::as_str) == Some("binary")
                || schema.get("format").and_then(Value::as_str) == Some("byte")
            {
                (Some("string".to_string()), false)
            } else {
                (None, false)
            }
        }
    }
}

struct EnumValues {
    values: Vec<String>,
    nullable: bool,
}

fn string_enum_values(schema: &Value) -> Option<EnumValues> {
    let values = schema.get("enum")?.as_array()?;
    let mut enum_values = Vec::new();
    let mut nullable = false;
    for value in values {
        if value.is_null() {
            nullable = true;
        } else if let Some(member) = value.as_str() {
            enum_values.push(member.to_string());
        } else {
            return None;
        }
    }
    enum_values.sort();
    enum_values.dedup();
    Some(EnumValues {
        values: enum_values,
        nullable,
    })
}

fn string_type(schema: &Value) -> Type {
    match schema.get("format").and_then(Value::as_str) {
        Some("uuid") => Type::WellKnown(WellKnown::Uuid),
        Some("date-time") => Type::WellKnown(WellKnown::DateTime),
        Some("date") => Type::WellKnown(WellKnown::Date),
        Some("email") => Type::WellKnown(WellKnown::Email),
        Some("uri" | "url") => Type::WellKnown(WellKnown::Uri),
        Some("binary" | "byte") => Type::Primitive(Prim::Bytes),
        _ if schema.get("type").and_then(Value::as_str) == Some("file") => {
            Type::Primitive(Prim::Bytes)
        }
        _ => Type::Primitive(Prim::String),
    }
}

fn is_binary_response_schema(schema: &Value) -> bool {
    matches!(
        schema.get("format").and_then(Value::as_str),
        Some("binary" | "byte")
    ) || schema.get("type").and_then(Value::as_str) == Some("file")
}

fn integer_bits(schema: &Value) -> u16 {
    match schema.get("format").and_then(Value::as_str) {
        Some("int32") => 32,
        _ => 64,
    }
}

fn number_bits(schema: &Value) -> u16 {
    match schema.get("format").and_then(Value::as_str) {
        Some("float") => 32,
        _ => 64,
    }
}

fn required_set(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn field_meta_from_schema(schema: &Value) -> FieldMeta {
    FieldMeta {
        constraints: Constraints {
            min_length: schema.get("minLength").and_then(Value::as_u64),
            max_length: schema.get("maxLength").and_then(Value::as_u64),
            minimum: schema.get("minimum").map(json_number_or_string),
            maximum: schema.get("maximum").map(json_number_or_string),
            exclusive_minimum: schema.get("exclusiveMinimum").map(json_number_or_string),
            exclusive_maximum: schema.get("exclusiveMaximum").map(json_number_or_string),
            pattern: schema
                .get("pattern")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            enum_values: string_enum_values(schema).map_or_else(Vec::new, |values| values.values),
        },
        default: schema.get("default").and_then(literal_value),
        format: schema
            .get("format")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        extensions: Vec::new(),
    }
}

fn json_number_or_string(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), ToString::to_string)
}

fn literal_value(value: &Value) -> Option<LiteralValue> {
    if value.is_null() {
        Some(LiteralValue::Null)
    } else if let Some(value) = value.as_str() {
        Some(LiteralValue::String(value.to_string()))
    } else if let Some(value) = value.as_bool() {
        Some(LiteralValue::Bool(value))
    } else if value.is_number() {
        Some(LiteralValue::Number(value.to_string()))
    } else {
        None
    }
}

fn security_requirement_scheme_ids(security: Option<&Value>) -> BTreeSet<String> {
    security
        .and_then(Value::as_array)
        .map(|requirements| {
            requirements
                .iter()
                .filter_map(Value::as_object)
                .flat_map(|requirement| requirement.keys().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn choose_content(content: &serde_json::Map<String, Value>) -> Option<(&str, &Value)> {
    for preferred in [
        "application/json",
        "multipart/form-data",
        "application/x-www-form-urlencoded",
    ] {
        if let Some(media) = content.get(preferred) {
            return Some((preferred, media));
        }
    }
    content
        .iter()
        .next()
        .map(|(media_type, media)| (media_type.as_str(), media))
}

fn is_supported_request_media(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/json" | "multipart/form-data" | "application/x-www-form-urlencoded"
    )
}

fn type_contains_bytes(ty: &Type) -> bool {
    match ty {
        Type::Primitive(Prim::Bytes) => true,
        Type::Array(item) => type_contains_bytes(item),
        Type::Map { key, value } => type_contains_bytes(key) || type_contains_bytes(value),
        Type::Object(fields) => fields
            .iter()
            .any(|field| type_contains_bytes(&field.schema)),
        Type::Union(variants) => variants.iter().any(type_contains_bytes),
        Type::Primitive(_) | Type::WellKnown(_) | Type::Named(_) | Type::Enum(_) | Type::Any {} => {
            false
        }
    }
}

fn is_http_method(method: &str) -> bool {
    matches!(
        method,
        "get" | "put" | "post" | "delete" | "patch" | "options" | "head" | "trace"
    )
}

fn is_lowerable_method(method: &str) -> bool {
    matches!(method, "get" | "put" | "post" | "patch" | "delete")
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        format!("/{}", trimmed.trim_matches('/'))
    }
}

fn server_url_path(url: &str) -> String {
    if url.starts_with('/') {
        return normalize_path(url);
    }
    if let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) {
        if let Some(path_start) = after_scheme.find('/') {
            return normalize_path(&after_scheme[path_start..]);
        }
    }
    "/".to_string()
}

fn module_slug(title: &str) -> String {
    let slug = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}

fn sanitize_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.is_empty() {
                out.push(ch.to_ascii_lowercase());
            } else if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        "operation".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("op{out}")
    } else {
        out
    }
}

fn generated_operation_id(method: &str, path: &str) -> String {
    let joined = format!("{method}_{path}");
    sanitize_identifier(&joined)
}

fn type_name(id: &str) -> String {
    let mut out = String::new();
    for segment in id.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        "Schema".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("Model{out}")
    } else {
        out
    }
}

fn unique_name(base: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    let mut index = 2_u32;
    loop {
        let candidate = format!("{base}{index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn unique_synthetic_id(suggested: &str, raw_schemas: &BTreeMap<String, Value>) -> String {
    let base = type_name(suggested);
    if !raw_schemas.contains_key(&base) {
        return base;
    }
    let mut index = 2_u32;
    loop {
        let candidate = format!("{base}{index}");
        if !raw_schemas.contains_key(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::load_openapi;
    use super::validate_openapi_artifact;
    use super::{detect_version, import_openapi_document, parse_json_or_yaml, SpecVersion};
    use crate::analyze::facts::{Prim, Type};
    use crate::lower::to_openapi;
    use crate::sdk::builtins::{OpenApi, OpenApi31Json, TsSdk};
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::{Cx, Pipeline};

    #[test]
    fn detects_supported_spec_versions() {
        let path = std::path::Path::new("openapi.yaml");
        let swagger = parse_json_or_yaml(
            "swagger: '2.0'\ninfo: {title: T, version: v}\npaths: {}",
            path,
        )
        .unwrap();
        let oas30 = parse_json_or_yaml(
            "openapi: 3.0.3\ninfo: {title: T, version: v}\npaths: {}",
            path,
        )
        .unwrap();
        let oas31 = parse_json_or_yaml(
            "openapi: 3.1.0\ninfo: {title: T, version: v}\npaths: {}",
            path,
        )
        .unwrap();
        assert_eq!(
            detect_version(&swagger, path).unwrap(),
            SpecVersion::Swagger2
        );
        assert_eq!(
            detect_version(&oas30, path).unwrap(),
            SpecVersion::OpenApi30
        );
        assert_eq!(
            detect_version(&oas31, path).unwrap(),
            SpecVersion::OpenApi31
        );
    }

    #[test]
    fn validate_openapi_artifact_checks_local_refs_and_operation_ids() {
        let path = std::path::Path::new("generated/openapi.yaml");
        let valid = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Ready API", "version": "1.0.0" },
  "paths": {
    "/books": {
      "get": {
        "operationId": "listBooks",
        "responses": {
          "200": {
            "description": "OK",
            "content": {
              "application/json": {
                "schema": { "$ref": "#/components/schemas/Book" }
              }
            }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "Book": { "type": "object" }
    }
  }
}"##;
        validate_openapi_artifact(valid, path).unwrap();

        let broken_ref = valid.replace("#/components/schemas/Book", "#/components/schemas/Missing");
        let err = validate_openapi_artifact(&broken_ref, path).unwrap_err();
        assert!(
            err.to_string().contains("unresolved local ref"),
            "unexpected error: {err}"
        );

        let missing_id = valid.replace("\"operationId\": \"listBooks\",\n", "");
        let err = validate_openapi_artifact(&missing_id, path).unwrap_err();
        assert!(
            err.to_string().contains("missing operationId"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn imports_openapi31_all_of_nullable_maps_and_tags() {
        let text = r"
openapi: 3.1.0
info: { title: Brownfield API, version: 1.0.0 }
servers: [{ url: /v1 }]
paths:
  /books/{id}:
    get:
      tags: [Books]
      operationId: getBook
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: string, format: uuid }
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema: { $ref: '#/components/schemas/legacy.Book' }
components:
  schemas:
    BookBase:
      type: object
      required: [title]
      properties:
        title: { type: string }
    legacy.Book:
      allOf:
        - { $ref: '#/components/schemas/BookBase' }
        - type: object
          required: [id, metadata]
          properties:
            id: { type: string, format: uuid }
            status: { type: string, enum: [draft, published] }
            note: { type: [string, 'null'] }
            metadata:
              type: object
              additionalProperties: { type: string }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();
        assert_eq!(graph.base_path, "/v1");
        assert_eq!(graph.operations[0].group.as_deref(), Some("Books"));
        let book = graph
            .schemas
            .iter()
            .find(|schema| schema.id == "legacy.Book")
            .unwrap();
        let Type::Object(fields) = &book.body else {
            panic!("book should import as object");
        };
        assert!(fields
            .iter()
            .any(|field| field.json_name == "title" && field.required));
        assert!(fields
            .iter()
            .any(|field| field.json_name == "note" && field.nullable));
        assert!(fields.iter().any(|field| {
            field.json_name == "metadata" && matches!(field.schema, Type::Map { .. })
        }));
    }

    #[test]
    fn imports_openapi31_patch_and_binary_response() {
        let text = r"
openapi: 3.1.0
info: { title: Files API, version: 1.0.0 }
paths:
  /files/{id}:
    patch:
      tags: [Files]
      operationId: updateFile
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: string }
      responses:
        '200':
          description: file
          content:
            application/pdf:
              schema: { type: string, format: binary }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();
        assert_eq!(graph.operations.len(), 1);
        let op = &graph.operations[0];
        assert_eq!(op.method, "PATCH");
        assert_eq!(op.id, "updateFile");
        assert_eq!(op.group.as_deref(), Some("Files"));
        assert_eq!(op.responses.len(), 1);
        let response = &op.responses[0];
        assert_eq!(response.status, 200);
        assert!(response.body.is_none());
        assert_eq!(response.body_kind, "binary");
        assert_eq!(response.content_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn imports_openapi31_binary_response_preserves_same_kind_content_types() {
        let text = r"
openapi: 3.1.0
info: { title: Files API, version: 1.0.0 }
paths:
  /exports/{id}:
    get:
      operationId: downloadExport
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: string }
      responses:
        '200':
          description: raw export
          content:
            application/json:
              schema: { type: string, format: binary }
            application/pdf:
              schema: { type: string, format: binary }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();

        let response = &graph.operations[0].responses[0];
        assert_eq!(response.body_kind, "binary");
        assert_eq!(response.content_type.as_deref(), Some("application/json"));
        assert_eq!(
            response.content_types,
            vec![
                "application/json".to_string(),
                "application/pdf".to_string()
            ]
        );
    }

    #[test]
    fn imports_openapi31_json_response_preserves_same_kind_content_types() {
        let text = r"
openapi: 3.1.0
info: { title: Files API, version: 1.0.0 }
paths:
  /reports/{id}:
    get:
      operationId: getReport
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: string }
      responses:
        '200':
          description: report metadata
          content:
            application/json:
              schema:
                type: object
                properties:
                  id: { type: string }
            application/vnd.acme.report+json:
              schema:
                type: object
                properties:
                  id: { type: string }
            application/pdf:
              schema: { type: string, format: binary }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();

        let response = &graph.operations[0].responses[0];
        assert_eq!(response.body_kind, "json");
        assert_eq!(response.content_type, None);
        assert_eq!(
            response.content_types,
            vec![
                "application/json".to_string(),
                "application/vnd.acme.report+json".to_string()
            ]
        );
    }

    #[test]
    fn imports_swagger20_form_data_file_upload() {
        let text = r"
swagger: '2.0'
info: { title: Upload API, version: 1.0.0 }
basePath: /api
paths:
  /upload:
    post:
      operationId: uploadFile
      consumes: [multipart/form-data]
      parameters:
        - name: file
          in: formData
          required: true
          type: file
        - name: note
          in: formData
          type: string
      responses:
        '201':
          description: created
          schema: { $ref: '#/definitions/UploadResponse' }
definitions:
  UploadResponse:
    type: object
    required: [id]
    properties:
      id: { type: string }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("swagger.yaml"),
            text,
        )
        .unwrap();
        assert_eq!(graph.base_path, "/api");
        assert!(graph.operations[0].request_body_required);
        let request = graph
            .schemas
            .iter()
            .find(|schema| schema.id == "UploadFileFormRequest")
            .unwrap();
        let Type::Object(fields) = &request.body else {
            panic!("request should import as object");
        };
        assert!(fields.iter().any(|field| {
            field.json_name == "file" && matches!(field.schema, Type::Primitive(Prim::Bytes))
        }));
    }

    #[test]
    fn imports_openapi31_security_schemes_and_operation_security() {
        let text = r"
openapi: 3.1.0
info: { title: Secure API, version: 1.0.0 }
security:
  - ApiKeyAuth: []
    QueryAuth: []
    BearerAuth: []
paths:
  /write:
    post:
      operationId: writeEndpoint
      security:
        - CSRFAuth: []
          BasicAuth: []
      responses: { '204': { description: ok } }
components:
  securitySchemes:
    ApiKeyAuth:
      type: apiKey
      in: header
      name: X-API-Key
    QueryAuth:
      type: apiKey
      in: query
      name: api_key
    BearerAuth:
      type: http
      scheme: bearer
    BasicAuth:
      type: http
      scheme: basic
    CSRFAuth:
      type: apiKey
      in: header
      name: X-CSRF-Token
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();
        assert_eq!(graph.security.len(), 5);
        assert!(graph
            .security
            .iter()
            .any(|scheme| scheme.id == "ApiKeyAuth" && scheme.global));
        assert!(graph.security.iter().any(|scheme| {
            scheme.id == "QueryAuth" && scheme.location == "query" && scheme.name == "api_key"
        }));
        assert!(graph.security.iter().any(|scheme| {
            scheme.id == "BearerAuth"
                && scheme.kind == "http"
                && scheme.location.is_empty()
                && scheme.name == "bearer"
                && scheme.global
        }));
        assert!(graph.security.iter().any(|scheme| {
            scheme.id == "BasicAuth"
                && scheme.kind == "http"
                && scheme.location.is_empty()
                && scheme.name == "basic"
                && !scheme.global
        }));
        let write = graph
            .operations
            .iter()
            .find(|operation| operation.id == "writeEndpoint")
            .unwrap();
        assert_eq!(write.security, vec!["BasicAuth", "CSRFAuth"]);
        assert!(write.security_overrides_global);

        let yaml = to_openapi(&graph, "Secure API", "/", &graph.security).unwrap();
        let write_block = yaml
            .split("operationId: writeEndpoint")
            .nth(1)
            .expect("writeEndpoint operation");
        let write_block = write_block
            .split("responses:")
            .next()
            .unwrap_or(write_block);
        assert!(
            write_block.contains("security:\n        - BasicAuth: []")
                && write_block.contains("CSRFAuth: []"),
            "imported operation security must keep OpenAPI override semantics:\n{write_block}"
        );
        assert!(
            write_block.contains("BasicAuth: []"),
            "imported operation security must keep HTTP basic override:\n{write_block}"
        );
        assert!(
            !write_block.contains("ApiKeyAuth: []"),
            "imported operation security must not inherit top-level security:\n{write_block}"
        );
    }

    #[test]
    fn rejects_openapi_security_shapes_that_graph_cannot_represent() {
        let top_level_or = r"
openapi: 3.1.0
info: { title: Secure API, version: 1.0.0 }
security:
  - ApiKeyAuth: []
  - PartnerAuth: []
paths: {}
";
        let err = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            top_level_or,
        )
        .unwrap_err();
        assert!(err.to_string().contains("alternative top-level security"));

        let security_removal = r"
openapi: 3.1.0
info: { title: Secure API, version: 1.0.0 }
security:
  - ApiKeyAuth: []
paths:
  /public:
    get:
      operationId: publicEndpoint
      security: []
      responses: { '204': { description: ok } }
components:
  securitySchemes:
    ApiKeyAuth:
      type: apiKey
      in: header
      name: X-API-Key
";
        let err = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            security_removal,
        )
        .unwrap_err();
        assert!(err.to_string().contains("disables inherited security"));

        let missing_scheme = r"
openapi: 3.1.0
info: { title: Secure API, version: 1.0.0 }
security:
  - MissingAuth: []
paths: {}
";
        let err = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            missing_scheme,
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing security scheme"));

        let unsupported_scheme = r"
openapi: 3.1.0
info: { title: Secure API, version: 1.0.0 }
security:
  - OAuth: []
paths: {}
components:
  securitySchemes:
    OAuth:
      type: oauth2
      flows: {}
";
        let err = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            unsupported_scheme,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported security scheme"));

        let anonymous_global_security = r"
openapi: 3.1.0
info: { title: Public API, version: 1.0.0 }
security:
  - {}
paths:
  /public:
    get:
      operationId: publicEndpoint
      security: []
      responses: { '204': { description: ok } }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            anonymous_global_security,
        )
        .unwrap();
        assert_eq!(graph.operations[0].id, "publicEndpoint");
    }

    #[test]
    fn rejects_openapi_default_response_key() {
        let text = r"
openapi: 3.1.0
info: { title: Default Response API, version: 1.0.0 }
paths:
  /items:
    get:
      operationId: listItems
      responses:
        default:
          description: fallback
";
        let err = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-numeric response key"));

        let unsupported_method = r"
openapi: 3.1.0
info: { title: Default Response API, version: 1.0.0 }
paths:
  /items:
    options:
      operationId: itemOptions
      responses:
        default:
          description: fallback
    get:
      operationId: listItems
      responses:
        '204':
          description: ok
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            unsupported_method,
        )
        .unwrap();
        assert_eq!(graph.operations.len(), 1);
        assert_eq!(graph.operations[0].id, "listItems");
    }

    #[test]
    fn imports_openapi31_sse_response() {
        let text = r"
openapi: 3.1.0
info: { title: Events API, version: 1.0.0 }
paths:
  /events:
    get:
      operationId: streamEvents
      responses:
        '200':
          description: events
          content:
            application/json:
              schema: { type: object }
            text/event-stream:
              schema: { $ref: '#/components/schemas/EventEnvelope' }
components:
  schemas:
    EventEnvelope:
      type: object
      required: [message]
      properties:
        message: { type: string }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("openapi.yaml"),
            text,
        )
        .unwrap();
        let response = &graph.operations[0].responses[0];
        assert_eq!(response.body_kind, "sse");
        assert_eq!(response.content_type.as_deref(), Some("text/event-stream"));
        assert_eq!(
            response.body.as_ref().map(|body| body.ref_id.as_str()),
            Some("EventEnvelope")
        );
    }

    #[test]
    fn imports_swagger20_file_response_as_binary() {
        let text = r"
swagger: '2.0'
info: { title: Files API, version: 1.0.0 }
basePath: /api
produces: [application/pdf]
paths:
  /download:
    get:
      operationId: downloadFile
      responses:
        '200':
          description: file
          schema: { type: file }
";
        let graph = import_openapi_document(
            std::path::Path::new("."),
            std::path::PathBuf::from("swagger.yaml"),
            text,
        )
        .unwrap();
        let response = &graph.operations[0].responses[0];
        assert!(response.body.is_none());
        assert_eq!(response.body_kind, "binary");
        assert_eq!(response.content_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn loads_brownfield_fixture_versions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for input in [
            "fixtures/brownfield-openapi/swagger20.yaml",
            "fixtures/brownfield-openapi/openapi30.yaml",
            "fixtures/brownfield-openapi/openapi31.yaml",
        ] {
            let graph = load_openapi(&root, input).unwrap();
            assert!(
                graph
                    .operations
                    .iter()
                    .any(|operation| operation.id == "getBook"),
                "{input} should import getBook"
            );
            assert!(
                graph
                    .schemas
                    .iter()
                    .any(|schema| schema.id == "legacy.Book"),
                "{input} should import dotted schema names"
            );
            assert!(
                graph
                    .schemas
                    .iter()
                    .any(|schema| schema.name == "LegacyBook2"),
                "{input} should disambiguate naming collisions"
            );
        }

        let graph = load_openapi(&root, "fixtures/brownfield-openapi/openapi31.yaml").unwrap();
        assert!(
            graph
                .schemas
                .iter()
                .any(|schema| schema.id == "Shared.Error"),
            "OpenAPI 3.1 fixture should import a relative external $ref"
        );
    }

    #[test]
    fn public_pipeline_generates_openapi_and_typescript_from_openapi_source() {
        let root = temp_project("pipeline");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/brownfield-openapi/openapi30.yaml"),
            root.join("openapi.yaml"),
        )
        .unwrap();

        let outcome = Pipeline::new()
            .source(OpenApi::new().input("openapi.yaml"))
            .target(OpenApi31Json::new().to("generated/openapi.json"))
            .target(
                TsSdk::new()
                    .module("@acme/books")
                    .to("generated/ts")
                    .profile(SdkProfile::typescript_fetch_compat()),
            )
            .run(&Cx::new(&root))
            .unwrap();

        let paths = outcome
            .artifacts
            .files()
            .iter()
            .map(|artifact| artifact.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"generated/openapi.json"));
        assert!(paths.contains(&"generated/ts/index.ts"));
        assert!(paths.contains(&"generated/ts/runtime.ts"));
        assert!(paths.contains(&"generated/ts/apis/index.ts"));
        assert!(paths.contains(&"generated/ts/models/index.ts"));

        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_project(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gnr8-openapi-source-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
