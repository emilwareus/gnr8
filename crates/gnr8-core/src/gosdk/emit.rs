//! `format!`-based Go SDK emitters (D-05: no template engine; small internal templating only).
//!
//! Each emitter turns the router-agnostic [`crate::graph::ApiGraph`] into one idiomatic Go source file
//! matching the `fixtures/goalservice/expected/sdk/{client,models,errors,operations}.go` shape:
//!
//! - [`emit_models`]   — one struct per object [`Schema`], one `type X string` newtype + const block
//!   per enum [`Schema`]; Go field names are exported-CamelCase of the json tag (with Go initialisms),
//!   json tags carry `,omitempty` for optional fields, types follow TARGET-API.md §4.
//! - [`emit_client`]   — the functional-options `Client` (`NewClient`, `WithHTTPClient`, `WithAPIKey`).
//! - [`emit_operations`] — the single generic `operations.go` surface: typed methods on `*Client`,
//!   `context.Context` first, path params as positional string args, a params struct for query-bearing
//!   ops, a typed body input; each method marshals the body, builds the request, sets `X-API-Key`,
//!   decodes 2xx into the success model and non-2xx into an [`APIError`].
//! - [`emit_errors`]   — the typed `APIError` (`StatusCode`/`Message`/`Slug`/`Hints`) + `Error()` +
//!   `IsNotFound()`.
//!
//! Determinism (RESEARCH Pitfall 4): every collection is consumed in the graph's already-sorted order,
//! tags are sorted lexically, and no [`std::collections::HashMap`] is iterated. Import sets are COMPUTED
//! from the emitted content (RESEARCH Pitfall 3 — `gofmt` does not drop unused imports; `go build`
//! fails on them). Every un-representable fact (dangling `$ref`, unknown `kind`) returns
//! [`crate::CoreError::SdkGen`]; there is no prod `unwrap`/`expect`/`panic` (RUST-04).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::graph::{
    ApiGraph, Field, Operation, PaginationMode, PaginationPolicy, PaginationTermination, Prim,
    RuntimePolicy, Schema, Type, WellKnown,
};
use crate::sdk::emit_common::{
    check_unique_schema_names, error_response_bodies_of, join_path, operation_api_key_headers,
    operation_api_key_queries, operation_api_key_schemes, operation_http_auth_schemes, path_tokens,
    path_tokens_match, quoted_string_literal, request_body_model_of, split_words,
    success_responses_of, ApiKeyLocation, HttpAuthScheme, RequestBodyEncoding, SuccessResponses,
};
use crate::sdk::go::{GoSdkOptions, RequiredPointerConstructorPolicy};
use crate::sdk::surface::ResolvedTypeAlias;
use crate::CoreError;

#[derive(Debug, Clone, Default)]
pub(crate) struct GoEmitOptions {
    pub(crate) compat_model_helpers: bool,
    pub(crate) sdk: GoSdkOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompatRequestBodyEncoding {
    None,
    Json,
    Multipart,
    FormUrlEncoded,
}

/// Fold an indentation/`format!` write error into a typed [`CoreError::SdkGen`].
///
/// `write!`/`writeln!` into a `String` is infallible in practice, but the `fmt::Write` trait is
/// fallible; mapping the error keeps the path `unwrap`/`expect`-free (RUST-04).
fn sink(err: std::fmt::Error) -> CoreError {
    CoreError::SdkGen {
        message: format!("failed to format Go source: {err}"),
    }
}

/// Convert a json/handler identifier to an exported Go name (CamelCase + Go initialisms).
///
/// Splits on `_`/`-` and ASCII-case boundaries, upper-cases the first letter of each word, and special-
/// cases the common Go initialisms (`id`→`ID`, `uuid`→`UUID`, `url`→`URL`, `api`→`API`, `http`→`HTTP`,
/// `json`→`JSON`) so `workflowChainIds`→`WorkflowChainIDs` and `uuid`→`UUID` like `expected/sdk`.
pub(crate) fn exported(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for word in split_words(name) {
        let lower = word.to_ascii_lowercase();
        match lower.as_str() {
            "id" => out.push_str("ID"),
            "ids" => out.push_str("IDs"),
            "uuid" => out.push_str("UUID"),
            "url" => out.push_str("URL"),
            "urls" => out.push_str("URLs"),
            "api" => out.push_str("API"),
            "http" => out.push_str("HTTP"),
            "json" => out.push_str("JSON"),
            _ => {
                let mut chars = word.chars();
                if let Some(first) = chars.next() {
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                }
            }
        }
    }
    if out.is_empty() {
        out.push_str("Value");
    } else if !out.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic()) {
        out.insert_str(0, "Value");
    }
    out
}

pub(crate) fn compat_exported(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for word in split_words(name) {
        let lower = word.to_ascii_lowercase();
        if word.chars().all(|ch| ch.is_ascii_uppercase()) {
            match lower.as_str() {
                "id" | "uuid" | "url" | "api" | "http" | "json" => {
                    out.push_str(&lower.to_ascii_uppercase());
                    continue;
                }
                _ => {}
            }
        }
        let mut chars = lower.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        out.push_str("Value");
    } else if !out.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic()) {
        out.insert_str(0, "Value");
    }
    out
}

/// Map a neutral graph [`Type`] to its Go SDK type (TARGET-API.md §4), resolving refs to model names.
///
/// ALL Go-specific type mapping lives HERE — this is the correct home for per-target mapping (IR-03 /
/// docs/extensibility.md §2a): `WellKnown::DateTime → time.Time`, `Int → int64`, and each floating
/// point width is preserved,
/// `Map`/`Any → map[string]any`. The match over [`Type`] is exhaustive — no `_ =>` / `other =>` arm —
/// so a future variant fails to compile here until handled (T-03).
///
/// `nullable` controls pointer wrapping for value types (`*float64`, `*string`, `*bool`,
/// `*TargetDirection`, …): a NULLABLE value type becomes `*T`. Slices and maps are already nilable in
/// Go and are not pointer-wrapped. The optional axis is NOT read here — it drives `,omitempty` in
/// [`json_tag`], not the pointer (the two are distinct).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling `Named` ref, or a [`Type`] the Go target cannot
/// represent (e.g. [`Type::Union`] — Go has no sum types).
fn go_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        // A base scalar maps to its Go type. Floating-point width is preserved so an OpenAPI number
        // (64-bit by default) is never silently narrowed.
        Type::Primitive(prim) => go_primitive(prim).to_string(),
        // A well-known scalar maps to the Go type that carries it: a date-time is a `time.Time`, a
        // uuid is a string (Go-ism LOCAL to this target — never in lowering, IR-03).
        Type::WellKnown(well_known) => go_well_known(well_known).to_string(),
        Type::Array(items) => {
            // Slice elements are never nullable-pointer-wrapped.
            return Ok(format!("[]{}", go_type(items, false, graph)?));
        }
        Type::Map { key, value } => {
            return Ok(format!(
                "map[{}]{}",
                go_type(key, false, graph)?,
                go_type(value, false, graph)?
            ));
        }
        Type::Any {} => "any".to_string(),
        Type::Named(ref_id) => {
            let target = graph
                .schemas
                .iter()
                .find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            // Both objects and enum newtypes are referenced by their exported Go name; a NULLABLE
            // value ref becomes a pointer.
            return Ok(maybe_pointer(
                target.name.clone(),
                nullable,
                is_value_ref(target),
            ));
        }
        // An inline (anonymous) object is not emitted as a Go type in this PoC (every object is a
        // named DTO via a $ref) — an explicit error arm, not a catch-all (T-03).
        Type::Object(_) => {
            return Err(CoreError::SdkGen {
                message: "inline object type is unsupported by the Go SDK target \
                          (expected a named $ref)"
                    .to_string(),
            });
        }
        // An inline enum is likewise only emitted as a named newtype; an inline one is unsupported.
        Type::Enum(_) => {
            return Err(CoreError::SdkGen {
                message: "inline enum type is unsupported by the Go SDK target \
                          (expected a named $ref)"
                    .to_string(),
            });
        }
        // Go has no sum types: a union is a target capability gap, surfaced as an EXPLICIT typed error
        // arm (T-03), never a silent catch-all. (The Go fixture exercises no unions.)
        Type::Union(_) => {
            return Err(CoreError::SdkGen {
                message: "union type is unsupported by the Go SDK target (Go has no sum types)"
                    .to_string(),
            });
        }
    };
    // Strings are value types too: *string distinguishes JSON null from "". Slices/maps remain
    // naturally nilable and do not need an additional pointer layer.
    let is_value = matches!(
        base.as_str(),
        "string" | "bool" | "int64" | "float32" | "float64" | "time.Time"
    );
    Ok(maybe_pointer(base, nullable, is_value))
}

/// Map a neutral [`Prim`] to its Go type (Go-ism LOCAL to this target — IR-03). Integers carry as
/// `int64`; floating-point width is preserved; a byte string maps to Go `[]byte`.
fn go_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String => "string",
        Prim::Bool => "bool",
        Prim::Int { .. } => "int64",
        Prim::Float { bits: 32 } => "float32",
        Prim::Float { .. } => "float64",
        Prim::Bytes => "[]byte",
    }
}

/// Map a neutral [`WellKnown`] to the Go type that carries it (Go-ism LOCAL to this target — IR-03):
/// a date-time is a `time.Time`; the remaining well-knowns carry as a Go `string` in this `PoC`.
fn go_well_known(well_known: &WellKnown) -> &'static str {
    match well_known {
        WellKnown::DateTime => "time.Time",
        WellKnown::Uuid
        | WellKnown::Date
        | WellKnown::Duration
        | WellKnown::Decimal
        | WellKnown::Email
        | WellKnown::Uri => "string",
    }
}

/// Whether a referenced schema lowers to a Go *value* type that needs a pointer to be nullable.
///
/// Enum newtypes are string-backed value types (`*TargetDirection` when nullable, per `expected/sdk`);
/// object refs are structs and are pointer-wrapped only when nullable too. The match over the named
/// schema's neutral body is exhaustive (T-03).
fn is_value_ref(target: &Schema) -> bool {
    match &target.body {
        // Both enums and structs are value types in Go; a nullable field is a pointer either way.
        Type::Enum(_) | Type::Object(_) => true,
        // A named schema whose body is a scalar/array/map/union/any is not a Go struct/enum newtype;
        // it is not pointer-wrapped on the named-ref path (its own mapping handles nilability).
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Array(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Union(_)
        | Type::Any {} => false,
    }
}

/// Wrap `base` in a Go pointer when the field's value may be explicitly null AND the underlying Go type
/// is a value type. Pointer-wrapping reads the NULLABLE axis (a nilable `*T`), NOT the optional axis
/// (which drives `,omitempty` in [`json_tag`]) — the two are distinct (RESEARCH Pitfall 4).
fn maybe_pointer(base: String, nullable: bool, is_value: bool) -> String {
    if nullable && is_value {
        format!("*{base}")
    } else {
        base
    }
}

/// Build the Go json struct tag for a field, adding the `,omitempty` option when the field is OPTIONAL
/// (the presence axis — the key may be absent). Independent of nullability (RESEARCH Pitfall 4).
fn json_tag(json_name: &str, optional: bool) -> String {
    if optional {
        format!("`json:\"{json_name},omitempty\"`")
    } else {
        format!("`json:\"{json_name}\"`")
    }
}

/// Whether emitting a field of neutral [`Type`] requires the `time` import (a `time.Time` value
/// anywhere). The match recurses through arrays and is exhaustive over [`Type`] (T-03).
fn field_needs_time(schema: &Type) -> bool {
    match schema {
        Type::WellKnown(WellKnown::DateTime) => true,
        Type::Array(items) => field_needs_time(items),
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Object(_)
        | Type::Enum(_)
        | Type::Union(_)
        | Type::Any {} => false,
    }
}

fn type_needs_time(schema: &Type, graph: &ApiGraph) -> bool {
    match schema {
        Type::WellKnown(WellKnown::DateTime) => true,
        Type::Array(items) => type_needs_time(items, graph),
        Type::Map { key, value } => type_needs_time(key, graph) || type_needs_time(value, graph),
        Type::Named(ref_id) => graph
            .schemas
            .iter()
            .find(|schema| schema.id == *ref_id)
            .is_some_and(|schema| type_needs_time(&schema.body, graph)),
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Object(_)
        | Type::Enum(_)
        | Type::Union(_)
        | Type::Any {} => false,
    }
}

/// Emit `models.go`: one struct per object schema + one `type X string` newtype + const block per enum.
///
/// Schemas are consumed in the graph's id-sorted order; fields in their json-name-sorted order — both
/// already guaranteed by the graph (GRAPH-02), so the output is deterministic without re-sorting here.
///
/// `package` is the SDK package name (derived from config, the single source) used in the file frame.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if any field's schema cannot be mapped to a Go type.
#[cfg(test)]
pub(crate) fn emit_models(graph: &ApiGraph, package: &str) -> Result<String, CoreError> {
    emit_models_with_options(graph, package, &GoEmitOptions::default())
}

pub(crate) fn emit_models_with_options(
    graph: &ApiGraph,
    package: &str,
    options: &GoEmitOptions,
) -> Result<String, CoreError> {
    check_unique_schema_names(graph, "Go SDK")?;

    let mut body = String::new();
    let mut needs_time = false;

    let mut first = true;
    for schema in &graph.schemas {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        match &schema.body {
            Type::Enum(members) => {
                emit_enum(
                    &mut body,
                    &schema.name,
                    members,
                    options.compat_model_helpers,
                )?;
            }
            Type::Object(fields) => {
                if !options.compat_model_helpers {
                    for field in fields {
                        if field_needs_time(&field.schema) {
                            needs_time = true;
                        }
                    }
                }
                emit_struct(&mut body, &schema.name, fields, graph, options)?;
            }
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Any {} => {
                if type_needs_time(&schema.body, graph) {
                    needs_time = true;
                }
                emit_type_alias(&mut body, &schema.name, &schema.body, graph)?;
            }
            Type::Union(_) => {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "schema '{}' has an unsupported union body (Go SDK cannot represent sum types)",
                        schema.id
                    ),
                });
            }
        }
    }

    let imports = if options.compat_model_helpers {
        let mut imports = vec!["encoding/json", "reflect", "strings"];
        if needs_time {
            imports.push("time");
        }
        imports
    } else if needs_time {
        vec!["time"]
    } else {
        Vec::new()
    };
    Ok(file(package, &imports, &body))
}

pub(crate) fn emit_model_schema_with_options(
    graph: &ApiGraph,
    package: &str,
    schema: &Schema,
    options: &GoEmitOptions,
) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut needs_time = false;
    match &schema.body {
        Type::Enum(members) => {
            emit_enum(
                &mut body,
                &schema.name,
                members,
                options.compat_model_helpers,
            )?;
        }
        Type::Object(fields) => {
            if !options.compat_model_helpers {
                for field in fields {
                    if field_needs_time(&field.schema) {
                        needs_time = true;
                    }
                }
            }
            emit_struct(&mut body, &schema.name, fields, graph, options)?;
        }
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Array(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Any {} => {
            if type_needs_time(&schema.body, graph) {
                needs_time = true;
            }
            emit_type_alias(&mut body, &schema.name, &schema.body, graph)?;
        }
        Type::Union(_) => {
            return Err(CoreError::SdkGen {
                message: format!(
                    "schema '{}' has an unsupported union body (Go SDK cannot represent sum types)",
                    schema.id
                ),
            });
        }
    }
    let imports = match (&schema.body, options.compat_model_helpers, needs_time) {
        (Type::Object(_), true, _) => vec!["encoding/json", "reflect", "strings"],
        (_, _, true) => vec!["time"],
        _ => Vec::new(),
    };
    Ok(file(package, &imports, &body))
}

/// Emit a single object struct: one exported field per graph field with its Go type and json tag.
fn emit_struct(
    body: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    let fields = go_field_emissions(name, fields, options)?;
    writeln!(body, "type {name} struct {{").map_err(sink)?;
    for field in &fields {
        emit_struct_field(body, name, field.field, &field.go_name, graph, options)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    if options.compat_model_helpers {
        emit_compat_model_helpers(body, name, &fields, graph, options)?;
    }
    Ok(())
}

struct GoFieldEmission<'a> {
    field: &'a Field,
    go_name: String,
    arg_name: String,
}

fn go_field_emissions<'a>(
    _owner_name: &str,
    fields: &'a [Field],
    options: &GoEmitOptions,
) -> Result<Vec<GoFieldEmission<'a>>, CoreError> {
    let mut used_go = BTreeSet::new();
    let mut used_args = BTreeSet::new();
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let go_base = go_field_name(&field.json_name, options);
        let arg_base = lower_camel(&field.json_name);
        out.push(GoFieldEmission {
            field,
            go_name: unique_ident(go_base, &mut used_go)?,
            arg_name: unique_ident(arg_base, &mut used_args)?,
        });
    }
    Ok(out)
}

fn unique_ident(base: String, used: &mut BTreeSet<String>) -> Result<String, CoreError> {
    if used.insert(base.clone()) {
        return Ok(base);
    }
    let mut suffix = 2_u64;
    loop {
        let candidate = format!("{base}{suffix}");
        if used.insert(candidate.clone()) {
            return Ok(candidate);
        }
        suffix = suffix.checked_add(1).ok_or_else(|| CoreError::SdkGen {
            message: format!("could not make Go identifier {base:?} unique"),
        })?;
    }
}

fn emit_type_alias(
    body: &mut String,
    name: &str,
    schema: &Type,
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    let ty = go_type(schema, false, graph)?;
    writeln!(body, "type {name} = {ty}").map_err(sink)?;
    Ok(())
}

/// Emit one struct field line: the exported Go name, its Go type, and the json struct tag.
///
/// Pointer-wrapping reads the field's NULLABLE axis; `,omitempty` reads the OPTIONAL axis — the two are
/// distinct (RESEARCH Pitfall 4): an optional-not-nullable value stays a non-pointer `T` with omitempty;
/// a nullable value becomes `*T`.
fn emit_struct_field(
    body: &mut String,
    owner_name: &str,
    field: &Field,
    go_name: &str,
    graph: &ApiGraph,
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    let go_ty = go_field_type(field, graph, options, Some(owner_name))?;
    let tag = json_tag(
        &field.json_name,
        if options.compat_model_helpers {
            !field.required
        } else {
            field.optional
        },
    );
    writeln!(body, "{go_name} {go_ty} {tag}").map_err(sink)?;
    Ok(())
}

fn go_field_name(name: &str, options: &GoEmitOptions) -> String {
    if options.compat_model_helpers {
        compat_exported(name)
    } else {
        exported(name)
    }
}

fn go_field_type(
    field: &Field,
    graph: &ApiGraph,
    options: &GoEmitOptions,
    _owner_name: Option<&str>,
) -> Result<String, CoreError> {
    if options.compat_model_helpers {
        let base = go_compat_base_type(&field.schema, graph)?;
        if field.nullable {
            if base == "map[string]any" {
                return Ok(base);
            }
            return Ok(format!("*{base}"));
        }
        if field.required {
            return Ok(base);
        }
        if compat_type_is_nilable(&base) {
            return Ok(base);
        }
        return Ok(format!("*{base}"));
    }
    go_type(&field.schema, field.nullable, graph)
}

fn go_compat_base_type(schema: &Type, graph: &ApiGraph) -> Result<String, CoreError> {
    match schema {
        Type::Primitive(Prim::Int { bits, .. }) => Ok(match bits {
            64 => "int64".to_string(),
            _ => "int32".to_string(),
        }),
        Type::Primitive(Prim::Float { bits, .. }) => Ok(match bits {
            64 => "float64".to_string(),
            _ => "float32".to_string(),
        }),
        Type::Primitive(prim) => Ok(go_primitive(prim).to_string()),
        Type::WellKnown(WellKnown::DateTime) => Ok("string".to_string()),
        Type::WellKnown(well_known) => Ok(go_well_known(well_known).to_string()),
        Type::Array(items) => Ok(format!("[]{}", go_compat_base_type(items, graph)?)),
        Type::Map { key, value } => Ok(format!(
            "map[{}]{}",
            go_compat_base_type(key, graph)?,
            go_compat_base_type(value, graph)?
        )),
        Type::Any {} => Ok("map[string]any".to_string()),
        Type::Named(ref_id) => {
            let target = graph
                .schemas
                .iter()
                .find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            Ok(target.name.clone())
        }
        Type::Object(_) => Err(CoreError::SdkGen {
            message:
                "inline object type is unsupported by the Go SDK target (expected a named $ref)"
                    .to_string(),
        }),
        Type::Enum(_) => Err(CoreError::SdkGen {
            message: "inline enum type is unsupported by the Go SDK target (expected a named $ref)"
                .to_string(),
        }),
        Type::Union(_) => Err(CoreError::SdkGen {
            message: "union type is unsupported by the Go SDK target (Go has no sum types)"
                .to_string(),
        }),
    }
}

fn compat_type_is_nilable(go_type: &str) -> bool {
    go_type == "any" || go_type.starts_with("[]") || go_type.starts_with("map[")
}

fn compat_constructor_arg_type(
    field: &Field,
    graph: &ApiGraph,
    options: &GoEmitOptions,
    owner_name: Option<&str>,
) -> Result<String, CoreError> {
    let ty = go_field_type(field, graph, options, owner_name)?;
    if compat_constructor_arg_takes_value(field, graph, options, owner_name)? {
        Ok(ty.trim_start_matches('*').to_string())
    } else {
        Ok(ty)
    }
}

fn compat_constructor_arg_takes_value(
    field: &Field,
    graph: &ApiGraph,
    options: &GoEmitOptions,
    owner_name: Option<&str>,
) -> Result<bool, CoreError> {
    if options.sdk.required_pointer_constructor_policy
        != RequiredPointerConstructorPolicy::ValueParam
    {
        return Ok(false);
    }
    Ok(go_field_type(field, graph, options, owner_name)?.starts_with('*'))
}

fn emit_compat_model_helpers(
    body: &mut String,
    name: &str,
    fields: &[GoFieldEmission<'_>],
    graph: &ApiGraph,
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{name}WithDefaults() *{name} {{").map_err(sink)?;
    writeln!(body, "this := {name}{{}}").map_err(sink)?;
    writeln!(body, "return &this").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let required: Vec<&GoFieldEmission<'_>> = fields
        .iter()
        .filter(|field| compat_constructor_requires_field(name, field.field))
        .collect();
    let args: Result<Vec<_>, _> = required
        .iter()
        .map(|field| {
            Ok(format!(
                "{} {}",
                field.arg_name,
                compat_constructor_arg_type(field.field, graph, options, Some(name))?
            ))
        })
        .collect();
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{name}({}) *{name} {{", args?.join(", ")).map_err(sink)?;
    writeln!(body, "this := {name}{{}}").map_err(sink)?;
    for field in &required {
        if compat_constructor_arg_takes_value(field.field, graph, options, Some(name))? {
            writeln!(body, "this.{} = &{}", field.go_name, field.arg_name).map_err(sink)?;
        } else {
            writeln!(body, "this.{} = {}", field.go_name, field.arg_name).map_err(sink)?;
        }
    }
    writeln!(body, "return &this").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    for field in fields {
        emit_compat_field_helpers(body, name, field.field, &field.go_name, graph, options)?;
    }
    writeln!(body).map_err(sink)?;
    writeln!(body, "func (o {name}) MarshalJSON() ([]byte, error) {{").map_err(sink)?;
    writeln!(body, "type Alias {name}").map_err(sink)?;
    writeln!(body, "return json.Marshal(Alias(o))").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (o {name}) ToMap() (map[string]interface{{}}, error) {{"
    )
    .map_err(sink)?;
    writeln!(body, "raw := map[string]interface{{}}{{}}").map_err(sink)?;
    writeln!(body, "value := reflect.ValueOf(o)").map_err(sink)?;
    writeln!(body, "typ := reflect.TypeOf(o)").map_err(sink)?;
    writeln!(body, "for i := 0; i < value.NumField(); i++ {{").map_err(sink)?;
    writeln!(body, "fieldInfo := typ.Field(i)").map_err(sink)?;
    writeln!(body, "jsonName := fieldInfo.Tag.Get(\"json\")").map_err(sink)?;
    writeln!(body, "if jsonName == \"-\" {{").map_err(sink)?;
    writeln!(body, "continue").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "omitempty := false").map_err(sink)?;
    writeln!(
        body,
        "if comma := strings.Index(jsonName, \",\"); comma >= 0 {{"
    )
    .map_err(sink)?;
    writeln!(
        body,
        "omitempty = strings.Contains(jsonName[comma+1:], \"omitempty\")"
    )
    .map_err(sink)?;
    writeln!(body, "jsonName = jsonName[:comma]").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if jsonName == \"\" {{").map_err(sink)?;
    writeln!(body, "jsonName = fieldInfo.Name").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "field := value.Field(i)").map_err(sink)?;
    writeln!(body, "if omitempty && field.IsZero() {{").map_err(sink)?;
    writeln!(body, "continue").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "raw[jsonName] = field.Interface()").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "return raw, nil").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_field_helpers(
    body: &mut String,
    name: &str,
    field: &Field,
    field_name: &str,
    graph: &ApiGraph,
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    let ty = go_field_type(field, graph, options, Some(name))?;
    let value_ty = ty.strip_prefix('*').unwrap_or(&ty);
    writeln!(body).map_err(sink)?;
    writeln!(body, "func (o *{name}) Get{field_name}() {value_ty} {{").map_err(sink)?;
    if ty.starts_with('*') {
        writeln!(body, "if o == nil || o.{field_name} == nil {{").map_err(sink)?;
        writeln!(body, "var ret {value_ty}").map_err(sink)?;
        writeln!(body, "return ret").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return *o.{field_name}").map_err(sink)?;
    } else {
        writeln!(body, "if o == nil {{").map_err(sink)?;
        writeln!(body, "var ret {ty}").map_err(sink)?;
        writeln!(body, "return ret").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return o.{field_name}").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;

    writeln!(body).map_err(sink)?;
    if ty.starts_with('*') {
        writeln!(
            body,
            "func (o *{name}) Get{field_name}Ok() (*{value_ty}, bool) {{"
        )
        .map_err(sink)?;
        writeln!(body, "if o == nil || o.{field_name} == nil {{").map_err(sink)?;
        writeln!(body, "return nil, false").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return o.{field_name}, true").map_err(sink)?;
    } else if ty.starts_with("[]") || ty.starts_with("map[") {
        writeln!(body, "func (o *{name}) Get{field_name}Ok() ({ty}, bool) {{").map_err(sink)?;
        writeln!(body, "if o == nil || IsNil(o.{field_name}) {{").map_err(sink)?;
        writeln!(body, "return nil, false").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return o.{field_name}, true").map_err(sink)?;
    } else {
        writeln!(
            body,
            "func (o *{name}) Get{field_name}Ok() (*{ty}, bool) {{"
        )
        .map_err(sink)?;
        writeln!(body, "if o == nil {{").map_err(sink)?;
        writeln!(body, "return nil, false").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return &o.{field_name}, true").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;

    if !field.required {
        writeln!(body).map_err(sink)?;
        writeln!(body, "func (o *{name}) Has{field_name}() bool {{").map_err(sink)?;
        if ty.starts_with('*') || ty.starts_with("[]") || ty.starts_with("map[") {
            writeln!(body, "return o != nil && !IsNil(o.{field_name})").map_err(sink)?;
        } else {
            writeln!(body, "return o != nil").map_err(sink)?;
        }
        writeln!(body, "}}").map_err(sink)?;
    }

    writeln!(body).map_err(sink)?;
    writeln!(body, "func (o *{name}) Set{field_name}(v {value_ty}) {{").map_err(sink)?;
    if ty.starts_with('*') {
        writeln!(body, "o.{field_name} = &v").map_err(sink)?;
    } else {
        writeln!(body, "o.{field_name} = v").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Emit a string-enum newtype + a const block of `NameValue Name = "value"` (values in graph order).
fn emit_enum(
    body: &mut String,
    name: &str,
    members: &[String],
    emit_compat_aliases: bool,
) -> Result<(), CoreError> {
    writeln!(body, "type {name} string").map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "const (").map_err(sink)?;
    for value in members {
        let const_name = format!("{name}{}", exported(value));
        writeln!(body, "{const_name} {name} = \"{value}\"").map_err(sink)?;
    }
    writeln!(body, ")").map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "const (").map_err(sink)?;
    let mut emitted_compat_consts = BTreeSet::new();
    for value in members {
        let const_name = format!("{}{}", compat_exported(value), name);
        if emitted_compat_consts.insert(const_name.clone()) {
            writeln!(body, "{const_name} {name} = \"{value}\"").map_err(sink)?;
        }
        if emit_compat_aliases {
            for alias in compat_enum_constant_aliases(name, value) {
                if emitted_compat_consts.insert(alias.clone()) {
                    writeln!(body, "{alias} {name} = \"{value}\"").map_err(sink)?;
                }
            }
        }
    }
    writeln!(body, ")").map_err(sink)?;
    Ok(())
}

fn compat_enum_constant_aliases(name: &str, value: &str) -> Vec<String> {
    let mut aliases = BTreeSet::new();
    let value_name = compat_exported(value);
    aliases.extend(compat_initialism_aliases(&format!("{value_name}{name}")));
    aliases.extend(compat_initialism_aliases(&format!(
        "{name}{}",
        exported(value)
    )));
    aliases.insert(format!("{name}Type{}", exported(value)));
    if let Some(owner_suffix) = name.strip_suffix("ValueType") {
        if !owner_suffix.is_empty() {
            aliases.insert(format!("{value_name}Type"));
        }
    }
    if name.ends_with("ConditionType") {
        if let Some(stripped) = value_name.strip_suffix("Static") {
            if !stripped.is_empty() {
                aliases.insert(format!("{stripped}ConditionType"));
            }
        }
    }
    if let Some(owner_suffix) = name.strip_suffix("Preference") {
        if !owner_suffix.is_empty() {
            aliases.insert(format!("{owner_suffix}{}", exported(value)));
        }
    }
    if let Some(owner_suffix) = name.strip_prefix("LLM") {
        if !owner_suffix.is_empty() {
            let alias = format!("{owner_suffix}{}", exported(value));
            aliases.insert(alias.clone());
            aliases.extend(compat_initialism_aliases(&alias));
        }
    }
    if let Some((_, subtype)) = value.rsplit_once('/') {
        let subtype_name = subtype
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(compat_exported)
            .collect::<String>();
        if !subtype_name.is_empty() {
            let alias = format!("{name}{subtype_name}");
            aliases.insert(alias.clone());
            aliases.extend(compat_initialism_aliases(&alias));
        }
    }
    aliases.into_iter().collect()
}

fn compat_initialism_aliases(name: &str) -> Vec<String> {
    const INITIALISMS: &[(&str, &str)] = &[
        ("Ai", "AI"),
        ("Api", "API"),
        ("Gpt", "GPT"),
        ("Http", "HTTP"),
        ("Id", "ID"),
        ("Json", "JSON"),
        ("Jpeg", "JPEG"),
        ("Gif", "GIF"),
        ("Llm", "LLM"),
        ("Oauth", "OAuth"),
        ("Openai", "OpenAI"),
        ("Pdf", "PDF"),
        ("Png", "PNG"),
        ("Url", "URL"),
        ("Uuid", "UUID"),
        ("Webp", "WebP"),
    ];
    let mut aliases = BTreeSet::new();
    let mut pending = vec![name.to_string()];
    for (from, to) in INITIALISMS {
        let current = pending.clone();
        for item in current {
            for candidate in replace_word_like_token(&item, from, to) {
                if candidate != item && aliases.insert(candidate.clone()) {
                    pending.push(candidate);
                }
            }
        }
    }
    aliases.into_iter().collect()
}

fn replace_word_like_token(name: &str, from: &str, to: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    let mut offset = 0;
    while let Some(found) = name[offset..].find(from) {
        let start = offset + found;
        let end = start + from.len();
        let first = name[start..].chars().next();
        let before_ok = start == 0
            || name[..start]
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_ascii_alphanumeric())
            || first.is_some_and(|ch| ch.is_ascii_uppercase());
        let after_ok = name[end..]
            .chars()
            .next()
            .is_none_or(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
        if before_ok && after_ok {
            let mut alias = String::with_capacity(name.len() + to.len().saturating_sub(from.len()));
            alias.push_str(&name[..start]);
            alias.push_str(to);
            alias.push_str(&name[end..]);
            aliases.push(alias);
        }
        offset = end;
    }
    aliases
}

fn compat_constructor_requires_field(_owner_name: &str, field: &Field) -> bool {
    field.required
}

/// Emit `client.go`: the functional-options `Client` + `Option` + `WithHTTPClient`/`WithAPIKey`/`NewClient`.
///
/// `net/http` + `time` are always needed (the default client carries a `30 * time.Second` timeout). The
/// doc comment names the SDK by its `package` (derived from config, the single source) rather than a
/// hard-coded fixture name.
#[expect(
    clippy::too_many_lines,
    reason = "the generated runtime client is one fixed source block with options, hooks, retry helpers, and transport helpers"
)]
pub(crate) fn emit_client(
    package: &str,
    has_api_key_auth: bool,
    has_bearer_auth: bool,
    has_basic_auth: bool,
    runtime: &RuntimePolicy,
) -> String {
    let api_key_field = if has_api_key_auth {
        "apiKey string\napiKeys map[string]string\n"
    } else {
        ""
    };
    let bearer_field = if has_bearer_auth {
        "bearerToken string\n"
    } else {
        ""
    };
    let basic_field = if has_basic_auth {
        "basicUsername string\nbasicPassword string\n"
    } else {
        ""
    };
    let api_key_option = if has_api_key_auth {
        "\n// WithAPIKey sets a fallback API key sent for any configured auth header without a specific key.\nfunc WithAPIKey(key string) Option {\nreturn func(c *Client) { c.apiKey = key }\n}\n\n// WithAPIKeyHeader sets the API key sent in one specific auth header.\nfunc WithAPIKeyHeader(header, key string) Option {\nreturn func(c *Client) {\nif c.apiKeys == nil {\nc.apiKeys = map[string]string{}\n}\nc.apiKeys[header] = key\n}\n}\n".to_string()
    } else {
        String::new()
    };
    let bearer_option = if has_bearer_auth {
        "\n// WithBearerToken sets the bearer token sent to operations secured by HTTP bearer auth.\nfunc WithBearerToken(token string) Option {\nreturn func(c *Client) { c.bearerToken = token }\n}\n".to_string()
    } else {
        String::new()
    };
    let basic_option = if has_basic_auth {
        "\n// WithBasicAuth sets the credentials sent to operations secured by HTTP basic auth.\nfunc WithBasicAuth(username, password string) Option {\nreturn func(c *Client) {\nc.basicUsername = username\nc.basicPassword = password\n}\n}\n".to_string()
    } else {
        String::new()
    };
    let api_key_init = if has_api_key_auth {
        "apiKeys: map[string]string{},\n"
    } else {
        ""
    };
    let default_timeout = runtime
        .default_timeout_ms
        .map_or_else(|| "30 * time.Second".to_string(), go_duration_ms);
    let retry_statuses = go_retry_status_map(runtime);
    let retry_unsafe_methods = runtime.retry_unsafe_methods;
    let max_retries = runtime.max_retries;
    let body = format!(
        "\
// Client is the {package} SDK entrypoint. Tag-grouped operation methods hang
// off this type; it is constructed with functional options.
type Client struct {{
baseURL string
httpClient *http.Client
timeout time.Duration
maxRetries int
retryStatuses map[int]bool
retryUnsafeMethods bool
requestHooks []RequestHook
responseHooks []ResponseHook
errorHooks []ErrorHook
{api_key_field}{bearer_field}{basic_field}}}

// Option mutates a Client during construction (functional-options pattern).
type Option func(*Client)

// WithHTTPClient overrides the default *http.Client (timeouts, transport, etc.).
func WithHTTPClient(hc *http.Client) Option {{
return func(c *Client) {{ c.httpClient = hc }}
}}

// WithTimeout sets the client-level default request timeout.
func WithTimeout(timeout time.Duration) Option {{
return func(c *Client) {{ c.timeout = timeout }}
}}

// WithMaxRetries sets the client-level default retry count.
func WithMaxRetries(maxRetries int) Option {{
return func(c *Client) {{ c.maxRetries = maxRetries }}
}}

// WithRequestHook installs a hook that runs before each HTTP attempt.
func WithRequestHook(hook RequestHook) Option {{
return func(c *Client) {{ c.requestHooks = append(c.requestHooks, hook) }}
}}

// WithResponseHook installs a hook that runs after each HTTP response.
func WithResponseHook(hook ResponseHook) Option {{
return func(c *Client) {{ c.responseHooks = append(c.responseHooks, hook) }}
}}

// WithErrorHook installs a hook that runs for transport failures and final non-2xx responses.
func WithErrorHook(hook ErrorHook) Option {{
return func(c *Client) {{ c.errorHooks = append(c.errorHooks, hook) }}
}}

// RequestOptions overrides runtime behavior for one operation call.
type RequestOptions struct {{
Timeout time.Duration
MaxRetries *int
IdempotencyKey string
Metadata map[string]string
}}

// RequestOption mutates per-request runtime options.
type RequestOption func(*RequestOptions)

// WithRequestTimeout overrides the timeout for one operation call.
func WithRequestTimeout(timeout time.Duration) RequestOption {{
return func(o *RequestOptions) {{ o.Timeout = timeout }}
}}

// WithRequestMaxRetries overrides max retries for one operation call.
func WithRequestMaxRetries(maxRetries int) RequestOption {{
return func(o *RequestOptions) {{ o.MaxRetries = &maxRetries }}
}}

// WithIdempotencyKey sets the idempotency key sent by explicitly idempotent operations.
func WithIdempotencyKey(key string) RequestOption {{
return func(o *RequestOptions) {{ o.IdempotencyKey = key }}
}}

// WithRequestMetadata attaches hook-visible metadata to one operation call.
func WithRequestMetadata(metadata map[string]string) RequestOption {{
return func(o *RequestOptions) {{ o.Metadata = metadata }}
}}

// RequestContext describes one generated SDK transport attempt.
type RequestContext struct {{
OperationID string
Method string
PathTemplate string
URL string
Headers http.Header
RequestMetadata map[string]string
StatusCode int
ResponseHeaders http.Header
}}

type RequestHook func(context.Context, RequestContext, *http.Request) error
type ResponseHook func(context.Context, RequestContext, *http.Response) error
type ErrorHook func(context.Context, RequestContext, error)

type runtimeRequestOptions struct {{
OperationID string
PathTemplate string
Idempotent bool
IdempotencyKeyHeader string
Options RequestOptions
}}
{api_key_option}
{bearer_option}
{basic_option}

// NewClient builds a Client for the given base URL, applying any options. A
// sensible default *http.Client is used unless WithHTTPClient overrides it.
func NewClient(baseURL string, opts ...Option) *Client {{
c := &Client{{
baseURL: baseURL,
httpClient: &http.Client{{Timeout: {default_timeout}}},
timeout: {default_timeout},
maxRetries: {max_retries},
retryStatuses: {retry_statuses},
retryUnsafeMethods: {retry_unsafe_methods},
{api_key_init}
}}
for _, opt := range opts {{
opt(c)
}}
return c
}}

func newRequestOptions(opts ...RequestOption) RequestOptions {{
var options RequestOptions
for _, opt := range opts {{
opt(&options)
}}
return options
}}

func (c *Client) do(req *http.Request, runtime runtimeRequestOptions) (*http.Response, error) {{
timeout := c.timeout
if runtime.Options.Timeout > 0 {{
timeout = runtime.Options.Timeout
}}
ctx := req.Context()
var cancel context.CancelFunc
if timeout > 0 {{
ctx, cancel = context.WithTimeout(ctx, timeout)
defer cancel()
req = req.Clone(ctx)
}}
if runtime.Idempotent && runtime.Options.IdempotencyKey != \"\" {{
header := runtime.IdempotencyKeyHeader
if header == \"\" {{
header = \"Idempotency-Key\"
}}
req.Header.Set(header, runtime.Options.IdempotencyKey)
}}
maxRetries := c.maxRetries
if runtime.Options.MaxRetries != nil {{
maxRetries = *runtime.Options.MaxRetries
}}
if maxRetries < 0 {{
maxRetries = 0
}}
allowRetries := c.retryUnsafeMethods || runtime.Idempotent || retryableMethod(req.Method)
if !allowRetries {{
maxRetries = 0
}}
var lastErr error
for attempt := 0; attempt <= maxRetries; attempt++ {{
attemptReq, err := cloneRequestForAttempt(req, attempt)
if err != nil {{
return nil, err
}}
ctx := requestContext(runtime, attemptReq)
for _, hook := range c.requestHooks {{
if err := hook(attemptReq.Context(), ctx, attemptReq); err != nil {{
c.callErrorHooks(attemptReq.Context(), ctx, err)
return nil, err
}}
}}
resp, err := c.httpClient.Do(attemptReq)
if err != nil {{
lastErr = err
if attempt < maxRetries {{
continue
}}
c.callErrorHooks(attemptReq.Context(), ctx, err)
return nil, err
}}
ctx.StatusCode = resp.StatusCode
ctx.ResponseHeaders = resp.Header.Clone()
for _, hook := range c.responseHooks {{
if err := hook(attemptReq.Context(), ctx, resp); err != nil {{
_ = resp.Body.Close()
c.callErrorHooks(attemptReq.Context(), ctx, err)
return nil, err
}}
}}
if shouldRetryStatus(resp.StatusCode, c.retryStatuses) && attempt < maxRetries {{
sleepRetryAfter(resp)
_, _ = io.Copy(io.Discard, resp.Body)
_ = resp.Body.Close()
continue
}}
if resp.StatusCode < 200 || resp.StatusCode >= 300 {{
c.callErrorHooks(attemptReq.Context(), ctx, &APIError{{StatusCode: resp.StatusCode, Headers: resp.Header.Clone(), RequestID: resp.Header.Get(\"X-Request-ID\")}})
}}
return resp, nil
}}
if lastErr != nil {{
return nil, lastErr
}}
return nil, errors.New(\"request failed without response\")
}}

func cloneRequestForAttempt(req *http.Request, attempt int) (*http.Request, error) {{
cloned := req.Clone(req.Context())
if attempt == 0 || req.Body == nil {{
return cloned, nil
}}
if req.GetBody == nil {{
return nil, errors.New(\"request body cannot be replayed for retry\")
}}
body, err := req.GetBody()
if err != nil {{
return nil, err
}}
cloned.Body = body
return cloned, nil
}}

func requestContext(runtime runtimeRequestOptions, req *http.Request) RequestContext {{
return RequestContext{{
OperationID: runtime.OperationID,
Method: req.Method,
PathTemplate: runtime.PathTemplate,
URL: req.URL.String(),
Headers: req.Header.Clone(),
RequestMetadata: runtime.Options.Metadata,
}}
}}

func (c *Client) callErrorHooks(ctx context.Context, requestContext RequestContext, err error) {{
for _, hook := range c.errorHooks {{
hook(ctx, requestContext, err)
}}
}}

func retryableMethod(method string) bool {{
switch method {{
case http.MethodGet, http.MethodHead, http.MethodOptions, http.MethodPut, http.MethodDelete:
return true
default:
return false
}}
}}

func shouldRetryStatus(status int, retryStatuses map[int]bool) bool {{
return retryStatuses[status] || status >= 500
}}

func sleepRetryAfter(resp *http.Response) {{
retryAfter := resp.Header.Get(\"Retry-After\")
if retryAfter == \"\" {{
return
}}
seconds, err := strconv.Atoi(retryAfter)
if err != nil || seconds <= 0 {{
return
}}
time.Sleep(time.Duration(seconds) * time.Second)
}}
"
    );
    file(
        package,
        &["context", "errors", "io", "net/http", "strconv", "time"],
        &body,
    )
}

fn go_duration_ms(timeout_ms: u64) -> String {
    format!("{timeout_ms} * time.Millisecond")
}

fn go_retry_status_map(runtime: &RuntimePolicy) -> String {
    let mut statuses = runtime.retry_statuses.clone();
    if statuses.is_empty() {
        statuses.extend([408, 429]);
    }
    statuses.sort_unstable();
    statuses.dedup();
    if statuses.is_empty() {
        return "map[int]bool{}".to_string();
    }
    let entries = statuses
        .into_iter()
        .map(|status| format!("{status}: true"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("map[int]bool{{{entries}}}")
}

/// Emit language-native compatibility aliases.
pub(crate) fn emit_type_aliases(
    graph: &ApiGraph,
    package: &str,
    aliases: &[ResolvedTypeAlias],
    options: &GoEmitOptions,
) -> Result<String, CoreError> {
    let mut body = String::new();
    for alias in aliases {
        let _ = writeln!(body, "type {} = {}", alias.alias, alias.canonical);
    }
    if options.compat_model_helpers {
        for alias in aliases {
            let Some(schema) = graph
                .schemas
                .iter()
                .find(|schema| schema.name == alias.canonical)
            else {
                continue;
            };
            let Type::Object(fields) = &schema.body else {
                continue;
            };
            emit_compat_alias_constructors(
                &mut body,
                &alias.alias,
                &alias.canonical,
                fields,
                graph,
                options,
            )?;
        }
    }
    let mut imports = Vec::new();
    if !options.compat_model_helpers
        && aliases.iter().any(|alias| {
            graph
                .schemas
                .iter()
                .find(|schema| schema.name == alias.canonical)
                .is_some_and(|schema| match &schema.body {
                    Type::Object(fields) => {
                        fields.iter().any(|field| field_needs_time(&field.schema))
                    }
                    _ => false,
                })
        })
    {
        imports.push("time");
    }
    Ok(file(package, &imports, &body))
}

pub(crate) fn emit_compat_helpers(package: &str) -> String {
    let body = "\
func IsNil(value any) bool {
if value == nil {
return true
}
reflected := reflect.ValueOf(value)
switch reflected.Kind() {
case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Ptr, reflect.Slice:
return reflected.IsNil()
default:
return false
}
}
";
    file(package, &["reflect"], body)
}

pub(crate) fn emit_compat_client_surface(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, CoreError> {
    let mut body = String::new();
    emit_compat_client_prelude(&mut body);

    let services = compat_services(graph);
    let query_setters = compat_query_setters(graph);
    for service in &services {
        writeln!(body, "type {service}APIService service").map_err(sink)?;
    }
    writeln!(body).map_err(sink)?;
    emit_compat_api_client(&mut body, &services)?;
    let options = GoEmitOptions {
        compat_model_helpers: true,
        sdk: GoSdkOptions::default(),
    };
    for op in &graph.operations {
        emit_compat_request(&mut body, op, graph, base_path, &query_setters, &options)?;
    }

    Ok(file(
        package,
        &[
            "bytes",
            "context",
            "encoding/json",
            "fmt",
            "io",
            "mime/multipart",
            "net/http",
            "net/url",
            "path/filepath",
            "reflect",
            "strings",
        ],
        &body,
    ))
}

#[expect(
    clippy::too_many_lines,
    reason = "the optional user-configured request-builder client is emitted as one Go declaration block"
)]
fn emit_compat_client_prelude(body: &mut String) {
    let default_auth_header = "Authorization";
    let _ = writeln!(
        body,
        "\
type GenericOpenAPIError struct {{
body []byte
model any
error string
}}

func (e GenericOpenAPIError) Error() string {{
return e.error
}}

func (e GenericOpenAPIError) Body() []byte {{
return e.body
}}

func (e GenericOpenAPIError) Model() any {{
return e.model
}}

type compatNamedReader interface {{
io.Reader
Name() string
}}

func compatMultipartFileBody(file any, fields map[string]any) (*bytes.Reader, string, error) {{
var buf bytes.Buffer
writer := multipart.NewWriter(&buf)
for key, value := range fields {{
if err := writer.WriteField(key, compatQueryValue(value)); err != nil {{
return nil, \"\", err
}}
}}
if file != nil {{
reader, ok := file.(compatNamedReader)
if !ok {{
return nil, \"\", fmt.Errorf(\"file must implement io.Reader and Name() string\")
}}
part, err := writer.CreateFormFile(\"file\", filepath.Base(reader.Name()))
if err != nil {{
return nil, \"\", err
}}
if _, err := io.Copy(part, reader); err != nil {{
return nil, \"\", err
}}
if closer, ok := file.(io.Closer); ok {{
_ = closer.Close()
}}
}}
if err := writer.Close(); err != nil {{
return nil, \"\", err
}}
return bytes.NewReader(buf.Bytes()), writer.FormDataContentType(), nil
}}

type APIKey struct {{
Key string
Prefix string
}}

type contextKey string

const ContextAPIKeys contextKey = \"apiKeys\"

func WithAPIKey(ctx context.Context, name string, key APIKey) context.Context {{
if ctx == nil {{
ctx = context.Background()
}}
values, _ := ctx.Value(ContextAPIKeys).(map[string]APIKey)
next := map[string]APIKey{{}}
for k, v := range values {{
next[k] = v
}}
next[name] = key
return context.WithValue(ctx, ContextAPIKeys, next)
}}

type ServerVariable struct {{
Description string
DefaultValue string
EnumValues []string
}}

type ServerConfiguration struct {{
URL string
Description string
Variables map[string]ServerVariable
}}

type ServerConfigurations []ServerConfiguration

type Configuration struct {{
DefaultHeader map[string]string
UserAgent string
Servers ServerConfigurations
HTTPClient *http.Client
}}

func NewConfiguration() *Configuration {{
return &Configuration{{
DefaultHeader: map[string]string{{}},
UserAgent: \"gnr8-compat/go\",
Servers: ServerConfigurations{{{{URL: \"\"}}}},
HTTPClient: http.DefaultClient,
}}
}}

func (c *Configuration) AddDefaultHeader(key string, value string) {{
if c.DefaultHeader == nil {{
c.DefaultHeader = map[string]string{{}}
}}
c.DefaultHeader[key] = value
}}

func (c *Configuration) serverURL() string {{
if c != nil && len(c.Servers) > 0 {{
return c.Servers[0].URL
}}
return \"\"
}}

func (c *Configuration) ServerURLWithContext(_ context.Context, _ string) (string, error) {{
return c.serverURL(), nil
}}

func reportError(format string, args ...any) error {{
return fmt.Errorf(format, args...)
}}

func compatEncodeJSONBody(v any) (*bytes.Reader, error) {{
var buf bytes.Buffer
if err := json.NewEncoder(&buf).Encode(v); err != nil {{
return nil, err
}}
return bytes.NewReader(buf.Bytes()), nil
}}

func compatSetBodyField(body any, key string, value any) any {{
switch typed := body.(type) {{
case nil:
next := map[string]any{{}}
next[key] = value
return next
case map[string]any:
typed[key] = value
return typed
case map[string]string:
typed[key] = compatQueryValue(value)
return typed
case url.Values:
compatSetQueryValue(typed, key, value)
return typed
default:
next := map[string]any{{}}
next[key] = value
return next
}}
}}

func compatEncodeFormBody(v any) (*bytes.Reader, error) {{
values := url.Values{{}}
if err := compatAddFormValues(values, v); err != nil {{
return nil, err
}}
return bytes.NewReader([]byte(values.Encode())), nil
}}

func compatAddFormValues(values url.Values, value any) error {{
if value == nil {{
return nil
}}
switch typed := value.(type) {{
case url.Values:
for key, items := range typed {{
for _, item := range items {{
values.Add(key, item)
}}
}}
return nil
case map[string]any:
for key, item := range typed {{
compatSetQueryValue(values, key, item)
}}
return nil
case map[string]string:
for key, item := range typed {{
values.Set(key, item)
}}
return nil
}}

reflected := reflect.ValueOf(value)
for reflected.Kind() == reflect.Ptr || reflected.Kind() == reflect.Interface {{
if reflected.IsNil() {{
return nil
}}
reflected = reflected.Elem()
}}
switch reflected.Kind() {{
case reflect.Map:
if reflected.Type().Key().Kind() != reflect.String {{
return fmt.Errorf(\"form body map keys must be strings\")
}}
iter := reflected.MapRange()
for iter.Next() {{
compatSetQueryValue(values, iter.Key().String(), iter.Value().Interface())
}}
case reflect.Struct:
typ := reflected.Type()
for i := 0; i < reflected.NumField(); i++ {{
field := typ.Field(i)
if field.PkgPath != \"\" {{
continue
}}
name, omitempty := compatFormFieldName(field)
if name == \"\" || name == \"-\" {{
continue
}}
fieldValue := reflected.Field(i)
if omitempty && fieldValue.IsZero() {{
continue
}}
compatSetQueryValue(values, name, fieldValue.Interface())
}}
default:
return fmt.Errorf(\"form body must be a map, url.Values, or struct\")
}}
return nil
}}

func compatFormFieldName(field reflect.StructField) (string, bool) {{
tag := field.Tag.Get(\"form\")
if tag == \"\" {{
tag = field.Tag.Get(\"json\")
}}
if tag == \"-\" {{
return \"-\", false
}}
omitempty := false
if tag != \"\" {{
parts := strings.Split(tag, \",\")
for _, option := range parts[1:] {{
if option == \"omitempty\" {{
omitempty = true
}}
}}
if parts[0] != \"\" {{
return parts[0], omitempty
}}
}}
return field.Name, omitempty
}}

func compatDefaultAuthHeader() string {{
return {}
}}

func compatQueryValue(value any) string {{
v := reflect.ValueOf(value)
if !v.IsValid() {{
return \"\"
}}
if v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {{
if v.IsNil() {{
return \"\"
}}
return compatQueryValue(v.Elem().Interface())
}}
if v.Kind() != reflect.Slice && v.Kind() != reflect.Array {{
return fmt.Sprint(value)
}}
parts := make([]string, 0, v.Len())
for i := 0; i < v.Len(); i++ {{
parts = append(parts, fmt.Sprint(v.Index(i).Interface()))
}}
return strings.Join(parts, \",\")
}}

func compatSetQueryValue(q url.Values, key string, value any) {{
q.Set(key, compatQueryValue(value))
}}

func parameterAddToHeaderOrQuery(headerOrQueryParams any, key string, value any, _ string) {{
switch params := headerOrQueryParams.(type) {{
case url.Values:
params.Set(key, compatQueryValue(value))
case http.Header:
params.Set(key, compatQueryValue(value))
}}
}}

func compatApplyAPIKey(req *http.Request, ctx context.Context, scheme string, header string) {{
if req.Header.Get(header) != \"\" || ctx == nil {{
return
}}
values, _ := ctx.Value(ContextAPIKeys).(map[string]APIKey)
apiKey, ok := values[scheme]
if !ok {{
apiKey, ok = values[header]
}}
if !ok || apiKey.Key == \"\" {{
return
}}
value := apiKey.Key
if apiKey.Prefix != \"\" {{
value = apiKey.Prefix + \" \" + value
}}
req.Header.Set(header, value)
}}
",
        quoted_string_literal(default_auth_header)
    );
}

fn emit_compat_api_client(body: &mut String, services: &[String]) -> Result<(), CoreError> {
    writeln!(body, "type APIClient struct {{").map_err(sink)?;
    writeln!(body, "cfg *Configuration").map_err(sink)?;
    writeln!(body, "httpClient *http.Client").map_err(sink)?;
    writeln!(body, "common service").map_err(sink)?;
    for service in services {
        writeln!(body, "{service}API *{service}APIService").map_err(sink)?;
        for alias in compat_initialism_aliases(service) {
            writeln!(body, "{alias}API *{service}APIService").map_err(sink)?;
        }
    }
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "type service struct {{").map_err(sink)?;
    writeln!(body, "client *APIClient").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "func NewAPIClient(cfg *Configuration) *APIClient {{").map_err(sink)?;
    writeln!(body, "if cfg == nil {{").map_err(sink)?;
    writeln!(body, "cfg = NewConfiguration()").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if cfg.HTTPClient == nil {{").map_err(sink)?;
    writeln!(body, "cfg.HTTPClient = http.DefaultClient").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(
        body,
        "c := &APIClient{{cfg: cfg, httpClient: cfg.HTTPClient}}"
    )
    .map_err(sink)?;
    writeln!(body, "c.common.client = c").map_err(sink)?;
    for service in services {
        writeln!(body, "c.{service}API = (*{service}APIService)(&c.common)").map_err(sink)?;
        for alias in compat_initialism_aliases(service) {
            writeln!(body, "c.{alias}API = c.{service}API").map_err(sink)?;
        }
    }
    writeln!(body, "return c").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "func (c *APIClient) GetConfig() *Configuration {{").map_err(sink)?;
    writeln!(body, "return c.cfg").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

struct CompatMultipartField {
    wire_name: String,
    setter: String,
    arg_name: String,
    arg_type: String,
    is_file: bool,
}

fn compat_multipart_fields(
    model: Option<&str>,
    graph: &ApiGraph,
) -> Result<Vec<CompatMultipartField>, CoreError> {
    let Some(model) = model else {
        return Ok(Vec::new());
    };
    let Some(schema) = graph.schemas.iter().find(|schema| schema.name == model) else {
        return Ok(Vec::new());
    };
    let Type::Object(fields) = &schema.body else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let setter = compat_exported(&field.json_name);
        let is_file = is_multipart_file_field(field);
        let arg_type = if is_file {
            "any".to_string()
        } else {
            go_type(&field.schema, field.nullable, graph)?
        };
        out.push(CompatMultipartField {
            wire_name: field.json_name.clone(),
            arg_name: compat_arg_name(&setter),
            setter,
            arg_type,
            is_file,
        });
    }
    Ok(out)
}

fn is_multipart_file_field(field: &Field) -> bool {
    matches!(&field.schema, Type::Primitive(Prim::Bytes))
}

fn compat_param_type(param: &crate::graph::Param, graph: &ApiGraph) -> Result<String, CoreError> {
    go_type(&param.schema, false, graph)
}

#[expect(
    clippy::too_many_lines,
    reason = "one request-builder operation is emitted in a single deterministic pass"
)]
fn emit_compat_request(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    global_query_setters: &[(String, String)],
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    let method_name = compat_operation_name(op);
    let request_name = compat_request_name(op);
    let service = compat_service_name(op);
    let mut path_params: Vec<&crate::graph::Param> =
        op.params.iter().filter(|p| p.location == "path").collect();
    let path_order = path_tokens(&op.path);
    path_params.sort_by_key(|p| {
        path_order
            .iter()
            .position(|token| token == &p.name)
            .unwrap_or(usize::MAX)
    });
    let query_params: Vec<&crate::graph::Param> =
        op.params.iter().filter(|p| p.location == "query").collect();
    let body_model = request_body_model_of(op, graph)?;
    let multipart_fields =
        compat_multipart_fields(body_model.as_ref().map(|body| body.model.as_str()), graph)?;
    let has_multipart_body = op
        .request_body_content_type
        .as_deref()
        .is_some_and(|content_type| content_type.eq_ignore_ascii_case("multipart/form-data"))
        && !multipart_fields.is_empty();
    let has_form_body = op
        .request_body_content_type
        .as_deref()
        .is_some_and(|content_type| {
            content_type.eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
        && body_model.is_some()
        && !has_multipart_body;
    let file_field = multipart_fields.iter().find(|field| field.is_file);
    let has_json_body = body_model.is_some() && !has_multipart_body && !has_form_body;
    let body_encoding = if has_multipart_body {
        CompatRequestBodyEncoding::Multipart
    } else if has_form_body {
        CompatRequestBodyEncoding::FormUrlEncoded
    } else if has_json_body {
        CompatRequestBodyEncoding::Json
    } else {
        CompatRequestBodyEncoding::None
    };
    let has_body_value = has_json_body || has_form_body;
    let body_field_setters = if has_body_value {
        compat_body_field_setters(body_model.as_ref().map(|body| body.model.as_str()), graph)
    } else {
        Vec::new()
    };
    let body_setters = if has_body_value {
        compat_body_setters(
            body_model.as_ref().map(|body| body.model.as_str()),
            &service,
        )
    } else {
        Vec::new()
    };
    let has_selective_any_query_param = query_params.iter().any(|param| {
        compat_method_names(&compat_exported(&param.name))
            .iter()
            .any(|setter| {
                options
                    .sdk
                    .query_setter_argument_policy
                    .is_any_for(&request_name, setter)
            })
    });
    let has_extra_query = !global_query_setters.is_empty()
        || has_multipart_body
        || has_selective_any_query_param
        || !options
            .sdk
            .request_builder_aliases
            .query_aliases_for(&request_name)
            .is_empty();
    let has_extra_header = !operation_api_key_schemes(graph, op)?.is_empty();
    let return_model = compat_success_return_model(op, graph)?;

    writeln!(body).map_err(sink)?;
    writeln!(body, "type {request_name} struct {{").map_err(sink)?;
    writeln!(body, "ctx context.Context").map_err(sink)?;
    writeln!(body, "ApiService *{service}APIService").map_err(sink)?;
    for param in &path_params {
        writeln!(
            body,
            "{} {}",
            lower_camel(&param.name),
            compat_param_type(param, graph)?
        )
        .map_err(sink)?;
    }
    for param in &query_params {
        writeln!(
            body,
            "{} *{}",
            lower_camel(&param.name),
            compat_param_type(param, graph)?
        )
        .map_err(sink)?;
    }
    if has_body_value {
        writeln!(body, "body any").map_err(sink)?;
    }
    if file_field.is_some() {
        writeln!(body, "file any").map_err(sink)?;
    }
    if has_extra_query {
        writeln!(body, "extraQuery map[string]any").map_err(sink)?;
    }
    if has_extra_header {
        writeln!(body, "extraHeader map[string]string").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;

    let mut emitted_methods = BTreeSet::new();
    for setter in options
        .sdk
        .request_builder_aliases
        .body_aliases_for(&request_name)
    {
        for setter in compat_method_names(&setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_body_setter(body, &request_name, &setter)?;
        }
    }
    for param in &query_params {
        let setter = compat_exported(&param.name);
        for setter in compat_method_names(&setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            if options
                .sdk
                .query_setter_argument_policy
                .is_any_for(&request_name, &setter)
            {
                emit_compat_extra_query_setter(body, &request_name, &setter, &param.name, "any")?;
            } else {
                emit_compat_query_setter(
                    body,
                    &request_name,
                    &setter,
                    &lower_camel(&param.name),
                    &compat_param_type(param, graph)?,
                )?;
            }
        }
    }
    for alias in options
        .sdk
        .request_builder_aliases
        .query_aliases_for(&request_name)
    {
        for setter in compat_method_names(&alias.setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_extra_query_setter(body, &request_name, &setter, &alias.query_name, "any")?;
        }
    }
    for (setter, query_name) in global_query_setters {
        for setter in compat_method_names(setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_extra_query_setter(body, &request_name, &setter, query_name, "any")?;
        }
    }
    for (setter, field_name) in &body_field_setters {
        for setter in compat_method_names(setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_body_field_setter(body, &request_name, &setter, field_name)?;
        }
    }
    for field in &multipart_fields {
        for setter in compat_method_names(&field.setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            if field.is_file {
                emit_compat_file_setter(body, &request_name, &setter, &field.arg_name)?;
            } else {
                emit_compat_extra_query_setter(
                    body,
                    &request_name,
                    &setter,
                    &field.wire_name,
                    &field.arg_type,
                )?;
            }
        }
    }
    if file_field.is_some() {
        for setter in compat_method_names("File") {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_file_setter(body, &request_name, &setter, &compat_arg_name(&setter))?;
        }
    }
    for setter in body_setters {
        for setter in compat_method_names(&setter) {
            if compat_request_reserved_method(&setter) || !emitted_methods.insert(setter.clone()) {
                continue;
            }
            emit_compat_body_setter(body, &request_name, &setter)?;
        }
    }
    if has_extra_header {
        emit_compat_auth_setter(body, &request_name)?;
    }

    let preserve_legacy_execute = options
        .sdk
        .execute_compatibility
        .preserves(&request_name, &op.id);
    writeln!(body).map_err(sink)?;
    if preserve_legacy_execute && return_model != "struct{}" {
        writeln!(
            body,
            "func (r {request_name}) Execute() (*http.Response, error) {{"
        )
        .map_err(sink)?;
        writeln!(body, "_, resp, err := r.ExecuteTyped()").map_err(sink)?;
        writeln!(body, "return resp, err").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body).map_err(sink)?;
        let return_ty = compat_return_type(&return_model);
        writeln!(
            body,
            "func (r {request_name}) ExecuteTyped() ({return_ty}, *http.Response, error) {{"
        )
        .map_err(sink)?;
    } else if return_model == "struct{}" {
        writeln!(
            body,
            "func (r {request_name}) Execute() (*http.Response, error) {{"
        )
        .map_err(sink)?;
    } else {
        let return_ty = compat_return_type(&return_model);
        writeln!(
            body,
            "func (r {request_name}) Execute() ({return_ty}, *http.Response, error) {{"
        )
        .map_err(sink)?;
    }
    emit_compat_execute_body(
        body,
        op,
        graph,
        base_path,
        body_model.as_ref().map(|body| body.required),
        body_encoding,
        file_field.map(|field| field.wire_name.as_str()),
        has_extra_header,
        has_extra_query && body_encoding != CompatRequestBodyEncoding::Multipart,
        &return_model,
        &path_params,
        &query_params,
    )?;
    writeln!(body, "}}").map_err(sink)?;

    let args: Result<Vec<_>, _> = path_params
        .iter()
        .map(|param| {
            Ok(format!(
                "{} {}",
                lower_camel(&param.name),
                compat_param_type(param, graph)?
            ))
        })
        .collect();
    let args = args?;
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (a *{service}APIService) {method_name}(ctx context.Context{}) {request_name} {{",
        if args.is_empty() {
            String::new()
        } else {
            format!(", {}", args.join(", "))
        }
    )
    .map_err(sink)?;
    writeln!(body, "return {request_name}{{").map_err(sink)?;
    writeln!(body, "ApiService: a,").map_err(sink)?;
    writeln!(body, "ctx: ctx,").map_err(sink)?;
    for param in &path_params {
        let field = lower_camel(&param.name);
        writeln!(body, "{field}: {field},").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_extra_query_setter(
    body: &mut String,
    request_name: &str,
    setter: &str,
    query_name: &str,
    arg_type: &str,
) -> Result<(), CoreError> {
    let arg = compat_arg_name(setter);
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) {setter}({arg} {arg_type}) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "if r.extraQuery == nil {{").map_err(sink)?;
    writeln!(body, "r.extraQuery = map[string]any{{}}").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(
        body,
        "r.extraQuery[{}] = {arg}",
        quoted_string_literal(query_name)
    )
    .map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_query_setter(
    body: &mut String,
    request_name: &str,
    setter: &str,
    field: &str,
    arg_type: &str,
) -> Result<(), CoreError> {
    let arg = compat_arg_name(setter);
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) {setter}({arg} {arg_type}) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "r.{field} = &{arg}").map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_body_setter(
    body: &mut String,
    request_name: &str,
    setter: &str,
) -> Result<(), CoreError> {
    let arg = compat_arg_name(setter);
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) {setter}({arg} any) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "r.body = {arg}").map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_body_field_setter(
    body: &mut String,
    request_name: &str,
    setter: &str,
    field_name: &str,
) -> Result<(), CoreError> {
    let arg = compat_arg_name(setter);
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) {setter}({arg} any) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(
        body,
        "r.body = compatSetBodyField(r.body, {}, {arg})",
        quoted_string_literal(field_name)
    )
    .map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn compat_arg_name(name: &str) -> String {
    let mut candidate = lower_camel(name);
    if name.ends_with("Id") && candidate.ends_with("ID") {
        candidate.truncate(candidate.len() - 2);
        candidate.push_str("Id");
    }
    match candidate.as_str() {
        "any" | "bool" | "byte" | "comparable" | "complex64" | "complex128" | "error"
        | "float32" | "float64" | "int" | "int8" | "int16" | "int32" | "int64" | "rune"
        | "string" | "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr" => {
            format!("{candidate}Value")
        }
        _ => candidate,
    }
}

fn emit_compat_file_setter(
    body: &mut String,
    request_name: &str,
    setter: &str,
    arg: &str,
) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) {setter}({arg} any) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "r.file = {arg}").map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_auth_setter(body: &mut String, request_name: &str) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) Authorization(authorization string) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "if r.extraHeader == nil {{").map_err(sink)?;
    writeln!(body, "r.extraHeader = map[string]string{{}}").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "r.extraHeader[\"Authorization\"] = authorization").map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_compat_execute_body(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    declared_body_required: Option<bool>,
    body_encoding: CompatRequestBodyEncoding,
    multipart_file_field: Option<&str>,
    has_extra_header: bool,
    include_extra_query: bool,
    return_model: &str,
    path_params: &[&crate::graph::Param],
    query_params: &[&crate::graph::Param],
) -> Result<(), CoreError> {
    let returns_value = return_model != "struct{}";
    let returns_slice = return_model.starts_with("[]");
    let returns_map = return_model.starts_with("map[");
    let success = success_responses_of(op, graph)?;
    if returns_value {
        writeln!(
            body,
            "var localVarReturnValue {}",
            compat_return_type(return_model)
        )
        .map_err(sink)?;
    }
    writeln!(body, "var reqBody *bytes.Reader").map_err(sink)?;
    writeln!(body, "var reqContentType string").map_err(sink)?;
    writeln!(body, "var err error").map_err(sink)?;
    if body_encoding == CompatRequestBodyEncoding::Multipart {
        let file_field = multipart_file_field.unwrap_or("file");
        let file_arg = if multipart_file_field.is_some() {
            "r.file"
        } else {
            "nil"
        };
        writeln!(
            body,
            "var contentType string\nreqBody, contentType, err = compatMultipartFileBody({}, {file_arg}, r.extraQuery)",
            quoted_string_literal(file_field)
        )
        .map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqContentType = contentType").map_err(sink)?;
    } else if body_encoding == CompatRequestBodyEncoding::FormUrlEncoded
        && matches!(declared_body_required, Some(true))
    {
        writeln!(body, "bodyValue := r.body").map_err(sink)?;
        writeln!(body, "if bodyValue == nil {{").map_err(sink)?;
        writeln!(body, "bodyValue = map[string]any{{}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "encodedBody, err := compatEncodeFormBody(bodyValue)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(
            body,
            "reqContentType = \"application/x-www-form-urlencoded\""
        )
        .map_err(sink)?;
    } else if body_encoding == CompatRequestBodyEncoding::FormUrlEncoded
        && declared_body_required.is_some()
    {
        writeln!(body, "if r.body != nil {{").map_err(sink)?;
        writeln!(body, "encodedBody, err := compatEncodeFormBody(r.body)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(
            body,
            "reqContentType = \"application/x-www-form-urlencoded\""
        )
        .map_err(sink)?;
        writeln!(body, "}} else {{").map_err(sink)?;
        writeln!(body, "reqBody = bytes.NewReader(nil)").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    } else if matches!(declared_body_required, Some(true)) {
        writeln!(body, "bodyValue := r.body").map_err(sink)?;
        writeln!(body, "if bodyValue == nil {{").map_err(sink)?;
        writeln!(body, "bodyValue = map[string]any{{}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "encodedBody, err := compatEncodeJSONBody(bodyValue)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(body, "reqContentType = \"application/json\"").map_err(sink)?;
    } else if declared_body_required.is_some() {
        writeln!(body, "if r.body != nil {{").map_err(sink)?;
        writeln!(body, "encodedBody, err := compatEncodeJSONBody(r.body)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(body, "reqContentType = \"application/json\"").map_err(sink)?;
        writeln!(body, "}} else {{").map_err(sink)?;
        writeln!(body, "reqBody = bytes.NewReader(nil)").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    } else {
        writeln!(body, "reqBody = bytes.NewReader(nil)").map_err(sink)?;
    }

    emit_compat_request_url(body, op, base_path, path_params, returns_value)?;
    emit_compat_query(body, query_params, include_extra_query)?;
    writeln!(
        body,
        "req, err := http.NewRequestWithContext(r.ctx, {}, parsedURL.String(), reqBody)",
        quoted_string_literal(&op.method)
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if reqContentType != \"\" {{").map_err(sink)?;
    writeln!(body, "req.Header.Set(\"Content-Type\", reqContentType)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(
        body,
        "req.Header.Set(\"Accept\", {})",
        quoted_string_literal(&compat_accept_header(&success))
    )
    .map_err(sink)?;
    writeln!(
        body,
        "for key, value := range r.ApiService.client.cfg.DefaultHeader {{"
    )
    .map_err(sink)?;
    writeln!(body, "req.Header.Set(key, value)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    if has_extra_header {
        writeln!(body, "for key, value := range r.extraHeader {{").map_err(sink)?;
        writeln!(body, "req.Header.Set(key, value)").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    for scheme in operation_api_key_schemes(graph, op)? {
        if scheme.location != ApiKeyLocation::Header {
            continue;
        }
        writeln!(
            body,
            "compatApplyAPIKey(req, r.ctx, {}, {})",
            quoted_string_literal(&scheme.id),
            quoted_string_literal(&scheme.name)
        )
        .map_err(sink)?;
    }
    writeln!(body, "resp, err := r.ApiService.client.httpClient.Do(req)").map_err(sink)?;
    writeln!(body, "if err != nil || resp == nil {{").map_err(sink)?;
    write_compat_return(body, returns_value, "localVarReturnValue", "resp", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "localVarBody, readErr := io.ReadAll(resp.Body)").map_err(sink)?;
    writeln!(body, "resp.Body.Close()").map_err(sink)?;
    writeln!(
        body,
        "resp.Body = io.NopCloser(bytes.NewBuffer(localVarBody))"
    )
    .map_err(sink)?;
    writeln!(body, "if readErr != nil {{").map_err(sink)?;
    write_compat_return(
        body,
        returns_value,
        "localVarReturnValue",
        "resp",
        "readErr",
    )?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if resp.StatusCode >= 300 {{").map_err(sink)?;
    write_compat_return(
        body,
        returns_value,
        "localVarReturnValue",
        "resp",
        "&GenericOpenAPIError{body: localVarBody, error: resp.Status}",
    )?;
    writeln!(body, "}}").map_err(sink)?;
    if returns_value {
        if success.has_binary_body() {
            writeln!(
                body,
                "if {} {{",
                go_status_match("resp.StatusCode", &success.binary_statuses)
            )
            .map_err(sink)?;
            writeln!(body, "localVarReturnValue = localVarBody").map_err(sink)?;
            write_compat_return(body, true, "localVarReturnValue", "resp", "nil")?;
            writeln!(body, "}}").map_err(sink)?;
            if !success.has_bodyless_alternative() {
                write_compat_return(
                    body,
                    true,
                    "localVarReturnValue",
                    "resp",
                    "&GenericOpenAPIError{body: localVarBody, error: resp.Status}",
                )?;
            }
            return Ok(());
        }
        if !returns_slice && !returns_map {
            writeln!(body, "localVarReturnValue = new({return_model})").map_err(sink)?;
        }
        writeln!(body, "if len(localVarBody) > 0 {{").map_err(sink)?;
        let unmarshal_target = if returns_slice || returns_map {
            "&localVarReturnValue"
        } else {
            "localVarReturnValue"
        };
        writeln!(
            body,
            "if err := json.Unmarshal(localVarBody, {unmarshal_target}); err != nil {{"
        )
        .map_err(sink)?;
        write_compat_return(
            body,
            returns_value,
            "localVarReturnValue",
            "resp",
            "&GenericOpenAPIError{body: localVarBody, error: err.Error()}",
        )?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    write_compat_return(body, returns_value, "localVarReturnValue", "resp", "nil")?;
    Ok(())
}

fn compat_accept_header(success: &SuccessResponses) -> String {
    if success.has_binary_body() {
        success
            .binary_content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string())
    } else {
        "application/json".to_string()
    }
}

fn emit_compat_request_url(
    body: &mut String,
    op: &Operation,
    base_path: &str,
    path_params: &[&crate::graph::Param],
    returns_value: bool,
) -> Result<(), CoreError> {
    let abs = join_path(base_path, &op.path);
    let tokens = path_tokens(&abs);
    if tokens.is_empty() {
        writeln!(
            body,
            "reqURL := r.ApiService.client.cfg.serverURL() + \"{abs}\""
        )
        .map_err(sink)?;
    } else {
        let mut format_str = abs.clone();
        let mut args = Vec::new();
        for token in &tokens {
            format_str = format_str.replace(&format!("{{{token}}}"), "%s");
            args.push(format!(
                "url.PathEscape(fmt.Sprint(r.{}))",
                lower_camel(token)
            ));
        }
        if !path_tokens_match(
            &tokens,
            &path_params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
        ) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation '{}' path '{}' templated tokens {:?} do not match its path params",
                    op.id, abs, tokens
                ),
            });
        }
        writeln!(
            body,
            "reqURL := r.ApiService.client.cfg.serverURL() + fmt.Sprintf(\"{format_str}\", {})",
            args.join(", ")
        )
        .map_err(sink)?;
    }
    writeln!(body, "parsedURL, err := url.Parse(reqURL)").map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    write_compat_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_compat_query(
    body: &mut String,
    query_params: &[&crate::graph::Param],
    include_extra_query: bool,
) -> Result<(), CoreError> {
    if !query_params.is_empty() {
        writeln!(body, "q := parsedURL.Query()").map_err(sink)?;
        for param in query_params {
            let field = lower_camel(&param.name);
            writeln!(body, "if r.{field} != nil {{").map_err(sink)?;
            writeln!(
                body,
                "compatSetQueryValue(q, {}, *r.{field})",
                quoted_string_literal(&param.name)
            )
            .map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        writeln!(body, "parsedURL.RawQuery = q.Encode()").map_err(sink)?;
    }
    if include_extra_query {
        writeln!(body, "if len(r.extraQuery) > 0 {{").map_err(sink)?;
        writeln!(body, "q := parsedURL.Query()").map_err(sink)?;
        writeln!(body, "for key, value := range r.extraQuery {{").map_err(sink)?;
        writeln!(body, "compatSetQueryValue(q, key, value)").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "parsedURL.RawQuery = q.Encode()").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    Ok(())
}

fn compat_services(graph: &ApiGraph) -> Vec<String> {
    graph
        .operations
        .iter()
        .map(compat_service_name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn compat_query_setters(graph: &ApiGraph) -> Vec<(String, String)> {
    let mut setters = BTreeSet::new();
    for query_name in [
        "client_id",
        "client_secret",
        "code",
        "code_verifier",
        "force",
        "grant_type",
        "redirect_uri",
    ] {
        setters.insert((compat_exported(query_name), query_name.to_string()));
    }
    for op in &graph.operations {
        for param in &op.params {
            let setter = compat_exported(&param.name);
            if compat_request_reserved_method(&setter) {
                continue;
            }
            setters.insert((setter, param.name.clone()));
        }
    }
    for schema in graph
        .schemas
        .iter()
        .filter(|schema| compat_query_schema(schema))
    {
        let Type::Object(fields) = &schema.body else {
            continue;
        };
        for field in fields {
            let setter = compat_exported(&field.json_name);
            if compat_request_reserved_method(&setter) {
                continue;
            }
            setters.insert((setter, field.json_name.clone()));
        }
    }
    setters.into_iter().collect()
}

pub(crate) fn compat_method_names(name: &str) -> Vec<String> {
    const CANONICAL_TO_LEGACY: &[(&str, &str)] = &[
        ("ID", "Id"),
        ("UUID", "Uuid"),
        ("URL", "Url"),
        ("API", "Api"),
        ("HTTP", "Http"),
        ("JSON", "Json"),
    ];
    const LEGACY_TO_CANONICAL: &[(&str, &str)] = &[
        ("Id", "ID"),
        ("Uuid", "UUID"),
        ("Url", "URL"),
        ("Api", "API"),
        ("Http", "HTTP"),
        ("Json", "JSON"),
    ];
    let mut names = BTreeSet::from([name.to_string()]);
    for (from, to) in CANONICAL_TO_LEGACY.iter().chain(LEGACY_TO_CANONICAL.iter()) {
        for alias in replace_word_like_token(name, from, to) {
            if !alias.is_empty() {
                names.insert(alias);
            }
        }
    }
    names.into_iter().collect()
}

fn compat_query_schema(schema: &Schema) -> bool {
    let name = schema.name.to_ascii_lowercase();
    name.contains("query")
        || name.contains("filter")
        || name.contains("form")
        || name.contains("oauth")
        || name.contains("pdf")
        || name.contains("search")
        || name.contains("pagination")
        || name.contains("paginated")
        || name.contains("upload")
}

fn compat_body_field_setters(model: Option<&str>, graph: &ApiGraph) -> Vec<(String, String)> {
    let Some(model) = model else {
        return Vec::new();
    };
    let Some(schema) = graph.schemas.iter().find(|schema| schema.name == model) else {
        return Vec::new();
    };
    let Type::Object(fields) = &schema.body else {
        return Vec::new();
    };
    let mut setters = BTreeSet::new();
    for field in fields {
        let setter = compat_exported(&field.json_name);
        if compat_request_reserved_method(&setter) {
            continue;
        }
        setters.insert((setter, field.json_name.clone()));
    }
    setters.into_iter().collect()
}

fn compat_service_name(op: &Operation) -> String {
    op.group.as_deref().map_or_else(
        || {
            op.path
                .split('/')
                .find(|part| !part.trim().is_empty())
                .map_or_else(|| "Default".to_string(), compat_exported)
        },
        compat_exported,
    )
}

pub(crate) fn compat_request_name(op: &Operation) -> String {
    format!("Api{}Request", compat_operation_name(op))
}

pub(crate) fn compat_operation_name(op: &Operation) -> String {
    compat_exported(&op.handler)
}

fn compat_body_setters(model: Option<&str>, service: &str) -> Vec<String> {
    let mut setters = BTreeSet::from(["Body".to_string()]);
    setters.insert(service.to_string());
    setters.insert(format!("{service}Config"));
    if let Some(singular) = singular_compat_name(service) {
        setters.insert(singular.clone());
        setters.insert(format!("{singular}Config"));
    }
    let Some(model) = model else {
        return filter_compat_body_setters(setters);
    };
    let words = split_words(model);
    for start in 0..words.len() {
        let suffix = words[start..].join("");
        let exported_suffix = compat_exported(&suffix);
        if !exported_suffix.is_empty() {
            setters.insert(exported_suffix.clone());
            for trimmed in compat_trimmed_body_setters(&exported_suffix) {
                setters.insert(trimmed);
            }
        }
    }
    for word in &words {
        let setter = compat_exported(word);
        if !setter.is_empty() {
            setters.insert(setter);
        }
    }
    if words.iter().any(|word| word.eq_ignore_ascii_case("assign")) {
        setters.insert("Assignment".to_string());
    }
    setters.insert("Request".to_string());
    filter_compat_body_setters(setters)
}

fn singular_compat_name(name: &str) -> Option<String> {
    if name.ends_with("ies") && name.len() > 3 {
        return Some(format!("{}y", &name[..name.len() - 3]));
    }
    if name.ends_with('s') && !name.ends_with("ss") && name.len() > 1 {
        return Some(name[..name.len() - 1].to_string());
    }
    None
}

fn filter_compat_body_setters(setters: BTreeSet<String>) -> Vec<String> {
    setters
        .into_iter()
        .filter(|setter| !compat_request_reserved_method(setter))
        .collect()
}

pub(crate) fn compat_request_reserved_method(name: &str) -> bool {
    matches!(name, "Authorization" | "Execute")
}

fn compat_trimmed_body_setters(name: &str) -> Vec<String> {
    const LEADING: &[&str] = &[
        "Commandquery",
        "CommandQuery",
        "Command",
        "Query",
        "Dto",
        "Create",
        "Update",
        "Delete",
        "Get",
        "List",
        "Post",
        "Put",
        "Patch",
        "Upload",
    ];
    const TRAILING: &[&str] = &[
        "Input", "Request", "Output", "Response", "Dto", "Body", "Payload",
    ];
    let mut out = BTreeSet::new();
    let mut current = name.to_string();
    loop {
        let mut changed = false;
        for prefix in LEADING {
            if current.len() > prefix.len() && current.starts_with(prefix) {
                out.insert((*prefix).to_string());
                current = current[prefix.len()..].to_string();
                changed = true;
                break;
            }
        }
        for suffix in TRAILING {
            if current.len() > suffix.len() && current.ends_with(suffix) {
                current.truncate(current.len() - suffix.len());
                changed = true;
                break;
            }
        }
        if !current.is_empty() {
            out.insert(current.clone());
        }
        if !changed {
            break;
        }
    }
    if out.contains("VerifyEmail") {
        out.insert("Verification".to_string());
    }
    out.into_iter().collect()
}

fn compat_return_type(return_model: &str) -> String {
    if return_model.starts_with("[]") || return_model.starts_with("map[") {
        return_model.to_string()
    } else {
        format!("*{return_model}")
    }
}

fn compat_success_return_model(op: &Operation, graph: &ApiGraph) -> Result<String, CoreError> {
    if success_responses_of(op, graph)?.has_binary_body() {
        return Ok("[]byte".to_string());
    }
    for resp in &op.responses {
        if !(200..300).contains(&resp.status) {
            continue;
        }
        let Some(body) = &resp.body else {
            return Ok("struct{}".to_string());
        };
        let schema = graph
            .schemas
            .iter()
            .find(|schema| schema.id == body.ref_id)
            .ok_or_else(|| CoreError::SdkGen {
                message: format!(
                    "operation '{}' success response references dangling $ref '{}'",
                    op.id, body.ref_id
                ),
            })?;
        return match &schema.body {
            Type::Object(_) | Type::Enum(_) => Ok(schema.name.clone()),
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Any {} => go_compat_base_type(&schema.body, graph),
            Type::Union(_) => Err(CoreError::SdkGen {
                message: format!(
                    "operation '{}' success response schema '{}' has an unsupported union body",
                    op.id, schema.id
                ),
            }),
        };
    }
    Ok("struct{}".to_string())
}

fn write_compat_return(
    body: &mut String,
    returns_value: bool,
    value: &str,
    resp: &str,
    err: &str,
) -> Result<(), CoreError> {
    if returns_value {
        writeln!(body, "return {value}, {resp}, {err}").map_err(sink)
    } else {
        writeln!(body, "return {resp}, {err}").map_err(sink)
    }
}

fn emit_compat_alias_constructors(
    body: &mut String,
    alias: &str,
    canonical: &str,
    fields: &[Field],
    graph: &ApiGraph,
    options: &GoEmitOptions,
) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{alias}WithDefaults() *{alias} {{").map_err(sink)?;
    writeln!(body, "return (*{alias})(New{canonical}WithDefaults())").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let required: Vec<&Field> = fields
        .iter()
        .filter(|field| compat_constructor_requires_field(canonical, field))
        .collect();
    let args: Result<Vec<_>, _> = required
        .iter()
        .map(|field| {
            Ok(format!(
                "{} {}",
                lower_camel(&field.json_name),
                compat_constructor_arg_type(field, graph, options, Some(canonical))?
            ))
        })
        .collect();
    let names: Vec<_> = required
        .iter()
        .map(|field| lower_camel(&field.json_name))
        .collect();
    writeln!(body).map_err(sink)?;
    if !required.is_empty() && required.iter().all(|field| field.nullable) {
        writeln!(body, "func New{alias}() *{alias} {{").map_err(sink)?;
        writeln!(body, "return New{alias}WithDefaults()").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    } else {
        writeln!(body, "func New{alias}({}) *{alias} {{", args?.join(", ")).map_err(sink)?;
        writeln!(
            body,
            "return (*{alias})(New{canonical}({}))",
            names
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    Ok(())
}

/// Emit `errors.go`: the typed `APIError` (status + headers + raw/decoded body) + helpers.
///
/// The `Error()` string is prefixed with the SDK `package` (derived from config, the single source) so
/// the message names the actual SDK rather than a hard-coded fixture name.
pub(crate) fn emit_errors(package: &str) -> String {
    let body = format!(
        "\
// APIError is returned by operation methods on non-2xx responses. It exposes the
// HTTP status, response metadata, raw body, parsed JSON body, and decoded error body.
type APIError struct {{
StatusCode int
Headers http.Header
RequestID string
RawBody []byte
JSONBody any
Body any
Message string
Slug string
Hints []string
}}

// Error implements the error interface.
func (e *APIError) Error() string {{
return fmt.Sprintf(\"{package}: %d %s (%s)\", e.StatusCode, e.Message, e.Slug)
}}

// IsNotFound reports whether the error is a 404.
func (e *APIError) IsNotFound() bool {{
return e.StatusCode == 404
}}

func apiErrorObject(body any) map[string]any {{
object, ok := body.(map[string]any)
if !ok {{
return nil
}}
return object
}}

func apiErrorStringField(body any, key string) string {{
object := apiErrorObject(body)
if object == nil {{
return \"\"
}}
v := object[key]
switch value := v.(type) {{
case string:
return value
default:
return \"\"
}}
}}

func apiErrorStringSliceField(body any, key string) []string {{
object := apiErrorObject(body)
if object == nil {{
return nil
}}
v := object[key]
switch value := v.(type) {{
case []string:
return value
case []any:
out := make([]string, 0, len(value))
for _, item := range value {{
text, ok := item.(string)
if !ok {{
continue
}}
out = append(out, text)
}}
return out
default:
return nil
}}
}}
"
    );
    file(package, &["fmt", "net/http"], &body)
}

/// Emit the single `operations.go` resource surface: ctx-first typed methods on `*Client`.
///
/// `ops` are all of the graph's operations, in graph order. Each method:
/// - takes `ctx context.Context` first, then path params as positional `string` args, then a generated
///   `<Method>Params` struct for query-bearing ops, then a typed body input;
/// - marshals the body with `encoding/json`, builds the request to `baseURL + <absolute path>`, sets
///   `X-API-Key` when the client's apiKey is non-empty, and decodes a 2xx body into the success model
///   or a non-2xx body into an [`APIError`].
///
/// `package` is the SDK package name (derived from config, the single source) used in the file frame.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling body/response `$ref` for any op in the group.
pub(crate) fn emit_operations(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    ops: &[&Operation],
) -> Result<String, CoreError> {
    emit_operations_inner(graph, package, base_path, ops, true)
}

pub(crate) fn emit_operations_without_facades(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    ops: &[&Operation],
) -> Result<String, CoreError> {
    emit_operations_inner(graph, package, base_path, ops, false)
}

fn emit_operations_inner(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    ops: &[&Operation],
    include_facades: bool,
) -> Result<String, CoreError> {
    let mut body = String::new();
    let body_encodings = request_body_encodings(ops, graph)?;
    let mut first = true;
    for op in ops {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        emit_operation(&mut body, op, graph, base_path)?;
        emit_pagination_helpers(&mut body, op, graph)?;
    }
    emit_request_body_helpers(&mut body, &body_encodings)?;
    // Operation methods always touch context/net-http/encoding-json (request build + decode). Body
    // operations additionally need bytes; templated paths need fmt + net/url; non-string query params
    // need strconv/time. This stays correct when split layout emits one operation per file.
    let mut imports: Vec<&str> = vec!["context", "encoding/json", "io", "net/http"];
    if body_encodings.iter().any(|encoding| {
        matches!(
            encoding,
            RequestBodyEncoding::Json
                | RequestBodyEncoding::Multipart
                | RequestBodyEncoding::Binary
        )
    }) {
        imports.push("bytes");
    }
    if body_encodings.iter().any(|encoding| {
        matches!(
            encoding,
            RequestBodyEncoding::Text | RequestBodyEncoding::FormUrlEncoded
        )
    }) {
        imports.push("strings");
    }
    if body_encodings
        .iter()
        .any(|encoding| matches!(encoding, RequestBodyEncoding::Text))
    {
        imports.push("fmt");
    }
    if body_encodings.iter().any(|encoding| {
        matches!(
            encoding,
            RequestBodyEncoding::FormUrlEncoded | RequestBodyEncoding::Multipart
        )
    }) {
        imports.extend(["fmt", "net/url", "reflect", "strings"]);
    }
    if body_encodings
        .iter()
        .any(|encoding| matches!(encoding, RequestBodyEncoding::Multipart))
    {
        imports.push("mime/multipart");
    }
    let mut needs_io = ops
        .iter()
        .any(|op| op.request_body.is_some() && !op.request_body_required);
    for op in ops {
        if success_responses_of(op, graph)?.has_binary_body() {
            needs_io = true;
            break;
        }
    }
    if needs_io {
        imports.push("io");
    }
    imports.extend(query_imports(ops, graph)?);
    // WR-04: any op with a templated path interpolates `url.PathEscape(...)`, which needs `net/url`.
    if ops
        .iter()
        .any(|op| op.params.iter().any(|p| p.location == "path"))
    {
        imports.push("fmt");
        imports.push("net/url");
    }
    if include_facades {
        emit_group_facades(&mut body, graph, ops)?;
    }
    Ok(file(package, &imports, &body))
}

pub(crate) fn emit_facades(
    graph: &ApiGraph,
    package: &str,
    ops: &[&Operation],
) -> Result<Option<String>, CoreError> {
    let mut body = String::new();
    emit_group_facades(&mut body, graph, ops)?;
    if body.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(file(package, &[], &body)))
}

fn emit_group_facades(
    body: &mut String,
    graph: &ApiGraph,
    ops: &[&Operation],
) -> Result<(), CoreError> {
    let mut groups = BTreeMap::new();
    let mut facade_methods = BTreeMap::new();
    let mut facade_types = BTreeMap::new();
    let method_names: BTreeSet<String> = ops.iter().map(|op| exported(&op.handler)).collect();
    let schema_names: BTreeSet<&str> = graph
        .schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect();
    for op in ops {
        let Some(group) = &op.group else {
            continue;
        };
        if group == "default" {
            continue;
        }
        let method_name = exported(group);
        let type_name = format!("{method_name}API");
        if schema_names.contains(type_name.as_str()) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation group {group:?} cannot be emitted as Go facade type {type_name} because a schema uses that name"
                ),
            });
        }
        if method_names.contains(&method_name) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation group {group:?} cannot be emitted as a Go Client facade method"
                ),
            });
        }
        if let Some(existing) = facade_methods.insert(method_name.clone(), group.clone()) {
            if existing != *group {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "operation groups {existing:?} and {group:?} both emit Go Client facade method {method_name}"
                    ),
                });
            }
        }
        if let Some(existing) = facade_types.insert(type_name.clone(), group.clone()) {
            if existing != *group {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "operation groups {existing:?} and {group:?} both emit Go facade type {type_name}"
                    ),
                });
            }
        }
        groups.insert(group.clone(), (method_name, type_name));
    }
    for (_group, (method_name, type_name)) in groups {
        writeln!(body).map_err(sink)?;
        writeln!(body, "type {type_name} struct {{").map_err(sink)?;
        writeln!(body, "*Client").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body).map_err(sink)?;
        writeln!(body, "func (c *Client) {method_name}() *{type_name} {{").map_err(sink)?;
        writeln!(body, "return &{type_name}{{Client: c}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    Ok(())
}

/// Emit a single operation method, including its `<Method>Params` struct when the op has query params.
fn emit_operation(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
) -> Result<(), CoreError> {
    let method_name = exported(&op.handler);
    let path_params: Vec<&str> = op
        .params
        .iter()
        .filter(|p| p.location == "path")
        .map(|p| p.name.as_str())
        .collect();
    let query_params: Vec<&crate::graph::Param> =
        op.params.iter().filter(|p| p.location == "query").collect();

    // A params struct is emitted (and taken as an arg) only when the op has query params.
    if !query_params.is_empty() {
        emit_params_struct(body, &method_name, &query_params, graph)?;
        writeln!(body).map_err(sink)?;
    }

    let body_model = request_body_model_of(op, graph)?;
    let success = success_responses_of(op, graph)?;
    let auth_headers = operation_api_key_headers(graph, op)?;
    let auth_queries = operation_api_key_queries(graph, op)?;
    let auth_http = operation_http_auth_schemes(graph, op)?;
    // The return type is the success model when one exists, else an empty struct.
    let return_model = if success.has_binary_body() {
        "[]byte".to_string()
    } else {
        success
            .body_model
            .as_deref()
            .unwrap_or("struct{}")
            .to_string()
    };

    // Build the signature argument list.
    let mut args = vec!["ctx context.Context".to_string()];
    for p in &path_params {
        args.push(format!("{} string", lower_camel(p)));
    }
    if !query_params.is_empty() {
        args.push(format!("params {method_name}Params"));
    }
    if let Some(body_model) = &body_model {
        if body_model.required {
            args.push(format!("in {}", body_model.model));
        } else {
            args.push(format!("in *{}", body_model.model));
        }
    }
    args.push("opts ...RequestOption".to_string());

    writeln!(
        body,
        "// {method_name} -> {} {}",
        op.method,
        join_path(base_path, &op.path)
    )
    .map_err(sink)?;
    writeln!(
        body,
        "func (c *Client) {method_name}({}) ({return_model}, error) {{",
        args.join(", ")
    )
    .map_err(sink)?;
    writeln!(body, "var out {return_model}").map_err(sink)?;

    let has_decode = success.body_model.is_some();
    let has_binary = success.has_binary_body();
    let dispatch_returns = (has_decode || has_binary) && !success.has_bodyless_alternative();
    emit_request_dispatch(
        body,
        op,
        graph,
        base_path,
        &path_params,
        &query_params,
        &success,
        &auth_headers,
        &auth_queries,
        &auth_http,
    )?;
    if !dispatch_returns {
        writeln!(body, "return out, nil").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

struct GoPaginationInfo {
    page_type: String,
    item_type: String,
    items_field: String,
    next_cursor_field: Option<String>,
}

fn emit_pagination_helpers(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    let Some(policy) = pagination_policy_for(graph, op) else {
        return Ok(());
    };
    let method_name = exported(&op.handler);
    let pages_name = format!("{method_name}Pages");
    let items_name = format!("Iterate{method_name}");
    let info = go_pagination_info(graph, op, policy)?;
    let PaginationArgs { args, call_args } = go_pagination_args(op, graph)?;

    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "// {pages_name} follows the configured pagination policy for {method_name}."
    )
    .map_err(sink)?;
    writeln!(
        body,
        "func (c *Client) {pages_name}({}) ([]{}, error) {{",
        args.join(", "),
        info.page_type
    )
    .map_err(sink)?;
    writeln!(body, "var pages []{}", info.page_type).map_err(sink)?;
    emit_go_pagination_initialization(body, op, policy)?;
    writeln!(body, "for {{").map_err(sink)?;
    writeln!(
        body,
        "page, err := c.{method_name}({})",
        call_args.join(", ")
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    writeln!(body, "return nil, err").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    if policy.termination == PaginationTermination::EmptyItems {
        writeln!(body, "if len(page.{}) == 0 {{", info.items_field).map_err(sink)?;
        writeln!(body, "break").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body, "pages = append(pages, page)").map_err(sink)?;
    emit_go_pagination_advance(body, op, policy, &info, "break")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "return pages, nil").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let mut iter_args = args.clone();
    let opts = iter_args.pop().ok_or_else(|| CoreError::SdkGen {
        message: format!(
            "pagination helper for operation '{}' has no opts argument",
            op.id
        ),
    })?;
    iter_args.push(format!("yield func({}) bool", info.item_type));
    iter_args.push(opts);
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "// {items_name} visits every item from {pages_name} until yield returns false."
    )
    .map_err(sink)?;
    writeln!(
        body,
        "func (c *Client) {items_name}({}) error {{",
        iter_args.join(", ")
    )
    .map_err(sink)?;
    emit_go_pagination_initialization(body, op, policy)?;
    writeln!(body, "for {{").map_err(sink)?;
    writeln!(
        body,
        "page, err := c.{method_name}({})",
        call_args.join(", ")
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    writeln!(body, "return err").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    if policy.termination == PaginationTermination::EmptyItems {
        writeln!(body, "if len(page.{}) == 0 {{", info.items_field).map_err(sink)?;
        writeln!(body, "return nil").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body, "for _, item := range page.{} {{", info.items_field).map_err(sink)?;
    writeln!(body, "if !yield(item) {{").map_err(sink)?;
    writeln!(body, "return nil").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    emit_go_pagination_advance(body, op, policy, &info, "return nil")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

fn emit_go_pagination_advance(
    body: &mut String,
    op: &Operation,
    policy: &PaginationPolicy,
    info: &GoPaginationInfo,
    terminate: &str,
) -> Result<(), CoreError> {
    match policy.mode {
        PaginationMode::Cursor => {
            let cursor_param = policy
                .cursor_param
                .as_deref()
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!(
                        "pagination policy for operation '{}' is cursor mode without cursor_param",
                        op.id
                    ),
                })?;
            let next_field = info.next_cursor_field.as_deref().ok_or_else(|| {
                CoreError::SdkGen {
                    message: format!(
                        "pagination policy for operation '{}' is cursor mode without next_cursor_field",
                        op.id
                    ),
                }
            })?;
            let param = go_query_param(op, cursor_param)?;
            let param_field = exported(&param.name);
            writeln!(body, "nextCursor := page.{next_field}").map_err(sink)?;
            writeln!(body, "if nextCursor == \"\" {{").map_err(sink)?;
            writeln!(body, "{terminate}").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
            if param.required {
                writeln!(body, "params.{param_field} = nextCursor").map_err(sink)?;
            } else {
                writeln!(body, "params.{param_field} = &nextCursor").map_err(sink)?;
            }
        }
        PaginationMode::Page => {
            let page_param = policy
                .page_param
                .as_deref()
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!(
                        "pagination policy for operation '{}' is page mode without page_param",
                        op.id
                    ),
                })?;
            let param = go_query_param(op, page_param)?;
            let field = exported(&param.name);
            if param.required {
                writeln!(body, "params.{field} += 1").map_err(sink)?;
            } else {
                writeln!(body, "*params.{field} += 1").map_err(sink)?;
            }
        }
        PaginationMode::Offset => {
            let offset_param = policy
                .offset_param
                .as_deref()
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!(
                        "pagination policy for operation '{}' is offset mode without offset_param",
                        op.id
                    ),
                })?;
            let param = go_query_param(op, offset_param)?;
            let field = exported(&param.name);
            writeln!(body, "itemCount := int64(len(page.{}))", info.items_field).map_err(sink)?;
            if param.required {
                writeln!(body, "params.{field} += itemCount").map_err(sink)?;
            } else {
                writeln!(body, "*params.{field} += itemCount").map_err(sink)?;
            }
        }
    }
    Ok(())
}

fn emit_go_pagination_initialization(
    body: &mut String,
    op: &Operation,
    policy: &PaginationPolicy,
) -> Result<(), CoreError> {
    match policy.mode {
        PaginationMode::Cursor => {}
        PaginationMode::Page => {
            let Some(page_param) = policy.page_param.as_deref() else {
                return Ok(());
            };
            if go_query_param(op, page_param)?.required {
                return Ok(());
            }
            let field = exported(page_param);
            writeln!(body, "if params.{field} == nil {{").map_err(sink)?;
            writeln!(body, "initialPage := int64(1)").map_err(sink)?;
            writeln!(body, "params.{field} = &initialPage").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        PaginationMode::Offset => {
            let Some(offset_param) = policy.offset_param.as_deref() else {
                return Ok(());
            };
            if go_query_param(op, offset_param)?.required {
                return Ok(());
            }
            let field = exported(offset_param);
            writeln!(body, "if params.{field} == nil {{").map_err(sink)?;
            writeln!(body, "initialOffset := int64(0)").map_err(sink)?;
            writeln!(body, "params.{field} = &initialOffset").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
    }
    Ok(())
}

struct PaginationArgs {
    args: Vec<String>,
    call_args: Vec<String>,
}

fn go_pagination_args(op: &Operation, graph: &ApiGraph) -> Result<PaginationArgs, CoreError> {
    let method_name = exported(&op.handler);
    let path_params: Vec<&str> = op
        .params
        .iter()
        .filter(|p| p.location == "path")
        .map(|p| p.name.as_str())
        .collect();
    let query_params: Vec<&crate::graph::Param> =
        op.params.iter().filter(|p| p.location == "query").collect();
    let body_model = request_body_model_of(op, graph)?;

    let mut args = vec!["ctx context.Context".to_string()];
    let mut call_args = vec!["ctx".to_string()];
    for p in &path_params {
        let ident = lower_camel(p);
        args.push(format!("{ident} string"));
        call_args.push(ident);
    }
    if !query_params.is_empty() {
        args.push(format!("params {method_name}Params"));
        call_args.push("params".to_string());
    }
    if let Some(body_model) = &body_model {
        if body_model.required {
            args.push(format!("in {}", body_model.model));
        } else {
            args.push(format!("in *{}", body_model.model));
        }
        call_args.push("in".to_string());
    }
    args.push("opts ...RequestOption".to_string());
    call_args.push("opts...".to_string());
    Ok(PaginationArgs { args, call_args })
}

fn go_pagination_info(
    graph: &ApiGraph,
    op: &Operation,
    policy: &PaginationPolicy,
) -> Result<GoPaginationInfo, CoreError> {
    validate_go_pagination_params(op, policy)?;
    let success = success_responses_of(op, graph)?;
    let page_type = success.body_model.ok_or_else(|| CoreError::SdkGen {
        message: format!(
            "pagination policy for operation '{}' requires a JSON success response model",
            op.id
        ),
    })?;
    let schema = graph
        .schemas
        .iter()
        .find(|schema| schema.name == page_type)
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' references missing response model '{}'",
                op.id, page_type
            ),
        })?;
    let Type::Object(fields) = &schema.body else {
        return Err(CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' requires object response model '{}'",
                op.id, page_type
            ),
        });
    };
    let options = GoEmitOptions::default();
    let items = fields
        .iter()
        .find(|field| field.json_name == policy.items_field)
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' references missing response items field '{}'",
                op.id, policy.items_field
            ),
        })?;
    let Type::Array(item_schema) = &items.schema else {
        return Err(CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' response items field '{}' is not an array",
                op.id, policy.items_field
            ),
        });
    };
    let next_cursor_field = if let Some(next_cursor) = policy.next_cursor_field.as_deref() {
        let field = fields
            .iter()
            .find(|field| field.json_name == next_cursor)
            .ok_or_else(|| CoreError::SdkGen {
                message: format!(
                    "pagination policy for operation '{}' references missing next cursor field '{}'",
                    op.id, next_cursor
                ),
            })?;
        Some(go_field_name(&field.json_name, &options))
    } else {
        None
    };
    Ok(GoPaginationInfo {
        page_type,
        item_type: go_type(item_schema, false, graph)?,
        items_field: go_field_name(&items.json_name, &options),
        next_cursor_field,
    })
}

fn pagination_policy_for<'a>(graph: &'a ApiGraph, op: &Operation) -> Option<&'a PaginationPolicy> {
    graph
        .pagination
        .iter()
        .find(|policy| policy.operation_id == op.id)
}

fn go_query_param<'a>(
    op: &'a Operation,
    param_name: &str,
) -> Result<&'a crate::graph::Param, CoreError> {
    op.params
        .iter()
        .find(|param| param.location == "query" && param.name == param_name)
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' references missing query parameter '{}'",
                op.id, param_name
            ),
        })
}

fn validate_go_pagination_params(
    op: &Operation,
    policy: &PaginationPolicy,
) -> Result<(), CoreError> {
    for param_name in [policy.page_param.as_deref(), policy.offset_param.as_deref()]
        .into_iter()
        .flatten()
    {
        let param = go_query_param(op, param_name)?;
        if !matches!(param.schema, Type::Primitive(Prim::Int { .. })) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "pagination policy for operation '{}' requires numeric query parameter '{}'",
                    op.id, param_name
                ),
            });
        }
    }
    Ok(())
}

fn emit_required_request_body(
    body: &mut String,
    encoding: RequestBodyEncoding,
) -> Result<(), CoreError> {
    match encoding {
        RequestBodyEncoding::Json => {
            writeln!(body, "payload, err := json.Marshal(in)").map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
            writeln!(body, "reqBody := bytes.NewReader(payload)").map_err(sink)?;
        }
        RequestBodyEncoding::Text => {
            writeln!(body, "reqBody := strings.NewReader(fmt.Sprint(in))").map_err(sink)?;
        }
        RequestBodyEncoding::FormUrlEncoded => {
            writeln!(body, "reqBody, err := encodeFormBody(in)").map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        RequestBodyEncoding::Multipart => {
            writeln!(
                body,
                "reqBody, reqContentType, err := encodeMultipartBody(in)"
            )
            .map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        RequestBodyEncoding::Binary => {
            writeln!(body, "reqBody := bytes.NewReader([]byte(in))").map_err(sink)?;
        }
    }
    Ok(())
}

fn emit_optional_request_body(
    body: &mut String,
    encoding: RequestBodyEncoding,
) -> Result<(), CoreError> {
    writeln!(body, "var reqBody io.Reader").map_err(sink)?;
    if encoding == RequestBodyEncoding::Multipart {
        writeln!(body, "var reqContentType string").map_err(sink)?;
    }
    writeln!(body, "if in != nil {{").map_err(sink)?;
    match encoding {
        RequestBodyEncoding::Json => {
            writeln!(body, "payload, err := json.Marshal(in)").map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
            writeln!(body, "reqBody = bytes.NewReader(payload)").map_err(sink)?;
        }
        RequestBodyEncoding::Text => {
            writeln!(body, "reqBody = strings.NewReader(fmt.Sprint(*in))").map_err(sink)?;
        }
        RequestBodyEncoding::FormUrlEncoded => {
            writeln!(body, "var err error").map_err(sink)?;
            writeln!(body, "reqBody, err = encodeFormBody(in)").map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        RequestBodyEncoding::Multipart => {
            writeln!(body, "var err error").map_err(sink)?;
            writeln!(body, "var reader *bytes.Reader").map_err(sink)?;
            writeln!(
                body,
                "reader, reqContentType, err = encodeMultipartBody(in)"
            )
            .map_err(sink)?;
            writeln!(body, "if err != nil {{").map_err(sink)?;
            writeln!(body, "return out, err").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
            writeln!(body, "reqBody = reader").map_err(sink)?;
        }
        RequestBodyEncoding::Binary => {
            writeln!(body, "reqBody = bytes.NewReader([]byte(*in))").map_err(sink)?;
        }
    }
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Emit the body-marshal → URL → request-build → query → auth → execute → decode sequence of a method.
///
/// Split out of [`emit_operation`] so each half stays under the clippy `too_many_lines` ceiling; the
/// caller has already written the doc comment, signature, and `var out` line.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_request_dispatch(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    path_params: &[&str],
    query_params: &[&crate::graph::Param],
    success: &SuccessResponses,
    auth_headers: &[String],
    auth_queries: &[String],
    auth_http: &[HttpAuthScheme],
) -> Result<(), CoreError> {
    let body_model = request_body_model_of(op, graph)?;
    let has_body = body_model.is_some();
    let has_decode = success.body_model.is_some();
    let has_binary = success.has_binary_body();

    // Body marshalling.
    if let Some(body_info) = &body_model {
        if body_info.required {
            emit_required_request_body(body, body_info.encoding)?;
        } else {
            emit_optional_request_body(body, body_info.encoding)?;
        }
    }

    // URL construction: baseURL + absolute path with path params interpolated.
    emit_url(body, op, base_path, path_params)?;

    // Request build.
    let body_arg = if has_body { "reqBody" } else { "nil" };
    writeln!(
        body,
        "req, err := http.NewRequestWithContext(ctx, \"{}\", reqURL, {body_arg})",
        op.method
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    writeln!(body, "return out, err").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    if let Some(body_info) = &body_model {
        let content_type = quoted_string_literal(&body_info.content_type);
        if body_info.required {
            if body_info.encoding == RequestBodyEncoding::Multipart {
                writeln!(body, "req.Header.Set(\"Content-Type\", reqContentType)").map_err(sink)?;
            } else {
                writeln!(body, "req.Header.Set(\"Content-Type\", {content_type})").map_err(sink)?;
            }
        } else {
            writeln!(body, "if in != nil {{").map_err(sink)?;
            if body_info.encoding == RequestBodyEncoding::Multipart {
                writeln!(body, "req.Header.Set(\"Content-Type\", reqContentType)").map_err(sink)?;
            } else {
                writeln!(body, "req.Header.Set(\"Content-Type\", {content_type})").map_err(sink)?;
            }
            writeln!(body, "}}").map_err(sink)?;
        }
    }

    // Query parameter encoding.
    if !query_params.is_empty() || !auth_queries.is_empty() {
        writeln!(body, "q := req.URL.Query()").map_err(sink)?;
        for p in query_params {
            let field = exported(&p.name);
            // WR-02: `url.Values.Set` takes a string, so a non-string typed query field is coerced to
            // a string at the call site (the conversion is the identity for string fields, keeping the
            // all-string fixture byte-identical). The required path reads the value directly; the
            // optional path dereferences the pointer inside the nil-guard.
            let value_ty = go_type(&p.schema, false, graph)?;
            if p.required {
                let expr = query_string_expr(&value_ty, &format!("params.{field}"))?;
                writeln!(body, "q.Set(\"{}\", {expr})", p.name).map_err(sink)?;
            } else {
                writeln!(body, "if params.{field} != nil {{").map_err(sink)?;
                let expr = query_string_expr(&value_ty, &format!("*params.{field}"))?;
                writeln!(body, "q.Set(\"{}\", {expr})", p.name).map_err(sink)?;
                writeln!(body, "}}").map_err(sink)?;
            }
        }
        for query in auth_queries {
            writeln!(
                body,
                "if key := c.apiKeys[{}]; key != \"\" {{",
                quoted_string_literal(query)
            )
            .map_err(sink)?;
            writeln!(body, "q.Set({}, key)", quoted_string_literal(query)).map_err(sink)?;
            writeln!(body, "}} else if c.apiKey != \"\" {{").map_err(sink)?;
            writeln!(body, "q.Set({}, c.apiKey)", quoted_string_literal(query)).map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        writeln!(body, "req.URL.RawQuery = q.Encode()").map_err(sink)?;
    }

    // Auth headers.
    for header in auth_headers {
        writeln!(
            body,
            "if key := c.apiKeys[{}]; key != \"\" {{",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(
            body,
            "req.Header.Set({}, key)",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(body, "}} else if c.apiKey != \"\" {{").map_err(sink)?;
        writeln!(
            body,
            "req.Header.Set({}, c.apiKey)",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    for scheme in auth_http {
        match scheme {
            HttpAuthScheme::Bearer => {
                writeln!(body, "if c.bearerToken != \"\" {{").map_err(sink)?;
                writeln!(
                    body,
                    "req.Header.Set(\"Authorization\", \"Bearer \"+c.bearerToken)"
                )
                .map_err(sink)?;
                writeln!(body, "}}").map_err(sink)?;
            }
            HttpAuthScheme::Basic => {
                writeln!(
                    body,
                    "if c.basicUsername != \"\" || c.basicPassword != \"\" {{"
                )
                .map_err(sink)?;
                writeln!(body, "req.SetBasicAuth(c.basicUsername, c.basicPassword)")
                    .map_err(sink)?;
                writeln!(body, "}}").map_err(sink)?;
            }
        }
    }

    let runtime = go_operation_runtime(graph, op);
    let idempotency_header = runtime.idempotency_key_header.unwrap_or("Idempotency-Key");
    // Execute.
    writeln!(body, "resp, err := c.do(req, runtimeRequestOptions{{").map_err(sink)?;
    writeln!(body, "OperationID: {},", quoted_string_literal(&op.id)).map_err(sink)?;
    writeln!(body, "PathTemplate: {},", quoted_string_literal(&op.path)).map_err(sink)?;
    writeln!(body, "Idempotent: {},", runtime.idempotent).map_err(sink)?;
    writeln!(
        body,
        "IdempotencyKeyHeader: {},",
        quoted_string_literal(idempotency_header)
    )
    .map_err(sink)?;
    writeln!(body, "Options: newRequestOptions(opts...),").map_err(sink)?;
    writeln!(body, "}})").map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    writeln!(body, "return out, err").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "defer resp.Body.Close()").map_err(sink)?;

    // Non-2xx → typed APIError, decoding the graph's actual error model (CR-01).
    writeln!(
        body,
        "if resp.StatusCode < 200 || resp.StatusCode >= 300 {{"
    )
    .map_err(sink)?;
    emit_error_decode(body, op, graph)?;
    writeln!(body, "}}").map_err(sink)?;

    // 2xx → read binary success bodies or decode JSON only for statuses that declare that body.
    if has_binary {
        writeln!(
            body,
            "if {} {{",
            go_status_match("resp.StatusCode", &success.binary_statuses)
        )
        .map_err(sink)?;
        writeln!(body, "data, err := io.ReadAll(resp.Body)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        writeln!(body, "return out, err").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return data, nil").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        if !success.has_bodyless_alternative() {
            writeln!(body, "return out, &APIError{{StatusCode: resp.StatusCode}}").map_err(sink)?;
        }
    } else if has_decode {
        writeln!(
            body,
            "if {} {{",
            go_status_match("resp.StatusCode", &success.body_statuses)
        )
        .map_err(sink)?;
        writeln!(
            body,
            "if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {{"
        )
        .map_err(sink)?;
        writeln!(body, "return out, err").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "return out, nil").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        if !success.has_bodyless_alternative() {
            writeln!(body, "return out, &APIError{{StatusCode: resp.StatusCode}}").map_err(sink)?;
        }
    }
    Ok(())
}

struct GoOperationRuntime<'a> {
    idempotent: bool,
    idempotency_key_header: Option<&'a str>,
}

fn go_operation_runtime<'a>(graph: &'a ApiGraph, op: &Operation) -> GoOperationRuntime<'a> {
    graph
        .operation_runtime
        .iter()
        .find(|policy| policy.operation_id == op.id)
        .map_or(
            GoOperationRuntime {
                idempotent: false,
                idempotency_key_header: None,
            },
            |policy| GoOperationRuntime {
                idempotent: policy.idempotent,
                idempotency_key_header: policy.idempotency_key_header.as_deref(),
            },
        )
}

fn go_status_match(expr: &str, statuses: &[u16]) -> String {
    statuses
        .iter()
        .map(|status| format!("{expr} == {status}"))
        .collect::<Vec<_>>()
        .join(" || ")
}

/// Emit the non-2xx error-decode block: read raw bytes once, parse generic JSON once, decode any
/// explicit status error schema, then return a populated `*APIError`.
fn emit_error_decode(body: &mut String, op: &Operation, graph: &ApiGraph) -> Result<(), CoreError> {
    let error_bodies = error_response_bodies_of(op, graph)?;
    writeln!(body, "rawBody, _ := io.ReadAll(resp.Body)").map_err(sink)?;
    writeln!(body, "var jsonBody any").map_err(sink)?;
    writeln!(body, "if len(rawBody) > 0 {{").map_err(sink)?;
    writeln!(body, "_ = json.Unmarshal(rawBody, &jsonBody)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "var typedBody any").map_err(sink)?;
    if !error_bodies.is_empty() {
        writeln!(body, "switch resp.StatusCode {{").map_err(sink)?;
        for error_body in &error_bodies {
            writeln!(body, "case {}:", error_body.status).map_err(sink)?;
            writeln!(body, "var decoded {}", error_body.model).map_err(sink)?;
            writeln!(body, "if len(rawBody) > 0 {{").map_err(sink)?;
            writeln!(body, "_ = json.Unmarshal(rawBody, &decoded)").map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
            writeln!(body, "typedBody = decoded").map_err(sink)?;
        }
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body, "if typedBody == nil {{").map_err(sink)?;
    writeln!(body, "typedBody = jsonBody").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "return out, &APIError{{").map_err(sink)?;
    writeln!(body, "StatusCode: resp.StatusCode,").map_err(sink)?;
    writeln!(body, "Headers: resp.Header.Clone(),").map_err(sink)?;
    writeln!(body, "RequestID: resp.Header.Get(\"X-Request-ID\"),").map_err(sink)?;
    writeln!(body, "RawBody: rawBody,").map_err(sink)?;
    writeln!(body, "JSONBody: jsonBody,").map_err(sink)?;
    writeln!(body, "Body: typedBody,").map_err(sink)?;
    writeln!(body, "Message: apiErrorStringField(jsonBody, \"message\"),").map_err(sink)?;
    writeln!(body, "Slug: apiErrorStringField(jsonBody, \"slug\"),").map_err(sink)?;
    writeln!(
        body,
        "Hints: apiErrorStringSliceField(jsonBody, \"hints\"),"
    )
    .map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Build the Go expression that coerces a query-param value of Go type `value_ty` to a `string` for
/// `url.Values.Set` (WR-02).
///
/// A `string` field is passed through unchanged (so the all-string fixture stays byte-identical);
/// the supported scalar Go types are converted precisely via `strconv`; a `time.Time` is formatted
/// as RFC 3339. Any other Go type (e.g. a slice or a named struct) is an unsupported query-param
/// shape and returns a typed [`CoreError::SdkGen`] rather than emitting non-compiling Go.
///
/// `accessor` is the Go expression that reads the value (e.g. `params.Page` or `*params.Cursor`).
fn query_string_expr(value_ty: &str, accessor: &str) -> Result<String, CoreError> {
    match value_ty {
        "string" => Ok(accessor.to_string()),
        "int64" => Ok(format!("strconv.FormatInt({accessor}, 10)")),
        "float32" => Ok(format!("strconv.FormatFloat(float64({accessor}), 'g', -1, 32)")),
        "float64" => Ok(format!("strconv.FormatFloat({accessor}, 'g', -1, 64)")),
        "bool" => Ok(format!("strconv.FormatBool({accessor})")),
        "time.Time" => Ok(format!("{accessor}.Format(time.RFC3339)")),
        other => Err(CoreError::SdkGen {
            message: format!(
                "unsupported query-param Go type '{other}': only string/int64/float32/float64/bool/time.Time \
                 query parameters can be URL-encoded"
            ),
        }),
    }
}

/// The stdlib import a query-param value of Go type `value_ty` needs to be URL-encoded (WR-02), if any.
///
/// `string` needs nothing; the `strconv`-converted scalars need `strconv`; `time.Time` needs `time`.
/// Returns `None` for a type with no extra import (or an unsupported one — the error surfaces later in
/// [`query_string_expr`] during emission, so this stays infallible for the import pre-scan).
fn query_extra_import(value_ty: &str) -> Option<&'static str> {
    match value_ty {
        "int64" | "float32" | "float64" | "bool" => Some("strconv"),
        "time.Time" => Some("time"),
        _ => None,
    }
}

/// Collect the extra stdlib imports the operations file needs for non-string query-param encoding.
///
/// Scans every query param of every operation in the file; returns the sorted, de-duplicated set of
/// imports beyond the always-present request-plumbing set. For the all-string fixture this is empty,
/// so the import block (and the whole file) stays byte-identical (WR-02).
fn query_imports(ops: &[&Operation], graph: &ApiGraph) -> Result<Vec<&'static str>, CoreError> {
    let mut extra: BTreeSet<&'static str> = BTreeSet::new();
    for op in ops {
        for p in op.params.iter().filter(|p| p.location == "query") {
            let value_ty = go_type(&p.schema, false, graph)?;
            if let Some(imp) = query_extra_import(&value_ty) {
                extra.insert(imp);
            }
        }
    }
    Ok(extra.into_iter().collect())
}

fn request_body_encodings(
    ops: &[&Operation],
    graph: &ApiGraph,
) -> Result<Vec<RequestBodyEncoding>, CoreError> {
    let mut encodings = Vec::new();
    for op in ops {
        if let Some(body) = request_body_model_of(op, graph)? {
            encodings.push(body.encoding);
        }
    }
    encodings.sort_by_key(|encoding| match encoding {
        RequestBodyEncoding::Json => 0_u8,
        RequestBodyEncoding::Text => 1,
        RequestBodyEncoding::FormUrlEncoded => 2,
        RequestBodyEncoding::Multipart => 3,
        RequestBodyEncoding::Binary => 4,
    });
    encodings.dedup();
    Ok(encodings)
}

#[expect(
    clippy::too_many_lines,
    reason = "request media helper emission writes fixed Go helper source blocks in one deterministic section"
)]
fn emit_request_body_helpers(
    body: &mut String,
    encodings: &[RequestBodyEncoding],
) -> Result<(), CoreError> {
    let needs_form = encodings
        .iter()
        .any(|encoding| matches!(encoding, RequestBodyEncoding::FormUrlEncoded));
    let needs_multipart = encodings
        .iter()
        .any(|encoding| matches!(encoding, RequestBodyEncoding::Multipart));
    if !needs_form && !needs_multipart {
        return Ok(());
    }
    if needs_form {
        writeln!(
            body,
            r"
func encodeFormBody(v any) (*strings.Reader, error) {{
values := url.Values{{}}
if err := addFormValues(values, v); err != nil {{
return nil, err
}}
return strings.NewReader(values.Encode()), nil
}}
"
        )
        .map_err(sink)?;
    }
    if needs_multipart {
        writeln!(
            body,
            r#"
func encodeMultipartBody(v any) (*bytes.Reader, string, error) {{
var buf bytes.Buffer
writer := multipart.NewWriter(&buf)
if err := addMultipartValues(writer, v); err != nil {{
return nil, "", err
}}
if err := writer.Close(); err != nil {{
return nil, "", err
}}
return bytes.NewReader(buf.Bytes()), writer.FormDataContentType(), nil
}}
"#
        )
        .map_err(sink)?;
    }
    writeln!(
        body,
        r#"
func addFormValues(values url.Values, value any) error {{
if value == nil {{
return nil
}}
reflected := reflect.ValueOf(value)
for reflected.Kind() == reflect.Ptr || reflected.Kind() == reflect.Interface {{
if reflected.IsNil() {{
return nil
}}
reflected = reflected.Elem()
}}
if reflected.Kind() != reflect.Struct {{
return fmt.Errorf("form body must be a struct")
}}
typ := reflected.Type()
for i := 0; i < reflected.NumField(); i++ {{
field := typ.Field(i)
if field.PkgPath != "" {{
continue
}}
name, omitempty := formFieldName(field)
if name == "" || name == "-" {{
continue
}}
fieldValue := reflected.Field(i)
if omitempty && fieldValue.IsZero() {{
continue
}}
	addFormField(values, name, fieldValue.Interface())
	}}
	return nil
	}}

	func addFormField(values url.Values, name string, value any) {{
	v := reflect.ValueOf(value)
	if !v.IsValid() {{
	values.Set(name, "")
	return
	}}
	for v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {{
	if v.IsNil() {{
	return
	}}
	v = v.Elem()
	}}
	if v.Kind() == reflect.Slice || v.Kind() == reflect.Array {{
	values.Del(name)
	for i := 0; i < v.Len(); i++ {{
	values.Add(name, formValue(v.Index(i).Interface()))
	}}
	return
	}}
values.Set(name, formValue(value))
}}

func formFieldName(field reflect.StructField) (string, bool) {{
tag := field.Tag.Get("form")
if tag == "" {{
tag = field.Tag.Get("json")
}}
if tag == "-" {{
return "-", false
}}
omitempty := false
if tag != "" {{
parts := strings.Split(tag, ",")
for _, option := range parts[1:] {{
if option == "omitempty" {{
omitempty = true
}}
}}
if parts[0] != "" {{
return parts[0], omitempty
}}
}}
return field.Name, omitempty
}}

func formValue(value any) string {{
v := reflect.ValueOf(value)
if !v.IsValid() {{
return ""
}}
if v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {{
if v.IsNil() {{
return ""
}}
return formValue(v.Elem().Interface())
}}
if v.Kind() != reflect.Slice && v.Kind() != reflect.Array {{
return fmt.Sprint(value)
}}
parts := make([]string, 0, v.Len())
for i := 0; i < v.Len(); i++ {{
parts = append(parts, formValue(v.Index(i).Interface()))
}}
return strings.Join(parts, ",")
}}
"#
    )
    .map_err(sink)?;
    if needs_multipart {
        writeln!(
            body,
            r#"
func addMultipartValues(writer *multipart.Writer, value any) error {{
if value == nil {{
return nil
}}
reflected := reflect.ValueOf(value)
for reflected.Kind() == reflect.Ptr || reflected.Kind() == reflect.Interface {{
if reflected.IsNil() {{
return nil
}}
reflected = reflected.Elem()
}}
if reflected.Kind() != reflect.Struct {{
return fmt.Errorf("multipart body must be a struct")
}}
typ := reflected.Type()
for i := 0; i < reflected.NumField(); i++ {{
field := typ.Field(i)
if field.PkgPath != "" {{
continue
}}
name, omitempty := formFieldName(field)
if name == "" || name == "-" {{
continue
}}
fieldValue := reflected.Field(i)
if omitempty && fieldValue.IsZero() {{
continue
}}
if err := writeMultipartField(writer, name, fieldValue.Interface()); err != nil {{
return err
}}
}}
return nil
}}

func writeMultipartField(writer *multipart.Writer, name string, value any) error {{
v := reflect.ValueOf(value)
if !v.IsValid() {{
return writer.WriteField(name, "")
}}
if v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {{
if v.IsNil() {{
return nil
}}
return writeMultipartField(writer, name, v.Elem().Interface())
}}
	if v.Kind() == reflect.Slice && v.Type().Elem().Kind() == reflect.Uint8 {{
	part, err := writer.CreateFormFile(name, name)
	if err != nil {{
	return err
	}}
	_, err = part.Write(v.Bytes())
	return err
	}}
	if v.Kind() == reflect.Slice || v.Kind() == reflect.Array {{
	for i := 0; i < v.Len(); i++ {{
	if err := writeMultipartField(writer, name, v.Index(i).Interface()); err != nil {{
	return err
	}}
	}}
	return nil
	}}
	return writer.WriteField(name, formValue(value))
	}}
"#
        )
        .map_err(sink)?;
    }
    Ok(())
}

/// Emit the `url :=` line, interpolating path params via `fmt.Sprintf` when the path is templated.
///
/// WR-03: the set of `{token}`s in the absolute path is asserted to equal the set of declared path
/// params before emitting, so a token with no matching arg (a runtime `%!s(MISSING)` in the URL) or
/// an arg with no matching token becomes a typed [`CoreError::SdkGen`] at generation time instead.
///
/// WR-04: each interpolated path value is wrapped in `url.PathEscape(...)` so a value containing
/// `/`, `?`, `#`, `%`, or `..` can never restructure the request URL.
fn emit_url(
    body: &mut String,
    op: &Operation,
    base_path: &str,
    path_params: &[&str],
) -> Result<(), CoreError> {
    let abs = join_path(base_path, &op.path);
    let tokens = path_tokens(&abs);

    // WR-03: the templated tokens must be exactly the declared path params (order-independent set
    // equality), so neither a dangling token nor an unused arg can slip through.
    if !path_tokens_match(&tokens, path_params) {
        return Err(CoreError::SdkGen {
            message: format!(
                "operation '{}' path '{}' templated tokens {:?} do not match its path params {:?}",
                op.id, abs, tokens, path_params
            ),
        });
    }

    if tokens.is_empty() {
        writeln!(body, "reqURL := c.baseURL + \"{abs}\"").map_err(sink)?;
        return Ok(());
    }

    // Replace each {token} with %s and pass the escaped positional arg, in PATH order (so the
    // Sprintf verbs and args line up regardless of the param sort order).
    let mut format_str = abs.clone();
    let mut args: Vec<String> = Vec::new();
    for token in &tokens {
        let placeholder = format!("{{{token}}}");
        format_str = format_str.replace(&placeholder, "%s");
        // WR-04: percent-encode the value so it cannot inject extra path/query segments.
        args.push(format!("url.PathEscape({})", lower_camel(token)));
    }
    writeln!(
        body,
        "reqURL := c.baseURL + fmt.Sprintf(\"{format_str}\", {})",
        args.join(", ")
    )
    .map_err(sink)?;
    Ok(())
}

/// Emit a `<Method>Params` struct for a query-bearing operation (required → value, optional → pointer).
fn emit_params_struct(
    body: &mut String,
    method_name: &str,
    query_params: &[&crate::graph::Param],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(
        body,
        "// {method_name}Params carries the query parameters for {method_name}."
    )
    .map_err(sink)?;
    writeln!(body, "type {method_name}Params struct {{").map_err(sink)?;
    for p in query_params {
        let go_name = exported(&p.name);
        // Query params are strings (the graph infers `string` for untyped query params). Unlike struct
        // fields, an OPTIONAL query param is always a pointer so the SDK can distinguish "unset" from
        // "empty string" when encoding the query (matches expected/sdk ListGoalsParams: `Cursor
        // *string`, required `Aggregation string`). Use the value Go type, then pointer-wrap when optional.
        let value_ty = go_type(&p.schema, false, graph)?;
        let go_ty = if p.required {
            value_ty
        } else {
            format!("*{value_ty}")
        };
        writeln!(body, "{go_name} {go_ty}").map_err(sink)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Lower-camelCase a path-param identifier for use as an idiomatic Go function argument.
///
/// The FIRST word is fully lower-cased (so the initialism-aware [`exported`] does not yield `uUID` for
/// `uuid`), and subsequent words use the exported (initialism-aware) form: `uuid`→`uuid`,
/// `goalId`→`goalID`, `page_size`→`pageSize`. An unexported leading word avoids exporting the local
/// argument while keeping `gofmt`-clean, compiling Go (03-03 `go build`).
fn lower_camel(name: &str) -> String {
    let words = split_words(name);
    let mut out = String::new();
    for (i, word) in words.iter().enumerate() {
        if i == 0 {
            out.push_str(&word.to_ascii_lowercase());
        } else {
            out.push_str(&exported(word));
        }
    }
    if out.is_empty() {
        out.push_str("value");
    } else if !out.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic()) {
        out.insert_str(0, "value");
    }
    if is_go_keyword(&out) {
        out.push_str("Value");
    }
    out
}

fn is_go_keyword(value: &str) -> bool {
    matches!(
        value,
        "break"
            | "default"
            | "func"
            | "interface"
            | "select"
            | "case"
            | "defer"
            | "go"
            | "map"
            | "struct"
            | "chan"
            | "else"
            | "goto"
            | "package"
            | "switch"
            | "const"
            | "fallthrough"
            | "if"
            | "range"
            | "type"
            | "continue"
            | "for"
            | "import"
            | "return"
            | "var"
    )
}

/// Frame a Go file: `package <package>`, a computed import block, then the body.
///
/// `package` is the SDK package name, derived from `output.go_module` (the single source of truth);
/// imports are sorted + de-duplicated (a `BTreeSet`) so the block is deterministic and `gofmt`-stable.
/// A single import emits the one-line form; multiple imports emit the parenthesized block — `gofmt`
/// canonicalizes either, so this is just to keep the pre-format text tidy.
fn file(package: &str, imports: &[&str], body: &str) -> String {
    // `write!` into a String is infallible in practice; the trait is fallible, so swallow the unit
    // error with `let _ =` rather than `unwrap` (RUST-04) — there is no failure mode to surface.
    let mut out = String::new();
    let _ = writeln!(out, "package {package}");
    let set: BTreeSet<&str> = imports.iter().copied().collect();
    if !set.is_empty() {
        out.push('\n');
        if set.len() == 1 {
            for imp in &set {
                let _ = writeln!(out, "import \"{imp}\"");
            }
        } else {
            out.push_str("import (\n");
            for imp in &set {
                let _ = writeln!(out, "\"{imp}\"");
            }
            out.push_str(")\n");
        }
    }
    out.push('\n');
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        emit_client, emit_errors, emit_models, emit_operations, exported, go_type, join_path,
        lower_camel,
    };
    use crate::graph::ApiGraph;

    /// A facts document covering: optional pointer fields, a required field, an enum, uuid/time/number
    /// types, a nested ref, and one POST + one GET-with-query operation — enough to exercise every
    /// emitter branch without depending on the live Go toolchain.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "handler": "createGoal",
          "operation_id": "createGoal", "params": [],
          "request_body": { "ref_id": "dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "dto.CommandMessage" } },
            { "status": 400, "body": { "ref_id": "dto.HttpError" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listGoals",
          "operation_id": "listGoals", "params": [
            { "name": "aggregation", "location": "query", "required": true,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } },
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.GoalResponse" } } ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        }
      ],
      "schemas": [
        {
          "id": "dto.CommandMessage", "name": "CommandMessage",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.CreateGoalInput", "name": "CreateGoalInput",
          "body": { "type": "object", "of": [
            { "json_name": "analyticsQuery", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "named", "of": "dto.GoalAnalyticsQuery" },
              "description": null, "example": null },
            { "json_name": "createdAt", "required": false, "optional": false, "nullable": false,
              "schema": { "type": "well_known", "of": "date_time" },
              "description": null, "example": null },
            { "json_name": "name", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null },
            { "json_name": "targetDirection", "required": false, "optional": true, "nullable": true,
              "schema": { "type": "named", "of": "dto.TargetDirection" },
              "description": null, "example": null },
            { "json_name": "targetValue", "required": false, "optional": true, "nullable": true,
              "schema": { "type": "primitive", "of": { "prim": "float", "bits": 32 } },
              "description": null, "example": null },
            { "json_name": "workflowChainIds", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "array", "of": { "type": "well_known", "of": "uuid" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.GoalAnalyticsQuery", "name": "GoalAnalyticsQuery",
          "body": { "type": "object", "of": [
            { "json_name": "windowDays", "required": false, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.GoalResponse", "name": "GoalResponse",
          "body": { "type": "object", "of": [
            { "json_name": "metadata", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "any", "of": {} },
              "description": null, "example": null },
            { "json_name": "uuid", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "well_known", "of": "uuid" },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "dto.HttpError", "name": "HttpError",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.TargetDirection", "name": "TargetDirection",
          "body": { "type": "enum", "of": ["gte","lte"] },
          "span": { "file": "/root/c.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE GET operation whose `400` response references an error model named
    /// `{error_name}` (NOT `HttpError`) — used to prove CR-01 derives the error type from the graph
    /// rather than hard-coding `HttpError`. The error model carries `message` + `slug` (no `hints`).
    fn error_model_graph(error_name: &str) -> ApiGraph {
        let facts = format!(
            r#"{{
              "module": "github.com/acme/svc",
              "routes": [
                {{
                  "method": "GET", "path": "/list", "handler": "listGoals",
                  "operation_id": "listGoals", "params": [],
                  "request_body": null,
                  "responses": [
                    {{ "status": 200, "body": {{ "ref_id": "dto.GoalResponse" }} }},
                    {{ "status": 400, "body": {{ "ref_id": "dto.{error_name}" }} }}
                  ],
                  "span": {{ "file": "/root/http.go", "start_line": 1, "end_line": 1 }}
                }}
              ],
              "schemas": [
                {{
                  "id": "dto.GoalResponse", "name": "GoalResponse",
                  "body": {{ "type": "object", "of": [
                    {{ "json_name": "uuid", "required": true, "optional": false, "nullable": false,
                      "schema": {{ "type": "well_known", "of": "uuid" }},
                      "description": null, "example": null }}
                  ] }},
                  "span": {{ "file": "/root/g.go", "start_line": 1, "end_line": 1 }}
                }},
                {{
                  "id": "dto.{error_name}", "name": "{error_name}",
                  "body": {{ "type": "object", "of": [
                    {{ "json_name": "message", "required": true, "optional": false, "nullable": false,
                      "schema": {{ "type": "primitive", "of": {{ "prim": "string" }} }},
                      "description": null, "example": null }},
                    {{ "json_name": "slug", "required": false, "optional": true, "nullable": false,
                      "schema": {{ "type": "primitive", "of": {{ "prim": "string" }} }},
                      "description": null, "example": null }}
                  ] }},
                  "span": {{ "file": "/root/e.go", "start_line": 1, "end_line": 1 }}
                }}
              ],
              "diagnostics": []
            }}"#
        );
        let facts = serde_json::from_str(&facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE DELETE operation on a `/{uuid}` templated path with a matching
    /// `uuid` path param — used to prove WR-04 percent-escapes the interpolated path value.
    fn path_param_graph() -> ApiGraph {
        let facts = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "DELETE", "path": "/{uuid}", "handler": "deleteGoal",
              "operation_id": "deleteGoal", "params": [
                { "name": "uuid", "location": "path", "required": true,
                  "schema": { "type": "well_known", "of": "uuid" },
                  "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } }
              ],
              "request_body": null,
              "responses": [ { "status": 200, "body": null } ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [],
          "diagnostics": []
        }"#;
        let facts = serde_json::from_slice(facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph whose path templates a `{uuid}` token but declares a path param named `id`
    /// (token set != param set) — used to prove WR-03 rejects the mismatch as a typed error.
    fn mismatched_path_graph() -> ApiGraph {
        let facts = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "DELETE", "path": "/{uuid}", "handler": "deleteGoal",
              "operation_id": "deleteGoal", "params": [
                { "name": "id", "location": "path", "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } }
              ],
              "request_body": null,
              "responses": [ { "status": 200, "body": null } ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [],
          "diagnostics": []
        }"#;
        let facts = serde_json::from_slice(facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE GET operation carrying a required `integer` query param (`page`) and
    /// an optional `boolean` query param (`active`) — used to prove WR-02 converts non-string query
    /// params to strings via `strconv` (and imports it) instead of emitting `q.Set(string, int64)`.
    fn typed_query_graph() -> ApiGraph {
        let facts = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "GET", "path": "/list", "handler": "listGoals",
              "operation_id": "listGoals", "params": [
                { "name": "page", "location": "query", "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                  "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } },
                { "name": "active", "location": "query", "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "bool" } },
                  "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
              ],
              "request_body": null,
              "responses": [ { "status": 200, "body": { "ref_id": "dto.GoalResponse" } } ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.GoalResponse", "name": "GoalResponse",
              "body": { "type": "object", "of": [
                { "json_name": "uuid", "required": true, "optional": false, "nullable": false,
                  "schema": { "type": "well_known", "of": "uuid" },
                  "description": null, "example": null }
              ] },
              "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "diagnostics": []
        }"#;
        let facts = serde_json::from_slice(facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE POST operation whose only success response is a body-less `{status}`
    /// (no response body) — used to prove WR-01 accepts successful statuses without decoding a body.
    fn body_less_success_graph(status: u16) -> ApiGraph {
        let facts = format!(
            r#"{{
              "module": "github.com/acme/svc",
              "routes": [
                {{
                  "method": "POST", "path": "/", "handler": "createGoal",
                  "operation_id": "createGoal", "params": [],
                  "request_body": null,
                  "responses": [
                    {{ "status": {status}, "body": null }}
                  ],
                  "span": {{ "file": "/root/http.go", "start_line": 1, "end_line": 1 }}
                }}
              ],
              "schemas": [],
              "diagnostics": []
            }}"#
        );
        let facts = serde_json::from_str(&facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE PATCH operation whose JSON body is optional.
    fn optional_body_graph() -> ApiGraph {
        let facts = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "PATCH", "path": "/read", "handler": "markRead",
              "operation_id": "markRead", "params": [],
              "request_body": { "ref_id": "dto.MarkReadRequest" },
              "request_body_required": false,
              "responses": [ { "status": 204, "body": null } ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.MarkReadRequest", "name": "MarkReadRequest",
              "body": { "type": "object", "of": [
                { "json_name": "lastId", "required": true, "optional": false, "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null, "example": null }
              ] },
              "span": { "file": "/root/m.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "diagnostics": []
        }"#;
        let facts = serde_json::from_slice(facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// A minimal graph with ONE GET operation that declares ONLY a `200` response (no error body),
    /// so the SDK has no graph error model and must fall back to an anonymous struct (CR-01).
    fn no_error_response_graph() -> ApiGraph {
        let facts = br#"{
          "module": "github.com/acme/svc",
          "routes": [
            {
              "method": "GET", "path": "/list", "handler": "listGoals",
              "operation_id": "listGoals", "params": [],
              "request_body": null,
              "responses": [
                { "status": 200, "body": { "ref_id": "dto.GoalResponse" } }
              ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.GoalResponse", "name": "GoalResponse",
              "body": { "type": "object", "of": [
                { "json_name": "uuid", "required": true, "optional": false, "nullable": false,
                  "schema": { "type": "well_known", "of": "uuid" },
                  "description": null, "example": null }
              ] },
              "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "diagnostics": []
        }"#;
        let facts = serde_json::from_slice(facts).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    mod exported_names {
        use super::{exported, lower_camel};

        #[test]
        fn handles_go_initialisms_and_word_boundaries() {
            assert_eq!(exported("workflowChainIds"), "WorkflowChainIDs");
            assert_eq!(exported("uuid"), "UUID");
            assert_eq!(exported("page_size"), "PageSize");
            assert_eq!(exported("createGoal"), "CreateGoal");
            assert_eq!(exported("nextCursor"), "NextCursor");
            assert_eq!(exported("message"), "Message");
            assert_eq!(exported("openai/gpt-image-2"), "OpenaiGptImage2");
            assert_eq!(exported("3d-model"), "Value3dModel");
            assert_eq!(exported("///"), "Value");
        }

        #[test]
        fn lower_camel_lowercases_the_first_word_for_idiomatic_args() {
            // The first word is fully lower-cased so an all-caps initialism does not become `uUID`.
            assert_eq!(lower_camel("uuid"), "uuid");
            assert_eq!(lower_camel("page_size"), "pageSize");
            assert_eq!(lower_camel("goalId"), "goalID");
            assert_eq!(lower_camel("id"), "id");
            assert_eq!(lower_camel("3d-model"), "value3dModel");
        }
    }

    mod models {
        use super::super::{emit_models, go_field_emissions, GoEmitOptions};
        use super::sample_graph;
        use crate::analyze::facts::FieldMeta;
        use crate::graph::{Field, Prim, Type};

        #[test]
        fn optional_field_is_pointer_with_omitempty_required_is_plain() {
            let out = emit_models(&sample_graph(), "goalservice").unwrap();
            // Optional number → *float32 + omitempty.
            assert!(
                out.contains("TargetValue *float32 `json:\"targetValue,omitempty\"`"),
                "optional number must be *float32 omitempty:\n{out}"
            );
            // Required string → no omitempty, no pointer.
            assert!(
                out.contains("Name string `json:\"name\"`"),
                "required string must be plain:\n{out}"
            );
        }

        #[test]
        fn colliding_json_spellings_get_unique_go_field_names() {
            let fields = vec![
                Field {
                    json_name: "authorizedByWorkspaceMemberId".to_string(),
                    required: false,
                    optional: true,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
                Field {
                    json_name: "authorized_by_workspace_member_id".to_string(),
                    required: false,
                    optional: true,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
            ];

            let emitted = go_field_emissions(
                "TokenIdentifyResponse",
                &fields,
                &GoEmitOptions {
                    compat_model_helpers: true,
                    sdk: crate::sdk::go::GoSdkOptions::default(),
                },
            )
            .unwrap();

            assert_eq!(emitted[0].go_name, "AuthorizedByWorkspaceMemberId");
            assert_eq!(emitted[1].go_name, "AuthorizedByWorkspaceMemberId2");
        }

        #[test]
        fn enum_emits_newtype_and_sorted_const_block() {
            let out = emit_models(&sample_graph(), "goalservice").unwrap();
            assert!(out.contains("type TargetDirection string"), "{out}");
            assert!(
                out.contains("TargetDirectionGte TargetDirection = \"gte\""),
                "{out}"
            );
            assert!(
                out.contains("TargetDirectionLte TargetDirection = \"lte\""),
                "{out}"
            );
            // Sorted: gte before lte.
            let gte = out.find("TargetDirectionGte").unwrap();
            let lte = out.find("TargetDirectionLte").unwrap();
            assert!(gte < lte, "enum consts must be in sorted order:\n{out}");
        }

        #[test]
        fn maps_uuid_to_string_datetime_to_time_and_array_of_uuid_to_string_slice() {
            let out = emit_models(&sample_graph(), "goalservice").unwrap();
            // uuid → string.
            assert!(out.contains("UUID string `json:\"uuid\"`"), "{out}");
            // date-time → time.Time.
            assert!(
                out.contains("CreatedAt time.Time `json:\"createdAt\"`"),
                "{out}"
            );
            // []uuid → []string.
            assert!(
                out.contains("WorkflowChainIDs []string `json:\"workflowChainIds,omitempty\"`"),
                "{out}"
            );
            // free-form any → any.
            assert!(
                out.contains("Metadata any `json:\"metadata,omitempty\"`"),
                "{out}"
            );
        }

        #[test]
        fn imports_time_only_when_a_time_field_exists() {
            let out = emit_models(&sample_graph(), "goalservice").unwrap();
            // GoalResponse.createdAt is a date-time, so `time` must be imported.
            assert!(out.contains("import \"time\""), "{out}");
        }

        #[test]
        fn nested_ref_uses_referenced_model_name() {
            let out = emit_models(&sample_graph(), "goalservice").unwrap();
            // analyticsQuery (ref, required) → the referenced struct's Go name, no pointer.
            assert!(
                out.contains("AnalyticsQuery GoalAnalyticsQuery `json:\"analyticsQuery\"`"),
                "{out}"
            );
            // optional enum ref → *TargetDirection.
            assert!(
                out.contains(
                    "TargetDirection *TargetDirection `json:\"targetDirection,omitempty\"`"
                ),
                "{out}"
            );
        }
    }

    mod operations {
        use super::{emit_operations, sample_graph};

        #[test]
        fn method_signature_is_ctx_first_with_body_and_return_model() {
            let graph = sample_graph();
            let ops: Vec<&crate::graph::Operation> = graph
                .operations
                .iter()
                .filter(|o| o.handler == "createGoal")
                .collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains(
                    "func (c *Client) CreateGoal(ctx context.Context, in CreateGoalInput, opts ...RequestOption) (CommandMessage, error)"
                ),
                "ctx must be first, body typed, return the 201 model:\n{out}"
            );
        }

        #[test]
        fn query_op_emits_params_struct_with_required_value_and_optional_pointer() {
            let graph = sample_graph();
            let ops: Vec<&crate::graph::Operation> = graph
                .operations
                .iter()
                .filter(|o| o.handler == "listGoals")
                .collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(out.contains("type ListGoalsParams struct"), "{out}");
            assert!(
                out.contains("Aggregation string"),
                "required query → value:\n{out}"
            );
            assert!(
                out.contains("Cursor *string"),
                "optional query → pointer:\n{out}"
            );
            assert!(
                out.contains(
                    "func (c *Client) ListGoals(ctx context.Context, params ListGoalsParams, opts ...RequestOption) (GoalResponse, error)"
                ),
                "{out}"
            );
        }

        #[test]
        fn binary_success_reads_bytes_without_success_json_decode() {
            let mut graph = sample_graph();
            let op = graph
                .operations
                .iter_mut()
                .find(|op| op.handler == "listGoals")
                .unwrap();
            op.responses[0].body = None;
            op.responses[0].body_kind = "binary".to_string();
            op.responses[0].content_type = None;
            op.responses[0].content_types = vec!["application/pdf".to_string()];

            let ops: Vec<&crate::graph::Operation> = graph
                .operations
                .iter()
                .filter(|op| op.handler == "listGoals")
                .collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("func (c *Client) ListGoals(ctx context.Context, params ListGoalsParams, opts ...RequestOption) ([]byte, error)"),
                "binary success should return raw bytes:\n{out}"
            );
            assert!(out.contains("data, err := io.ReadAll(resp.Body)"), "{out}");
            assert!(
                !out.contains("json.NewDecoder(resp.Body).Decode(&out)"),
                "binary success must not decode JSON into out:\n{out}"
            );
        }

        #[test]
        fn ops_file_imports_the_request_plumbing_set() {
            let graph = sample_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            for imp in ["bytes", "context", "encoding/json", "net/http"] {
                assert!(
                    out.contains(&format!("\"{imp}\"")),
                    "missing import {imp}:\n{out}"
                );
            }
        }

        #[test]
        fn optional_body_uses_nil_safe_io_reader() {
            let graph = super::optional_body_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("func (c *Client) MarkRead(ctx context.Context, in *MarkReadRequest, opts ...RequestOption) (struct{}, error)"),
                "optional bodies must take a pointer input:\n{out}"
            );
            assert!(
                out.contains("\"io\""),
                "optional body request construction needs io.Reader:\n{out}"
            );
            assert!(
                out.contains("var reqBody io.Reader"),
                "omitted optional body must leave a true nil reader interface:\n{out}"
            );
            assert!(
                !out.contains("var reqBody *bytes.Reader"),
                "typed nil *bytes.Reader can panic when boxed into io.Reader:\n{out}"
            );
        }

        #[test]
        fn error_decode_uses_the_graphs_error_model_name_not_a_hardcoded_httperror() {
            // CR-01 generality: a graph whose error response model is named `ApiError` (NOT
            // `HttpError`) must decode into `ApiError`, referencing the type the graph actually
            // carries. A hard-coded `HttpError` here would be `undefined` and fail `go build`.
            let graph = super::error_model_graph("ApiError");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("var decoded ApiError"),
                "error decode must use the graph's error model name `ApiError`:\n{out}"
            );
            assert!(
                !out.contains("var decoded HttpError"),
                "error decode must NOT reference a hard-coded `HttpError`:\n{out}"
            );
            assert!(out.contains("typedBody = decoded"), "{out}");
            assert!(out.contains("Body: typedBody,"), "{out}");
        }

        #[test]
        fn error_decode_falls_back_to_parsed_json_when_no_error_response_exists() {
            // An operation with no typed non-2xx response has no graph error model; the SDK must NOT
            // fabricate a dependency on a named type. It exposes the parsed JSON body as the generic
            // Body fallback.
            let graph = super::no_error_response_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("typedBody = jsonBody"),
                "absent error model must fall back to parsed JSON:\n{out}"
            );
            assert!(
                !out.contains("var decoded HttpError"),
                "absent error model must not reference any named error type:\n{out}"
            );
            assert!(!out.contains("switch resp.StatusCode {"), "{out}");
        }

        #[test]
        fn templated_path_escapes_each_arg_and_imports_net_url() {
            // WR-04: a `{uuid}` path param must be interpolated through `url.PathEscape` so a value
            // containing `/`, `?`, `#`, or `..` cannot restructure the request URL, and the file must
            // import `net/url`. The local URL var is `reqURL` to avoid shadowing the `url` package.
            let graph = super::path_param_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains(
                    "reqURL := c.baseURL + fmt.Sprintf(\"/goal/%s\", url.PathEscape(uuid))"
                ),
                "path arg must be wrapped in url.PathEscape:\n{out}"
            );
            assert!(
                out.contains("\"net/url\""),
                "a templated path must import net/url:\n{out}"
            );
        }

        #[test]
        fn mismatched_path_token_and_param_is_a_typed_error() {
            // WR-03: a path declaring a `{uuid}` token but a path param named `id` (token set !=
            // param set) must be a typed SdkGen error, not a silent `%!s(MISSING)` at runtime.
            let graph = super::mismatched_path_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let err = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("do not match its path params"),
                "expected a path-token mismatch SdkGen error, got: {msg}"
            );
        }

        #[test]
        fn grouped_facade_name_collision_is_a_typed_error() {
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/a", "handler": "listA",
                  "operation_id": "listA", "group": "foo-bar",
                  "params": [], "request_body": null,
                  "responses": [ { "status": 204, "body": null } ],
                  "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 } },
                { "method": "GET", "path": "/b", "handler": "listB",
                  "operation_id": "listB", "group": "foo_bar",
                  "params": [], "request_body": null,
                  "responses": [ { "status": 204, "body": null } ],
                  "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 } }
              ],
              "schemas": [],
              "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let graph = crate::graph::ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let err = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap_err();
            assert!(
                err.to_string()
                    .contains("both emit Go Client facade method FooBar"),
                "{err}"
            );
        }

        #[test]
        fn grouped_facade_type_collision_with_schema_is_a_typed_error() {
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/a", "handler": "listA",
                  "operation_id": "listA", "group": "foo-bar",
                  "params": [], "request_body": null,
                  "responses": [ { "status": 204, "body": null } ],
                  "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [
                {
                  "id": "app.FooBarAPI",
                  "name": "FooBarAPI",
                  "body": { "type": "object", "of": [] },
                  "span": { "file": "/root/models.go", "start_line": 1, "end_line": 1 }
                }
              ],
              "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let graph = crate::graph::ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let err = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap_err();
            assert!(
                err.to_string().contains("because a schema uses that name"),
                "{err}"
            );
        }

        #[test]
        fn non_string_query_params_are_converted_to_string_with_strconv() {
            // WR-02: an `integer` query param (Go int64) and a `boolean` query param (Go bool) cannot
            // be passed to `q.Set` directly; they must be converted to string via strconv, and the
            // file must import `strconv`. The all-string fixture stays unaffected (no strconv import).
            let graph = super::typed_query_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("q.Set(\"page\", strconv.FormatInt(params.Page, 10))"),
                "required int64 query param must be strconv.FormatInt:\n{out}"
            );
            assert!(
                out.contains("q.Set(\"active\", strconv.FormatBool(*params.Active))"),
                "optional bool query param must be strconv.FormatBool of the deref:\n{out}"
            );
            assert!(
                out.contains("\"strconv\""),
                "the ops file must import strconv for non-string query encoding:\n{out}"
            );
        }

        #[test]
        fn string_query_params_emit_no_conversion_and_no_strconv_import() {
            // WR-02 regression guard: the all-string query path (the fixture's shape) must emit the
            // bare `q.Set(name, value)` with NO strconv conversion or import — byte-identity preserved.
            let graph = sample_graph();
            let ops: Vec<&crate::graph::Operation> = graph
                .operations
                .iter()
                .filter(|o| o.handler == "listGoals")
                .collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("q.Set(\"aggregation\", params.Aggregation)"),
                "string query param must pass through unconverted:\n{out}"
            );
            assert!(
                !out.contains("strconv"),
                "all-string query encoding must not import strconv:\n{out}"
            );
        }

        #[test]
        fn body_less_201_accepts_the_2xx_range() {
            // WR-01: body-less 2xx responses must not be rejected just because they are not 200.
            let graph = super::body_less_success_graph(201);
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("if resp.StatusCode < 200 || resp.StatusCode >= 300 {"),
                "body-less success must reject only non-2xx responses:\n{out}"
            );
            assert!(
                !out.contains("resp.StatusCode !="),
                "body-less success must not compare one exact status:\n{out}"
            );
            assert!(
                out.contains("(struct{}, error)"),
                "a body-less success returns an empty struct:\n{out}"
            );
        }

        #[test]
        fn body_less_204_accepts_the_2xx_range() {
            let graph = super::body_less_success_graph(204);
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("if resp.StatusCode < 200 || resp.StatusCode >= 300 {"),
                "body-less 204 must reject only non-2xx responses:\n{out}"
            );
        }

        #[test]
        fn typed_success_with_bodyless_alternate_decodes_only_body_status() {
            let mut graph = super::no_error_response_graph();
            graph.operations[0].responses.push(crate::graph::Response {
                status: 204,
                body: None,
                body_kind: "empty".to_string(),
                content_type: None,
                content_types: Vec::new(),
            });
            graph.operations[0]
                .responses
                .sort_by_key(|response| response.status);
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("if resp.StatusCode == 200 {"),
                "only the body-bearing success status should decode:\n{out}"
            );
        }

        #[test]
        fn error_decode_reads_standard_fields_from_generic_json_body() {
            // Standard message/slug/hints fields are read from the parsed JSON object, not from the
            // declared model type, so a narrower error model still compiles.
            let graph = super::error_model_graph("ProblemDetails");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops).unwrap();
            assert!(
                out.contains("Message: apiErrorStringField(jsonBody, \"message\"),"),
                "{out}"
            );
            assert!(
                out.contains("Slug: apiErrorStringField(jsonBody, \"slug\"),"),
                "{out}"
            );
            assert!(
                out.contains("Hints: apiErrorStringSliceField(jsonBody, \"hints\"),"),
                "{out}"
            );
            assert!(
                !out.contains("apiErr.Hints"),
                "must not read a `Hints` field the error model does not declare:\n{out}"
            );
        }
    }

    mod client_and_errors {
        use super::{emit_client, emit_errors};

        #[test]
        fn client_emits_functional_options_constructor() {
            let out = emit_client(
                "goalservice",
                false,
                false,
                false,
                &crate::graph::RuntimePolicy::default(),
            );
            assert!(
                out.contains("func NewClient(baseURL string, opts ...Option) *Client"),
                "{out}"
            );
            assert!(
                out.contains("func WithHTTPClient(hc *http.Client) Option"),
                "{out}"
            );
            assert!(!out.contains("func WithAPIKey(key string) Option"), "{out}");
            let secured = emit_client(
                "goalservice",
                true,
                true,
                true,
                &crate::graph::RuntimePolicy::default(),
            );
            assert!(
                secured.contains("func WithAPIKey(key string) Option"),
                "{secured}"
            );
            assert!(
                secured.contains("func WithBearerToken(token string) Option"),
                "{secured}"
            );
            assert!(
                secured.contains("func WithBasicAuth(username, password string) Option"),
                "{secured}"
            );
            // computed imports.
            assert!(out.contains("\"net/http\""), "{out}");
            assert!(out.contains("\"time\""), "{out}");
        }

        #[test]
        fn errors_emit_apierror_with_error_method() {
            let out = emit_errors("goalservice");
            assert!(out.contains("type APIError struct"), "{out}");
            assert!(out.contains("StatusCode int"), "{out}");
            assert!(out.contains("Headers http.Header"), "{out}");
            assert!(out.contains("RawBody []byte"), "{out}");
            assert!(out.contains("JSONBody any"), "{out}");
            assert!(out.contains("Body any"), "{out}");
            assert!(out.contains("func (e *APIError) Error() string"), "{out}");
            assert!(out.contains("\"fmt\""), "{out}");
            assert!(out.contains("\"net/http\""), "{out}");
        }
    }

    mod type_mapping {
        use super::{go_type, join_path, sample_graph};
        use crate::graph::{Prim, Type, WellKnown};

        #[test]
        fn openapi_number_preserves_float64_precision() {
            let graph = sample_graph();
            let number = Type::Primitive(Prim::Float { bits: 64 });
            assert_eq!(go_type(&number, false, &graph).unwrap(), "float64");
        }

        #[test]
        fn nullable_string_uses_pointer_representation() {
            let graph = sample_graph();
            let string = Type::Primitive(Prim::String);
            assert_eq!(go_type(&string, true, &graph).unwrap(), "*string");
        }

        #[test]
        fn value_types_get_a_pointer_only_when_nullable() {
            // Pointer-wrapping reads the NULLABLE axis (RESEARCH Pitfall 4): a nullable value type is
            // `*T`, a non-nullable value type is `T`.
            let graph = sample_graph();
            let number = Type::Primitive(Prim::Float { bits: 32 });
            assert_eq!(go_type(&number, true, &graph).unwrap(), "*float32");
            assert_eq!(go_type(&number, false, &graph).unwrap(), "float32");

            let boolean = Type::Primitive(Prim::Bool);
            assert_eq!(go_type(&boolean, true, &graph).unwrap(), "*bool");

            let integer = Type::Primitive(Prim::Int {
                bits: 64,
                signed: true,
            });
            assert_eq!(go_type(&integer, false, &graph).unwrap(), "int64");

            // A nullable string must distinguish JSON null from the empty string.
            let string = Type::Primitive(Prim::String);
            assert_eq!(go_type(&string, true, &graph).unwrap(), "*string");

            let date_time = Type::WellKnown(WellKnown::DateTime);
            assert_eq!(go_type(&date_time, false, &graph).unwrap(), "time.Time");
            // a nullable date-time (a value type) becomes a pointer.
            assert_eq!(go_type(&date_time, true, &graph).unwrap(), "*time.Time");
        }

        #[test]
        fn union_type_is_an_explicit_target_error_not_a_catch_all() {
            // Go has no sum types: a union must be an EXPLICIT typed SdkGen error (T-03), proving the
            // arm exists rather than being swallowed by a catch-all.
            let graph = sample_graph();
            let union = Type::Union(vec![
                Type::Primitive(Prim::String),
                Type::Primitive(Prim::Bool),
            ]);
            let err = go_type(&union, false, &graph).unwrap_err();
            assert!(
                err.to_string().contains("union type is unsupported"),
                "{err}"
            );
        }

        #[test]
        fn join_path_prefixes_the_service_base() {
            assert_eq!(join_path("/goal", "/"), "/goal/");
            assert_eq!(join_path("/goal", "/list"), "/goal/list");
            assert_eq!(join_path("/goal", "/{uuid}"), "/goal/{uuid}");
            // A trailing slash on the base is collapsed, never doubled (mirrors lowering::join_base).
            assert_eq!(join_path("/goal/", "/list"), "/goal/list");
        }
    }

    /// Pointer (nullable) vs `,omitempty` (optional) are DISTINCT axes (RESEARCH Pitfall 4): the three
    /// cases prove the conflation is fixed end-to-end through `emit_models`.
    mod optional_vs_nullable {
        use super::emit_models;
        use crate::analyze::facts::FieldMeta;
        use crate::graph::{ApiGraph, Field, Prim, Type};

        /// A one-object graph with a single value field carrying the given optional/nullable axes.
        fn graph_with_field(optional: bool, nullable: bool) -> ApiGraph {
            let mut graph = ApiGraph::default();
            graph.schemas.push(crate::graph::Schema {
                id: "dto.S".to_string(),
                name: "S".to_string(),
                body: Type::Object(vec![Field {
                    json_name: "value".to_string(),
                    required: !optional,
                    optional,
                    nullable,
                    // a float is a Go value type (float32) — pointer-eligible when nullable.
                    schema: Type::Primitive(Prim::Float { bits: 32 }),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                }]),
                enum_source_order: Vec::new(),
                provenance: crate::graph::SourceSpan {
                    file: "s.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            });
            graph
        }

        #[test]
        fn optional_not_nullable_value_is_non_pointer_with_omitempty() {
            let out = emit_models(&graph_with_field(true, false), "svc").unwrap();
            assert!(
                out.contains("Value float32 `json:\"value,omitempty\"`"),
                "optional-not-nullable value must be a non-pointer T WITH omitempty:\n{out}"
            );
        }

        #[test]
        fn nullable_not_optional_value_is_pointer_without_omitempty() {
            let out = emit_models(&graph_with_field(false, true), "svc").unwrap();
            assert!(
                out.contains("Value *float32 `json:\"value\"`"),
                "nullable-not-optional value must be *T WITHOUT omitempty:\n{out}"
            );
        }

        #[test]
        fn nullable_and_optional_value_is_pointer_with_omitempty() {
            let out = emit_models(&graph_with_field(true, true), "svc").unwrap();
            assert!(
                out.contains("Value *float32 `json:\"value,omitempty\"`"),
                "nullable-and-optional value must be *T WITH omitempty:\n{out}"
            );
        }
    }
}
