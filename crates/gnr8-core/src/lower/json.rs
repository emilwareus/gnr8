//! Deterministic, spec-facing `OpenAPI` 3.1.0 JSON writer.
//!
//! The typed lower model intentionally uses Rust-friendly/internal field names such as
//! `operation_id`, `schema_ref`, and `type_name`. This writer is the JSON counterpart to the
//! hand-written YAML writer: it walks that model and emits OpenAPI/JSON Schema keys such as
//! `operationId`, `$ref`, `type`, `enum`, and object-valued maps.

use super::model::{
    Components, Info, OpenApiDoc, Operation, Parameter, PathItem, RequestBody, ResponseObj,
    SchemaObject, SecurityRequirement, SecurityScheme,
};
use crate::analyze::facts::LiteralValue;
use serde_json::{Map, Value};

/// Serialize a typed [`OpenApiDoc`] to a spec-facing JSON value.
pub(crate) fn write(doc: &OpenApiDoc) -> Value {
    let mut out = Map::new();
    out.insert(
        "openapi".to_string(),
        Value::String(doc.openapi.to_string()),
    );
    out.insert("info".to_string(), write_info(&doc.info));
    if !doc.security.is_empty() {
        out.insert("security".to_string(), write_security(&doc.security));
    }
    out.insert("paths".to_string(), write_paths(&doc.paths));
    out.insert("components".to_string(), write_components(&doc.components));
    Value::Object(out)
}

fn write_info(info: &Info) -> Value {
    let mut out = Map::new();
    out.insert("title".to_string(), Value::String(info.title.clone()));
    out.insert("version".to_string(), Value::String(info.version.clone()));
    if let Some(description) = &info.description {
        out.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    Value::Object(out)
}

fn write_security(security: &[SecurityRequirement]) -> Value {
    let mut entry = Map::new();
    for req in security {
        entry.insert(
            req.scheme.clone(),
            Value::Array(
                req.scopes
                    .iter()
                    .map(|scope| Value::String(scope.clone()))
                    .collect(),
            ),
        );
    }
    Value::Array(vec![Value::Object(entry)])
}

fn write_paths(paths: &[(String, PathItem)]) -> Value {
    let mut out = Map::new();
    for (path, item) in paths {
        out.insert(path.clone(), write_path_item(item));
    }
    Value::Object(out)
}

fn write_path_item(item: &PathItem) -> Value {
    let mut out = Map::new();
    for (method, op) in [
        ("get", &item.get),
        ("post", &item.post),
        ("put", &item.put),
        ("patch", &item.patch),
        ("delete", &item.delete),
    ] {
        if let Some(op) = op {
            out.insert(method.to_string(), write_operation(op));
        }
    }
    Value::Object(out)
}

fn write_operation(op: &Operation) -> Value {
    let mut out = Map::new();
    out.insert(
        "operationId".to_string(),
        Value::String(op.operation_id.clone()),
    );
    if !op.tags.is_empty() {
        out.insert(
            "tags".to_string(),
            Value::Array(
                op.tags
                    .iter()
                    .map(|tag| Value::String(tag.clone()))
                    .collect(),
            ),
        );
    }
    if !op.security.is_empty() {
        out.insert("security".to_string(), write_security(&op.security));
    }
    if !op.parameters.is_empty() {
        out.insert(
            "parameters".to_string(),
            Value::Array(op.parameters.iter().map(write_parameter).collect()),
        );
    }
    if let Some(body) = &op.request_body {
        out.insert("requestBody".to_string(), write_request_body(body));
    }
    out.insert("responses".to_string(), write_responses(&op.responses));
    Value::Object(out)
}

fn write_parameter(param: &Parameter) -> Value {
    let mut out = Map::new();
    out.insert("name".to_string(), Value::String(param.name.clone()));
    out.insert("in".to_string(), Value::String(param.location.clone()));
    out.insert("required".to_string(), Value::Bool(param.required));
    out.insert("schema".to_string(), write_schema(&param.schema));
    Value::Object(out)
}

fn write_request_body(body: &RequestBody) -> Value {
    let mut media = Map::new();
    media.insert("schema".to_string(), ref_schema(&body.schema_ref));

    let mut content = Map::new();
    content.insert(body.content_type.clone(), Value::Object(media));

    let mut out = Map::new();
    out.insert("required".to_string(), Value::Bool(body.required));
    out.insert("content".to_string(), Value::Object(content));
    Value::Object(out)
}

fn write_responses(responses: &[(String, ResponseObj)]) -> Value {
    let mut out = Map::new();
    if responses.is_empty() {
        let mut response = Map::new();
        response.insert(
            "description".to_string(),
            Value::String("Default response".to_string()),
        );
        out.insert("default".to_string(), Value::Object(response));
        return Value::Object(out);
    }

    for (status, response) in responses {
        out.insert(status.clone(), write_response(response));
    }
    Value::Object(out)
}

fn write_response(response: &ResponseObj) -> Value {
    let mut out = Map::new();
    out.insert(
        "description".to_string(),
        Value::String(response.description.clone()),
    );
    if response.binary {
        let mut schema = Map::new();
        schema.insert("type".to_string(), Value::String("string".to_string()));
        schema.insert("format".to_string(), Value::String("binary".to_string()));

        let mut media = Map::new();
        media.insert("schema".to_string(), Value::Object(schema));

        let mut content = Map::new();
        content.insert(
            response
                .content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            Value::Object(media),
        );
        out.insert("content".to_string(), Value::Object(content));
    } else if response.event_stream {
        let mut media = Map::new();
        let schema = response.schema_ref.as_ref().map_or_else(
            || {
                let mut schema = Map::new();
                schema.insert("type".to_string(), Value::String("string".to_string()));
                Value::Object(schema)
            },
            |schema_ref| ref_schema(schema_ref),
        );
        media.insert("schema".to_string(), schema);

        let mut content = Map::new();
        content.insert(
            response
                .content_type
                .clone()
                .unwrap_or_else(|| "text/event-stream".to_string()),
            Value::Object(media),
        );
        out.insert("content".to_string(), Value::Object(content));
    } else if let Some(schema_ref) = &response.schema_ref {
        let mut media = Map::new();
        media.insert("schema".to_string(), ref_schema(schema_ref));

        let mut content = Map::new();
        content.insert(
            response
                .content_type
                .clone()
                .unwrap_or_else(|| "application/json".to_string()),
            Value::Object(media),
        );
        out.insert("content".to_string(), Value::Object(content));
    }
    Value::Object(out)
}

fn write_components(components: &Components) -> Value {
    let mut out = Map::new();
    if !components.security_schemes.is_empty() {
        let mut schemes = Map::new();
        for (name, scheme) in &components.security_schemes {
            schemes.insert(name.clone(), write_security_scheme(scheme));
        }
        out.insert("securitySchemes".to_string(), Value::Object(schemes));
    }

    let mut schemas = Map::new();
    for (name, schema) in &components.schemas {
        schemas.insert(name.clone(), write_schema(schema));
    }
    out.insert("schemas".to_string(), Value::Object(schemas));
    Value::Object(out)
}

fn write_security_scheme(scheme: &SecurityScheme) -> Value {
    let mut out = Map::new();
    out.insert("type".to_string(), Value::String(scheme.kind.clone()));
    if scheme.kind == "http" {
        out.insert("scheme".to_string(), Value::String(scheme.name.clone()));
    } else {
        out.insert("in".to_string(), Value::String(scheme.location.clone()));
        out.insert("name".to_string(), Value::String(scheme.name.clone()));
    }
    Value::Object(out)
}

#[allow(clippy::too_many_lines)]
fn write_schema(schema: &SchemaObject) -> Value {
    if let Some(schema_ref) = &schema.schema_ref {
        return ref_schema(schema_ref);
    }
    let mut out = Map::new();
    if !schema.one_of.is_empty() {
        out.insert(
            "oneOf".to_string(),
            Value::Array(schema.one_of.iter().map(write_schema).collect()),
        );
    }
    if let Some(type_name) = &schema.type_name {
        if schema.nullable {
            out.insert(
                "type".to_string(),
                Value::Array(vec![
                    Value::String(type_name.clone()),
                    Value::String("null".to_string()),
                ]),
            );
        } else {
            out.insert("type".to_string(), Value::String(type_name.clone()));
        }
    }
    if let Some(format) = &schema.format {
        out.insert("format".to_string(), Value::String(format.clone()));
    }
    if let Some(description) = &schema.description {
        out.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    if !schema.enum_values.is_empty() {
        out.insert(
            "enum".to_string(),
            Value::Array(
                schema
                    .enum_values
                    .iter()
                    .map(|member| Value::String(member.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(min_length) = schema.min_length {
        out.insert("minLength".to_string(), Value::from(min_length));
    }
    if let Some(max_length) = schema.max_length {
        out.insert("maxLength".to_string(), Value::from(max_length));
    }
    if let Some(minimum) = &schema.minimum {
        out.insert("minimum".to_string(), number_or_string(minimum));
    }
    if let Some(maximum) = &schema.maximum {
        out.insert("maximum".to_string(), number_or_string(maximum));
    }
    if let Some(exclusive_minimum) = &schema.exclusive_minimum {
        out.insert(
            "exclusiveMinimum".to_string(),
            number_or_string(exclusive_minimum),
        );
    }
    if let Some(exclusive_maximum) = &schema.exclusive_maximum {
        out.insert(
            "exclusiveMaximum".to_string(),
            number_or_string(exclusive_maximum),
        );
    }
    if let Some(pattern) = &schema.pattern {
        out.insert("pattern".to_string(), Value::String(pattern.clone()));
    }
    if let Some(default_value) = &schema.default_value {
        out.insert("default".to_string(), literal(default_value));
    }
    if let Some(example) = &schema.example {
        out.insert("example".to_string(), literal(example));
    }
    for extension in &schema.extensions {
        out.insert(extension.name.clone(), literal(&extension.value));
    }
    if !schema.required.is_empty() {
        out.insert(
            "required".to_string(),
            Value::Array(
                schema
                    .required
                    .iter()
                    .map(|field| Value::String(field.clone()))
                    .collect(),
            ),
        );
    }
    if !schema.properties.is_empty() {
        let mut properties = Map::new();
        for (name, prop) in &schema.properties {
            properties.insert(name.clone(), write_schema(prop));
        }
        out.insert("properties".to_string(), Value::Object(properties));
    }
    if let Some(items) = &schema.items {
        out.insert("items".to_string(), write_schema(items));
    }
    if let Some(value_schema) = &schema.additional_properties_schema {
        out.insert(
            "additionalProperties".to_string(),
            write_schema(value_schema),
        );
    } else if schema.additional_properties == Some(true) {
        out.insert("additionalProperties".to_string(), Value::Bool(true));
    }
    Value::Object(out)
}

fn ref_schema(name: &str) -> Value {
    let mut out = Map::new();
    out.insert(
        "$ref".to_string(),
        Value::String(format!("#/components/schemas/{name}")),
    );
    Value::Object(out)
}

fn literal(value: &LiteralValue) -> Value {
    match value {
        LiteralValue::String(value) => Value::String(value.clone()),
        LiteralValue::Number(value) => number_or_string(value),
        LiteralValue::Bool(value) => Value::Bool(*value),
        LiteralValue::Null => Value::Null,
    }
}

fn number_or_string(value: &str) -> Value {
    if let Ok(value) = value.parse::<i64>() {
        return Value::Number(value.into());
    }
    if let Ok(value) = value.parse::<u64>() {
        return Value::Number(value.into());
    }
    value
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or_else(|| Value::String(value.to_string()), Value::Number)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::write;
    use crate::lower::model::{
        Components, Info, OpenApiDoc, Operation, PathItem, RequestBody, ResponseObj, SchemaObject,
        SecurityRequirement, SecurityScheme,
    };

    fn sample_doc() -> OpenApiDoc {
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
            paths: vec![(
                "/goal/".to_string(),
                PathItem {
                    post: Some(Operation {
                        operation_id: "createGoal".to_string(),
                        tags: vec!["goals".to_string()],
                        security: Vec::new(),
                        parameters: vec![],
                        request_body: Some(RequestBody {
                            required: true,
                            content_type: "application/json".to_string(),
                            schema_ref: "CreateGoalInput".to_string(),
                        }),
                        responses: vec![(
                            "201".to_string(),
                            ResponseObj {
                                description: "Goal created".to_string(),
                                schema_ref: Some("CommandMessage".to_string()),
                                content_type: None,
                                binary: false,
                                event_stream: false,
                            },
                        )],
                    }),
                    ..PathItem::default()
                },
            )],
            components: Components {
                security_schemes: vec![(
                    "ApiKeyAuth".to_string(),
                    SecurityScheme {
                        kind: "apiKey".to_string(),
                        location: "header".to_string(),
                        name: "X-API-Key".to_string(),
                    },
                )],
                schemas: vec![(
                    "CreateGoalInput".to_string(),
                    SchemaObject {
                        type_name: Some("object".to_string()),
                        required: vec!["name".to_string()],
                        properties: vec![
                            ("name".to_string(), SchemaObject::primitive("string", None)),
                            (
                                "direction".to_string(),
                                SchemaObject {
                                    type_name: Some("string".to_string()),
                                    enum_values: vec!["gte".to_string(), "lte".to_string()],
                                    ..SchemaObject::default()
                                },
                            ),
                            (
                                "meta".to_string(),
                                SchemaObject {
                                    type_name: Some("object".to_string()),
                                    additional_properties: Some(true),
                                    ..SchemaObject::default()
                                },
                            ),
                        ],
                        ..SchemaObject::default()
                    },
                )],
            },
        }
    }

    #[test]
    fn emits_spec_keys_and_object_maps() {
        let json = write(&sample_doc());
        assert_eq!(json["paths"]["/goal/"]["post"]["operationId"], "createGoal");
        assert!(json["paths"].is_object());
        assert!(json["components"]["securitySchemes"].is_object());
        assert!(json["components"]["schemas"]["CreateGoalInput"]["properties"].is_object());
        assert_eq!(
            json["paths"]["/goal/"]["post"]["requestBody"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/CreateGoalInput"
        );
        assert_eq!(
            json["components"]["schemas"]["CreateGoalInput"]["properties"]["direction"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn never_leaks_internal_model_keys() {
        let text = serde_json::to_string_pretty(&write(&sample_doc())).unwrap();
        for internal in [
            "operation_id",
            "security_schemes",
            "type_name",
            "enum_values",
            "schema_ref",
            "request_body",
            "one_of",
        ] {
            assert!(
                !text.contains(internal),
                "internal key {internal:?} leaked into JSON:\n{text}"
            );
        }
    }

    #[test]
    fn security_entries_emit_one_requirement_object() {
        let mut doc = sample_doc();
        doc.security.push(SecurityRequirement {
            scheme: "CSRFAuth".to_string(),
            scopes: vec![],
        });

        let json = write(&doc);
        let security = json["security"].as_array().unwrap();
        assert_eq!(security.len(), 1, "{json}");
        assert!(security[0]["ApiKeyAuth"].is_array(), "{json}");
        assert!(security[0]["CSRFAuth"].is_array(), "{json}");
    }
}
