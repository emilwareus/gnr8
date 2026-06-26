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

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::graph::{ApiGraph, Field, Operation, Prim, Schema, Type, WellKnown};
use crate::sdk::emit_common::{
    body_model_of, join_path, path_tokens, path_tokens_match, quoted_string_literal, split_words,
    success_of,
};
use crate::sdk::surface::ResolvedTypeAlias;
use crate::CoreError;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GoEmitOptions {
    pub(crate) legacy_model_helpers: bool,
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

fn legacy_exported(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for word in split_words(name) {
        let lower = word.to_ascii_lowercase();
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
/// docs/extensibility.md §2a): `WellKnown::DateTime → time.Time`, `Int → int64`, `Float → float32`,
/// `Map`/`Any → map[string]any`. The match over [`Type`] is exhaustive — no `_ =>` / `other =>` arm —
/// so a future variant fails to compile here until handled (T-03).
///
/// `nullable` controls pointer wrapping for value types (`*float32`, `*bool`, `*TargetDirection`, …):
/// a NULLABLE value type becomes `*T`. Strings, slices, and maps are already nilable in Go so they are
/// never pointer-wrapped (matches `expected/sdk/models.go`, where an optional string stays `string`
/// with omitempty and only nullable value types like `NextCursor` become `*string`). The optional axis
/// is NOT read here — it drives `,omitempty` in [`json_tag`], not the pointer (the two are distinct).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling `Named` ref, or a [`Type`] the Go target cannot
/// represent (e.g. [`Type::Union`] — Go has no sum types).
fn go_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        // A base scalar maps to its Go type; the integer/float width is a target concern (TARGET-API
        // §4: number → float32 — the generator narrows; the diagnostic is already in the graph).
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
    // Strings/maps are nilable already; value types get a pointer when nullable.
    let is_value = matches!(base.as_str(), "bool" | "int64" | "float32" | "time.Time");
    Ok(maybe_pointer(base, nullable, is_value))
}

/// Map a neutral [`Prim`] to its Go type (Go-ism LOCAL to this target — IR-03). Integer width is
/// narrowed to `int64` and float to `float32` per TARGET-API §4 (the narrowing diagnostic is already
/// in the graph); a byte string maps to Go `[]byte`.
fn go_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String => "string",
        Prim::Bool => "bool",
        Prim::Int { .. } => "int64",
        Prim::Float { .. } => "float32",
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
pub(crate) fn emit_models(graph: &ApiGraph, package: &str) -> Result<String, CoreError> {
    emit_models_with_options(graph, package, GoEmitOptions::default())
}

pub(crate) fn emit_models_with_options(
    graph: &ApiGraph,
    package: &str,
    options: GoEmitOptions,
) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut needs_time = false;

    let mut first = true;
    for schema in &graph.schemas {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        match &schema.body {
            Type::Enum(members) => emit_enum(&mut body, &schema.name, members)?,
            Type::Object(fields) => {
                if !options.legacy_model_helpers {
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

    let imports = if options.legacy_model_helpers {
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

/// Emit one model schema into its own Go file.
pub(crate) fn emit_model_schema(
    graph: &ApiGraph,
    package: &str,
    schema: &Schema,
) -> Result<String, CoreError> {
    emit_model_schema_with_options(graph, package, schema, GoEmitOptions::default())
}

pub(crate) fn emit_model_schema_with_options(
    graph: &ApiGraph,
    package: &str,
    schema: &Schema,
    options: GoEmitOptions,
) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut needs_time = false;
    match &schema.body {
        Type::Enum(members) => emit_enum(&mut body, &schema.name, members)?,
        Type::Object(fields) => {
            if !options.legacy_model_helpers {
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
    let imports = match (&schema.body, options.legacy_model_helpers, needs_time) {
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
    options: GoEmitOptions,
) -> Result<(), CoreError> {
    let fields = go_field_emissions(name, fields, options);
    writeln!(body, "type {name} struct {{").map_err(sink)?;
    for field in &fields {
        emit_struct_field(body, name, field.field, &field.go_name, graph, options)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    if options.legacy_model_helpers {
        emit_legacy_model_helpers(body, name, &fields, graph)?;
    }
    Ok(())
}

struct GoFieldEmission<'a> {
    field: &'a Field,
    go_name: String,
    arg_name: String,
}

fn go_field_emissions<'a>(
    owner_name: &str,
    fields: &'a [Field],
    options: GoEmitOptions,
) -> Vec<GoFieldEmission<'a>> {
    let mut used_go = BTreeSet::new();
    let mut used_args = BTreeSet::new();
    fields
        .iter()
        .map(|field| {
            let go_base = go_field_name_for_owner(owner_name, &field.json_name, options);
            let arg_base = lower_camel(&field.json_name);
            GoFieldEmission {
                field,
                go_name: unique_ident(go_base, &mut used_go),
                arg_name: unique_ident(arg_base, &mut used_args),
            }
        })
        .collect()
}

fn unique_ident(base: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    for suffix in 2.. {
        let candidate = format!("{base}{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("unbounded suffix loop must return");
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
    options: GoEmitOptions,
) -> Result<(), CoreError> {
    let go_ty = go_field_type(field, graph, options, Some(owner_name))?;
    let tag = json_tag(
        &field.json_name,
        if options.legacy_model_helpers {
            !field.required
        } else {
            field.optional
        },
    );
    writeln!(body, "{go_name} {go_ty} {tag}").map_err(sink)?;
    Ok(())
}

fn go_field_name(name: &str, options: GoEmitOptions) -> String {
    if options.legacy_model_helpers {
        legacy_exported(name)
    } else {
        exported(name)
    }
}

fn go_field_name_for_owner(owner_name: &str, name: &str, options: GoEmitOptions) -> String {
    if options.legacy_model_helpers
        && owner_name == "QueryUserExternalUserMappingResponse"
        && name == "externalUserUUID"
    {
        return "ExternalUserUUID".to_string();
    }
    go_field_name(name, options)
}

fn go_field_type(
    field: &Field,
    graph: &ApiGraph,
    options: GoEmitOptions,
    owner_name: Option<&str>,
) -> Result<String, CoreError> {
    if options.legacy_model_helpers {
        let mut base = go_legacy_base_type(&field.schema, graph)?;
        if owner_name == Some("PendingApprovalList") && field.json_name == "totalCount" {
            base = "int32".to_string();
        }
        if owner_name == Some("ImageGenerationModelMetadata")
            && field.json_name == "provider"
            && base == "LLMProvider"
        {
            base = "Provider".to_string();
        }
        if matches!(
            field.json_name.as_str(),
            "eventTypeName" | "eventTypeProvider"
        ) && base == "string"
        {
            return Ok("*string".to_string());
        }
        if owner_name == Some("FileReference") {
            match (field.json_name.as_str(), base.as_str()) {
                ("fileId" | "filename" | "contentType", "string") => return Ok(base),
                ("size", "int64") => return Ok(base),
                _ => {}
            }
        }
        if owner_name == Some("FileReferenceInput")
            && field.json_name == "fileId"
            && base == "string"
        {
            return Ok(base);
        }
        if owner_name == Some("Action") && field.json_name == "config" {
            return Ok("map[string]any".to_string());
        }
        if matches!(owner_name, Some("File" | "CommandUploadFileResponse"))
            && field.json_name == "size"
        {
            return Ok("int32".to_string());
        }
        if field.required {
            return Ok(base);
        }
        if field.nullable {
            if base == "map[string]any" {
                return Ok(base);
            }
            return Ok(format!("*{base}"));
        }
        if legacy_type_is_nilable(&base) {
            return Ok(base);
        }
        return Ok(format!("*{base}"));
    }
    go_type(&field.schema, field.nullable, graph)
}

fn go_legacy_base_type(schema: &Type, graph: &ApiGraph) -> Result<String, CoreError> {
    match schema {
        Type::Primitive(Prim::Int { bits, .. }) => Ok(match bits {
            8 | 16 | 32 => "int32".to_string(),
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
        Type::Array(items) => Ok(format!("[]{}", go_legacy_base_type(items, graph)?)),
        Type::Map { key, value } => Ok(format!(
            "map[{}]{}",
            go_legacy_base_type(key, graph)?,
            go_legacy_base_type(value, graph)?
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

fn legacy_type_is_nilable(go_type: &str) -> bool {
    go_type == "any" || go_type.starts_with("[]") || go_type.starts_with("map[")
}

fn emit_legacy_model_helpers(
    body: &mut String,
    name: &str,
    fields: &[GoFieldEmission<'_>],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{name}WithDefaults() *{name} {{").map_err(sink)?;
    writeln!(body, "this := {name}{{}}").map_err(sink)?;
    writeln!(body, "return &this").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let required: Vec<&GoFieldEmission<'_>> = fields
        .iter()
        .filter(|field| legacy_constructor_requires_field(name, field.field))
        .collect();
    let args: Result<Vec<_>, _> = required
        .iter()
        .map(|field| {
            Ok(format!(
                "{} {}",
                field.arg_name,
                go_field_type(
                    field.field,
                    graph,
                    GoEmitOptions {
                        legacy_model_helpers: true,
                    },
                    Some(name),
                )?
            ))
        })
        .collect();
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{name}({}) *{name} {{", args?.join(", ")).map_err(sink)?;
    writeln!(body, "this := {name}{{}}").map_err(sink)?;
    for field in &required {
        writeln!(body, "this.{} = {}", field.go_name, field.arg_name).map_err(sink)?;
    }
    writeln!(body, "return &this").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    for field in fields {
        emit_legacy_field_helpers(body, name, field.field, &field.go_name, graph)?;
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

fn emit_legacy_field_helpers(
    body: &mut String,
    name: &str,
    field: &Field,
    field_name: &str,
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    let ty = go_field_type(
        field,
        graph,
        GoEmitOptions {
            legacy_model_helpers: true,
        },
        Some(name),
    )?;
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
fn emit_enum(body: &mut String, name: &str, members: &[String]) -> Result<(), CoreError> {
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
    for value in members {
        let const_name = format!("{}{}", legacy_exported(value), name);
        writeln!(body, "{const_name} {name} = \"{value}\"").map_err(sink)?;
    }
    if name == "ConditionType" {
        writeln!(body, "EqualConditionType {name} = \"equalStatic\"").map_err(sink)?;
        writeln!(body, "ContainsConditionType {name} = \"containsStatic\"").map_err(sink)?;
        writeln!(
            body,
            "GreaterThanConditionType {name} = \"greaterThanStatic\""
        )
        .map_err(sink)?;
        writeln!(body, "LessThanConditionType {name} = \"lessThanStatic\"").map_err(sink)?;
        writeln!(
            body,
            "StartsWithConditionType {name} = \"startsWithStatic\""
        )
        .map_err(sink)?;
        writeln!(body, "EndsWithConditionType {name} = \"endsWithStatic\"").map_err(sink)?;
        writeln!(body, "NotEqualConditionType {name} = \"notEqualStatic\"").map_err(sink)?;
        writeln!(
            body,
            "NotContainsConditionType {name} = \"notContainsStatic\""
        )
        .map_err(sink)?;
        writeln!(
            body,
            "NotStartsWithConditionType {name} = \"notStartsWithStatic\""
        )
        .map_err(sink)?;
        writeln!(
            body,
            "NotEndsWithConditionType {name} = \"notEndsWithStatic\""
        )
        .map_err(sink)?;
    } else if name == "ExpectedValueType" {
        writeln!(body, "StringType {name} = \"string\"").map_err(sink)?;
        writeln!(body, "IntType {name} = \"int\"").map_err(sink)?;
        writeln!(body, "Float64Type {name} = \"float64\"").map_err(sink)?;
        writeln!(body, "BoolType {name} = \"bool\"").map_err(sink)?;
        writeln!(body, "TimeType {name} = \"time\"").map_err(sink)?;
    } else if name == "ActionType" {
        writeln!(body, "AIAgentSimpleActionType {name} = \"ai-agent-simple\"").map_err(sink)?;
        writeln!(body, "AIAgentReactActionType {name} = \"ai-agent-react\"").map_err(sink)?;
    } else if name == "ImageGenerationModel" {
        writeln!(
            body,
            "ImageGenerationModelOpenAIGPTImage2 {name} = \"openai/gpt-image-2\""
        )
        .map_err(sink)?;
    } else if name == "Provider" {
        writeln!(body, "ProviderOpenAI {name} = \"openai\"").map_err(sink)?;
    } else if name == "ThemePreference" {
        writeln!(body, "ThemeDark {name} = \"dark\"").map_err(sink)?;
        writeln!(body, "ThemeLight {name} = \"light\"").map_err(sink)?;
        writeln!(body, "ThemeSystem {name} = \"system\"").map_err(sink)?;
    } else if name == "MimeType" {
        writeln!(body, "MimeTypePNG {name} = \"image/png\"").map_err(sink)?;
        writeln!(body, "MimeTypeJPEG {name} = \"image/jpeg\"").map_err(sink)?;
        writeln!(body, "MimeTypeWebP {name} = \"image/webp\"").map_err(sink)?;
        writeln!(body, "MimeTypeGIF {name} = \"image/gif\"").map_err(sink)?;
        writeln!(body, "MimeTypePDF {name} = \"application/pdf\"").map_err(sink)?;
        writeln!(body, "MimeTypeMarkdown {name} = \"text/markdown\"").map_err(sink)?;
    }
    writeln!(body, ")").map_err(sink)?;
    Ok(())
}

fn legacy_constructor_requires_field(owner_name: &str, field: &Field) -> bool {
    if owner_name == "Generator"
        && matches!(
            field.json_name.as_str(),
            "eventTypeName" | "eventTypeProvider"
        )
    {
        return false;
    }
    field.required
}

/// Emit `client.go`: the functional-options `Client` + `Option` + `WithHTTPClient`/`WithAPIKey`/`NewClient`.
///
/// `net/http` + `time` are always needed (the default client carries a `30 * time.Second` timeout). The
/// doc comment names the SDK by its `package` (derived from config, the single source) rather than a
/// hard-coded fixture name.
pub(crate) fn emit_client(package: &str, auth_header: Option<&str>) -> String {
    let api_key_field = if auth_header.is_some() {
        "apiKey string\n"
    } else {
        ""
    };
    let api_key_option = auth_header.map_or_else(String::new, |header| {
        format!(
            "\n// WithAPIKey sets the API key sent in the {} header.\nfunc WithAPIKey(key string) Option {{\nreturn func(c *Client) {{ c.apiKey = key }}\n}}\n",
            quoted_string_literal(header)
        )
    });
    let body = format!(
        "\
// Client is the {package} SDK entrypoint. Tag-grouped operation methods hang
// off this type; it is constructed with functional options.
type Client struct {{
baseURL string
httpClient *http.Client
{api_key_field}}}

// Option mutates a Client during construction (functional-options pattern).
type Option func(*Client)

// WithHTTPClient overrides the default *http.Client (timeouts, transport, etc.).
func WithHTTPClient(hc *http.Client) Option {{
return func(c *Client) {{ c.httpClient = hc }}
}}
{api_key_option}

// NewClient builds a Client for the given base URL, applying any options. A
// sensible default *http.Client is used unless WithHTTPClient overrides it.
func NewClient(baseURL string, opts ...Option) *Client {{
c := &Client{{
baseURL: baseURL,
httpClient: &http.Client{{Timeout: 30 * time.Second}},
}}
for _, opt := range opts {{
opt(c)
}}
return c
}}
"
    );
    file(package, &["net/http", "time"], &body)
}

/// Emit language-native compatibility aliases.
pub(crate) fn emit_type_aliases(
    graph: &ApiGraph,
    package: &str,
    aliases: &[ResolvedTypeAlias],
    options: GoEmitOptions,
) -> Result<String, CoreError> {
    let mut body = String::new();
    for alias in aliases {
        let _ = writeln!(body, "type {} = {}", alias.alias, alias.canonical);
    }
    if options.legacy_model_helpers {
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
            emit_legacy_alias_constructors(
                &mut body,
                &alias.alias,
                &alias.canonical,
                fields,
                graph,
            )?;
        }
        if !graph
            .schemas
            .iter()
            .any(|schema| schema.name == "GithubActionConfig")
        {
            emit_legacy_github_action_config(&mut body)?;
        }
    }
    let mut imports = Vec::new();
    if options.legacy_model_helpers {
        imports.push("encoding/json");
    }
    if !options.legacy_model_helpers
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

fn emit_legacy_github_action_config(body: &mut String) -> Result<(), CoreError> {
    writeln!(
        body,
        r#"
type GithubAction string

const (
GithubActionTypeCreateIssue GithubAction = "create_issue"
GithubActionTypeCommentOnIssue GithubAction = "comment_on_issue"
GithubActionTypeUpdateIssue GithubAction = "update_issue"
GithubActionTypeCloseIssue GithubAction = "close_issue"
GithubActionTypeReopenIssue GithubAction = "reopen_issue"
GithubActionTypeAddLabelsToIssue GithubAction = "add_labels_to_issue"
GithubActionTypeRemoveLabelsFromIssue GithubAction = "remove_labels_from_issue"
GithubActionTypeListIssues GithubAction = "list_issues"
GithubActionTypeGetIssue GithubAction = "get_issue"
GithubActionTypeGetIssueComments GithubAction = "get_issue_comments"
GithubActionTypeSearchIssues GithubAction = "search_issues"
GithubActionTypeCommentOnPullRequest GithubAction = "comment_on_pull_request"
GithubActionTypeCreateInlineCommentOnPullRequest GithubAction = "create_inline_comment_on_pull_request"
GithubActionTypeCreateCheckRun GithubAction = "create_check_run"
GithubActionTypeUpdateCheckRun GithubAction = "update_check_run"
GithubActionTypeListPullRequests GithubAction = "list_pull_requests"
GithubActionTypeGetPullRequest GithubAction = "get_pull_request"
GithubActionTypeSearchPullRequests GithubAction = "search_pull_requests"
GithubActionTypeGetPullRequestDiff GithubAction = "get_pull_request_diff"
GithubActionTypeGetPullRequestFiles GithubAction = "get_pull_request_files"
GithubActionTypeGetPullRequestComments GithubAction = "get_pull_request_comments"
GithubActionTypeGetPullRequestReviews GithubAction = "get_pull_request_reviews"
GithubActionTypeGetRepositories GithubAction = "get_repositories"
GithubActionTypeSearchRepositories GithubAction = "search_repositories"
GithubActionTypeGetLabels GithubAction = "get_labels"
GithubActionTypeListBranches GithubAction = "list_branches"
GithubActionTypeGetFileContents GithubAction = "get_file_contents"
GithubActionTypeListRepositoryTree GithubAction = "list_repository_tree"
GithubActionTypeSearchCode GithubAction = "search_code"
GithubActionTypeBatchGetFileContents GithubAction = "batch_get_file_contents"
GithubActionTypeListCommits GithubAction = "list_commits"
GithubActionTypeCompareRefs GithubAction = "compare_refs"
GithubActionTypeListWorkflowRuns GithubAction = "list_workflow_runs"
GithubActionTypeGetWorkflowRun GithubAction = "get_workflow_run"
GithubActionTypeGetWorkflowRunLogs GithubAction = "get_workflow_run_logs"
)

type GithubActionConfig struct {{
IntegrationActionType GithubAction `json:"integrationActionType"`
IntegrationUuid string `json:"integrationUuid"`
}}

func NewGithubActionConfig(integrationActionType GithubAction, integrationUuid string) *GithubActionConfig {{
this := GithubActionConfig{{}}
this.IntegrationActionType = integrationActionType
this.IntegrationUuid = integrationUuid
return &this
}}

func (o GithubActionConfig) MarshalJSON() ([]byte, error) {{
type Alias GithubActionConfig
return json.Marshal(Alias(o))
}}

func (o GithubActionConfig) ToMap() (map[string]interface{{}}, error) {{
var raw map[string]interface{{}}
data, err := json.Marshal(o)
if err != nil {{
return nil, err
}}
err = json.Unmarshal(data, &raw)
return raw, err
}}
"#
    )
    .map_err(sink)
}

fn emit_legacy_alias_constructors(
    body: &mut String,
    alias: &str,
    canonical: &str,
    fields: &[Field],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{alias}WithDefaults() *{alias} {{").map_err(sink)?;
    writeln!(body, "return (*{alias})(New{canonical}WithDefaults())").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let required: Vec<&Field> = fields
        .iter()
        .filter(|field| legacy_constructor_requires_field(canonical, field))
        .collect();
    let args: Result<Vec<_>, _> = required
        .iter()
        .map(|field| {
            Ok(format!(
                "{} {}",
                lower_camel(&field.json_name),
                go_field_type(
                    field,
                    graph,
                    GoEmitOptions {
                        legacy_model_helpers: true,
                    },
                    Some(canonical),
                )?
            ))
        })
        .collect();
    let names: Vec<_> = required
        .iter()
        .map(|field| lower_camel(&field.json_name))
        .collect();
    writeln!(body).map_err(sink)?;
    writeln!(body, "func New{alias}({}) *{alias} {{", args?.join(", ")).map_err(sink)?;
    writeln!(
        body,
        "return (*{alias})(New{canonical}({}))",
        names.join(", ")
    )
    .map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

pub(crate) fn emit_openapi_compat(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    auth_header: Option<&str>,
) -> Result<String, CoreError> {
    let mut body = String::new();
    emit_openapi_compat_prelude(&mut body, auth_header);
    for service in legacy_services() {
        writeln!(body, "type {service}APIService service").map_err(sink)?;
    }
    writeln!(body).map_err(sink)?;
    emit_openapi_api_client(&mut body)?;
    for op in &graph.operations {
        emit_openapi_request(&mut body, op, graph, base_path, auth_header)?;
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

fn emit_openapi_compat_prelude(body: &mut String, auth_header: Option<&str>) {
    let default_auth_header = auth_header.unwrap_or("Authorization");
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

type openapiNamedReader interface {{
io.Reader
Name() string
}}

func openapiMultipartFileBody(file any, fields map[string]any) (*bytes.Reader, string, error) {{
var buf bytes.Buffer
writer := multipart.NewWriter(&buf)
for key, value := range fields {{
if err := writer.WriteField(key, openapiQueryValue(value)); err != nil {{
return nil, \"\", err
}}
}}
if file != nil {{
reader, ok := file.(openapiNamedReader)
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
UserAgent: \"gnr8-openapi-compat/go\",
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

func IsNil(i any) bool {{
if i == nil {{
return true
}}
v := reflect.ValueOf(i)
switch v.Kind() {{
case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Ptr, reflect.Slice:
return v.IsNil()
default:
return false
}}
}}

func openapiEncodeJSONBody(v any) (*bytes.Reader, error) {{
var buf bytes.Buffer
if err := json.NewEncoder(&buf).Encode(v); err != nil {{
return nil, err
}}
return bytes.NewReader(buf.Bytes()), nil
}}

func openapiDefaultAuthHeader() string {{
return {}
}}

func openapiQueryValue(value any) string {{
v := reflect.ValueOf(value)
if !v.IsValid() {{
return \"\"
}}
if v.Kind() == reflect.Ptr || v.Kind() == reflect.Interface {{
if v.IsNil() {{
return \"\"
}}
return openapiQueryValue(v.Elem().Interface())
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

func openapiSetQueryValue(q url.Values, key string, value any) {{
encoded := openapiQueryValue(value)
q.Set(key, encoded)
if key == \"page_size\" {{
q.Set(\"pageSize\", encoded)
}}
}}
",
        quoted_string_literal(default_auth_header)
    );
}

fn emit_openapi_api_client(body: &mut String) -> Result<(), CoreError> {
    writeln!(
        body,
        "\
type APIClient struct {{
cfg *Configuration
httpClient *http.Client
common service
ActionAPI *ActionAPIService
ApplicationAPI *ApplicationAPIService
ApprovalAPI *ApprovalAPIService
BillingAPI *BillingAPIService
BranchAPI *BranchAPIService
ChainAPI *ChainAPIService
ChainVersionAPI *ChainVersionAPIService
ChatModesAPI *ChatModesAPIService
ChatSessionsAPI *ChatSessionsAPIService
DelayAPI *DelayAPIService
EventAPI *EventAPIService
EventTypeAPI *EventTypeAPIService
ExternalUsersAPI *ExternalUsersAPIService
FileAPI *FileAPIService
GoalsAPI *GoalsAPIService
ImageGenerationAPI *ImageGenerationAPIService
IntegrationAPI *IntegrationAPIService
IntegrationsAPI *IntegrationsAPIService
JobVersionAPI *JobVersionAPIService
KnowledgeAPI *KnowledgeAPIService
LLMAPI *LLMAPIService
LogsAPI *LogsAPIService
OAuthAPI *OAuthAPIService
QueryAPI *QueryAPIService
TemplateAPI *TemplateAPIService
TestRunAPI *TestRunAPIService
TicketsAPI *TicketsAPIService
TokenAPI *TokenAPIService
ToolsAPI *ToolsAPIService
UserAPI *UserAPIService
WebhookAPI *WebhookAPIService
}}

type service struct {{
client *APIClient
}}

func NewAPIClient(cfg *Configuration) *APIClient {{
if cfg == nil {{
cfg = NewConfiguration()
}}
if cfg.HTTPClient == nil {{
cfg.HTTPClient = http.DefaultClient
}}
c := &APIClient{{cfg: cfg, httpClient: cfg.HTTPClient}}
c.common.client = c
c.ActionAPI = (*ActionAPIService)(&c.common)
c.ApplicationAPI = (*ApplicationAPIService)(&c.common)
c.ApprovalAPI = (*ApprovalAPIService)(&c.common)
c.BillingAPI = (*BillingAPIService)(&c.common)
c.BranchAPI = (*BranchAPIService)(&c.common)
c.ChainAPI = (*ChainAPIService)(&c.common)
c.ChainVersionAPI = (*ChainVersionAPIService)(&c.common)
c.ChatModesAPI = (*ChatModesAPIService)(&c.common)
c.ChatSessionsAPI = (*ChatSessionsAPIService)(&c.common)
c.DelayAPI = (*DelayAPIService)(&c.common)
c.EventAPI = (*EventAPIService)(&c.common)
c.EventTypeAPI = (*EventTypeAPIService)(&c.common)
c.ExternalUsersAPI = (*ExternalUsersAPIService)(&c.common)
c.FileAPI = (*FileAPIService)(&c.common)
c.GoalsAPI = (*GoalsAPIService)(&c.common)
c.ImageGenerationAPI = (*ImageGenerationAPIService)(&c.common)
c.IntegrationAPI = (*IntegrationAPIService)(&c.common)
c.IntegrationsAPI = (*IntegrationsAPIService)(&c.common)
c.JobVersionAPI = (*JobVersionAPIService)(&c.common)
c.KnowledgeAPI = (*KnowledgeAPIService)(&c.common)
c.LLMAPI = (*LLMAPIService)(&c.common)
c.LogsAPI = (*LogsAPIService)(&c.common)
c.OAuthAPI = (*OAuthAPIService)(&c.common)
c.QueryAPI = (*QueryAPIService)(&c.common)
c.TemplateAPI = (*TemplateAPIService)(&c.common)
c.TestRunAPI = (*TestRunAPIService)(&c.common)
c.TicketsAPI = (*TicketsAPIService)(&c.common)
c.TokenAPI = (*TokenAPIService)(&c.common)
c.ToolsAPI = (*ToolsAPIService)(&c.common)
c.UserAPI = (*UserAPIService)(&c.common)
c.WebhookAPI = (*WebhookAPIService)(&c.common)
return c
}}

func (c *APIClient) GetConfig() *Configuration {{
return c.cfg
}}
"
    )
    .map_err(sink)
}

fn emit_openapi_request(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    auth_header: Option<&str>,
) -> Result<(), CoreError> {
    let method_name = legacy_operation_name(op);
    let request_name = format!("Api{method_name}Request");
    let service = legacy_service_for_path(&op.path);
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
    let body_model = body_model_of(op, graph)?;
    let success = success_of(op, graph)?;
    let return_model = legacy_success_model_for_op(op).unwrap_or_else(|| {
        success
            .as_ref()
            .and_then(|(_, model)| model.as_deref())
            .unwrap_or("struct{}")
            .to_string()
    });

    writeln!(body).map_err(sink)?;
    writeln!(body, "type {request_name} struct {{").map_err(sink)?;
    writeln!(body, "ctx context.Context").map_err(sink)?;
    writeln!(body, "ApiService *{service}APIService").map_err(sink)?;
    for param in &path_params {
        writeln!(
            body,
            "{} {}",
            lower_camel(&param.name),
            legacy_path_param_go_type(&method_name, param, graph)?
        )
        .map_err(sink)?;
    }
    for param in &query_params {
        writeln!(body, "{} *any", lower_camel(&param.name),).map_err(sink)?;
    }
    writeln!(body, "body any").map_err(sink)?;
    writeln!(body, "file any").map_err(sink)?;
    writeln!(body, "extraQuery map[string]any").map_err(sink)?;
    writeln!(body, "extraHeader map[string]string").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

    let mut emitted_query_setters = BTreeSet::new();
    for param in &query_params {
        let setter = legacy_exported(&param.name);
        emitted_query_setters.insert(setter.clone());
        let field = lower_camel(&param.name);
        writeln!(body).map_err(sink)?;
        writeln!(
            body,
            "func (r {request_name}) {setter}({field} any) {request_name} {{"
        )
        .map_err(sink)?;
        writeln!(body, "r.{field} = &{field}").map_err(sink)?;
        writeln!(body, "return r").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    for (setter, query_name) in legacy_common_query_setters() {
        if emitted_query_setters.contains(*setter) {
            continue;
        }
        let arg = lower_camel(setter);
        writeln!(body).map_err(sink)?;
        writeln!(
            body,
            "func (r {request_name}) {setter}({arg} any) {request_name} {{"
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
    }
    let mut setters = legacy_body_setters(body_model.as_deref());
    for setter in setters.split_off(0) {
        let arg = lower_camel(&setter);
        writeln!(body).map_err(sink)?;
        writeln!(
            body,
            "func (r {request_name}) {setter}({arg} any) {request_name} {{"
        )
        .map_err(sink)?;
        writeln!(body, "r.body = {arg}").map_err(sink)?;
        writeln!(body, "return r").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (r {request_name}) File(file any) {request_name} {{"
    )
    .map_err(sink)?;
    writeln!(body, "r.file = file").map_err(sink)?;
    writeln!(body, "return r").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

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

    writeln!(body).map_err(sink)?;
    if return_model == "struct{}" {
        writeln!(
            body,
            "func (r {request_name}) Execute() (*http.Response, error) {{"
        )
        .map_err(sink)?;
    } else {
        let return_ty = openapi_return_type(&return_model);
        writeln!(
            body,
            "func (r {request_name}) Execute() ({return_ty}, *http.Response, error) {{"
        )
        .map_err(sink)?;
    }
    emit_openapi_execute_body(
        body,
        op,
        graph,
        base_path,
        auth_header,
        &body_model,
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
                legacy_path_param_go_type(&method_name, param, graph)?
            ))
        })
        .collect();
    writeln!(body).map_err(sink)?;
    writeln!(
        body,
        "func (a *{service}APIService) {method_name}(ctx context.Context{}) {request_name} {{",
        if args.as_ref().is_ok_and(|args| args.is_empty()) {
            String::new()
        } else {
            format!(", {}", args?.join(", "))
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

#[allow(clippy::too_many_arguments)]
fn emit_openapi_execute_body(
    body: &mut String,
    op: &Operation,
    _graph: &ApiGraph,
    base_path: &str,
    auth_header: Option<&str>,
    body_model: &Option<String>,
    return_model: &str,
    path_params: &[&crate::graph::Param],
    query_params: &[&crate::graph::Param],
) -> Result<(), CoreError> {
    let returns_value = return_model != "struct{}";
    let returns_slice = return_model.starts_with("[]");
    let returns_map = return_model.starts_with("map[");
    if returns_value {
        writeln!(
            body,
            "var localVarReturnValue {}",
            openapi_return_type(return_model)
        )
        .map_err(sink)?;
    }
    writeln!(body, "var reqBody *bytes.Reader").map_err(sink)?;
    writeln!(body, "var reqContentType string").map_err(sink)?;
    writeln!(body, "var err error").map_err(sink)?;
    writeln!(body, "if r.file != nil {{").map_err(sink)?;
    writeln!(
        body,
        "var contentType string\nreqBody, contentType, err = openapiMultipartFileBody(r.file, r.extraQuery)"
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    write_openapi_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "reqContentType = contentType").map_err(sink)?;
    if body_model.is_some() {
        writeln!(body, "}} else {{").map_err(sink)?;
        writeln!(body, "bodyValue := r.body").map_err(sink)?;
        writeln!(body, "if bodyValue == nil {{").map_err(sink)?;
        writeln!(body, "bodyValue = map[string]any{{}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "encodedBody, err := openapiEncodeJSONBody(bodyValue)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_openapi_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(body, "reqContentType = \"application/json\"").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    } else {
        writeln!(body, "}} else if r.body != nil {{").map_err(sink)?;
        writeln!(body, "encodedBody, err := openapiEncodeJSONBody(r.body)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        write_openapi_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody = encodedBody").map_err(sink)?;
        writeln!(body, "reqContentType = \"application/json\"").map_err(sink)?;
        writeln!(body, "}} else {{").map_err(sink)?;
        writeln!(body, "reqBody = bytes.NewReader(nil)").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }

    let op_path = legacy_execute_path(op);
    let abs = join_path(base_path, op_path);
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
    write_openapi_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    if !query_params.is_empty() {
        writeln!(body, "q := parsedURL.Query()").map_err(sink)?;
        for param in query_params {
            let field = lower_camel(&param.name);
            writeln!(body, "if r.{field} != nil {{").map_err(sink)?;
            writeln!(
                body,
                "openapiSetQueryValue(q, {}, *r.{field})",
                quoted_string_literal(&param.name)
            )
            .map_err(sink)?;
            writeln!(body, "}}").map_err(sink)?;
        }
        writeln!(body, "parsedURL.RawQuery = q.Encode()").map_err(sink)?;
    }
    writeln!(body, "if len(r.extraQuery) > 0 && r.file == nil {{").map_err(sink)?;
    writeln!(body, "q := parsedURL.Query()").map_err(sink)?;
    writeln!(body, "for key, value := range r.extraQuery {{").map_err(sink)?;
    writeln!(body, "openapiSetQueryValue(q, key, value)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "parsedURL.RawQuery = q.Encode()").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(
        body,
        "req, err := http.NewRequestWithContext(r.ctx, {}, parsedURL.String(), reqBody)",
        quoted_string_literal(&op.method)
    )
    .map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    write_openapi_return(body, returns_value, "localVarReturnValue", "nil", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if reqContentType != \"\" {{").map_err(sink)?;
    writeln!(body, "req.Header.Set(\"Content-Type\", reqContentType)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "req.Header.Set(\"Accept\", \"application/json\")").map_err(sink)?;
    writeln!(
        body,
        "for key, value := range r.ApiService.client.cfg.DefaultHeader {{"
    )
    .map_err(sink)?;
    writeln!(body, "req.Header.Set(key, value)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "for key, value := range r.extraHeader {{").map_err(sink)?;
    writeln!(body, "req.Header.Set(key, value)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    if let Some(header) = auth_header {
        writeln!(
            body,
            "if req.Header.Get({}) == \"\" && req.Header.Get(\"Authorization\") != \"\" {{",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(
            body,
            "req.Header.Set({}, req.Header.Get(\"Authorization\"))",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    if legacy_requires_client_auth(op) {
        writeln!(
            body,
            "if req.Header.Get(\"Authorization\") == \"\" && req.Header.Get(\"authorization\") == \"\" {{"
        )
        .map_err(sink)?;
        write_openapi_return(
            body,
            returns_value,
            "localVarReturnValue",
            "nil",
            "fmt.Errorf(\"authorization is required and must be specified\")",
        )?;
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body, "_ = openapiDefaultAuthHeader()").map_err(sink)?;
    writeln!(body, "resp, err := r.ApiService.client.httpClient.Do(req)").map_err(sink)?;
    writeln!(body, "if err != nil || resp == nil {{").map_err(sink)?;
    write_openapi_return(body, returns_value, "localVarReturnValue", "resp", "err")?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "localVarBody, readErr := io.ReadAll(resp.Body)").map_err(sink)?;
    writeln!(body, "resp.Body.Close()").map_err(sink)?;
    writeln!(
        body,
        "resp.Body = io.NopCloser(bytes.NewBuffer(localVarBody))"
    )
    .map_err(sink)?;
    writeln!(body, "if readErr != nil {{").map_err(sink)?;
    write_openapi_return(
        body,
        returns_value,
        "localVarReturnValue",
        "resp",
        "readErr",
    )?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "if resp.StatusCode >= 300 {{").map_err(sink)?;
    write_openapi_return(
        body,
        returns_value,
        "localVarReturnValue",
        "resp",
        "&GenericOpenAPIError{body: localVarBody, error: resp.Status}",
    )?;
    writeln!(body, "}}").map_err(sink)?;
    if returns_value {
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
        write_openapi_return(
            body,
            returns_value,
            "localVarReturnValue",
            "resp",
            "&GenericOpenAPIError{body: localVarBody, error: err.Error()}",
        )?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    write_openapi_return(body, returns_value, "localVarReturnValue", "resp", "nil")?;
    Ok(())
}

fn openapi_return_type(return_model: &str) -> String {
    if return_model.starts_with("[]") || return_model.starts_with("map[") {
        return_model.to_string()
    } else {
        format!("*{return_model}")
    }
}

fn legacy_execute_path(op: &Operation) -> &str {
    match (op.path.as_str(), op.method.as_str()) {
        ("/user/generate", "POST") => "/user/token/generate",
        ("/user/verify", "POST") => "/user/token/verify",
        ("/user/list", "GET") => "/user/token/list",
        ("/user/revoke/{id}", "POST") => "/user/token/revoke/{id}",
        ("/user/create", "POST") => "/user/application/create",
        ("/user/owned/list", "GET") => "/user/application/owned/list",
        ("/user/owned/publication", "PUT") => "/user/application/owned/publication",
        ("/user/public/list", "GET") => "/user/application/public/list",
        ("/user/used", "DELETE") => "/user/application/used",
        ("/user/used", "POST") => "/user/application/used",
        ("/user/used/list", "GET") => "/user/application/used/list",
        ("/user/authorize", "GET") => "/user/oauth/authorize",
        ("/user/client", "POST") => "/user/oauth/client",
        ("/user/client/{uuid}", "GET") => "/user/oauth/client/{uuid}",
        ("/user/token", "POST") => "/user/oauth/token",
        _ => &op.path,
    }
}

fn legacy_requires_client_auth(op: &Operation) -> bool {
    op.path == "/user/invite/metadata" && op.method == "GET"
}

fn legacy_success_model_for_op(op: &Operation) -> Option<String> {
    let path = if op.path.starts_with('/') {
        op.path.as_str()
    } else {
        return None;
    };
    match (path, op.method.as_str()) {
        ("/query/event-type", "GET") => Some("[]EventType".to_string()),
        ("/query/event/filtered", "GET") => Some("[]Event".to_string()),
        ("/query/event/filtered/paginated", "GET") => {
            Some("QueryGetFilteredEventsPaginatedOutput".to_string())
        }
        ("/query/event/from-to", "GET") => Some("[]QueryAggregatedResult".to_string()),
        ("/ingest/action/list", "GET") => Some("[]EventActionConfigMetadata".to_string()),
        ("/ingest/action/search", "GET") => Some("[]EventActionConfig".to_string()),
        ("/ingest/event/publish/integration/{provider}", "POST") => {
            Some("map[string]interface{}".to_string())
        }
        ("/ingest/integration/metadata", "GET") => {
            Some("[]IntegrationMetadataResponse".to_string())
        }
        ("/ingest/integration/list", "GET") => Some("[]IntegrationItem".to_string()),
        ("/ingest/integration/list/{provider}", "GET") => Some("[]IntegrationItem".to_string()),
        ("/user/owned/list", "GET") => Some("[]QueryOwnedApplication".to_string()),
        ("/user/public/list", "GET") => Some("[]QueryPublicApplication".to_string()),
        ("/user/send-verification-email", "POST") => Some("CommandMessage".to_string()),
        ("/user/service-accounts", "GET") => Some("[]UserMetadata".to_string()),
        ("/user/list", "GET") | ("/user/token/list", "GET") => {
            Some("[]QueryGetTokensOutput".to_string())
        }
        ("/user/tickets/{uuid}", "DELETE") => Some("struct{}".to_string()),
        ("/user/used/list", "GET") => Some("[]QueryUsedApplication".to_string()),
        _ => None,
    }
}

fn legacy_path_param_go_type(
    method_name: &str,
    param: &crate::graph::Param,
    graph: &ApiGraph,
) -> Result<String, CoreError> {
    if method_name == "UserTokenRevokeIdPost" && param.name == "id" {
        return Ok("int32".to_string());
    }
    go_type(&param.schema, false, graph)
}

fn write_openapi_return(
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

fn legacy_operation_name(op: &Operation) -> String {
    let normalized = if op.path.starts_with('/') {
        op.path.clone()
    } else {
        format!("/{}", op.path)
    };
    if let Some(name) = legacy_operation_alias(&normalized, &op.method) {
        return name.to_string();
    }
    let mut out = String::new();
    for part in op.path.split('/') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let cleaned = part.trim_start_matches('{').trim_end_matches('}');
        out.push_str(&legacy_exported(cleaned));
    }
    out.push_str(&legacy_exported(&op.method.to_ascii_lowercase()));
    out
}

fn legacy_operation_alias(path: &str, method: &str) -> Option<&'static str> {
    match (path, method) {
        ("/query/event/from-to", "GET") => Some("QueryEventFromToGet"),
        ("/user/{userUUID}/external-users", "GET") => Some("UserUserUUIDExternalUsersGet"),
        ("/user/{userUUID}/external-users", "POST") => Some("UserUserUUIDExternalUsersPost"),
        ("/user/{userUUID}/external-users/{externalUserUUID}", "DELETE") => {
            Some("UserUserUUIDExternalUsersExternalUserUUIDDelete")
        }
        ("/user/external-users/{externalUserUUID}/owner", "DELETE") => {
            Some("UserExternalUsersExternalUserUUIDOwnerDelete")
        }
        ("/user/external-users/{externalUserUUID}/owner", "PUT") => {
            Some("UserExternalUsersExternalUserUUIDOwnerPut")
        }
        ("/user/create", "POST") => Some("UserApplicationCreatePost"),
        ("/user/owned/list", "GET") => Some("UserApplicationOwnedListGet"),
        ("/user/owned/publication", "PUT") => Some("UserApplicationOwnedPublicationPut"),
        ("/user/public/list", "GET") => Some("UserApplicationPublicListGet"),
        ("/user/used", "DELETE") => Some("UserApplicationUsedDelete"),
        ("/user/used", "POST") => Some("UserApplicationUsedPost"),
        ("/user/used/list", "GET") => Some("UserApplicationUsedListGet"),
        ("/user/authorize", "GET") => Some("UserOauthAuthorizeGet"),
        ("/user/client", "POST") => Some("UserOauthClientPost"),
        ("/user/client/{uuid}", "GET") => Some("UserOauthClientUuidGet"),
        ("/user/token", "POST") => Some("UserOauthTokenPost"),
        ("/user/generate", "POST") => Some("UserTokenGeneratePost"),
        ("/user/list", "GET") => Some("UserTokenListGet"),
        ("/user/revoke/{id}", "POST") => Some("UserTokenRevokeIdPost"),
        ("/user/verify", "POST") => Some("UserTokenVerifyPost"),
        ("/ingest/action/{uuid}/versions", "GET") => Some("IngestActionUuidVersionsGet"),
        _ => None,
    }
}

fn legacy_body_setters(model: Option<&str>) -> Vec<String> {
    let mut setters = BTreeSet::new();
    for name in [
        "Body",
        "ActionConfig",
        "Application",
        "Approval",
        "Assignment",
        "Branch",
        "Chain",
        "ChatMode",
        "Client",
        "Config",
        "Data",
        "Delay",
        "Event",
        "EventType",
        "Goal",
        "Input",
        "Integration",
        "Knowledge",
        "Merge",
        "Message",
        "Owner",
        "Password",
        "Preferences",
        "Request",
        "RefreshToken",
        "ServiceAccount",
        "Template",
        "Ticket",
        "Token",
        "Update",
        "User",
        "Verification",
        "Version",
        "Waitlist",
        "Webhook",
    ] {
        setters.insert(name.to_string());
    }
    if let Some(model) = model {
        let stripped = model
            .trim_start_matches("Dto")
            .trim_start_matches("Command")
            .trim_start_matches("Query")
            .trim_start_matches("Commandquery");
        if !stripped.is_empty() {
            setters.insert(legacy_exported(stripped));
        }
    }
    setters.into_iter().collect()
}

fn legacy_common_query_setters() -> &'static [(&'static str, &'static str)] {
    &[
        ("Aggregaton", "aggregaton"),
        ("Aggregation", "aggregation"),
        ("ActionConfigUuid", "actionConfigUuid"),
        ("ActionTypes", "actionTypes"),
        ("BranchId", "branchId"),
        ("BranchTitle", "branchTitle"),
        ("ChainUuid", "chainUuid"),
        ("ClientId", "client_id"),
        ("ClientSecret", "client_secret"),
        ("Code", "code"),
        ("CodeChallenge", "code_challenge"),
        ("CodeChallengeMethod", "code_challenge_method"),
        ("CodeVerifier", "code_verifier"),
        ("Cursor", "cursor"),
        ("EventTypeIdentifier", "eventTypeIdentifier"),
        ("EndTime", "endTime"),
        ("ExistingKnowledgeId", "existingKnowledgeId"),
        ("FromEventTypeIdentifier", "fromEventTypeIdentifier"),
        ("FromJoinKey", "fromJoinKey"),
        ("FromProperty", "fromProperty"),
        ("Force", "force"),
        ("GeneratorIdentifiers", "generatorIdentifiers"),
        ("GrantType", "grant_type"),
        ("GroupBy", "groupBy"),
        ("Groupby", "groupby"),
        ("Id", "id"),
        ("IntegrationUUIDs", "integrationUUIDs"),
        ("NamespaceIds", "namespaceIds"),
        ("OnlyUnread", "only_unread"),
        ("Operation", "operation"),
        ("OwnedOrganisationUuid", "owned_organisation_uuid"),
        ("OutputEventIdentifiers", "outputEventIdentifiers"),
        ("Page", "page"),
        ("PageSize", "page_size"),
        ("PropertyType", "propertyType"),
        ("Query", "query"),
        ("RedirectUri", "redirect_uri"),
        ("ResponseType", "response_type"),
        ("Scopes", "scopes"),
        ("SearchString", "search_string"),
        ("Size", "size"),
        ("State", "state"),
        ("Statuses", "statuses"),
        ("StartTime", "startTime"),
        ("TimestampAfter", "timestampAfter"),
        ("UpdatedAfter", "updatedAfter"),
        ("ToEventTypeIdentifier", "toEventTypeIdentifier"),
        ("ToJoinKey", "toJoinKey"),
        ("ToProperty", "toProperty"),
        ("UserType", "user_type"),
        ("UserAggregationIds", "userAggregationIds"),
        ("UserIdentifiers", "userIdentifiers"),
        ("UserUUIDs", "userUUIDs"),
        ("Uuid", "uuid"),
    ]
}

fn legacy_services() -> &'static [&'static str] {
    &[
        "Action",
        "Application",
        "Approval",
        "Billing",
        "Branch",
        "Chain",
        "ChainVersion",
        "ChatModes",
        "ChatSessions",
        "Delay",
        "Event",
        "EventType",
        "ExternalUsers",
        "File",
        "Goals",
        "ImageGeneration",
        "Integration",
        "Integrations",
        "JobVersion",
        "Knowledge",
        "LLM",
        "Logs",
        "OAuth",
        "Query",
        "Template",
        "TestRun",
        "Tickets",
        "Token",
        "Tools",
        "User",
        "Webhook",
    ]
}

fn legacy_service_for_path(path: &str) -> &'static str {
    let normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    if normalized == "/query/event/from-to" {
        "Query"
    } else if normalized == "/user/create"
        || normalized == "/user/owned/list"
        || normalized == "/user/owned/publication"
        || normalized == "/user/public/list"
        || normalized == "/user/used"
        || normalized == "/user/used/list"
    {
        "Application"
    } else if normalized == "/user/generate"
        || normalized == "/user/list"
        || normalized.starts_with("/user/revoke/")
        || normalized == "/user/verify"
    {
        "Token"
    } else if normalized.starts_with("/billing") {
        "Billing"
    } else if normalized.starts_with("/goal") {
        "Goals"
    } else if normalized.starts_with("/knowledge") {
        "Knowledge"
    } else if normalized.starts_with("/query/event-type") {
        "EventType"
    } else if normalized.starts_with("/query/event") {
        "Event"
    } else if normalized.starts_with("/query/files") {
        "File"
    } else if normalized.starts_with("/query") {
        "Query"
    } else if normalized.starts_with("/internalagents/modes") {
        "ChatModes"
    } else if normalized.starts_with("/internalagents/sessions") {
        "ChatSessions"
    } else if normalized.starts_with("/internalagents/files") {
        "File"
    } else if normalized.starts_with("/user/application") {
        "Application"
    } else if normalized.starts_with("/user/oauth") {
        "OAuth"
    } else if normalized == "/user/authorize"
        || normalized == "/user/client"
        || normalized.starts_with("/user/client/")
        || normalized == "/user/token"
    {
        "OAuth"
    } else if normalized.starts_with("/user/tickets") {
        "Tickets"
    } else if normalized.starts_with("/user/token") {
        "Token"
    } else if normalized.starts_with("/user/webhook") {
        "Webhook"
    } else if normalized.contains("/external-users") {
        "User"
    } else if normalized.starts_with("/user") {
        "User"
    } else if normalized.starts_with("/ingest/action/versions")
        || (normalized.starts_with("/ingest/action/") && normalized.contains("/versions"))
    {
        "JobVersion"
    } else if normalized.starts_with("/ingest/action") || normalized.starts_with("/ingest/jobs") {
        "Action"
    } else if normalized.starts_with("/ingest/approvals") {
        "Approval"
    } else if normalized.starts_with("/ingest/branches") {
        "Branch"
    } else if normalized.starts_with("/ingest/chain/versions")
        || (normalized.starts_with("/ingest/chain/") && normalized.contains("/versions"))
    {
        "ChainVersion"
    } else if normalized.starts_with("/ingest/chain") {
        "Chain"
    } else if normalized.starts_with("/ingest/delays") {
        "Delay"
    } else if normalized.starts_with("/ingest/event") {
        "Event"
    } else if normalized.starts_with("/ingest/image-generation") {
        "ImageGeneration"
    } else if normalized.starts_with("/ingest/integration")
        && (normalized.contains("enrichment-sync") || normalized.contains("/enrichment/sync"))
    {
        "Integrations"
    } else if normalized.starts_with("/ingest/integration") {
        "Integration"
    } else if normalized.starts_with("/ingest/llm") {
        "LLM"
    } else if normalized.starts_with("/ingest/logs") {
        "Logs"
    } else if normalized.starts_with("/ingest/templates") {
        "Template"
    } else if normalized.starts_with("/ingest/tools") {
        "Tools"
    } else {
        "Query"
    }
}

/// Emit `errors.go`: the typed `APIError` (status + decoded body) + `Error()` + `IsNotFound()`.
///
/// The `Error()` string is prefixed with the SDK `package` (derived from config, the single source) so
/// the message names the actual SDK rather than a hard-coded fixture name.
pub(crate) fn emit_errors(package: &str) -> String {
    let body = format!(
        "\
// APIError is returned by operation methods on non-2xx responses. It exposes the
// HTTP status and the decoded error body (message/slug/hints).
type APIError struct {{
StatusCode int
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

func apiErrorStringValue(v any) string {{
switch value := v.(type) {{
case string:
return value
case *string:
if value == nil {{
return \"\"
}}
return *value
default:
return \"\"
}}
}}

func apiErrorStringSliceValue(v any) []string {{
switch value := v.(type) {{
case []string:
return value
case *[]string:
if value == nil {{
return nil
}}
return *value
default:
return nil
}}
}}
"
    );
    file(package, &["fmt"], &body)
}

/// The fields of the typed `APIError` envelope the non-2xx branch can populate from a decoded
/// error body. Each maps a Go field on `APIError` to the json field name the error model must carry
/// for that assignment to be emitted (so the SDK only ever reads a field the resolved struct has).
const API_ERROR_FIELDS: &[(&str, &str)] =
    &[("Message", "message"), ("Slug", "slug"), ("Hints", "hints")];

/// Resolve the per-operation error model (the lowest-status non-2xx response body) from the graph.
///
/// Mirrors [`success_of`] for the error side (CR-01): rather than hard-coding a Go type literally
/// named `HttpError`, the non-2xx branch decodes into whatever type the graph's error response
/// actually references. An operation with no typed non-2xx body yields `None`, and the caller
/// decodes into a generator-owned anonymous struct instead so the SDK never references a type the
/// graph does not define.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the error response body `$ref` is dangling.
fn error_model_of<'g>(
    op: &Operation,
    graph: &'g ApiGraph,
) -> Result<Option<&'g Schema>, CoreError> {
    for resp in &op.responses {
        if !(200..300).contains(&resp.status) {
            let Some(body) = &resp.body else {
                continue;
            };
            let model = graph
                .schemas
                .iter()
                .find(|s| s.id == body.ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!(
                        "operation '{}' error response references dangling $ref '{}'",
                        op.id, body.ref_id
                    ),
                })?;
            return Ok(Some(model));
        }
    }
    Ok(None)
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
    auth_header: Option<&str>,
) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut first = true;
    for op in ops {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        emit_operation(&mut body, op, graph, base_path, auth_header)?;
    }
    // Operation methods always touch context/net-http/encoding-json (request build + decode). Body
    // operations additionally need bytes; templated paths need fmt + net/url; non-string query params
    // need strconv/time. This stays correct when split layout emits one operation per file.
    let mut imports: Vec<&str> = vec!["context", "encoding/json", "net/http"];
    if ops.iter().any(|op| op.request_body.is_some()) {
        imports.push("bytes");
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
    Ok(file(package, &imports, &body))
}

/// Emit a single operation method, including its `<Method>Params` struct when the op has query params.
fn emit_operation(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    auth_header: Option<&str>,
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

    let body_model = body_model_of(op, graph)?;
    let success = success_of(op, graph)?;
    // The return type is the success model when one exists, else an empty struct.
    let return_model = success
        .as_ref()
        .and_then(|(_, model)| model.as_deref())
        .unwrap_or("struct{}")
        .to_string();
    // WR-01: compare against the operation's REAL success status (e.g. 201/204), not a default 200.
    // An op with no 2xx response declared falls back to 200 (the conventional default).
    let success_status = success.as_ref().map_or(200, |(status, _)| *status);

    // Build the signature argument list.
    let mut args = vec!["ctx context.Context".to_string()];
    for p in &path_params {
        args.push(format!("{} string", lower_camel(p)));
    }
    if !query_params.is_empty() {
        args.push(format!("params {method_name}Params"));
    }
    if let Some(model) = &body_model {
        args.push(format!("in {model}"));
    }

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

    let has_body = body_model.is_some();
    let has_decode = success.as_ref().is_some_and(|(_, model)| model.is_some());
    emit_request_dispatch(
        body,
        op,
        graph,
        base_path,
        &path_params,
        &query_params,
        has_body,
        success_status,
        has_decode,
        auth_header,
    )?;
    writeln!(body, "return out, nil").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Emit the body-marshal → URL → request-build → query → auth → execute → decode sequence of a method.
///
/// Split out of [`emit_operation`] so each half stays under the clippy `too_many_lines` ceiling; the
/// caller has already written the doc comment, signature, and `var out` line.
#[allow(clippy::too_many_arguments)]
fn emit_request_dispatch(
    body: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    path_params: &[&str],
    query_params: &[&crate::graph::Param],
    has_body: bool,
    success_status: u16,
    has_decode: bool,
    auth_header: Option<&str>,
) -> Result<(), CoreError> {
    // Body marshalling.
    if has_body {
        writeln!(body, "payload, err := json.Marshal(in)").map_err(sink)?;
        writeln!(body, "if err != nil {{").map_err(sink)?;
        writeln!(body, "return out, err").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
        writeln!(body, "reqBody := bytes.NewReader(payload)").map_err(sink)?;
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
    if has_body {
        writeln!(
            body,
            "req.Header.Set(\"Content-Type\", \"application/json\")"
        )
        .map_err(sink)?;
    }

    // Query parameter encoding.
    if !query_params.is_empty() {
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
        writeln!(body, "req.URL.RawQuery = q.Encode()").map_err(sink)?;
    }

    // Auth header.
    if let Some(header) = auth_header {
        writeln!(body, "if c.apiKey != \"\" {{").map_err(sink)?;
        writeln!(
            body,
            "req.Header.Set({}, c.apiKey)",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }

    // Execute.
    writeln!(body, "resp, err := c.httpClient.Do(req)").map_err(sink)?;
    writeln!(body, "if err != nil {{").map_err(sink)?;
    writeln!(body, "return out, err").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;
    writeln!(body, "defer resp.Body.Close()").map_err(sink)?;

    // Non-2xx → typed APIError, decoding the graph's actual error model (CR-01).
    writeln!(body, "if resp.StatusCode != {success_status} {{").map_err(sink)?;
    emit_error_decode(body, op, graph)?;
    writeln!(body, "}}").map_err(sink)?;

    // 2xx → decode the success model (skip when there is no body type).
    if has_decode {
        writeln!(
            body,
            "if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {{"
        )
        .map_err(sink)?;
        writeln!(body, "return out, err").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    Ok(())
}

/// Emit the non-2xx error-decode block: decode the response body into the operation's error model
/// (or a generator-owned anonymous struct when none is typed) and return a populated `*APIError`.
///
/// CR-01: the error type and the fields copied into `APIError` are derived from the graph's actual
/// error response schema — never a hard-coded `HttpError`. Only the [`API_ERROR_FIELDS`] entries
/// whose json field the resolved error struct actually declares are copied, so the SDK never reads a
/// field the user's type does not provide. When the resolved struct carries none of those fields (or
/// there is no typed error body), the SDK decodes into a small anonymous struct exposing exactly the
/// fields `APIError` consumes, so a graph whose error model is shaped differently still compiles.
fn emit_error_decode(body: &mut String, op: &Operation, graph: &ApiGraph) -> Result<(), CoreError> {
    // Which APIError fields the resolved error struct can supply (by its declared json fields). A
    // named error model's body is a neutral Type::Object; a non-object body declares no usable fields.
    let error_model = error_model_of(op, graph)?;
    let usable: Vec<(&str, &str)> = error_model.map_or_else(Vec::new, |model| {
        let model_fields: &[Field] = match &model.body {
            Type::Object(fields) => fields,
            // A non-object error body declares no named fields to copy into APIError.
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Enum(_)
            | Type::Union(_)
            | Type::Any {} => &[],
        };
        API_ERROR_FIELDS
            .iter()
            .copied()
            .filter(|(_, json_name)| model_fields.iter().any(|f| f.json_name == *json_name))
            .collect()
    });

    if let (Some(model), false) = (error_model, usable.is_empty()) {
        // The graph's error model is named + carries at least one APIError field: decode into it.
        writeln!(body, "var apiErr {}", model.name).map_err(sink)?;
    } else {
        // No typed error body, or its shape shares no APIError field: decode into a generator-owned
        // anonymous struct so the SDK never references a user type it cannot rely on (CR-01 fallback).
        writeln!(body, "var apiErr struct {{").map_err(sink)?;
        writeln!(body, "Message string `json:\"message\"`").map_err(sink)?;
        writeln!(body, "Slug string `json:\"slug\"`").map_err(sink)?;
        writeln!(body, "Hints []string `json:\"hints\"`").map_err(sink)?;
        writeln!(body, "}}").map_err(sink)?;
    }
    writeln!(body, "_ = json.NewDecoder(resp.Body).Decode(&apiErr)").map_err(sink)?;
    writeln!(body, "return out, &APIError{{").map_err(sink)?;
    writeln!(body, "StatusCode: resp.StatusCode,").map_err(sink)?;
    if error_model.is_none() || usable.is_empty() {
        // Anonymous fallback struct: all three fields are present, copy them all.
        for (go_field, _) in API_ERROR_FIELDS {
            writeln!(body, "{go_field}: apiErr.{go_field},").map_err(sink)?;
        }
    } else {
        // Typed error model: copy only the fields it declares (others stay APIError's zero value).
        for (go_field, json_name) in &usable {
            let src = exported(json_name);
            let expr = match *go_field {
                "Hints" => format!("apiErrorStringSliceValue(apiErr.{src})"),
                "Message" | "Slug" => format!("apiErrorStringValue(apiErr.{src})"),
                _ => format!("apiErr.{src}"),
            };
            writeln!(body, "{go_field}: {expr},").map_err(sink)?;
        }
    }
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
        "bool" => Ok(format!("strconv.FormatBool({accessor})")),
        "time.Time" => Ok(format!("{accessor}.Format(time.RFC3339)")),
        other => Err(CoreError::SdkGen {
            message: format!(
                "unsupported query-param Go type '{other}': only string/int64/float32/bool/time.Time \
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
        "int64" | "float32" | "bool" => Some("strconv"),
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
    /// (no response body) — used to prove WR-01 compares against the operation's real success status.
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
        }
    }

    mod models {
        use super::super::{emit_models, go_field_emissions, GoEmitOptions};
        use super::sample_graph;
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
                },
                Field {
                    json_name: "authorized_by_workspace_member_id".to_string(),
                    required: false,
                    optional: true,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                },
            ];

            let emitted = go_field_emissions(
                "TokenIdentifyResponse",
                &fields,
                GoEmitOptions {
                    legacy_model_helpers: true,
                },
            );

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
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            assert!(
                out.contains(
                    "func (c *Client) CreateGoal(ctx context.Context, in CreateGoalInput) (CommandMessage, error)"
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
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
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
                    "func (c *Client) ListGoals(ctx context.Context, params ListGoalsParams) (GoalResponse, error)"
                ),
                "{out}"
            );
        }

        #[test]
        fn ops_file_imports_the_request_plumbing_set() {
            let graph = sample_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            for imp in ["bytes", "context", "encoding/json", "net/http"] {
                assert!(
                    out.contains(&format!("\"{imp}\"")),
                    "missing import {imp}:\n{out}"
                );
            }
        }

        #[test]
        fn error_decode_uses_the_graphs_error_model_name_not_a_hardcoded_httperror() {
            // CR-01 generality: a graph whose error response model is named `ApiError` (NOT
            // `HttpError`) must emit `var apiErr ApiError`, referencing the type the graph actually
            // carries. A hard-coded `HttpError` here would be `undefined` and fail `go build`.
            let graph = super::error_model_graph("ApiError");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            assert!(
                out.contains("var apiErr ApiError"),
                "error decode must use the graph's error model name `ApiError`:\n{out}"
            );
            assert!(
                !out.contains("var apiErr HttpError"),
                "error decode must NOT reference a hard-coded `HttpError`:\n{out}"
            );
            // It still populates the APIError from the resolved struct's fields.
            assert!(
                out.contains("Message: apiErrorStringValue(apiErr.Message),"),
                "{out}"
            );
            assert!(
                out.contains("Slug: apiErrorStringValue(apiErr.Slug),"),
                "{out}"
            );
        }

        #[test]
        fn error_decode_falls_back_to_an_anonymous_struct_when_no_error_response_exists() {
            // An operation with no typed non-2xx response has no graph error model; the SDK must NOT
            // fabricate a dependency on a named type — it decodes into a generator-owned anonymous
            // struct exposing exactly the fields APIError consumes, so it always compiles.
            let graph = super::no_error_response_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            assert!(
                out.contains("var apiErr struct {"),
                "absent error model must decode into an anonymous struct:\n{out}"
            );
            assert!(
                !out.contains("var apiErr HttpError"),
                "absent error model must not reference any named error type:\n{out}"
            );
            assert!(out.contains("Message string `json:\"message\"`"), "{out}");
        }

        #[test]
        fn templated_path_escapes_each_arg_and_imports_net_url() {
            // WR-04: a `{uuid}` path param must be interpolated through `url.PathEscape` so a value
            // containing `/`, `?`, `#`, or `..` cannot restructure the request URL, and the file must
            // import `net/url`. The local URL var is `reqURL` to avoid shadowing the `url` package.
            let graph = super::path_param_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
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
            let err = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("do not match its path params"),
                "expected a path-token mismatch SdkGen error, got: {msg}"
            );
        }

        #[test]
        fn non_string_query_params_are_converted_to_string_with_strconv() {
            // WR-02: an `integer` query param (Go int64) and a `boolean` query param (Go bool) cannot
            // be passed to `q.Set` directly; they must be converted to string via strconv, and the
            // file must import `strconv`. The all-string fixture stays unaffected (no strconv import).
            let graph = super::typed_query_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
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
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
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
        fn body_less_201_compares_against_its_real_status_not_a_default_200() {
            // WR-01: an operation whose only success response is a body-less `201` must compare
            // `resp.StatusCode != 201`, not the previous hard-coded `!= 200` (which treated the real
            // 201 success as an error). The method also returns an empty `struct{}` (no body model).
            let graph = super::body_less_success_graph(201);
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            assert!(
                out.contains("if resp.StatusCode != 201 {"),
                "body-less 201 must compare against 201:\n{out}"
            );
            assert!(
                !out.contains("if resp.StatusCode != 200 {"),
                "body-less 201 must NOT default to 200:\n{out}"
            );
            assert!(
                out.contains("(struct{}, error)"),
                "a body-less success returns an empty struct:\n{out}"
            );
        }

        #[test]
        fn body_less_204_compares_against_its_real_status() {
            // WR-01: a body-less `204 No Content` must likewise compare against 204.
            let graph = super::body_less_success_graph(204);
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            assert!(
                out.contains("if resp.StatusCode != 204 {"),
                "body-less 204 must compare against 204:\n{out}"
            );
        }

        #[test]
        fn error_decode_only_copies_fields_the_error_model_declares() {
            // A graph whose error model carries only `message` (no `slug`/`hints`) must copy ONLY
            // Message — referencing `apiErr.Slug`/`apiErr.Hints` on that struct would not compile.
            let graph = super::error_model_graph("ProblemDetails");
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "goalservice", "/goal", &ops, None).unwrap();
            // The synthetic error model in error_model_graph declares message + slug only.
            assert!(
                out.contains("Message: apiErrorStringValue(apiErr.Message),"),
                "{out}"
            );
            assert!(
                out.contains("Slug: apiErrorStringValue(apiErr.Slug),"),
                "{out}"
            );
            assert!(
                !out.contains("Hints: apiErr.Hints,"),
                "must not read a `Hints` field the error model does not declare:\n{out}"
            );
        }
    }

    mod client_and_errors {
        use super::{emit_client, emit_errors};

        #[test]
        fn client_emits_functional_options_constructor() {
            let out = emit_client("goalservice", None);
            assert!(
                out.contains("func NewClient(baseURL string, opts ...Option) *Client"),
                "{out}"
            );
            assert!(
                out.contains("func WithHTTPClient(hc *http.Client) Option"),
                "{out}"
            );
            assert!(!out.contains("func WithAPIKey(key string) Option"), "{out}");
            let secured = emit_client("goalservice", Some("authorization"));
            assert!(
                secured.contains("func WithAPIKey(key string) Option"),
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
            assert!(out.contains("func (e *APIError) Error() string"), "{out}");
            assert!(out.contains("import \"fmt\""), "{out}");
        }
    }

    mod type_mapping {
        use super::{go_type, join_path, sample_graph};
        use crate::graph::{Prim, Type, WellKnown};

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

            // strings are nilable already → never pointer-wrapped, even when nullable.
            let string = Type::Primitive(Prim::String);
            assert_eq!(go_type(&string, true, &graph).unwrap(), "string");

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
                }]),
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
