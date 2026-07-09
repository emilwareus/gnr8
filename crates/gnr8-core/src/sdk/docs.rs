//! SDK documentation output policy and built-in markdown renderers.
//!
//! Documentation layout is part of the public SDK package surface during brownfield migrations. This
//! module keeps that policy explicit instead of treating docs as a boolean side effect of code
//! generation.

use std::fmt::Write as _;

use crate::graph::{ApiGraph, Field, MediaExample, OperationDocsPolicy, Type};
use crate::sdk::bundle::safe_frame_name;
use crate::sdk::emit_common::split_words;
use crate::sdk::model::{SdkModel, SdkSchema, SdkSchemaKind, SdkService};
use crate::sdk::Artifacts;
use crate::CoreError;

/// SDK documentation output mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkDocs {
    reference: bool,
    openapi_generator_compat: Option<OpenApiGeneratorDocs>,
    readme_links: bool,
}

/// OpenAPI Generator-compatible documentation layout options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiGeneratorDocs {
    dir: String,
}

impl SdkDocs {
    /// Do not emit generated SDK documentation.
    #[must_use]
    pub fn none() -> Self {
        Self {
            reference: false,
            openapi_generator_compat: None,
            readme_links: false,
        }
    }

    /// Emit the historical gnr8 `README.md` and `reference.md` files.
    #[must_use]
    pub fn reference() -> Self {
        Self {
            reference: true,
            openapi_generator_compat: None,
            readme_links: false,
        }
    }

    /// Emit OpenAPI Generator-compatible per-API and per-model markdown files under `docs/`.
    #[must_use]
    pub fn openapi_generator_compat() -> Self {
        Self {
            reference: false,
            openapi_generator_compat: Some(OpenApiGeneratorDocs::default()),
            readme_links: false,
        }
    }

    /// Emit both the historical gnr8 docs and OpenAPI Generator-compatible per-symbol docs.
    #[must_use]
    pub fn both() -> Self {
        Self {
            reference: true,
            openapi_generator_compat: Some(OpenApiGeneratorDocs::default()),
            readme_links: true,
        }
    }

    /// Set the OpenAPI Generator-compatible docs directory.
    ///
    /// This enables compatibility docs when they were not already enabled, matching the ergonomic
    /// `.docs(SdkDocs::openapi_generator_compat().dir("docs"))` call pattern.
    #[must_use]
    pub fn dir(mut self, dir: impl Into<String>) -> Self {
        let mut compat = self.openapi_generator_compat.unwrap_or_default();
        compat.dir = dir.into();
        self.openapi_generator_compat = Some(compat);
        self
    }

    /// Enable README links to generated reference and compatibility docs.
    #[must_use]
    pub const fn readme_links(mut self, enabled: bool) -> Self {
        self.readme_links = enabled;
        self
    }

    /// Whether no documentation should be emitted.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        !self.reference && self.openapi_generator_compat.is_none()
    }
}

impl Default for SdkDocs {
    fn default() -> Self {
        Self::reference()
    }
}

impl From<bool> for SdkDocs {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::reference()
        } else {
            Self::none()
        }
    }
}

impl OpenApiGeneratorDocs {
    /// Set the generated docs directory, relative to the SDK output directory.
    #[must_use]
    pub fn dir(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }

    fn normalized_dir(&self) -> Result<String, CoreError> {
        let dir = self.dir.trim_matches('/');
        if dir.is_empty() {
            return Err(CoreError::Config {
                message: "OpenAPI Generator-compatible SDK docs dir must not be empty".to_string(),
            });
        }
        safe_frame_name(dir).map_err(|err| CoreError::Config {
            message: format!("{err}"),
        })?;
        Ok(dir.to_string())
    }
}

impl Default for OpenApiGeneratorDocs {
    fn default() -> Self {
        Self {
            dir: "docs".to_string(),
        }
    }
}

/// Write SDK documentation artifacts under `dir`.
///
/// # Errors
///
/// Returns [`CoreError::Config`] when a configured docs directory is unsafe.
pub(crate) fn write_sdk_docs(
    out: &mut Artifacts,
    dir: &str,
    language: &str,
    package: &str,
    ir: &ApiGraph,
    model: &SdkModel,
    docs: &SdkDocs,
) -> Result<(), CoreError> {
    if docs.is_none() {
        return Ok(());
    }
    let dir = dir.trim_end_matches('/');
    let compat_dir = docs
        .openapi_generator_compat
        .as_ref()
        .map(OpenApiGeneratorDocs::normalized_dir)
        .transpose()?;

    if docs.reference {
        out.write(
            format!("{dir}/README.md"),
            sdk_readme(language, package, ir, docs, compat_dir.as_deref()),
        );
        out.write(
            format!("{dir}/reference.md"),
            sdk_reference(language, package, ir),
        );
    } else if docs.readme_links {
        out.write(
            format!("{dir}/README.md"),
            sdk_readme(language, package, ir, docs, compat_dir.as_deref()),
        );
    }

    if let Some(compat_dir) = compat_dir {
        write_openapi_generator_docs(out, dir, language, package, ir, model, &compat_dir);
    }

    Ok(())
}

fn write_openapi_generator_docs(
    out: &mut Artifacts,
    dir: &str,
    language: &str,
    package: &str,
    ir: &ApiGraph,
    model: &SdkModel,
    compat_dir: &str,
) {
    out.write(
        format!("{dir}/{compat_dir}/README.md"),
        compat_docs_index(language, package, model),
    );
    for service in &model.services {
        let symbol = api_doc_symbol(&service.name);
        let file_name = doc_file_name(&symbol);
        out.write(
            format!("{dir}/{compat_dir}/{file_name}"),
            api_doc_page(language, package, ir, model, service, &symbol),
        );
    }
    for schema in &model.schemas {
        let file_name = doc_file_name(&schema.name);
        out.write(
            format!("{dir}/{compat_dir}/{file_name}"),
            model_doc_page(language, package, ir, schema),
        );
    }
}

fn sdk_readme(
    language: &str,
    package: &str,
    ir: &ApiGraph,
    docs: &SdkDocs,
    compat_dir: Option<&str>,
) -> String {
    let mut text = format!(
        "# {title} {language} SDK\n\n\
         This directory is generated by `gnr8`. Do not edit generated files directly; edit \
         `.gnr8/src/main.rs` or the source service, then run `gnr8 generate`.\n\n\
         ## Package\n\n\
         - Language: {language}\n\
         - Package/module: `{package}`\n\
         - Base path: `{base_path}`\n\
         - Operations: {operation_count}\n\
         - Schemas: {schema_count}\n\n",
        title = ir.title,
        base_path = ir.base_path,
        operation_count = ir.operations.len(),
        schema_count = ir.schemas.len()
    );
    if docs.readme_links {
        text.push_str("## Documentation\n\n");
        if docs.reference {
            text.push_str("- [SDK reference](reference.md)\n");
        }
        if let Some(compat_dir) = compat_dir {
            let _ = writeln!(
                text,
                "- [OpenAPI Generator-compatible docs]({compat_dir}/README.md)"
            );
        }
        text.push('\n');
    }
    text.push_str("## Agent workflow\n\n");
    if docs.reference {
        text.push_str("1. Read `reference.md` in this directory for operation and schema names.\n");
    } else if let Some(compat_dir) = compat_dir {
        let _ = writeln!(
            text,
            "1. Read `{compat_dir}/README.md` for operation and model documentation."
        );
    } else {
        text.push_str("1. Inspect generated SDK source files for operation and schema names.\n");
    }
    text.push_str(
        "2. Construct the generated `Client` with the service base URL.\n\
         3. Pass typed request models and path/query parameters according to the generated method signatures.\n\
         4. Handle generated `APIError`/`ApiError` values for non-2xx responses.\n\n",
    );
    match language {
        "Go" => text.push_str(
            "## Go quick start\n\n\
             ```go\n\
             client := sdk.NewClient(\"https://api.example.com\")\n\
             // Call methods on client with context.Context first.\n\
             ```\n",
        ),
        "Python" => text.push_str(
            "## Python quick start\n\n\
             ```python\n\
             from sdk import Client\n\
             client = Client(\"https://api.example.com\")\n\
             # Call generated methods with typed models from this package.\n\
             ```\n",
        ),
        "TypeScript" => text.push_str(
            "## TypeScript quick start\n\n\
             ```typescript\n\
             import { Client } from './client';\n\
             const client = new Client('https://api.example.com');\n\
             // Call generated async methods with typed request objects.\n\
             ```\n",
        ),
        _ => {}
    }
    text
}

fn sdk_reference(language: &str, package: &str, ir: &ApiGraph) -> String {
    let mut text = format!(
        "# SDK Reference\n\n\
         Generated by `gnr8` for `{package}` ({language}).\n\n\
         ## Operations\n\n\
         | Method | Path | Operation | Request | Responses |\n\
         |--|--|--|--|--|\n"
    );
    for op in &ir.operations {
        let request = op
            .request_body
            .as_ref()
            .map_or("-", |request| request.ref_id.as_str());
        let responses = op
            .responses
            .iter()
            .map(|response| response.status.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            text,
            "| {} | `{}` | `{}` | `{}` | {} |",
            op.method, op.path, op.id, request, responses
        );
    }
    append_reference_operation_docs(&mut text, ir);
    text.push_str("\n## Schemas\n\n| Schema | Kind |\n|--|--|\n");
    for schema in &ir.schemas {
        let _ = writeln!(
            text,
            "| `{}` | {} |",
            schema.name,
            schema_kind(&schema.body)
        );
    }
    if !ir.diagnostics.is_empty() {
        text.push_str("\n## Diagnostics\n\n");
        for diagnostic in &ir.diagnostics {
            let _ = writeln!(
                text,
                "- {}: {} ({}:{})",
                diagnostic.severity, diagnostic.message, diagnostic.file, diagnostic.line
            );
        }
    }
    text
}

fn compat_docs_index(language: &str, package: &str, model: &SdkModel) -> String {
    let mut text = format!(
        "# Documentation for {package}\n\n\
         Generated by `gnr8` for the {language} SDK using an OpenAPI Generator-compatible markdown layout.\n\n"
    );
    text.push_str("## APIs\n\n");
    for service in &model.services {
        let symbol = api_doc_symbol(&service.name);
        let _ = writeln!(text, "- [{symbol}]({})", doc_file_name(&symbol));
    }
    text.push_str("\n## Models\n\n");
    for schema in &model.schemas {
        let _ = writeln!(text, "- [{}]({})", schema.name, doc_file_name(&schema.name));
    }
    text
}

fn api_doc_page(
    language: &str,
    package: &str,
    ir: &ApiGraph,
    model: &SdkModel,
    service: &SdkService,
    symbol: &str,
) -> String {
    let mut text = format!(
        "# {symbol}\n\n\
         Generated by `gnr8` for `{package}` ({language}).\n\n\
         ## Operations\n\n"
    );
    for operation_id in &service.operations {
        let Some(operation) = model
            .operations
            .iter()
            .find(|candidate| candidate.id == *operation_id)
        else {
            continue;
        };
        let absolute_path = join_paths(&model.base_path, &operation.path);
        let _ = writeln!(
            text,
            "### `{}`\n\n- Method: `{}`\n- Path: `{}`\n- SDK operation: `{}`\n",
            operation.id, operation.method, absolute_path, operation.handler
        );
        append_operation_docs(
            &mut text,
            model,
            ir.operations
                .iter()
                .find(|raw| raw.id == operation.id)
                .and_then(|raw| operation_docs_policy(ir, &raw.id)),
            &operation.id,
        );
        if let Some(request_schema) = &operation.request_schema {
            let _ = writeln!(
                text,
                "- Request body: [`{request_schema}`]({})",
                doc_file_name(request_schema)
            );
        }
        if !operation.response_schemas.is_empty() {
            text.push_str("- Responses:\n");
            for (status, schema) in &operation.response_schemas {
                let _ = writeln!(
                    text,
                    "  - `{status}`: [`{schema}`]({})",
                    doc_file_name(schema)
                );
            }
        } else if let Some(raw) = ir.operations.iter().find(|raw| raw.id == operation.id) {
            let statuses = raw
                .responses
                .iter()
                .map(|response| response.status.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            if !statuses.is_empty() {
                let _ = writeln!(text, "- Responses: {statuses}");
            }
        }
        text.push('\n');
    }
    text
}

fn append_reference_operation_docs(text: &mut String, ir: &ApiGraph) {
    if ir.operation_docs.is_empty() {
        return;
    }
    text.push_str("\n## Operation Documentation\n\n");
    for policy in &ir.operation_docs {
        let Some(op) = ir
            .operations
            .iter()
            .find(|candidate| candidate.id == policy.operation_id)
        else {
            continue;
        };
        let _ = writeln!(text, "### `{}`\n", op.id);
        append_policy_body(text, None, Some(policy));
    }
}

fn append_operation_docs(
    text: &mut String,
    model: &SdkModel,
    policy: Option<&OperationDocsPolicy>,
    operation_id: &str,
) {
    let docs = model
        .docs_metadata
        .operations
        .iter()
        .find(|docs| docs.operation_id == operation_id);
    if docs.is_none() && policy.is_none() {
        return;
    }
    let has_metadata = docs.is_some_and(|docs| {
        docs.summary.is_some()
            || docs.description.is_some()
            || docs.deprecated
            || !docs.tags.is_empty()
    }) || policy.is_some_and(|policy| {
        policy.summary.is_some()
            || policy.description.is_some()
            || policy.deprecated
            || !policy.tags.is_empty()
            || !policy.request_examples.is_empty()
            || policy
                .responses
                .iter()
                .any(|response| response.description.is_some() || !response.examples.is_empty())
    });
    if !has_metadata {
        return;
    }
    text.push_str("#### Documentation\n\n");
    append_policy_body(text, docs, policy);
}

fn append_policy_body(
    text: &mut String,
    docs: Option<&crate::sdk::model::SdkOperationDocs>,
    policy: Option<&OperationDocsPolicy>,
) {
    let summary = docs
        .and_then(|docs| docs.summary.as_deref())
        .or_else(|| policy.and_then(|policy| policy.summary.as_deref()));
    if let Some(summary) = summary {
        let _ = writeln!(text, "- Summary: {summary}");
    }
    let deprecated = docs.map_or_else(
        || policy.is_some_and(|policy| policy.deprecated),
        |docs| docs.deprecated,
    );
    if deprecated {
        text.push_str("- Deprecated: yes\n");
    }
    let tags = docs
        .map(|docs| docs.tags.as_slice())
        .filter(|tags| !tags.is_empty())
        .or_else(|| {
            policy
                .map(|policy| policy.tags.as_slice())
                .filter(|tags| !tags.is_empty())
        });
    if let Some(tags) = tags {
        let joined = tags
            .iter()
            .map(|tag| format!("`{tag}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(text, "- Tags: {joined}");
    }
    let description = docs
        .and_then(|docs| docs.description.as_deref())
        .or_else(|| policy.and_then(|policy| policy.description.as_deref()));
    if let Some(description) = description {
        let _ = writeln!(text, "\n{description}\n");
    } else if docs.is_some() || policy.is_some() {
        text.push('\n');
    }
    if let Some(policy) = policy {
        append_examples(text, "Request examples", &policy.request_examples);
        for response in &policy.responses {
            if response.description.is_none() && response.examples.is_empty() {
                continue;
            }
            let _ = writeln!(text, "- Response `{}`:", response.status);
            if let Some(description) = &response.description {
                let _ = writeln!(text, "  - Description: {description}");
            }
            append_examples_with_indent(text, "Examples", &response.examples, "  ");
        }
        if !policy.request_examples.is_empty()
            || policy
                .responses
                .iter()
                .any(|response| response.description.is_some() || !response.examples.is_empty())
        {
            text.push('\n');
        }
    }
}

fn append_examples(text: &mut String, label: &str, examples: &[MediaExample]) {
    append_examples_with_indent(text, label, examples, "");
}

fn append_examples_with_indent(
    text: &mut String,
    label: &str,
    examples: &[MediaExample],
    indent: &str,
) {
    if examples.is_empty() {
        return;
    }
    let _ = writeln!(text, "{indent}- {label}:");
    for example in examples {
        let value = serde_json::to_string(&example.value).unwrap_or_else(|_| "null".to_string());
        let _ = writeln!(
            text,
            "{indent}  - `{}` (`{}`): `{}`",
            example.name, example.content_type, value
        );
    }
}

fn operation_docs_policy<'a>(
    ir: &'a ApiGraph,
    operation_id: &str,
) -> Option<&'a OperationDocsPolicy> {
    ir.operation_docs
        .iter()
        .find(|policy| policy.operation_id == operation_id)
}

fn model_doc_page(language: &str, package: &str, ir: &ApiGraph, schema: &SdkSchema) -> String {
    let mut text = format!(
        "# {}\n\n\
         Generated by `gnr8` for `{package}` ({language}).\n\n\
         - Kind: {}\n",
        schema.name,
        sdk_schema_kind(schema.kind)
    );
    if let Some(raw) = ir
        .schemas
        .iter()
        .find(|candidate| candidate.name == schema.name)
    {
        match &raw.body {
            Type::Object(fields) => append_object_fields(&mut text, fields),
            Type::Enum(members) => append_enum_members(&mut text, members),
            other => {
                let _ = writeln!(text, "- Type: `{}`", schema_kind(other));
            }
        }
    }
    text
}

fn append_object_fields(text: &mut String, fields: &[Field]) {
    if fields.is_empty() {
        return;
    }
    text.push_str("\n## Properties\n\n| Name | Required | Type |\n|--|--|--|\n");
    for field in fields {
        let _ = writeln!(
            text,
            "| `{}` | {} | `{}` |",
            field.json_name,
            field.required,
            type_label(&field.schema)
        );
    }
}

fn append_enum_members(text: &mut String, members: &[String]) {
    if members.is_empty() {
        return;
    }
    text.push_str("\n## Enum Values\n\n");
    for member in members {
        let _ = writeln!(text, "- `{member}`");
    }
}

fn api_doc_symbol(service: &str) -> String {
    format!("{}Api", pascal_symbol(service))
}

fn doc_file_name(symbol: &str) -> String {
    let mut name = String::with_capacity(symbol.len() + 3);
    for ch in symbol.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            name.push(ch);
        }
    }
    if name.is_empty() {
        name.push_str("Value");
    }
    if name.starts_with(|ch: char| ch.is_ascii_digit()) {
        name.insert_str(0, "Value");
    }
    name.push_str(".md");
    name
}

fn pascal_symbol(name: &str) -> String {
    let mut out = String::new();
    for word in split_words(name) {
        let lower = word.to_ascii_lowercase();
        let mut chars = lower.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        "Default".to_string()
    } else {
        out
    }
}

fn join_paths(base_path: &str, path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{path}")
    }
}

fn sdk_schema_kind(kind: SdkSchemaKind) -> &'static str {
    match kind {
        SdkSchemaKind::Object => "object",
        SdkSchemaKind::Enum => "enum",
        SdkSchemaKind::Primitive => "primitive",
        SdkSchemaKind::WellKnown => "well-known",
        SdkSchemaKind::Array => "array",
        SdkSchemaKind::Map => "map",
        SdkSchemaKind::Reference => "reference",
        SdkSchemaKind::Union => "union",
        SdkSchemaKind::Any => "any",
    }
}

fn schema_kind(schema: &Type) -> &'static str {
    match schema {
        Type::Object(_) => "object",
        Type::Enum(_) => "enum",
        Type::Primitive(_) => "primitive",
        Type::WellKnown(_) => "well-known",
        Type::Array(_) => "array",
        Type::Map { .. } => "map",
        Type::Named(_) => "reference",
        Type::Union(_) => "union",
        Type::Any {} => "any",
    }
}

fn type_label(ty: &Type) -> String {
    match ty {
        Type::Named(name) => name.clone(),
        Type::Primitive(prim) => format!("{prim:?}").to_ascii_lowercase(),
        Type::WellKnown(well_known) => format!("{well_known:?}").to_ascii_lowercase(),
        Type::Array(inner) => format!("array[{}]", type_label(inner)),
        Type::Map { value, .. } => format!("map[string, {}]", type_label(value)),
        Type::Object(_) => "object".to_string(),
        Type::Enum(_) => "enum".to_string(),
        Type::Union(types) => types.iter().map(type_label).collect::<Vec<_>>().join(" | "),
        Type::Any {} => "any".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{api_doc_symbol, SdkDocs};

    #[test]
    fn bool_conversion_preserves_legacy_docs_switch() {
        assert!(!SdkDocs::from(true).is_none());
        assert!(SdkDocs::from(false).is_none());
    }

    #[test]
    fn api_doc_symbols_match_openapi_generator_style() {
        assert_eq!(api_doc_symbol("books"), "BooksApi");
        assert_eq!(api_doc_symbol("active-schools"), "ActiveSchoolsApi");
        assert_eq!(api_doc_symbol("default"), "DefaultApi");
    }
}
