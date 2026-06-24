//! Deterministic, key-ordered `OpenAPI` 3.1.0 YAML writer (RESEARCH Pattern 1).
//!
//! Walks the typed [`super::model::OpenApiDoc`] and emits block-style YAML with keys in a FIXED,
//! spec-conventional order (NOT serde field order, NOT a `HashMap`'s arbitrary order). No YAML crate
//! is used: `serde_yaml` is deprecated/absent (RESEARCH Alternatives), and byte-exact key ordering is
//! required to reconcile with `fixtures/goalservice/expected/openapi.yaml`.
//!
//! `OpenAPI` 3.1 specifics enforced here (RESEARCH Pattern 2): `openapi: 3.1.0`; NO `nullable` key and
//! NO `type: [T, "null"]` array form (optionality is required-omission only); `$ref` is JSON-pointer
//! form `'#/components/schemas/Name'` and QUOTED; `additionalProperties: true` for free-form maps;
//! `format` is emitted alongside `type`. Indentation is two-space block style.
//!
//! The writer is total: it returns a [`String`] and never fails (a programming-error empty `$ref` is
//! surfaced as a typed [`crate::CoreError`] by [`super::to_openapi`], not here).

use super::model::{
    Components, Info, OpenApiDoc, Operation, Parameter, PathItem, RequestBody, ResponseObj,
    SchemaObject, SecurityRequirement, SecurityScheme,
};
use std::fmt::Write as _;

/// Two spaces — the block-style indentation unit.
const INDENT: &str = "  ";

/// Serialize a typed [`OpenApiDoc`] to a deterministic `OpenAPI` 3.1.0 YAML string.
///
/// Keys are emitted in fixed spec order (`openapi`, `info`, `security`, `paths`, `components`); the
/// output is byte-identical across two calls over the same document (no `HashMap` iteration).
pub(crate) fn write(doc: &OpenApiDoc) -> String {
    let mut out = String::new();
    // `openapi: 3.1.0` is always the first line.
    let _ = writeln!(out, "openapi: {}", doc.openapi);
    write_info(&mut out, &doc.info);
    write_security(&mut out, &doc.security);
    write_paths(&mut out, &doc.paths);
    write_components(&mut out, &doc.components);
    out
}

/// Emit the `info` block (`title`, `version`, optional `description`).
fn write_info(out: &mut String, info: &Info) {
    let _ = writeln!(out, "info:");
    let _ = writeln!(out, "{INDENT}title: {}", scalar(&info.title));
    let _ = writeln!(out, "{INDENT}version: {}", scalar(&info.version));
    if let Some(desc) = &info.description {
        let _ = writeln!(out, "{INDENT}description: {}", scalar(desc));
    }
}

/// Emit the top-level `security` list (`- ApiKeyAuth: []`). Omitted entirely when empty.
fn write_security(out: &mut String, security: &[SecurityRequirement]) {
    if security.is_empty() {
        return;
    }
    let _ = writeln!(out, "security:");
    for req in security {
        let _ = writeln!(out, "{INDENT}- {}: {}", req.scheme, flow_seq(&req.scopes));
    }
}

/// Emit the `paths` map (each path → its method operations).
fn write_paths(out: &mut String, paths: &[(String, PathItem)]) {
    let _ = writeln!(out, "paths:");
    for (path, item) in paths {
        // Path keys are quoted: `/goal/{uuid}` contains `{` which YAML would otherwise mis-parse.
        let _ = writeln!(out, "{INDENT}{}:", quote(path));
        write_path_item(out, item, 2);
    }
}

/// Emit the operations of one [`PathItem`] in fixed HTTP-method order.
fn write_path_item(out: &mut String, item: &PathItem, depth: usize) {
    let pad = INDENT.repeat(depth);
    for (method, op) in [
        ("get", &item.get),
        ("post", &item.post),
        ("put", &item.put),
        ("delete", &item.delete),
    ] {
        if let Some(op) = op {
            let _ = writeln!(out, "{pad}{method}:");
            write_operation(out, op, depth + 1);
        }
    }
}

/// Emit one operation's keys in fixed order: `summary`, `operationId`, `tags`, `parameters`,
/// `requestBody`, `responses`.
fn write_operation(out: &mut String, op: &Operation, depth: usize) {
    let pad = INDENT.repeat(depth);
    if let Some(summary) = &op.summary {
        let _ = writeln!(out, "{pad}summary: {}", scalar(summary));
    }
    let _ = writeln!(out, "{pad}operationId: {}", scalar(&op.operation_id));
    if !op.tags.is_empty() {
        let _ = writeln!(out, "{pad}tags: {}", flow_seq(&op.tags));
    }
    if !op.parameters.is_empty() {
        let _ = writeln!(out, "{pad}parameters:");
        for param in &op.parameters {
            write_parameter(out, param, depth);
        }
    }
    if let Some(body) = &op.request_body {
        write_request_body(out, body, depth);
    }
    write_responses(out, &op.responses, depth);
}

/// Emit one parameter list entry (`- name: .. / in: .. / required: .. / description: .. / schema: ..`).
fn write_parameter(out: &mut String, param: &Parameter, depth: usize) {
    let pad = INDENT.repeat(depth);
    let _ = writeln!(out, "{pad}- name: {}", scalar(&param.name));
    let _ = writeln!(out, "{pad}  in: {}", param.location);
    let _ = writeln!(out, "{pad}  required: {}", param.required);
    if let Some(desc) = &param.description {
        let _ = writeln!(out, "{pad}  description: {}", scalar(desc));
    }
    let _ = writeln!(out, "{pad}  schema:");
    write_schema(out, &param.schema, depth + 2);
}

/// Emit a `requestBody` with `application/json` content referencing a component schema.
fn write_request_body(out: &mut String, body: &RequestBody, depth: usize) {
    let pad = INDENT.repeat(depth);
    let _ = writeln!(out, "{pad}requestBody:");
    let _ = writeln!(out, "{pad}{INDENT}required: {}", body.required);
    let _ = writeln!(out, "{pad}{INDENT}content:");
    let _ = writeln!(out, "{pad}{INDENT}{INDENT}application/json:");
    let _ = writeln!(out, "{pad}{INDENT}{INDENT}{INDENT}schema:");
    let _ = writeln!(
        out,
        "{pad}{INDENT}{INDENT}{INDENT}{INDENT}$ref: {}",
        ref_pointer(&body.schema_ref)
    );
}

/// Emit the `responses` map keyed by quoted status code.
fn write_responses(out: &mut String, responses: &[(String, ResponseObj)], depth: usize) {
    let pad = INDENT.repeat(depth);
    let _ = writeln!(out, "{pad}responses:");
    for (status, resp) in responses {
        // Status codes are quoted strings in OpenAPI YAML (`'201'`).
        let _ = writeln!(out, "{pad}{INDENT}'{status}':");
        let _ = writeln!(
            out,
            "{pad}{INDENT}{INDENT}description: {}",
            scalar(&resp.description)
        );
        if let Some(schema_ref) = &resp.schema_ref {
            let _ = writeln!(out, "{pad}{INDENT}{INDENT}content:");
            let _ = writeln!(out, "{pad}{INDENT}{INDENT}{INDENT}application/json:");
            let _ = writeln!(out, "{pad}{INDENT}{INDENT}{INDENT}{INDENT}schema:");
            let _ = writeln!(
                out,
                "{pad}{INDENT}{INDENT}{INDENT}{INDENT}{INDENT}$ref: {}",
                ref_pointer(schema_ref)
            );
        }
    }
}

/// Emit the `components` block (`securitySchemes` then `schemas`).
fn write_components(out: &mut String, components: &Components) {
    let _ = writeln!(out, "components:");
    if !components.security_schemes.is_empty() {
        let _ = writeln!(out, "{INDENT}securitySchemes:");
        for (name, scheme) in &components.security_schemes {
            write_security_scheme(out, name, scheme);
        }
    }
    let _ = writeln!(out, "{INDENT}schemas:");
    for (name, schema) in &components.schemas {
        let _ = writeln!(out, "{INDENT}{INDENT}{name}:");
        write_schema(out, schema, 3);
    }
}

/// Emit one named `apiKey`/`header`/`<name>` security scheme.
fn write_security_scheme(out: &mut String, name: &str, scheme: &SecurityScheme) {
    let _ = writeln!(out, "{INDENT}{INDENT}{name}:");
    let _ = writeln!(out, "{INDENT}{INDENT}{INDENT}type: {}", scheme.kind);
    let _ = writeln!(out, "{INDENT}{INDENT}{INDENT}in: {}", scheme.location);
    let _ = writeln!(
        out,
        "{INDENT}{INDENT}{INDENT}name: {}",
        scalar(&scheme.name)
    );
}

/// Emit a [`SchemaObject`] body with keys in fixed order: `type`, `format`, `description`, `enum`,
/// `required`, `properties`, `items`, `additionalProperties`, `$ref`.
fn write_schema(out: &mut String, schema: &SchemaObject, depth: usize) {
    let pad = INDENT.repeat(depth);
    // A bare `$ref` schema emits ONLY the `$ref` key (a `$ref` sibling-keys-are-ignored rule).
    if let Some(schema_ref) = &schema.schema_ref {
        let _ = writeln!(out, "{pad}$ref: {}", ref_pointer(schema_ref));
        return;
    }
    if let Some(type_name) = &schema.type_name {
        let _ = writeln!(out, "{pad}type: {type_name}");
    }
    if let Some(format) = &schema.format {
        let _ = writeln!(out, "{pad}format: {format}");
    }
    if let Some(description) = &schema.description {
        let _ = writeln!(out, "{pad}description: {}", scalar(description));
    }
    if !schema.enum_values.is_empty() {
        let _ = writeln!(out, "{pad}enum: {}", flow_seq(&schema.enum_values));
    }
    if !schema.required.is_empty() {
        let _ = writeln!(out, "{pad}required: {}", flow_seq(&schema.required));
    }
    if !schema.properties.is_empty() {
        let _ = writeln!(out, "{pad}properties:");
        for (prop_name, prop) in &schema.properties {
            let _ = writeln!(out, "{pad}{INDENT}{prop_name}:");
            write_schema(out, prop, depth + 2);
        }
    }
    if let Some(items) = &schema.items {
        let _ = writeln!(out, "{pad}items:");
        write_schema(out, items, depth + 1);
    }
    // NEVER emit `nullable`; free-form maps emit `additionalProperties: true` only.
    if schema.additional_properties == Some(true) {
        let _ = writeln!(out, "{pad}additionalProperties: true");
    }
}

/// Render a `$ref` value as the QUOTED JSON-pointer form `'#/components/schemas/Name'`.
fn ref_pointer(name: &str) -> String {
    format!("'#/components/schemas/{name}'")
}

/// Render a flow-style sequence `[a, b, c]`. Each item is scalar-escaped.
fn flow_seq(items: &[String]) -> String {
    let rendered: Vec<String> = items.iter().map(|item| scalar(item)).collect();
    format!("[{}]", rendered.join(", "))
}

/// Render a scalar value, quoting only when YAML would otherwise mis-parse it (keeps the output close
/// to the hand-authored fixture, which leaves plain scalars unquoted).
fn scalar(value: &str) -> String {
    if needs_quoting(value) {
        quote(value)
    } else {
        value.to_string()
    }
}

/// Wrap a value in single quotes, escaping embedded single quotes per YAML rules (`'` → `''`).
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Indicator characters that begin a non-plain scalar in YAML and so force quoting.
const LEADING_INDICATORS: &[u8] = b"!&*-?:,[]{}#|>@`\"'%";

/// Whether a plain scalar must be quoted to round-trip safely.
fn needs_quoting(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    // Leading/trailing whitespace, or any YAML-significant indicator, forces quoting.
    if value.trim() != value {
        return true;
    }
    let Some(&first) = value.as_bytes().first() else {
        return true;
    };
    if LEADING_INDICATORS.contains(&first) {
        return true;
    }
    // A `: ` or trailing `:` would start a mapping; `#` mid-value starts a comment.
    value.contains(": ") || value.ends_with(':') || value.contains(" #")
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::write;
    use crate::lower::model::{
        Components, Info, OpenApiDoc, Operation, PathItem, RequestBody, ResponseObj, SchemaObject,
        SecurityRequirement, SecurityScheme,
    };

    /// A minimal but non-trivial doc: one secured POST under `/goal/` with a request body + one
    /// response, plus one component schema with a uuid-format field and a free-form-map field.
    fn sample_doc() -> OpenApiDoc {
        let post = Operation {
            summary: Some("Create goal".to_string()),
            operation_id: "createGoal".to_string(),
            tags: vec!["Goals".to_string()],
            parameters: vec![],
            request_body: Some(RequestBody {
                required: true,
                schema_ref: "CreateGoalInput".to_string(),
            }),
            responses: vec![(
                "201".to_string(),
                ResponseObj {
                    description: "Goal created".to_string(),
                    schema_ref: Some("CommandMessageWithUUID".to_string()),
                },
            )],
        };
        let path_item = PathItem {
            post: Some(post),
            ..PathItem::default()
        };

        let foo_schema = SchemaObject {
            type_name: Some("object".to_string()),
            required: vec!["id".to_string()],
            properties: vec![
                (
                    "id".to_string(),
                    SchemaObject::primitive("string", Some("uuid".to_string())),
                ),
                (
                    "metadata".to_string(),
                    SchemaObject {
                        type_name: Some("object".to_string()),
                        additional_properties: Some(true),
                        ..SchemaObject::default()
                    },
                ),
            ],
            ..SchemaObject::default()
        };

        OpenApiDoc {
            openapi: "3.1.0",
            info: Info {
                title: "goalservice".to_string(),
                version: "0.1.0".to_string(),
                description: None,
            },
            security: vec![SecurityRequirement {
                scheme: "ApiKeyAuth".to_string(),
                scopes: vec![],
            }],
            paths: vec![("/goal/".to_string(), path_item)],
            components: Components {
                security_schemes: vec![(
                    "ApiKeyAuth".to_string(),
                    SecurityScheme {
                        kind: "apiKey",
                        location: "header",
                        name: "X-API-Key".to_string(),
                    },
                )],
                schemas: vec![("Foo".to_string(), foo_schema)],
            },
        }
    }

    #[test]
    fn emits_spec_conventional_top_level_key_order() {
        let yaml = write(&sample_doc());
        let first_line = yaml.lines().next().unwrap();
        assert_eq!(first_line, "openapi: 3.1.0");
        // Top-level keys must appear in spec order, not serde/field order.
        let pos = |key: &str| yaml.find(&format!("\n{key}:")).or_else(|| yaml.find(key));
        let openapi = yaml.find("openapi:").unwrap();
        let info = pos("info").unwrap();
        let security = pos("security").unwrap();
        let paths = pos("paths").unwrap();
        let components = pos("components").unwrap();
        assert!(
            openapi < info && info < security && security < paths && paths < components,
            "top-level key order wrong:\n{yaml}"
        );
    }

    #[test]
    fn ref_value_is_quoted_json_pointer_form() {
        let yaml = write(&sample_doc());
        assert!(
            yaml.contains("$ref: '#/components/schemas/CreateGoalInput'"),
            "expected quoted JSON-pointer ref:\n{yaml}"
        );
    }

    #[test]
    fn free_form_map_emits_additional_properties_true_and_never_nullable() {
        let yaml = write(&sample_doc());
        assert!(
            yaml.contains("additionalProperties: true"),
            "expected additionalProperties: true:\n{yaml}"
        );
        assert!(
            !yaml.contains("nullable"),
            "3.1 must never emit a nullable key:\n{yaml}"
        );
        assert!(
            !yaml.contains("\"null\""),
            "3.1 must never use a type: [T, \"null\"] array form:\n{yaml}"
        );
    }

    #[test]
    fn two_writes_are_byte_identical() {
        let doc = sample_doc();
        assert_eq!(
            write(&doc),
            write(&doc),
            "writer must be deterministic (no HashMap iteration)"
        );
    }

    #[test]
    fn string_field_with_uuid_format_emits_type_then_format() {
        let yaml = write(&sample_doc());
        let type_pos = yaml.find("type: string").unwrap();
        let format_pos = yaml.find("format: uuid").unwrap();
        assert!(
            type_pos < format_pos,
            "type must be emitted before format:\n{yaml}"
        );
    }
}
