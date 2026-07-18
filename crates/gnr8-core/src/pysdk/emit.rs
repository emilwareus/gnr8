//! `format!`-based Python SDK emitters (D-05: no template engine; small internal templating only).
//!
//! Each emitter turns the router-agnostic [`crate::graph::ApiGraph`] into one idiomatic Python source
//! file. Unlike [`crate::gosdk::emit`], there is NO `gofmt` normalization step (Python has no stdlib
//! formatter; `black`/`autopep8` are third-party — CLAUDE.md rule 2): every emitter produces
//! already-correct, significant-whitespace Python directly.
//!
//! - [`emit_models`]   — one Pydantic v2 `BaseModel` per object [`Schema`] by default (or one
//!   `@dataclass` in dataclass mode), plus one `class X(str, enum.Enum)` per named enum [`Schema`];
//!   Python types follow [`py_type`].
//! - [`emit_client`]   — the injectable `urllib.request.OpenerDirector`-backed `Client` (Task 3).
//! - [`emit_errors`]   — the typed `ApiError(Exception)` (Task 3).
//! - [`emit_operations`] / [`emit_init`] — the per-operation methods + the re-export surface (Task 3).
//!
//! THE LOAD-BEARING DIVERGENCE from the Go twin lives in [`py_type`]: where `go_type` returns an error
//! for [`Type::Union`], inline [`Type::Enum`], and inline [`Type::Object`] (Go has no sum types and the
//! Go target only emits named DTOs), Python *can* express sum/inline types, so `py_type` MUST map a
//! union to `Union[..]` and an inline enum to `Literal[..]`. The match over [`Type`] stays exhaustive —
//! no `_ =>` arm — so a future IR variant fails to compile until handled (rule 3).
//!
//! Determinism (PYSDK-03): every collection is consumed in the graph's already-sorted order, and each
//! file's import header is a FIXED string (no computed import set, no [`std::collections::HashMap`]
//! iteration). Every un-representable fact (a dangling `$ref`) returns [`crate::CoreError::SdkGen`];
//! there is no production `unwrap`/`expect`/`panic` (RUST-04).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::graph::{
    ApiGraph, Field, Operation, PaginationMode, PaginationPolicy, PaginationTermination, Param,
    Prim, RuntimePolicy, Type,
};
use crate::sdk::emit_common::{
    check_unique_schema_names, error_response_bodies_of, is_json_object_key, join_path,
    operation_api_key_headers, operation_api_key_queries, operation_http_auth_schemes, path_tokens,
    path_tokens_match, quoted_string_literal, request_body_model_of, split_words,
    success_responses_of, HttpAuthScheme, RequestBodyEncoding,
};
use crate::sdk::model_style::PyModelStyle;
use crate::sdk::surface::ResolvedTypeAlias;
use crate::CoreError;

/// Fold an indentation/`format!` write error into a typed [`CoreError::SdkGen`].
///
/// `write!`/`writeln!` into a `String` is infallible in practice, but the `fmt::Write` trait is
/// fallible; mapping the error keeps the path `unwrap`/`expect`-free (RUST-04).
fn sink(err: std::fmt::Error) -> CoreError {
    CoreError::SdkGen {
        message: format!("failed to format Python source: {err}"),
    }
}

/// Join import sections into a canonical `isort`/`ruff`-ordered block.
///
/// Each `section` is already in its own intra-section order; sections are emitted in the order given
/// (by convention: `__future__`, stdlib, third-party, first-party) separated by exactly one blank line,
/// with empty sections dropped. The returned block ends with a single trailing newline (or is empty when
/// nothing is imported). Computing the block per file — rather than emitting one FIXED superset header —
/// is what keeps the output free of unused imports (`ruff` F401) while staying deterministic: the same
/// graph yields the same section lists yields the same bytes (PYSDK-03).
fn import_block(sections: &[Vec<String>]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for section in sections {
        if !section.is_empty() {
            blocks.push(section.join("\n"));
        }
    }
    let mut out = blocks.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Build a `from typing import ...` line for exactly the `typing` names a file uses, in `ruff`/`isort`
/// `order_by_type` order (the SCREAMING `TYPE_CHECKING` constant first, then the capitalized names
/// alphabetically). Returns `None` when the file references no `typing` name (so no line is emitted and
/// no unused import lands — F401). `Dict`/`List` are deliberately absent: those map to the PEP 585
/// builtins `dict`/`list` (see [`py_type`]), which need no import.
fn typing_import_line(m: &ModelImports) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    if m.type_checking {
        names.push("TYPE_CHECKING");
    }
    if m.any {
        names.push("Any");
    }
    if m.literal {
        names.push("Literal");
    }
    if m.optional {
        names.push("Optional");
    }
    if m.union {
        names.push("Union");
    }
    if names.is_empty() {
        None
    } else {
        Some(format!("from typing import {}", names.join(", ")))
    }
}

/// Which imports a model FILE needs, derived from the schemas it contains (one source of truth, no fixed
/// superset). Every flag is set ONLY when a construct is actually emitted, so the header carries no unused
/// import (`ruff` F401) — the divergence from the old fixed-header scheme. This is a bag of independent
/// feature flags (one per importable symbol), so bools are the natural representation.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct ModelImports {
    /// `import enum` — a named `enum.Enum` class is emitted.
    enum_class: bool,
    /// A Pydantic `BaseModel`/`ConfigDict` (or a `@dataclass`) object model is emitted.
    object_model: bool,
    /// A Pydantic `Field(...)` right-hand side is emitted (a field needs an alias or a default).
    field: bool,
    /// `if TYPE_CHECKING:` split-model forward-ref imports are emitted.
    type_checking: bool,
    any: bool,
    literal: bool,
    optional: bool,
    union: bool,
}

/// Accumulate the `typing` constructs a `Type` uses into [`ModelImports`] (recursing through
/// arrays/maps/unions/inline objects). A nested inline `Type::Enum` needs `Literal`; a `Type::Union`
/// needs `Union`; a `Type::Any` needs `Any`. Named refs and primitives need nothing. Nullability/optional
/// axes are handled at the field level (they drive `Optional`), not here.
fn accumulate_type_imports(schema: &Type, m: &mut ModelImports) {
    match schema {
        Type::Primitive(_) | Type::WellKnown(_) | Type::Named(_) => {}
        Type::Any {} => m.any = true,
        Type::Array(items) => accumulate_type_imports(items, m),
        Type::Map { value, .. } => accumulate_type_imports(value, m),
        Type::Enum(_) => m.literal = true,
        Type::Union(variants) => {
            m.union = true;
            for variant in variants {
                accumulate_type_imports(variant, m);
            }
        }
        Type::Object(fields) => {
            for field in fields {
                accumulate_type_imports(&field.schema, m);
            }
        }
    }
}

/// Compute the [`ModelImports`] for the given schemas (all schemas for a compact `models.py`; a single
/// schema for a split model file). `type_checking` is threaded in by the split path when it emits an
/// `if TYPE_CHECKING:` block. A NON-object/NON-enum schema body is emitted as a module-level string alias
/// (`X = "Union[..]"`) whose type names live only inside an opaque string literal — `ruff` does not read
/// them, so it contributes NO `typing` import (importing `Union` for it would be an F401).
fn compute_model_imports(schemas: &[&crate::graph::Schema], type_checking: bool) -> ModelImports {
    let mut m = ModelImports {
        type_checking,
        ..ModelImports::default()
    };
    for schema in schemas {
        match &schema.body {
            Type::Enum(_) => m.enum_class = true,
            Type::Object(fields) => {
                m.object_model = true;
                for field in fields {
                    if field.optional || field.nullable {
                        m.optional = true;
                    }
                    if needs_alias(field) || field.optional {
                        m.field = true;
                    }
                    accumulate_type_imports(&field.schema, &mut m);
                }
            }
            // Alias body: emitted as a string literal — see the doc comment above; no imports.
            _ => {}
        }
    }
    // Every object model's `from_dict`/`to_dict` signatures reference `dict[str, Any]`.
    if m.object_model {
        m.any = true;
    }
    m
}

/// Assemble the import header for a model file from its computed [`ModelImports`] and style.
fn model_header(m: &ModelImports, model_style: PyModelStyle) -> String {
    let mut stdlib: Vec<String> = Vec::new();
    if m.enum_class {
        stdlib.push("import enum".to_string());
    }
    if let PyModelStyle::Dataclass = model_style {
        if m.object_model {
            stdlib.push("from dataclasses import dataclass".to_string());
        }
    }
    if let Some(line) = typing_import_line(m) {
        stdlib.push(line);
    }

    let mut third_party: Vec<String> = Vec::new();
    if let PyModelStyle::Pydantic = model_style {
        if m.object_model {
            let mut names = vec!["BaseModel", "ConfigDict"];
            if m.field {
                names.push("Field");
            }
            third_party.push(format!("from pydantic import {}", names.join(", ")));
        }
    }

    import_block(&[
        vec!["from __future__ import annotations".to_string()],
        stdlib,
        third_party,
    ])
}

/// Convert an identifier to `snake_case` (Python method/attribute name): `createBook` → `create_book`.
pub(crate) fn snake(name: &str) -> String {
    split_words(name)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

pub(crate) fn operation_method_name(op: &Operation) -> String {
    snake(&op.handler)
}

/// Convert an enum member value to a `SCREAMING_SNAKE` identifier: `out-of-stock` → `OUT_OF_STOCK`.
pub(crate) fn screaming_snake(value: &str) -> String {
    split_words(value)
        .iter()
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// The fixed set of Python reserved words that may NOT be used as bare identifiers.
///
/// Sourced from Python's `keyword.kwlist` (a FIXED set baked into the emitter — never shelled out,
/// CLAUDE.md rule 2). A field/param/local whose name lands on this list emits invalid Python, so
/// [`safe_ident`] suffixes a trailing `_` to produce a valid, deterministic identifier.
const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Reserved argument names a generated method already binds (`self`/`body`), which a path/query param
/// must not collide with (it would produce a `SyntaxError: duplicate argument` — WR-03).
const RESERVED_ARGS: &[&str] = &["self", "body"];

/// Make a snake/identifier-form string a SAFE Python identifier, deterministically.
///
/// A Python keyword (`from`/`class`/`id`-is-fine/`type`-is-fine but `import`/`def`/...) or a name that
/// starts with a digit is invalid as a bare identifier. This suffixes a single trailing `_` in that
/// case (a stable, collision-resistant transform). The WIRE name (JSON key / query key) is NEVER passed
/// through this — only the *Python identifier* is renamed; callers keep the original `p.name`/`json_name`
/// for the on-the-wire key (CR-02). One deterministic path, no fallback (rule 3).
pub(crate) fn safe_ident(s: &str) -> String {
    let candidate = if s
        .chars()
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && s.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
    {
        s.to_string()
    } else {
        let words = split_words(s)
            .iter()
            .map(|w| w.to_ascii_lowercase())
            .collect::<Vec<_>>();
        if words.is_empty() {
            "field".to_string()
        } else {
            words.join("_")
        }
    };

    if PY_KEYWORDS.contains(&candidate.as_str()) {
        format!("{candidate}_")
    } else if candidate.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{candidate}")
    } else {
        candidate
    }
}

/// Whether a Python field identifier differs from its wire key and needs a Pydantic alias.
fn needs_alias(field: &Field) -> bool {
    pydantic_field_ident(field) != field.json_name
}

fn pydantic_field_ident(field: &Field) -> String {
    safe_ident(&snake(&field.json_name))
}

/// Quote a Python string literal for generated source.
fn py_string_literal(value: &str) -> String {
    format!("{value:?}")
}

/// Emit a field default/metadata expression for a Pydantic v2 model.
fn pydantic_field_expr(field: &Field, has_default: bool) -> String {
    let default = if has_default { "default=None" } else { "..." };
    if needs_alias(field) {
        format!("{default}, alias={}", py_string_literal(&field.json_name))
    } else if has_default {
        "default=None".to_string()
    } else {
        String::new()
    }
}

/// Build the right-hand side for a Pydantic v2 field declaration.
fn pydantic_field_rhs(field: &Field, has_default: bool) -> String {
    let expr = pydantic_field_expr(field, has_default);
    if expr.is_empty() {
        String::new()
    } else {
        format!(" = Field({expr})")
    }
}

/// Keep nullability in the type hint while using the default solely for key absence.
fn optional_default_hint(field: &Field, graph: &ApiGraph) -> Result<String, CoreError> {
    let hint = py_type(&field.schema, field.nullable, graph)?;
    if field.nullable {
        Ok(hint)
    } else {
        Ok(format!("Optional[{hint}]"))
    }
}

fn pydantic_default_suffix(field: &Field, graph: &ApiGraph) -> Result<String, CoreError> {
    Ok(format!(
        "{}{}",
        optional_default_hint(field, graph)?,
        pydantic_field_rhs(field, true)
    ))
}

fn pydantic_required_suffix(field: &Field, graph: &ApiGraph) -> Result<String, CoreError> {
    Ok(format!(
        "{}{}",
        py_type(&field.schema, field.nullable, graph)?,
        pydantic_field_rhs(field, false)
    ))
}

/// Map a neutral graph [`Type`] to its Python type hint, resolving named refs to model names.
///
/// ALL Python-specific type mapping lives HERE (per-target mapping, IR-03). The match over [`Type`] is
/// exhaustive — NO `_ =>` / `other =>` arm — so a future variant fails to compile until handled (rule 3).
///
/// This is the load-bearing divergence from `gosdk::emit::go_type`: a [`Type::Union`] becomes
/// `Union[..]` and an inline [`Type::Enum`] becomes `Literal[..]` (Python has sum/literal types; the Go
/// target rejected both). An inline [`Type::Object`] keeps parity with the Go target — a typed
/// [`CoreError::SdkGen`] — because every object in the neutral IR is a named `$ref` (RESEARCH Open Q4).
///
/// `nullable` wraps the resulting hint in `Optional[..]` (the value may be explicitly `None`). The
/// `optional` axis is NOT read here — it drives the dataclass field default in [`emit_dataclass`], not
/// the type hint (the two are distinct axes — RESEARCH Pitfall 4).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling `Named` ref or an inline [`Type::Object`].
pub(crate) fn py_type(
    schema: &Type,
    nullable: bool,
    graph: &ApiGraph,
) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(prim) => py_primitive(prim).to_string(),
        // Every well-known scalar carries on the wire as a string in this SDK (a date-time is an
        // RFC-3339 `str`; a uuid/email/uri is a `str`) — A7. No `datetime` import, so model instances
        // marshal cleanly through `json`.
        Type::WellKnown(_) => "str".to_string(),
        // PEP 585 builtin generics (`list[..]`/`dict[..]`), NOT `typing.List`/`Dict` — the modern
        // spelling `ruff` UP006/UP035 require, and import-free. `from __future__ import annotations`
        // keeps every annotation a lazy string, and Pydantic's runtime resolution of `list`/`dict`
        // subscription works on Python 3.9+ (PEP 585), so this stays 3.9-safe.
        Type::Array(items) => format!("list[{}]", py_type(items, false, graph)?),
        Type::Map { key, value } => {
            if !is_json_object_key(key) {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "map key type {key:?} cannot be represented as a Python JSON object key"
                    ),
                });
            }
            format!("dict[str, {}]", py_type(value, false, graph)?)
        }
        Type::Any {} => "Any".to_string(),
        Type::Named(ref_id) => {
            let target = graph
                .schemas
                .iter()
                .find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            target.name.clone()
        }
        // An inline enum stays inline as a `Literal[..]` (members in graph-sorted order) — the case the
        // Go target rejects. A named enum (a top-level Schema body) is instead a class; see emit_models.
        Type::Enum(members) => {
            let lits: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
            format!("Literal[{}]", lits.join(", "))
        }
        // A sum type becomes a `Union[..]` (the case the Go target rejects — Go has no sum types).
        Type::Union(variants) => {
            let mut parts: Vec<String> = Vec::with_capacity(variants.len());
            for variant in variants {
                parts.push(py_type(variant, false, graph)?);
            }
            format!("Union[{}]", parts.join(", "))
        }
        // An inline (anonymous) object is not emitted as a Python type — every object in the IR is a
        // named `$ref`. Keep parity with the Go target: an EXPLICIT typed error arm, not a catch-all.
        Type::Object(_) => {
            return Err(CoreError::SdkGen {
                message: "inline object type is unsupported by the Python SDK target \
                          (expected a named $ref)"
                    .to_string(),
            });
        }
    };
    if nullable {
        Ok(format!("Optional[{base}]"))
    } else {
        Ok(base)
    }
}

/// Map a neutral [`Prim`] to its Python type. Integer width is irrelevant in Python (`int` is
/// arbitrary-precision) and a float is `float`; a byte string maps to `bytes`.
fn py_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String => "str",
        Prim::Bool => "bool",
        Prim::Int { .. } => "int",
        Prim::Float { .. } => "float",
        Prim::Bytes => "bytes",
    }
}

/// Emit `models.py`: one model class per object schema + one `class X(str, enum.Enum)` per named enum.
///
/// Schemas are consumed in the graph's id-sorted order (determinism). A schema whose body is
/// [`Type::Enum`] becomes a named enum class; a [`Type::Object`] becomes a Pydantic v2 model by default
/// or a dataclass in dataclass mode; every other body is a typed [`CoreError::SdkGen`] (mirror of the Go
/// twin's non-object/non-enum arm).
///
/// `package` is currently unused in the body (the file carries no package clause in Python) but is kept
/// in the signature to mirror the Go twin and the `generate` call site.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if a field's schema cannot be mapped or a schema body is unsupported.
#[cfg(test)]
pub(crate) fn emit_models(graph: &ApiGraph, package: &str) -> Result<String, CoreError> {
    emit_models_with_style(graph, package, PyModelStyle::default())
}

#[cfg(test)]
pub(crate) fn emit_models_with_style(
    graph: &ApiGraph,
    package: &str,
    model_style: PyModelStyle,
) -> Result<String, CoreError> {
    emit_models_with_style_and_aliases(graph, package, model_style, &[])
}

pub(crate) fn emit_models_with_style_and_aliases(
    graph: &ApiGraph,
    _package: &str,
    model_style: PyModelStyle,
    aliases: &[ResolvedTypeAlias],
) -> Result<String, CoreError> {
    check_unique_schema_names(graph, "Python SDK")?;

    let schema_refs: Vec<&crate::graph::Schema> = graph.schemas.iter().collect();
    let mut out = model_header(&compute_model_imports(&schema_refs, false), model_style);

    // The first top-level item is separated from the import block by isort's `lines-after-imports`: two
    // blank lines before a class/enum def, but only one before a bare alias assignment (a simple
    // statement). Every LATER item gets two blank lines (`ruff format` around defs). Getting the first
    // gap right is what keeps a leading alias I001-clean.
    let mut first = true;
    for schema in &graph.schemas {
        let is_def = matches!(&schema.body, Type::Enum(_) | Type::Object(_));
        out.push_str(top_level_separator(first, is_def));
        first = false;
        match &schema.body {
            // A named enum (top-level Schema body) → a `class X(str, enum.Enum)`. The `str` mixin makes
            // `json.dumps` serialize the member value as its string — the twin of Go's `type X string`.
            Type::Enum(members) => emit_enum_class(&mut out, &schema.name, members)?,
            Type::Object(fields) => {
                emit_model_class(&mut out, &schema.name, fields, graph, model_style)?;
            }
            // A named NON-object/NON-enum schema (e.g. `BookOrError = Union[Book, OutOfStock]`, or a
            // scalar/array/map alias) → a module-level type alias. This is the load-bearing divergence
            // from the Go twin, which rejected named unions outright (Go has no sum types). `py_type`
            // maps the body exhaustively, so a named union/array/scalar alias emits a valid Python hint.
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Union(_)
            | Type::Any {} => {
                // A module-level alias assignment is evaluated EAGERLY at import time (unlike a
                // model annotation, which `from __future__ import annotations` keeps lazy). The
                // schemas are id-sorted, so an alias may reference a class defined LATER in the file
                // (e.g. `BookOrError = Union[Book, OutOfStock]` precedes `OutOfStock`) — an eager RHS
                // raises `NameError` at import. Emit the RHS as a PEP-484 string forward reference so
                // the assignment binds a plain `str` (importable, re-exportable) without evaluating any
                // forward name. The value stays a valid type alias in annotation position (PYSDK-02).
                let alias = py_type(&schema.body, false, graph)?;
                writeln!(out, "{} = \"{alias}\"", schema.name).map_err(sink)?;
            }
        }
    }
    for alias in aliases {
        out.push_str(top_level_separator(first, false));
        first = false;
        writeln!(out, "{} = {}", alias.alias, alias.canonical).map_err(sink)?;
    }
    Ok(out)
}

/// The blank-line separator before a top-level item: two blank lines everywhere `ruff format` wants them
/// around defs, except the VERY FIRST item after the import block when it is a bare alias assignment —
/// isort's `lines-after-imports` allows only one blank line before a simple statement there (I001).
fn top_level_separator(first: bool, is_def: bool) -> &'static str {
    if first && !is_def {
        "\n"
    } else {
        "\n\n"
    }
}

/// Emit one model schema into its own Python module.
pub(crate) fn emit_model_schema(
    graph: &ApiGraph,
    schema: &crate::graph::Schema,
    model_style: PyModelStyle,
    dep_modules: &BTreeMap<String, String>,
) -> Result<String, CoreError> {
    // Forward-ref imports are needed ONLY for an object model's field types; an enum has none, and an
    // alias body is a string literal whose names `ruff` never reads (so importing them would be F401).
    let deps = match &schema.body {
        Type::Object(_) => model_dependencies(&schema.body, graph, &schema.name),
        _ => Vec::new(),
    };
    let imports = compute_model_imports(&[schema], !deps.is_empty());
    let mut out = model_header(&imports, model_style);
    if deps.is_empty() {
        // No forward-ref block: separate the class/enum from the imports by two blank lines, but a bare
        // alias assignment by only one (isort `lines-after-imports`, matching the compact path).
        let is_def = matches!(&schema.body, Type::Enum(_) | Type::Object(_));
        out.push_str(top_level_separator(true, is_def));
    } else {
        // A TYPE_CHECKING block (objects only) sits one blank line below the imports; the class then
        // follows two blank lines below the block.
        out.push('\n');
        writeln!(out, "if TYPE_CHECKING:").map_err(sink)?;
        for dep in deps {
            let module = dep_modules.get(&dep).ok_or_else(|| CoreError::SdkGen {
                message: format!(
                    "schema '{}' depends on model {dep:?}, but no Python module was generated for it",
                    schema.name
                ),
            })?;
            writeln!(out, "    from {module} import {dep}").map_err(sink)?;
        }
        out.push_str("\n\n");
    }
    match &schema.body {
        Type::Enum(members) => emit_enum_class(&mut out, &schema.name, members)?,
        Type::Object(fields) => {
            emit_model_class(&mut out, &schema.name, fields, graph, model_style)?;
        }
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Array(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Union(_)
        | Type::Any {} => {
            let alias = py_type(&schema.body, false, graph)?;
            writeln!(out, "{} = \"{alias}\"", schema.name).map_err(sink)?;
        }
    }
    Ok(out)
}

/// Emit a split-model compatibility alias shim.
pub(crate) fn emit_model_alias(alias: &ResolvedTypeAlias, canonical_module: &str) -> String {
    format!(
        "from __future__ import annotations\n\nfrom {canonical_module} import {} as {}\n\n__all__ = [\"{}\"]\n",
        alias.canonical,
        alias.alias,
        alias.alias
    )
}

/// Emit `models/__init__.py` for split-model layout.
pub(crate) fn emit_models_init(imports: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n\n");
    // isort orders relative imports by MODULE, which need not match graph order or custom templates.
    let mut imports = imports.to_vec();
    imports.sort();
    for (module, name) in &imports {
        let _ = writeln!(out, "from {module} import {name}");
    }
    out.push_str("\n__all__ = [\n");
    let mut names: Vec<&str> = imports.iter().map(|(_, name)| name.as_str()).collect();
    names.sort_unstable();
    for name in names {
        let _ = writeln!(out, "    \"{name}\",");
    }
    out.push_str("]\n\n");
    out.push_str("_types_namespace = {name: globals()[name] for name in __all__}\n");
    out.push_str("for _model in _types_namespace.values():\n");
    out.push_str("    if hasattr(_model, \"model_rebuild\"):\n");
    out.push_str("        _model.model_rebuild(_types_namespace=_types_namespace)\n");
    out.push_str("del _model, _types_namespace\n");
    out
}

fn model_dependencies(body: &Type, graph: &ApiGraph, self_name: &str) -> Vec<String> {
    let mut deps = Vec::new();
    collect_model_dependencies(body, graph, self_name, &mut deps);
    deps.sort();
    deps.dedup();
    deps
}

fn collect_model_dependencies(
    schema: &Type,
    graph: &ApiGraph,
    self_name: &str,
    out: &mut Vec<String>,
) {
    match schema {
        Type::Named(ref_id) => {
            if let Some(target) = graph.schemas.iter().find(|s| &s.id == ref_id) {
                if target.name != self_name {
                    out.push(target.name.clone());
                }
            }
        }
        Type::Array(items) => collect_model_dependencies(items, graph, self_name, out),
        Type::Map { value, .. } => collect_model_dependencies(value, graph, self_name, out),
        Type::Union(variants) => {
            for variant in variants {
                collect_model_dependencies(variant, graph, self_name, out);
            }
        }
        Type::Object(fields) => {
            for field in fields {
                collect_model_dependencies(&field.schema, graph, self_name, out);
            }
        }
        Type::Primitive(_) | Type::WellKnown(_) | Type::Enum(_) | Type::Any {} => {}
    }
}

/// Emit a named enum class: `class {name}(str, enum.Enum)` with `MEMBER = "value"` lines.
///
/// Members are emitted in graph order (already lexically sorted, GRAPH-02). The member identifier is the
/// `SCREAMING_SNAKE` form of the value; the value itself is the wire string.
fn emit_enum_class(out: &mut String, name: &str, members: &[String]) -> Result<(), CoreError> {
    writeln!(out, "class {name}(str, enum.Enum):").map_err(sink)?;
    if members.is_empty() {
        // An empty enum still needs a body to be valid Python.
        writeln!(out, "    pass").map_err(sink)?;
        return Ok(());
    }
    // Generate collision-free, identifier-safe member names deterministically (CR-03): two wire values
    // that normalize to the same SCREAMING_SNAKE form would emit a duplicate class key (TypeError at
    // import), and an empty/leading-digit normalization is an invalid identifier. The member's wire
    // `value` string is NEVER changed — only the Python member NAME is sanitized + disambiguated. The
    // `used` list preserves first-seen (graph) order, so the suffix assignment is deterministic.
    let mut used: Vec<String> = Vec::with_capacity(members.len());
    for value in members {
        let base = enum_member_ident(value);
        // Disambiguate on collision by appending `_2`, `_3`, ... (the first occurrence keeps the base).
        let mut member = base.clone();
        let mut n = 2u32;
        while used.contains(&member) {
            member = format!("{base}_{n}");
            n += 1;
        }
        used.push(member.clone());
        writeln!(out, "    {member} = \"{value}\"").map_err(sink)?;
    }
    Ok(())
}

/// Map an enum wire value to a SAFE, non-empty Python member identifier (CR-03).
///
/// `screaming_snake` can produce an empty string (`""`, punctuation-only) or a leading-digit form
/// (`"1"` → `1`), both invalid identifiers. This guards both: an empty normalization becomes a stable
/// placeholder `MEMBER`, and a leading-digit form is prefixed with `_`. Collision disambiguation is the
/// caller's concern (so the two transforms compose deterministically).
fn enum_member_ident(value: &str) -> String {
    let screamed = screaming_snake(value);
    if screamed.is_empty() {
        // No word characters at all (empty or pure punctuation) — emit a stable placeholder; the caller
        // disambiguates repeats (`MEMBER`, `MEMBER_2`, ...). The wire value stays intact.
        "MEMBER".to_string()
    } else if screamed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{screamed}")
    } else {
        screamed
    }
}

/// Resolve a [`Type::Named`] ref to the schema it points at (used to decide nested-decode strategy).
fn resolve_named<'g>(schema: &Type, graph: &'g ApiGraph) -> Option<&'g crate::graph::Schema> {
    match schema {
        Type::Named(ref_id) => graph.schemas.iter().find(|s| &s.id == ref_id),
        _ => None,
    }
}

/// Build the Python expression that decodes a single from-dict value `v` into a field's advertised type.
///
/// `v` is the bound raw JSON value for this field. The decode is RECURSIVE for nested dataclasses
/// (CR-04 #2): a named object-schema field becomes `Model.from_dict(v)`, a list-of-named-object becomes
/// a comprehension, and every other shape (scalar, enum value, union, map, Any) passes through unchanged
/// (the str-enum mixin accepts the raw value; a union/map has no single concrete constructor). One
/// deterministic mapping per field type, no fallback (rule 3).
fn decode_expr(schema: &Type, graph: &ApiGraph, value_var: &str) -> String {
    match schema {
        // A named ref to an OBJECT schema → recurse via its from_dict; a named enum (str mixin) or any
        // other named alias passes the raw value through.
        Type::Named(_) => match resolve_named(schema, graph) {
            Some(target) if matches!(target.body, Type::Object(_)) => {
                format!("{}.from_dict({value_var})", target.name)
            }
            _ => value_var.to_string(),
        },
        // A list whose items are a named object schema → decode each element recursively.
        Type::Array(items) => match resolve_named(items, graph) {
            Some(target) if matches!(target.body, Type::Object(_)) => format!(
                "[{}.from_dict(_item) for _item in {value_var}]",
                target.name
            ),
            _ => value_var.to_string(),
        },
        // Scalars, well-known, maps, Any, inline enums/unions, inline objects: pass through. A union has
        // no single constructor; an inline enum value is already the wire string.
        _ => value_var.to_string(),
    }
}

/// Emit a `@dataclass` for an object schema, partitioning fields required-first / optional-last.
///
/// PITFALL 1 (RESEARCH): `@dataclass` raises `TypeError: non-default argument follows default argument`
/// at class-definition (import) time if a no-default field follows a defaulted one. The graph sorts
/// fields alphabetically by `json_name`, which interleaves the two, so this partitions the fields —
/// required (no default) first, optional (default `= None`) last — before emitting. `kw_only=True` is
/// Python 3.10+ and unavailable on 3.9, so partitioning is the 3.9-safe fix. The reorder is a
/// presentation concern only: json keys are name-addressed, so wire behavior is unchanged.
fn emit_model_class(
    out: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
    model_style: PyModelStyle,
) -> Result<(), CoreError> {
    match model_style {
        PyModelStyle::Pydantic => emit_pydantic_model(out, name, fields, graph),
        PyModelStyle::Dataclass => emit_dataclass(out, name, fields, graph),
    }
}

/// Emit a Pydantic v2 `BaseModel` for an object schema.
fn emit_pydantic_model(
    out: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(out, "class {name}(BaseModel):").map_err(sink)?;
    writeln!(
        out,
        "    model_config = ConfigDict(populate_by_name=True, extra=\"ignore\")"
    )
    .map_err(sink)?;
    for field in fields {
        let ident = pydantic_field_ident(field);
        let suffix = if field.optional {
            pydantic_default_suffix(field, graph)?
        } else {
            pydantic_required_suffix(field, graph)?
        };
        writeln!(out, "    {ident}: {suffix}").map_err(sink)?;
    }
    writeln!(out).map_err(sink)?;
    writeln!(out, "    @classmethod").map_err(sink)?;
    writeln!(
        out,
        "    def from_dict(cls, _data: dict[str, Any]) -> {name}:"
    )
    .map_err(sink)?;
    writeln!(out, "        return cls.model_validate(_data)").map_err(sink)?;
    writeln!(out).map_err(sink)?;
    writeln!(out, "    def to_dict(self) -> dict[str, Any]:").map_err(sink)?;
    writeln!(
        out,
        "        return self.model_dump(mode=\"json\", by_alias=True, exclude_none=True)"
    )
    .map_err(sink)?;
    Ok(())
}

fn emit_dataclass(
    out: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(out, "@dataclass").map_err(sink)?;
    writeln!(out, "class {name}:").map_err(sink)?;
    if fields.is_empty() {
        // An empty dataclass still needs a forward-compatible from_dict so the decode site is uniform.
        writeln!(out, "    @classmethod").map_err(sink)?;
        writeln!(
            out,
            "    def from_dict(cls, _data: dict[str, Any]) -> {name}:"
        )
        .map_err(sink)?;
        writeln!(out, "        return cls()").map_err(sink)?;
        return Ok(());
    }
    // Partition preserving each group's (already-sorted) relative order: required (no default) first,
    // optional (defaulted) last — so defaulted fields are contiguous at the end (PITFALL 1).
    let (required, optional): (Vec<&Field>, Vec<&Field>) = fields.iter().partition(|f| !f.optional);
    for field in required {
        // The Python attribute name is keyword/digit-safe (CR-02); the wire key (`json_name`) is
        // preserved verbatim — only a keyword/leading-digit name is renamed, and the from-dict decode
        // (CR-04) maps the original JSON key onto the (possibly renamed) attribute by position.
        let ident = safe_ident(&field.json_name);
        let hint = py_type(&field.schema, field.nullable, graph)?;
        writeln!(out, "    {ident}: {hint}").map_err(sink)?;
    }
    for field in optional {
        let ident = safe_ident(&field.json_name);
        // An optional field (the key may be absent) defaults to None so the class imports and a caller
        // may omit it. Widen the hint to Optional[..] for the defaulted form so `= None` is not a
        // type-lie against a non-nullable value type (WR-02): the `optional` (key-absent) axis is
        // modeled in the hint, not only the default. A nullable field is already Optional[..]; wrapping
        // an already-Optional hint again is avoided by py_type carrying nullable, so only widen when the
        // value is not itself nullable.
        let hint = py_type(&field.schema, field.nullable, graph)?;
        let defaulted_hint = if field.nullable {
            hint
        } else {
            format!("Optional[{hint}]")
        };
        writeln!(out, "    {ident}: {defaulted_hint} = None").map_err(sink)?;
    }

    // A forward-compatible from_dict (CR-04): construct only from declared fields (ignore-unknown — a
    // newer server adding a response key no longer crashes the SDK), bind each by its ORIGINAL wire key
    // (json_name), and decode nested dataclasses recursively. Required fields read with `_data["key"]`
    // (a missing required key is a real protocol error → KeyError); optional fields use `.get` so an
    // absent key keeps the None default.
    writeln!(out, "    @classmethod").map_err(sink)?;
    writeln!(
        out,
        "    def from_dict(cls, _data: dict[str, Any]) -> {name}:"
    )
    .map_err(sink)?;
    writeln!(out, "        return cls(").map_err(sink)?;
    for field in fields {
        let ident = safe_ident(&field.json_name);
        let wire = &field.json_name;
        if field.optional {
            // Optional: only decode when present (and non-null), else keep the None default. The
            // conditional expression evaluates the decode lazily so a nested model still recurses.
            let decoded_present = decode_expr(&field.schema, graph, &format!("_data[\"{wire}\"]"));
            writeln!(
                out,
                "            {ident}=({decoded_present}) if \"{wire}\" in _data and _data[\"{wire}\"] is not None else None,"
            )
            .map_err(sink)?;
        } else {
            let decoded = decode_expr(&field.schema, graph, &format!("_data[\"{wire}\"]"));
            writeln!(out, "            {ident}={decoded},").map_err(sink)?;
        }
    }
    writeln!(out, "        )").map_err(sink)?;
    Ok(())
}

/// Emit `errors.py`: the typed `ApiError(Exception)` with status, response metadata, and decoded body.
///
/// `package` is unused in the body (no package clause in Python) but kept for call-site symmetry with
/// the Go twin's `emit_errors`. The `from __future__ import annotations` header keeps annotations lazy.
pub(crate) fn emit_errors(_package: &str) -> String {
    "\
from __future__ import annotations

from typing import Any, Optional


class ApiError(Exception):
    \"\"\"Raised by operation methods on a non-success response.

    Carries status, response metadata, raw body, parsed JSON, and decoded error body.
    \"\"\"

    def __init__(
        self,
        status_code: int,
        message: str = \"\",
        slug: str = \"\",
        hints: Optional[list[Any]] = None,
        *,
        headers: Optional[dict[str, str]] = None,
        request_id: str = \"\",
        raw_body: bytes = b\"\",
        json_body: Any = None,
        body: Any = None,
    ) -> None:
        super().__init__(f\"{status_code} {message} ({slug})\")
        self.status_code = status_code
        self.headers = headers or {}
        self.request_id = request_id
        self.raw_body = raw_body
        self.json_body = json_body
        self.body = body
        self.message = message
        self.slug = slug
        self.hints = hints if hints is not None else []

    def is_not_found(self) -> bool:
        return self.status_code == 404
"
    .to_string()
}

/// Emit `client.py`: the `Client` backed by an injectable `urllib` `OpenerDirector`.
///
/// The operation methods (one per graph operation) are appended to this same file by [`emit_operations`]
/// and re-frame into `client.py`. The `Client` holds a `base_url`, an optional `api_key`, and an
/// `OpenerDirector` defaulting to `urllib.request.build_opener()` — the swappable transport seam the
/// hermetic test injects (RESEARCH Pattern 3). `_do` builds a `urllib.request.Request`, sets the
/// `Content-Type`/`X-API-Key` headers, opens via the injected opener, and catches
/// `urllib.error.HTTPError` so 4xx/5xx return a `(code, body)` pair instead of raising (Pitfall 6).
#[cfg(test)]
pub(crate) fn emit_client(package: &str) -> String {
    emit_client_with_models(
        package,
        "models",
        PyModelStyle::default(),
        false,
        false,
        false,
        &[],
        &RuntimePolicy::default(),
        false,
        false,
    )
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn py_timeout_value(timeout_ms: Option<u64>) -> String {
    timeout_ms.map_or_else(
        || "30.0".to_string(),
        |ms| format!("{}.{:03}", ms / 1000, ms % 1000),
    )
}

fn py_retry_status_tuple(runtime: &RuntimePolicy) -> String {
    let mut statuses = runtime.retry_statuses.clone();
    if statuses.is_empty() {
        statuses.extend([408, 429]);
    }
    statuses.sort_unstable();
    statuses.dedup();
    match statuses.as_slice() {
        [] => "()".to_string(),
        [single] => format!("({single},)"),
        many => {
            let joined = many
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({joined})")
        }
    }
}

fn py_body_encoding(encoding: RequestBodyEncoding) -> &'static str {
    match encoding {
        RequestBodyEncoding::Json => "json",
        RequestBodyEncoding::Text => "text",
        RequestBodyEncoding::FormUrlEncoded => "form",
        RequestBodyEncoding::Multipart => "multipart",
        RequestBodyEncoding::Binary => "binary",
    }
}

struct PyOperationRuntime<'a> {
    idempotent: bool,
    idempotency_key_header: Option<&'a str>,
}

fn py_operation_runtime<'a>(graph: &'a ApiGraph, op: &Operation) -> PyOperationRuntime<'a> {
    graph
        .operation_runtime
        .iter()
        .find(|policy| policy.operation_id == op.id)
        .map_or(
            PyOperationRuntime {
                idempotent: false,
                idempotency_key_header: None,
            },
            |policy| PyOperationRuntime {
                idempotent: policy.idempotent,
                idempotency_key_header: policy.idempotency_key_header.as_deref(),
            },
        )
}

/// The set of model class names `client.py` references (each operation's request-body model and typed
/// success-response model), sorted + de-duplicated. These are imported EXPLICITLY (never `from .models
/// import *`) so the file carries no star import — `ruff` F403 (star import) / F405 (name may be
/// undefined) are structurally impossible, and every imported name is genuinely used (F401-clean).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling request-body / response `$ref` (surfaced by the same
/// resolvers the operation emitter uses).
pub(crate) fn client_referenced_models(
    graph: &ApiGraph,
    ops: &[&Operation],
) -> Result<Vec<String>, CoreError> {
    let mut names: Vec<String> = Vec::new();
    for op in ops {
        if let Some(body) = request_body_model_of(op, graph)? {
            names.push(body.model);
        }
        if let Some(model) = success_responses_of(op, graph)?.body_model {
            names.push(model);
        }
        if let Some(item_model) = pagination_item_model_name(graph, op) {
            names.push(item_model.to_string());
        }
        for error_body in error_response_bodies_of(op, graph)? {
            names.push(error_body.model);
        }
        for param in &op.params {
            collect_python_type_models(&param.schema, graph, &mut names)?;
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub(crate) fn operations_need_parameter_literals(ops: &[&Operation]) -> bool {
    ops.iter().any(|op| {
        op.params
            .iter()
            .any(|param| type_contains_inline_enum(&param.schema))
    })
}

fn type_contains_inline_enum(schema: &Type) -> bool {
    match schema {
        Type::Enum(_) => true,
        Type::Array(items) => type_contains_inline_enum(items),
        Type::Map { key, value } => {
            type_contains_inline_enum(key) || type_contains_inline_enum(value)
        }
        Type::Union(variants) => variants.iter().any(type_contains_inline_enum),
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Named(_)
        | Type::Object(_)
        | Type::Any {} => false,
    }
}

fn collect_python_type_models(
    schema: &Type,
    graph: &ApiGraph,
    names: &mut Vec<String>,
) -> Result<(), CoreError> {
    match schema {
        Type::Named(ref_id) => {
            let target = graph
                .schemas
                .iter()
                .find(|candidate| &candidate.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!(
                        "dangling parameter $ref '{ref_id}' is not among graph.schemas"
                    ),
                })?;
            names.push(target.name.clone());
        }
        Type::Array(items) => collect_python_type_models(items, graph, names)?,
        Type::Map { key, value } => {
            collect_python_type_models(key, graph, names)?;
            collect_python_type_models(value, graph, names)?;
        }
        Type::Union(variants) => {
            for variant in variants {
                collect_python_type_models(variant, graph, names)?;
            }
        }
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Object(_)
        | Type::Enum(_)
        | Type::Any {} => {}
    }
    Ok(())
}

fn pagination_item_model_name<'a>(graph: &'a ApiGraph, op: &Operation) -> Option<&'a str> {
    let policy = pagination_policy_for(graph, op)?;
    let success = success_responses_of(op, graph).ok()?;
    let page_model = success.body_model?;
    let schema = graph
        .schemas
        .iter()
        .find(|schema| schema.name == page_model)?;
    let Type::Object(fields) = &schema.body else {
        return None;
    };
    let items = fields
        .iter()
        .find(|field| field.json_name == policy.items_field)?;
    let Type::Array(item_schema) = &items.schema else {
        return None;
    };
    let Type::Named(ref_id) = item_schema.as_ref() else {
        return None;
    };
    graph
        .schemas
        .iter()
        .find(|schema| &schema.id == ref_id)
        .map(|schema| schema.name.as_str())
}

/// Emit `client.py` with a configurable model package import path and the explicit set of model names to
/// import (from [`client_referenced_models`]).
#[allow(clippy::too_many_lines)]
#[expect(
    clippy::too_many_arguments,
    reason = "the Python client emitter is parameterized by layout/auth/model/runtime facts without a builder object"
)]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "the Python client emitter receives independent auth and pagination feature switches"
)]
pub(crate) fn emit_client_with_models(
    _package: &str,
    model_module: &str,
    model_style: PyModelStyle,
    has_api_key_auth: bool,
    has_bearer_auth: bool,
    has_basic_auth: bool,
    model_refs: &[String],
    runtime: &RuntimePolicy,
    has_pagination: bool,
    has_parameter_literals: bool,
) -> String {
    let body_value = match model_style {
        PyModelStyle::Pydantic => "        if isinstance(body, BaseModel):\n            mode = \"python\" if body_encoding == \"multipart\" else \"json\"\n            body = body.model_dump(mode=mode, by_alias=True, exclude_unset=True)\n        return self._wire_value(body)\n",
        PyModelStyle::Dataclass => "        if body is not None and dataclasses.is_dataclass(body):\n            body = dataclasses.asdict(body)\n        return self._wire_value(body)\n",
    };

    // --- Import header, assembled per file in canonical isort order (no unused imports, F401-clean). ---
    let mut stdlib: Vec<String> = Vec::new();
    if has_basic_auth {
        stdlib.push("import base64".to_string());
    }
    if let PyModelStyle::Dataclass = model_style {
        stdlib.push("import dataclasses".to_string());
    }
    stdlib.push("import enum".to_string());
    stdlib.push("import json".to_string());
    stdlib.push("import secrets".to_string());
    stdlib.push("import time".to_string());
    stdlib.push("import urllib.error".to_string());
    stdlib.push("import urllib.parse".to_string());
    stdlib.push("import urllib.request".to_string());
    let collections = if has_pagination {
        "from collections.abc import Callable, Iterator"
    } else {
        "from collections.abc import Callable"
    };
    stdlib.push(collections.to_string());
    if has_parameter_literals {
        stdlib.push("from typing import Any, Literal, Optional".to_string());
    } else {
        stdlib.push("from typing import Any, Optional".to_string());
    }

    let mut third_party: Vec<String> = Vec::new();
    if let PyModelStyle::Pydantic = model_style {
        third_party.push("from pydantic import BaseModel".to_string());
    }

    let mut first_party: Vec<String> = vec!["from .errors import ApiError".to_string()];
    if !model_refs.is_empty() {
        // A parenthesized, one-name-per-line import with a trailing comma: the "magic trailing comma"
        // keeps `ruff format` from collapsing it and stays stable regardless of the name count.
        let mut import = format!("from .{model_module} import (\n");
        for name in model_refs {
            let _ = writeln!(import, "    {name},");
        }
        import.push(')');
        first_party.push(import);
    }

    let header = import_block(&[
        vec!["from __future__ import annotations".to_string()],
        stdlib,
        third_party,
        first_party,
    ]);

    let auth_init = if has_api_key_auth {
        "        api_keys: Optional[dict[str, str]] = None,\n"
    } else {
        ""
    };
    let bearer_init = if has_bearer_auth {
        "        bearer_token: Optional[str] = None,\n"
    } else {
        ""
    };
    let basic_init = if has_basic_auth {
        "        basic_auth: Optional[tuple[str, str]] = None,\n"
    } else {
        ""
    };
    let auth_field = if has_api_key_auth {
        "        self._api_keys = api_keys or {}\n"
    } else {
        ""
    };
    let bearer_field = if has_bearer_auth {
        "        self._bearer_token = bearer_token\n"
    } else {
        ""
    };
    let basic_field = if has_basic_auth {
        "        self._basic_auth = basic_auth\n"
    } else {
        ""
    };
    let auth_headers_arg = if has_api_key_auth {
        "        auth_headers: Optional[tuple[str, ...]] = None,\n"
    } else {
        ""
    };
    let auth_bearer_arg = if has_bearer_auth {
        "        auth_bearer: bool = False,\n"
    } else {
        ""
    };
    let auth_basic_arg = if has_basic_auth {
        "        auth_basic: bool = False,\n"
    } else {
        ""
    };
    let mut auth_loop = String::new();
    if has_api_key_auth {
        auth_loop.push_str("        for header in auth_headers or ():\n            key = self._api_keys.get(header) or self._api_key\n            if key:\n                headers[header] = key\n");
    }
    if has_bearer_auth {
        auth_loop.push_str("        if auth_bearer and self._bearer_token:\n            headers[\"Authorization\"] = f\"Bearer {self._bearer_token}\"\n");
    }
    if has_basic_auth {
        auth_loop.push_str("        if auth_basic and self._basic_auth is not None:\n            raw = f\"{self._basic_auth[0]}:{self._basic_auth[1]}\".encode(\"utf-8\")\n            headers[\"Authorization\"] = \"Basic \" + base64.b64encode(raw).decode(\"ascii\")\n");
    }
    let default_timeout = py_timeout_value(runtime.default_timeout_ms);
    let max_retries = runtime.max_retries;
    let retry_statuses = py_retry_status_tuple(runtime);
    let retry_unsafe_methods = py_bool(runtime.retry_unsafe_methods);
    // The `_do` signature is pre-exploded (one parameter per line, trailing comma) because the single-
    // line form exceeds the 88-column limit — matching `ruff format` so the output is format-stable.
    format!(
        "\
{header}

class RequestOptions:
    \"\"\"Per-request SDK runtime overrides.\"\"\"

    def __init__(
        self,
        *,
        timeout: Optional[float] = None,
        max_retries: Optional[int] = None,
        idempotency_key: Optional[str] = None,
        metadata: Optional[dict[str, str]] = None,
    ) -> None:
        self.timeout = timeout
        self.max_retries = max_retries
        self.idempotency_key = idempotency_key
        self.metadata = metadata or {{}}


class HookContext:
    \"\"\"Context passed to generated SDK runtime hooks.\"\"\"

    def __init__(
        self,
        *,
        operation_id: str,
        method: str,
        path_template: str,
        url: str,
        headers: dict[str, str],
        request_metadata: dict[str, str],
    ) -> None:
        self.operation_id = operation_id
        self.method = method
        self.path_template = path_template
        self.url = url
        self.headers = headers
        self.request_metadata = request_metadata
        self.status: Optional[int] = None
        self.response_headers: dict[str, str] = {{}}


class ClientHooks:
    \"\"\"Generated SDK runtime hooks.\"\"\"

    def __init__(
        self,
        *,
        request: Optional[
            list[Callable[[HookContext, urllib.request.Request], None]]
        ] = None,
        response: Optional[list[Callable[[HookContext], None]]] = None,
        error: Optional[list[Callable[[HookContext, BaseException], None]]] = None,
    ) -> None:
        self.request = request or []
        self.response = response or []
        self.error = error or []


class Client:
    \"\"\"SDK client over urllib (no requests/httpx).\"\"\"

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
{auth_init}{bearer_init}{basic_init}        opener: Optional[urllib.request.OpenerDirector] = None,
        timeout: Optional[float] = {default_timeout},
        max_retries: int = {max_retries},
        hooks: Optional[ClientHooks] = None,
    ) -> None:
        self._base_url = base_url.rstrip(\"/\")
        self._api_key = api_key
{auth_field}{bearer_field}{basic_field}        self._opener = opener or urllib.request.build_opener()
        self._timeout = timeout
        self._max_retries = max_retries
        self._retry_statuses = {retry_statuses}
        self._retry_unsafe_methods = {retry_unsafe_methods}
        self._hooks = hooks or ClientHooks()

    def _body_value(self, body: Any, body_encoding: str) -> Any:
{body_value}
    def _wire_value(self, value: Any) -> Any:
        if isinstance(value, enum.Enum):
            return self._wire_value(value.value)
        if isinstance(value, list):
            return [self._wire_value(item) for item in value]
        if isinstance(value, tuple):
            return tuple(self._wire_value(item) for item in value)
        if isinstance(value, dict):
            return {{key: self._wire_value(item) for key, item in value.items()}}
        return value

    @staticmethod
    def _parameter_scalar(value: Any) -> str:
        if isinstance(value, bool):
            return \"true\" if value else \"false\"
        return str(value)

    def _parameter_pairs(
        self,
        name: str,
        value: Any,
        style: str,
        explode: bool,
    ) -> list[tuple[str, str]]:
        value = self._wire_value(value)
        if style == \"spaceDelimited\":
            delimiter = \" \"
        elif style == \"pipeDelimited\":
            delimiter = \"|\"
        else:
            delimiter = \",\"
        if isinstance(value, (list, tuple)):
            parts = [self._parameter_scalar(item) for item in value]
            if explode and style == \"form\":
                return [(name, item) for item in parts]
            return [(name, delimiter.join(parts))]
        if isinstance(value, dict):
            entries = sorted(value.items())
            if style == \"deepObject\":
                return [
                    (f\"{{name}}[{{key}}]\", self._parameter_scalar(item))
                    for key, item in entries
                ]
            if explode and style == \"form\":
                return [
                    (str(key), self._parameter_scalar(item)) for key, item in entries
                ]
            parts = []
            for key, item in entries:
                if explode:
                    parts.append(f\"{{key}}={{self._parameter_scalar(item)}}\")
                else:
                    parts.extend((str(key), self._parameter_scalar(item)))
            return [(name, delimiter.join(parts))]
        return [(name, self._parameter_scalar(value))]

    @staticmethod
    def _encode_query(
        pairs: list[tuple[str, str]],
        allow_reserved: set[int],
    ) -> str:
        reserved = \":/?#[]@!$&'()*+,;=\"
        encoded = []
        for index, (key, value) in enumerate(pairs):
            safe = reserved if index in allow_reserved else \"\"
            encoded.append(
                urllib.parse.quote(str(key), safe=\"\")
                + \"=\"
                + urllib.parse.quote(str(value), safe=safe)
            )
        return \"&\".join(encoded)

    def _encode_body(
        self,
        body: Optional[Any],
        body_encoding: str,
        content_type: str,
    ) -> tuple[Optional[bytes], str]:
        if body is None:
            return None, content_type
        if body_encoding == \"binary\":
            if isinstance(body, bytes):
                return body, content_type
            if isinstance(body, bytearray):
                return bytes(body), content_type
            raise TypeError(\"binary request bodies must be bytes or bytearray\")
        if body_encoding == \"text\":
            return str(body).encode(), content_type
        value = self._body_value(body, body_encoding)
        if body_encoding == \"json\":
            return json.dumps(value).encode(), content_type
        if body_encoding == \"form\":
            encoded = urllib.parse.urlencode(value, doseq=True).encode()
            return encoded, content_type
        if body_encoding == \"multipart\":
            boundary = f\"gnr8-{{secrets.token_hex(16)}}\"
            return (
                self._encode_multipart(value, boundary),
                f\"multipart/form-data; boundary={{boundary}}\",
            )
        raise ValueError(f\"unsupported request body encoding: {{body_encoding}}\")

    def _encode_multipart(self, value: Any, boundary: str) -> bytes:
        if not isinstance(value, dict):
            raise TypeError(\"multipart request bodies must encode to a dict\")
        out = bytearray()
        for key, item in value.items():
            if item is None:
                continue
            items = item if isinstance(item, (list, tuple)) else (item,)
            for part in items:
                if part is None:
                    continue
                out.extend(f\"--{{boundary}}\\r\\n\".encode())
                if isinstance(part, (bytes, bytearray)):
                    out.extend(
                        (
                            f'Content-Disposition: form-data; name=\"{{key}}\"; '
                            f'filename=\"{{key}}\"\\r\\n'
                            \"Content-Type: application/octet-stream\\r\\n\\r\\n\"
                        ).encode()
                    )
                    out.extend(bytes(part))
                    out.extend(b\"\\r\\n\")
                else:
                    out.extend(
                        f'Content-Disposition: form-data; name=\"{{key}}\"\\r\\n\\r\\n'.encode()
                    )
                    out.extend(str(part).encode())
                    out.extend(b\"\\r\\n\")
        out.extend(f\"--{{boundary}}--\\r\\n\".encode())
        return bytes(out)

    def _do(
        self,
        method: str,
        path: str,
        *,
        body: Optional[Any] = None,
        request_headers: Optional[dict[str, str]] = None,
{auth_headers_arg}{auth_bearer_arg}{auth_basic_arg}        operation_id: str,
        path_template: str,
        content_type: str = \"application/json\",
        body_encoding: str = \"json\",
        request_options: Optional[RequestOptions] = None,
        idempotent: bool = False,
        idempotency_key_header: str = \"Idempotency-Key\",
    ) -> tuple:
        data, content_type = self._encode_body(body, body_encoding, content_type)
        options = request_options or RequestOptions()
        timeout = options.timeout if options.timeout is not None else self._timeout
        if options.max_retries is not None:
            max_retries = options.max_retries
        else:
            max_retries = self._max_retries
        if max_retries < 0:
            max_retries = 0
        if not (
            self._retry_unsafe_methods
            or idempotent
            or method in (\"GET\", \"HEAD\", \"OPTIONS\", \"PUT\", \"DELETE\")
        ):
            max_retries = 0
        headers: dict[str, str] = dict(request_headers or {{}})
        if data is not None:
            headers[\"Content-Type\"] = content_type
{auth_loop}        if idempotent and options.idempotency_key:
            headers[idempotency_key_header] = options.idempotency_key
        url = self._base_url + path
        last_error: Optional[BaseException] = None
        for attempt in range(max_retries + 1):
            req = urllib.request.Request(url, data=data, method=method)
            for key, value in headers.items():
                req.add_header(key, value)
            context = HookContext(
                operation_id=operation_id,
                method=method,
                path_template=path_template,
                url=url,
                headers=dict(headers),
                request_metadata=dict(options.metadata),
            )
            try:
                for hook in self._hooks.request:
                    hook(context, req)
                try:
                    with self._opener.open(req, timeout=timeout) as resp:
                        status = resp.status
                        response_headers = dict(resp.headers.items())
                        raw = resp.read()
                except urllib.error.HTTPError as e:
                    status = e.code
                    response_headers = dict(e.headers.items())
                    raw = e.read()
                context.status = status
                context.response_headers = response_headers
                for hook in self._hooks.response:
                    hook(context)
                if self._should_retry_status(status) and attempt < max_retries:
                    self._sleep_retry_after(response_headers)
                    continue
                if status < 200 or status >= 300:
                    self._call_error_hooks(
                        context,
                        ApiError(
                            status,
                            \"\",
                            \"\",
                            headers=response_headers,
                            raw_body=raw,
                        ),
                    )
                return status, response_headers, raw
            except urllib.error.URLError as e:
                last_error = e
                if attempt < max_retries:
                    continue
                self._call_error_hooks(context, e)
                raise
        if last_error is not None:
            raise last_error
        raise RuntimeError(\"request failed without response\")

    def _should_retry_status(self, status: int) -> bool:
        return status in self._retry_statuses or status >= 500

    @staticmethod
    def _sleep_retry_after(headers: dict[str, str]) -> None:
        retry_after = headers.get(\"Retry-After\") or headers.get(\"retry-after\")
        if not retry_after:
            return
        try:
            seconds = int(retry_after)
        except ValueError:
            return
        if seconds > 0:
            time.sleep(seconds)

    def _call_error_hooks(self, context: HookContext, error: BaseException) -> None:
        for hook in self._hooks.error:
            hook(context, error)

    @staticmethod
    def _raise(
        status: int,
        headers: dict[str, str],
        raw: bytes,
        error_model: Optional[type] = None,
    ) -> None:
        try:
            json_body = json.loads(raw) if raw else None
        except ValueError:
            json_body = None
        body = json_body
        if error_model is not None and isinstance(json_body, dict):
            try:
                body = error_model.from_dict(json_body)
            except Exception:
                body = json_body
        decoded = json_body if isinstance(json_body, dict) else {{}}
        request_id = headers.get(\"X-Request-ID\") or headers.get(\"x-request-id\", \"\")
        raise ApiError(
            status,
            decoded.get(\"message\", \"\"),
            decoded.get(\"slug\", \"\"),
            decoded.get(\"hints\"),
            headers=headers,
            request_id=request_id,
            raw_body=raw,
            json_body=json_body,
            body=body,
        )
"
    )
}

/// Emit `client.py`'s operation methods (appended to the client file by [`generate`]).
///
/// `ops` are all of the graph's operations, in graph order. Each method:
/// - takes `self`, then path params as positional args, then a typed `body` arg for body-bearing ops,
///   then optional query params (each defaulting to `None`);
/// - interpolates each path param through `urllib.parse.quote(str(value), safe="")` (V5 path-injection
///   mitigation — twin of Go `url.PathEscape`); builds the query with `urllib.parse.urlencode` over the
///   present optional params; joins `base_path` + `op.path`;
/// - calls `self._do`, raises `ApiError` via `self._raise` for non-2xx responses, and decodes JSON only
///   for success statuses that declare a body model.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling body/response `$ref`, or a path whose templated tokens do
/// not match its declared path params.
#[cfg(test)]
pub(crate) fn emit_operations(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    ops: &[&Operation],
) -> Result<String, CoreError> {
    emit_operations_with_style(graph, package, base_path, ops, PyModelStyle::default())
}

pub(crate) fn emit_operations_with_style(
    graph: &ApiGraph,
    _package: &str,
    base_path: &str,
    ops: &[&Operation],
    model_style: PyModelStyle,
) -> Result<String, CoreError> {
    let mut out = String::new();
    for op in ops {
        out.push('\n');
        emit_operation(&mut out, op, graph, base_path, model_style)?;
        emit_pagination_helpers(&mut out, op, graph, model_style)?;
    }
    Ok(out)
}

pub(crate) fn pagination_method_names(graph: &ApiGraph, op: &Operation) -> Vec<String> {
    if pagination_policy_for(graph, op).is_none() {
        return Vec::new();
    }
    let method = operation_method_name(op);
    vec![format!("{method}_pages"), format!("iter_{method}")]
}

/// The keyword/digit-safe, collision-checked Python identifiers for one operation's arguments.
///
/// Each `*_idents` vector aligns positionally with its params vector. Path params and required query
/// params are positional (no default); optional query params keep `= None` (WR-01 ordering).
struct ResolvedArgs<'op> {
    path_idents: Vec<String>,
    required_query: Vec<&'op Param>,
    required_query_idents: Vec<String>,
    optional_query: Vec<&'op Param>,
    optional_query_idents: Vec<String>,
}

/// Resolve + collision-check every operation argument's Python identifier (CR-02 / WR-03 / WR-01).
///
/// Each identifier is keyword/digit-safe ([`safe_ident`]); the set is tracked as it grows so a collision
/// (two params whose safe identifier matches, or a param colliding with the bound `self`/`body`) is a
/// typed [`CoreError::SdkGen`] rather than a `SyntaxError: duplicate argument`. Query params are split
/// required-first (positional) / optional-last (`= None`) so all non-defaulted args precede defaulted
/// ones (valid Python). One deterministic pass, no fallback (rule 3).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on an argument-identifier collision.
fn resolve_op_args<'op>(
    op: &Operation,
    path_params: &[&'op Param],
    query_params: &[&'op Param],
    has_body: bool,
) -> Result<ResolvedArgs<'op>, CoreError> {
    // Seed the reserved set: `self` always; `body` additionally when a typed body is bound (a param
    // named `body` is otherwise allowed since nothing binds it).
    let mut used_args: Vec<String> = RESERVED_ARGS.iter().map(|s| (*s).to_string()).collect();
    if !has_body {
        used_args.retain(|a| a != "body");
    }
    let mut reserve = |name: &str| -> Result<String, CoreError> {
        let ident = safe_ident(&snake(name));
        if used_args.contains(&ident) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter whose Python identifier '{ident}' collides \
                     with another argument (self/body or another param)",
                    op.id
                ),
            });
        }
        used_args.push(ident.clone());
        Ok(ident)
    };

    let mut path_idents: Vec<String> = Vec::with_capacity(path_params.len());
    for p in path_params {
        path_idents.push(reserve(&p.name)?);
    }
    // Required query params are positional (WR-01: a required query param MUST be supplied); optional
    // ones keep the `= None` default.
    let (required_query, optional_query): (Vec<&Param>, Vec<&Param>) =
        query_params.iter().copied().partition(|p| p.required);
    let mut required_query_idents: Vec<String> = Vec::with_capacity(required_query.len());
    for p in &required_query {
        required_query_idents.push(reserve(&p.name)?);
    }
    let mut optional_query_idents: Vec<String> = Vec::with_capacity(optional_query.len());
    for p in &optional_query {
        optional_query_idents.push(reserve(&p.name)?);
    }

    Ok(ResolvedArgs {
        path_idents,
        required_query,
        required_query_idents,
        optional_query,
        optional_query_idents,
    })
}

/// Render a method `def` header at 4-space (Client-method) indent, wrapping to one-argument-per-line
/// when the single-line form would exceed the 88-column limit — matching `ruff format` so the emitted
/// source is already format-stable (CLAUDE.md rule 2: no formatter dependency). When exploded, each
/// argument sits at 8-space indent with a trailing comma (the "magic trailing comma" that keeps the
/// formatter from re-collapsing it).
fn method_def(name: &str, args: &[String], ret: &str) -> String {
    let one_line = format!("    def {name}({}) -> {ret}:", args.join(", "));
    // Compare display columns (chars), not UTF-8 bytes — ruff's line-length is a column count, so a
    // non-ASCII identifier/hint must not trip a spurious wrap.
    if one_line.chars().count() <= 88 {
        return one_line;
    }
    let mut out = format!("    def {name}(\n");
    for arg in args {
        let _ = writeln!(out, "        {arg},");
    }
    let _ = write!(out, "    ) -> {ret}:");
    out
}

/// Emit a single operation method (4-space indented as a `Client` method body).
#[allow(clippy::too_many_lines)]
fn emit_operation(
    out: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    model_style: PyModelStyle,
) -> Result<(), CoreError> {
    let method_name = snake(&op.handler);
    let abs = join_path(base_path, &op.path);
    let tokens = path_tokens(&abs);

    let path_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "path").collect();
    let query_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "query").collect();
    let request_params: Vec<&Param> = op.params.iter().filter(|p| p.location != "path").collect();

    // The templated path tokens must be exactly the declared path params (order-independent set
    // equality), so neither a dangling token (a KeyError at runtime) nor an unused arg can slip through
    // (twin of WR-03). `param_set` is built sorted for a stable error message.
    let mut param_set: Vec<&str> = path_params.iter().map(|p| p.name.as_str()).collect();
    param_set.sort_unstable();
    if !path_tokens_match(&tokens, &param_set) {
        return Err(CoreError::SdkGen {
            message: format!(
                "operation '{}' path '{}' templated tokens {:?} do not match its path params {:?}",
                op.id, abs, tokens, param_set
            ),
        });
    }

    let body_model = request_body_model_of(op, graph)?;
    let success = success_responses_of(op, graph)?;
    let error_bodies = error_response_bodies_of(op, graph)?;
    let auth_headers = operation_api_key_headers(graph, op)?;
    let auth_queries = operation_api_key_queries(graph, op)?;
    let auth_http = operation_http_auth_schemes(graph, op)?;
    let return_model = success.body_model.clone();
    let return_hint = if success.has_binary_body() {
        if success.has_bodyless_alternative() {
            "Optional[bytes]".to_string()
        } else {
            "bytes".to_string()
        }
    } else {
        return_model.as_ref().map_or_else(
            || "Any".to_string(),
            |model| {
                if success.has_bodyless_alternative() {
                    format!("Optional[{model}]")
                } else {
                    model.clone()
                }
            },
        )
    };

    // Resolve every emitted argument's keyword/digit-safe Python identifier ONCE, collision-checked
    // against `self`/`body` and each other (CR-02 / WR-03 / WR-01 ordering); see [`resolve_op_args`].
    let ResolvedArgs {
        path_idents,
        required_query,
        required_query_idents,
        optional_query,
        optional_query_idents,
    } = resolve_op_args(op, &path_params, &request_params, body_model.is_some())?;

    // Signature: self, path params (positional), required body when present, required query
    // (positional), optional body when present, then optional query params (= None). This preserves
    // the established required-body surface while keeping defaulted args after all required args.
    let mut args: Vec<String> = vec!["self".to_string()];
    args.extend(path_idents.iter().cloned());
    if let Some(body) = body_model.as_ref().filter(|body| body.required) {
        args.push(format!("body: {}", body.model));
    }
    for (param, ident) in required_query.iter().zip(required_query_idents.iter()) {
        args.push(format!(
            "{ident}: {}",
            py_type(&param.schema, false, graph)?
        ));
    }
    if let Some(body) = body_model.as_ref().filter(|body| !body.required) {
        args.push(format!("body: Optional[{}] = None", body.model));
    }
    for (param, ident) in optional_query.iter().zip(optional_query_idents.iter()) {
        args.push(format!(
            "{ident}: Optional[{}] = None",
            py_type(&param.schema, false, graph)?
        ));
    }
    args.push("request_options: Optional[RequestOptions] = None".to_string());

    writeln!(out, "{}", method_def(&method_name, &args, &return_hint)).map_err(sink)?;

    // Build the path: f-string interpolation with each path param percent-escaped (V5). The token order
    // matches path_params order (set-equality was already asserted), so path_idents aligns by position.
    if tokens.is_empty() {
        writeln!(out, "        path = \"{abs}\"").map_err(sink)?;
    } else {
        let mut fstring = abs.clone();
        for token in &tokens {
            // The f-string interpolates the SAFE local identifier (WR-04), but the {token} placeholder
            // in the path template is the original wire token. Tokens and path params are set-equal (not
            // necessarily same order), so resolve this token's identifier by matching the path-param
            // whose name equals the token (path_idents[i] corresponds to path_params[i]).
            let ident = path_params
                .iter()
                .zip(path_idents.iter())
                .find(|(pp, _)| &pp.name == token)
                .map_or_else(|| safe_ident(&snake(token)), |(_, id)| id.clone());
            let placeholder = format!("{{{token}}}");
            // `safe=''` uses SINGLE quotes inside the double-quoted f-string: a backslash in an
            // f-string expression part is a `SyntaxError` on Python 3.9-3.11 ("f-string expression
            // part cannot include a backslash"), so escaped double-quotes (`safe=\"\"`) would not
            // compile. Single quotes need no escape and are valid on every Python 3.x (PYSDK-02).
            let escaped = format!("{{urllib.parse.quote(str({ident}), safe='')}}");
            fstring = fstring.replace(&placeholder, &escaped);
        }
        writeln!(out, "        path = f\"{fstring}\"").map_err(sink)?;
    }

    // Query encoding (WR-01 + WR-04): a REQUIRED query param is always sent (it is a positional arg, no
    // None guard); an OPTIONAL one is included only when present. The local read is the SAFE identifier;
    // the wire key stays the ORIGINAL `p.name`. Track allowReserved by emitted-pair index rather than
    // by name so exploded object keys retain the originating parameter's policy without accidentally
    // granting that policy to another parameter with the same wire key.
    if !query_params.is_empty() || !auth_queries.is_empty() {
        writeln!(out, "        _query: list[tuple[str, str]] = []").map_err(sink)?;
        writeln!(out, "        _allow_reserved: set[int] = set()").map_err(sink)?;
        for (p, ident) in required_query.iter().zip(required_query_idents.iter()) {
            if p.location == "query" {
                emit_py_query_parameter(out, p, ident, "        ")?;
            }
        }
        for (p, ident) in optional_query.iter().zip(optional_query_idents.iter()) {
            if p.location != "query" {
                continue;
            }
            writeln!(out, "        if {ident} is not None:").map_err(sink)?;
            emit_py_query_parameter(out, p, ident, "            ")?;
        }
        for query in &auth_queries {
            writeln!(
                out,
                "        _auth_query_{} = self._api_keys.get({}) or self._api_key",
                safe_ident(&snake(query)),
                quoted_string_literal(query)
            )
            .map_err(sink)?;
            writeln!(out, "        if _auth_query_{}:", safe_ident(&snake(query))).map_err(sink)?;
            writeln!(
                out,
                "            _query.append(({}, _auth_query_{}))",
                quoted_string_literal(query),
                safe_ident(&snake(query))
            )
            .map_err(sink)?;
        }
        writeln!(out, "        if _query:").map_err(sink)?;
        writeln!(
            out,
            "            path = path + \"?\" + self._encode_query(_query, _allow_reserved)"
        )
        .map_err(sink)?;
    }

    let has_request_headers = request_params
        .iter()
        .any(|param| param.location == "header" || param.location == "cookie");
    if has_request_headers {
        emit_py_header_cookie_parameters(
            out,
            &required_query,
            &required_query_idents,
            &optional_query,
            &optional_query_idents,
        )?;
    }

    // Dispatch: call _do, reject non-2xx responses, and decode only statuses with a declared body.
    let mut do_args = vec![quoted_string_literal(&op.method), "path".to_string()];
    if let Some(body) = body_model.as_ref() {
        do_args.push("body=body".to_string());
        do_args.push(format!(
            "content_type={}",
            quoted_string_literal(&body.content_type)
        ));
        do_args.push(format!(
            "body_encoding={}",
            quoted_string_literal(py_body_encoding(body.encoding))
        ));
    }
    if has_request_headers {
        do_args.push("request_headers=_request_headers".to_string());
    }
    if !auth_headers.is_empty() {
        do_args.push(format!("auth_headers={}", py_string_tuple(&auth_headers)));
    }
    if auth_http.contains(&HttpAuthScheme::Bearer) {
        do_args.push("auth_bearer=True".to_string());
    }
    if auth_http.contains(&HttpAuthScheme::Basic) {
        do_args.push("auth_basic=True".to_string());
    }
    let runtime = py_operation_runtime(graph, op);
    do_args.push(format!("operation_id={}", quoted_string_literal(&op.id)));
    do_args.push(format!("path_template={}", quoted_string_literal(&op.path)));
    do_args.push("request_options=request_options".to_string());
    do_args.push(format!("idempotent={}", py_bool(runtime.idempotent)));
    do_args.push(format!(
        "idempotency_key_header={}",
        quoted_string_literal(runtime.idempotency_key_header.unwrap_or("Idempotency-Key")),
    ));
    writeln!(out, "        _status, _headers, _raw = self._do(").map_err(sink)?;
    for arg in do_args {
        writeln!(out, "            {arg},").map_err(sink)?;
    }
    writeln!(out, "        )").map_err(sink)?;
    writeln!(out, "        if _status < 200 or _status >= 300:").map_err(sink)?;
    for error_body in &error_bodies {
        writeln!(out, "            if _status == {}:", error_body.status).map_err(sink)?;
        writeln!(
            out,
            "                self._raise(_status, _headers, _raw, {})",
            error_body.model
        )
        .map_err(sink)?;
    }
    writeln!(out, "            self._raise(_status, _headers, _raw)").map_err(sink)?;
    if success.has_binary_body() {
        writeln!(
            out,
            "        if _status in {}:",
            py_status_tuple(&success.binary_statuses)
        )
        .map_err(sink)?;
        writeln!(out, "            return _raw").map_err(sink)?;
        if success.has_bodyless_alternative() {
            writeln!(out, "        return None").map_err(sink)?;
        } else {
            writeln!(out, "        self._raise(_status, _headers, _raw)").map_err(sink)?;
        }
    } else if let Some(model) = &return_model {
        writeln!(
            out,
            "        if _status in {}:",
            py_status_tuple(&success.body_statuses)
        )
        .map_err(sink)?;
        writeln!(
            out,
            "            _data = json.loads(_raw) if _raw else {{}}"
        )
        .map_err(sink)?;
        match model_style {
            PyModelStyle::Pydantic => {
                writeln!(out, "            return {model}.model_validate(_data)").map_err(sink)?;
            }
            PyModelStyle::Dataclass => {
                writeln!(out, "            return {model}.from_dict(_data)").map_err(sink)?;
            }
        }
        if success.has_bodyless_alternative() {
            writeln!(out, "        return None").map_err(sink)?;
        } else {
            writeln!(out, "        self._raise(_status, _headers, _raw)").map_err(sink)?;
        }
    } else {
        writeln!(out, "        return json.loads(_raw) if _raw else None").map_err(sink)?;
    }
    Ok(())
}

fn py_parameter_style(param: &Param) -> &str {
    param
        .style
        .as_deref()
        .unwrap_or(match param.location.as_str() {
            "header" => "simple",
            _ => "form",
        })
}

fn py_parameter_explode(param: &Param) -> bool {
    param
        .explode
        .unwrap_or_else(|| py_parameter_style(param) == "form")
}

fn emit_py_query_parameter(
    out: &mut String,
    param: &Param,
    ident: &str,
    padding: &str,
) -> Result<(), CoreError> {
    let name = quoted_string_literal(&param.name);
    let style = quoted_string_literal(py_parameter_style(param));
    let explode = py_bool(py_parameter_explode(param));
    if param.allow_reserved {
        writeln!(
            out,
            "{padding}_pairs = self._parameter_pairs({name}, {ident}, {style}, {explode})"
        )
        .map_err(sink)?;
        writeln!(
            out,
            "{padding}_allow_reserved.update(range(len(_query), len(_query) + len(_pairs)))"
        )
        .map_err(sink)?;
        writeln!(out, "{padding}_query.extend(_pairs)").map_err(sink)?;
    } else {
        writeln!(
            out,
            "{padding}_query.extend(self._parameter_pairs({name}, {ident}, {style}, {explode}))"
        )
        .map_err(sink)?;
    }
    Ok(())
}

fn emit_py_header_cookie_parameters(
    out: &mut String,
    required_params: &[&Param],
    required_idents: &[String],
    optional_params: &[&Param],
    optional_idents: &[String],
) -> Result<(), CoreError> {
    writeln!(out, "        _request_headers: dict[str, str] = {{}}").map_err(sink)?;
    let has_cookie = required_params
        .iter()
        .chain(optional_params.iter())
        .any(|param| param.location == "cookie");
    if has_cookie {
        writeln!(out, "        _cookie_parts: list[str] = []").map_err(sink)?;
    }
    for (param, ident) in required_params.iter().zip(required_idents.iter()) {
        if param.location == "header" || param.location == "cookie" {
            emit_py_header_cookie_parameter(out, param, ident, "        ")?;
        }
    }
    for (param, ident) in optional_params.iter().zip(optional_idents.iter()) {
        if param.location != "header" && param.location != "cookie" {
            continue;
        }
        writeln!(out, "        if {ident} is not None:").map_err(sink)?;
        emit_py_header_cookie_parameter(out, param, ident, "            ")?;
    }
    if has_cookie {
        writeln!(out, "        if _cookie_parts:").map_err(sink)?;
        writeln!(
            out,
            "            _request_headers[\"Cookie\"] = \"; \".join(_cookie_parts)"
        )
        .map_err(sink)?;
    }
    Ok(())
}

fn emit_py_header_cookie_parameter(
    out: &mut String,
    param: &Param,
    ident: &str,
    padding: &str,
) -> Result<(), CoreError> {
    writeln!(
        out,
        "{padding}for _wire_name, _wire_value in self._parameter_pairs({}, {ident}, {}, {}):",
        quoted_string_literal(&param.name),
        quoted_string_literal(py_parameter_style(param)),
        py_bool(py_parameter_explode(param))
    )
    .map_err(sink)?;
    if param.location == "header" {
        writeln!(out, "{padding}    if _wire_name in _request_headers:").map_err(sink)?;
        writeln!(
            out,
            "{padding}        _request_headers[_wire_name] += \",\" + _wire_value"
        )
        .map_err(sink)?;
        writeln!(out, "{padding}    else:").map_err(sink)?;
        writeln!(
            out,
            "{padding}        _request_headers[_wire_name] = _wire_value"
        )
        .map_err(sink)?;
    } else {
        writeln!(
            out,
            "{padding}    _cookie_parts.append(urllib.parse.quote(_wire_name, safe=\"\") + \"=\" + urllib.parse.quote(_wire_value, safe=\"\"))"
        )
        .map_err(sink)?;
    }
    Ok(())
}

struct PyPaginationInfo {
    page_model: String,
    item_type: String,
    items_ident: String,
    next_cursor_ident: Option<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "Python pagination helper emission writes page and item helpers in one deterministic source block"
)]
fn emit_pagination_helpers(
    out: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    model_style: PyModelStyle,
) -> Result<(), CoreError> {
    let Some(policy) = pagination_policy_for(graph, op) else {
        return Ok(());
    };
    let method_name = operation_method_name(op);
    let pages_name = format!("{method_name}_pages");
    let items_name = format!("iter_{method_name}");
    let info = py_pagination_info(graph, op, policy, model_style)?;
    let (args, call_args) = py_pagination_args(op, graph)?;

    writeln!(out).map_err(sink)?;
    writeln!(
        out,
        "{}",
        method_def(
            &pages_name,
            &args,
            &format!("Iterator[{}]", info.page_model)
        )
    )
    .map_err(sink)?;
    emit_pagination_initialization(out, policy)?;
    writeln!(out, "        while True:").map_err(sink)?;
    writeln!(
        out,
        "            _page = self.{method_name}({})",
        call_args.join(", ")
    )
    .map_err(sink)?;
    writeln!(out, "            _items = _page.{} or []", info.items_ident).map_err(sink)?;
    if policy.termination == PaginationTermination::EmptyItems {
        writeln!(out, "            if not _items:").map_err(sink)?;
        writeln!(out, "                break").map_err(sink)?;
    }
    writeln!(out, "            yield _page").map_err(sink)?;
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
            let next_ident = info.next_cursor_ident.as_deref().ok_or_else(|| {
                CoreError::SdkGen {
                    message: format!(
                        "pagination policy for operation '{}' is cursor mode without next_cursor_field",
                        op.id
                    ),
                }
            })?;
            let cursor_ident = py_query_ident(op, cursor_param)?;
            writeln!(out, "            _next_cursor = _page.{next_ident}").map_err(sink)?;
            writeln!(out, "            if not _next_cursor:").map_err(sink)?;
            writeln!(out, "                break").map_err(sink)?;
            writeln!(out, "            {cursor_ident} = _next_cursor").map_err(sink)?;
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
            let page_ident = py_query_ident(op, page_param)?;
            writeln!(out, "            {page_ident} += 1").map_err(sink)?;
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
            let offset_ident = py_query_ident(op, offset_param)?;
            writeln!(out, "            {offset_ident} += len(_items)").map_err(sink)?;
        }
    }

    writeln!(out).map_err(sink)?;
    writeln!(
        out,
        "{}",
        method_def(&items_name, &args, &format!("Iterator[{}]", info.item_type))
    )
    .map_err(sink)?;
    writeln!(
        out,
        "        for _page in self.{pages_name}({}):",
        call_args.join(", ")
    )
    .map_err(sink)?;
    writeln!(
        out,
        "            for _item in _page.{} or []:",
        info.items_ident
    )
    .map_err(sink)?;
    writeln!(out, "                yield _item").map_err(sink)?;
    Ok(())
}

fn emit_pagination_initialization(
    out: &mut String,
    policy: &PaginationPolicy,
) -> Result<(), CoreError> {
    match policy.mode {
        PaginationMode::Cursor => {}
        PaginationMode::Page => {
            let Some(page_param) = policy.page_param.as_deref() else {
                return Ok(());
            };
            let ident = safe_ident(&snake(page_param));
            writeln!(out, "        if {ident} is None:").map_err(sink)?;
            writeln!(out, "            {ident} = 1").map_err(sink)?;
        }
        PaginationMode::Offset => {
            let Some(offset_param) = policy.offset_param.as_deref() else {
                return Ok(());
            };
            let ident = safe_ident(&snake(offset_param));
            writeln!(out, "        if {ident} is None:").map_err(sink)?;
            writeln!(out, "            {ident} = 0").map_err(sink)?;
        }
    }
    Ok(())
}

fn py_pagination_args(
    op: &Operation,
    graph: &ApiGraph,
) -> Result<(Vec<String>, Vec<String>), CoreError> {
    let path_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "path").collect();
    let request_params: Vec<&Param> = op.params.iter().filter(|p| p.location != "path").collect();
    let body_model = request_body_model_of(op, graph)?;
    let ResolvedArgs {
        path_idents,
        required_query,
        required_query_idents,
        optional_query,
        optional_query_idents,
    } = resolve_op_args(op, &path_params, &request_params, body_model.is_some())?;

    let mut args: Vec<String> = vec!["self".to_string()];
    let mut call_args: Vec<String> = Vec::new();
    for ident in &path_idents {
        args.push(ident.clone());
        call_args.push(format!("{ident}={ident}"));
    }
    if let Some(body) = body_model.as_ref().filter(|body| body.required) {
        args.push(format!("body: {}", body.model));
        call_args.push("body=body".to_string());
    }
    for (param, ident) in required_query.iter().zip(required_query_idents.iter()) {
        let hint = py_type(&param.schema, false, graph)?;
        args.push(format!("{ident}: {hint}"));
        call_args.push(format!("{ident}={ident}"));
    }
    if let Some(body) = body_model.as_ref().filter(|body| !body.required) {
        args.push(format!("body: Optional[{}] = None", body.model));
        call_args.push("body=body".to_string());
    }
    for (param, ident) in optional_query.iter().zip(optional_query_idents.iter()) {
        let hint = py_type(&param.schema, false, graph)?;
        args.push(format!("{ident}: Optional[{hint}] = None"));
        call_args.push(format!("{ident}={ident}"));
    }
    args.push("request_options: Optional[RequestOptions] = None".to_string());
    call_args.push("request_options=request_options".to_string());
    Ok((args, call_args))
}

fn py_pagination_info(
    graph: &ApiGraph,
    op: &Operation,
    policy: &PaginationPolicy,
    model_style: PyModelStyle,
) -> Result<PyPaginationInfo, CoreError> {
    validate_numeric_pagination_params(op, policy)?;
    let success = success_responses_of(op, graph)?;
    let page_model = success.body_model.ok_or_else(|| CoreError::SdkGen {
        message: format!(
            "pagination policy for operation '{}' requires a JSON success response model",
            op.id
        ),
    })?;
    let schema = graph
        .schemas
        .iter()
        .find(|schema| schema.name == page_model)
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' references missing response model '{}'",
                op.id, page_model
            ),
        })?;
    let Type::Object(fields) = &schema.body else {
        return Err(CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' requires object response model '{}'",
                op.id, page_model
            ),
        });
    };
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
    let next_cursor_ident = if let Some(next_cursor) = policy.next_cursor_field.as_deref() {
        let field = fields
            .iter()
            .find(|field| field.json_name == next_cursor)
            .ok_or_else(|| CoreError::SdkGen {
                message: format!(
                    "pagination policy for operation '{}' references missing next cursor field '{}'",
                    op.id, next_cursor
                ),
            })?;
        Some(py_field_ident(field, model_style))
    } else {
        None
    };
    Ok(PyPaginationInfo {
        page_model,
        item_type: py_type(item_schema, false, graph)?,
        items_ident: py_field_ident(items, model_style),
        next_cursor_ident,
    })
}

fn py_field_ident(field: &Field, model_style: PyModelStyle) -> String {
    match model_style {
        PyModelStyle::Pydantic => pydantic_field_ident(field),
        PyModelStyle::Dataclass => safe_ident(&field.json_name),
    }
}

fn pagination_policy_for<'a>(graph: &'a ApiGraph, op: &Operation) -> Option<&'a PaginationPolicy> {
    graph
        .pagination
        .iter()
        .find(|policy| policy.operation_id == op.id)
}

fn py_query_ident(op: &Operation, param_name: &str) -> Result<String, CoreError> {
    op.params
        .iter()
        .find(|param| param.location == "query" && param.name == param_name)
        .map(|param| safe_ident(&snake(&param.name)))
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "pagination policy for operation '{}' references missing query parameter '{}'",
                op.id, param_name
            ),
        })
}

fn validate_numeric_pagination_params(
    op: &Operation,
    policy: &PaginationPolicy,
) -> Result<(), CoreError> {
    for param_name in [policy.page_param.as_deref(), policy.offset_param.as_deref()]
        .into_iter()
        .flatten()
    {
        let param = op
            .params
            .iter()
            .find(|param| param.location == "query" && param.name == param_name)
            .ok_or_else(|| CoreError::SdkGen {
                message: format!(
                    "pagination policy for operation '{}' references missing query parameter '{}'",
                    op.id, param_name
                ),
            })?;
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

fn py_status_tuple(statuses: &[u16]) -> String {
    match statuses {
        [] => "()".to_string(),
        [single] => format!("({single},)"),
        many => {
            let joined = many
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({joined})")
        }
    }
}

fn py_string_tuple(values: &[String]) -> String {
    match values {
        [] => "()".to_string(),
        [single] => format!("({},)", quoted_string_literal(single)),
        many => {
            let joined = many
                .iter()
                .map(|value| quoted_string_literal(value))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({joined})")
        }
    }
}

/// Emit `__init__.py`: re-export `Client`, `ApiError`, and every model/enum class so `import <pkg>`
/// exposes the whole surface. Class names are emitted in graph order (deterministic). Twin of the Go
/// twin's single-package surface (Go has no `__init__`, so this is Python-specific but deterministic).
#[cfg(test)]
pub(crate) fn emit_init(graph: &ApiGraph, package: &str) -> String {
    emit_init_with_models(graph, package, "models")
}

/// Emit `__init__.py` with a configurable model package import path.
pub(crate) fn emit_init_with_models(
    graph: &ApiGraph,
    _package: &str,
    model_module: &str,
) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n\n");
    out.push_str("from .client import Client, ClientHooks, HookContext, RequestOptions\n");
    out.push_str("from .errors import ApiError\n");

    // Every named schema becomes a top-level symbol in models.py (class or alias) — re-export them all.
    let names: Vec<&str> = graph.schemas.iter().map(|s| s.name.as_str()).collect();
    if !names.is_empty() {
        let _ = writeln!(out, "from .{model_module} import (");
        for name in &names {
            let _ = writeln!(out, "    {name},");
        }
        out.push_str(")\n");
    }

    out.push_str("\n__all__ = [\n");
    out.push_str("    \"Client\",\n");
    out.push_str("    \"ClientHooks\",\n");
    out.push_str("    \"HookContext\",\n");
    out.push_str("    \"RequestOptions\",\n");
    out.push_str("    \"ApiError\",\n");
    for name in &names {
        let _ = writeln!(out, "    \"{name}\",");
    }
    out.push_str("]\n");
    out
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        emit_client, emit_client_with_models, emit_errors, emit_init, emit_models,
        emit_models_with_style, emit_operations, py_type, screaming_snake, snake,
    };
    use crate::graph::{ApiGraph, Operation, Prim, Type};
    use crate::sdk::model_style::PyModelStyle;

    /// A facts document covering the FastApi-bookstore shapes that diverge from the Go target: a named
    /// enum (`BookFormat`), a named union (`BookOrError`), an inline union field (`Book.rating:
    /// Optional[Union[int, float]]`), an inline enum field (`BookFilters.sort: Literal["asc","desc"]`),
    /// plus required/optional/nullable mixes to prove required-first dataclass ordering.
    const SAMPLE: &[u8] = br#"{
      "module": "app",
      "routes": [],
      "schemas": [
        {
          "id": "app.models.Author", "name": "Author",
          "body": { "type": "object", "of": [
            { "json_name": "name", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "app.models.Book", "name": "Book",
          "body": { "type": "object", "of": [
            { "json_name": "author", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "named", "of": "app.models.Author" },
              "description": null, "example": null },
            { "json_name": "rating", "required": false, "optional": true, "nullable": true,
              "schema": { "type": "union", "of": [
                { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                { "type": "primitive", "of": { "prim": "float", "bits": 64 } }
              ] },
              "description": null, "example": null },
            { "json_name": "title", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.BookFilters", "name": "BookFilters",
          "body": { "type": "object", "of": [
            { "json_name": "genre", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null },
            { "json_name": "in_stock", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "bool" } },
              "description": null, "example": null },
            { "json_name": "published", "required": true, "optional": false, "nullable": true,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null },
            { "json_name": "sort", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "enum", "of": ["asc", "desc"] },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "app.models.BookFormat", "name": "BookFormat",
          "body": { "type": "enum", "of": ["hardcover", "paperback"] },
          "span": { "file": "/root/m.py", "start_line": 4, "end_line": 4 }
        },
        {
          "id": "app.models.BookOrError", "name": "BookOrError",
          "body": { "type": "union", "of": [
            { "type": "named", "of": "app.models.Book" },
            { "type": "named", "of": "app.models.OutOfStock" }
          ] },
          "span": { "file": "/root/m.py", "start_line": 5, "end_line": 5 }
        },
        {
          "id": "app.models.OutOfStock", "name": "OutOfStock",
          "body": { "type": "object", "of": [
            { "json_name": "reason", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 6, "end_line": 6 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    mod casing {
        use super::{screaming_snake, snake};

        #[test]
        fn helpers_produce_python_casings() {
            assert_eq!(snake("createBook"), "create_book");
            assert_eq!(snake("listBooks"), "list_books");
            assert_eq!(screaming_snake("hardcover"), "HARDCOVER");
            assert_eq!(screaming_snake("out-of-stock"), "OUT_OF_STOCK");
        }
    }

    mod type_mapping {
        use super::{py_type, sample_graph, ApiGraph, Prim, Type};

        #[test]
        fn primitives_and_wellknown_map_to_python_scalars() {
            let g = ApiGraph::default();
            assert_eq!(
                py_type(&Type::Primitive(Prim::String), false, &g).unwrap(),
                "str"
            );
            assert_eq!(
                py_type(&Type::Primitive(Prim::Bool), false, &g).unwrap(),
                "bool"
            );
            assert_eq!(
                py_type(
                    &Type::Primitive(Prim::Int {
                        bits: 64,
                        signed: true
                    }),
                    false,
                    &g
                )
                .unwrap(),
                "int"
            );
            assert_eq!(
                py_type(&Type::Primitive(Prim::Float { bits: 64 }), false, &g).unwrap(),
                "float"
            );
            // a date-time well-known carries as a str (A7).
            assert_eq!(
                py_type(
                    &Type::WellKnown(crate::graph::WellKnown::DateTime),
                    false,
                    &g
                )
                .unwrap(),
                "str"
            );
        }

        #[test]
        fn nullable_wraps_the_hint_in_optional() {
            let g = ApiGraph::default();
            assert_eq!(
                py_type(&Type::Primitive(Prim::String), true, &g).unwrap(),
                "Optional[str]"
            );
        }

        #[test]
        fn inline_union_becomes_union_hint_the_go_target_rejects() {
            // Book.rating-shaped inline union: the case go_type errors on. Python emits Union[int, float].
            let g = ApiGraph::default();
            let rating = Type::Union(vec![
                Type::Primitive(Prim::Int {
                    bits: 64,
                    signed: true,
                }),
                Type::Primitive(Prim::Float { bits: 64 }),
            ]);
            assert_eq!(py_type(&rating, false, &g).unwrap(), "Union[int, float]");
            // nullable wraps the whole union.
            assert_eq!(
                py_type(&rating, true, &g).unwrap(),
                "Optional[Union[int, float]]"
            );
        }

        #[test]
        fn inline_enum_becomes_literal_the_go_target_rejects() {
            // BookFilters.sort-shaped inline enum: go_type errors; Python emits Literal in graph order.
            let g = ApiGraph::default();
            let sort = Type::Enum(vec!["asc".to_string(), "desc".to_string()]);
            assert_eq!(
                py_type(&sort, false, &g).unwrap(),
                "Literal[\"asc\", \"desc\"]"
            );
        }

        #[test]
        fn named_ref_resolves_to_the_schema_name() {
            let g = sample_graph();
            let named = Type::Named("app.models.BookFormat".to_string());
            assert_eq!(py_type(&named, false, &g).unwrap(), "BookFormat");
            assert_eq!(py_type(&named, true, &g).unwrap(), "Optional[BookFormat]");
        }

        #[test]
        fn named_union_resolves_each_variant_to_its_class_name() {
            // BookOrError = Union[Book, OutOfStock].
            let g = sample_graph();
            let body = g.schemas.iter().find(|s| s.name == "BookOrError").unwrap();
            assert_eq!(
                py_type(&body.body, false, &g).unwrap(),
                "Union[Book, OutOfStock]"
            );
        }

        #[test]
        fn array_and_map_and_any_map_to_typing_generics() {
            let g = ApiGraph::default();
            let arr = Type::Array(Box::new(Type::Primitive(Prim::String)));
            assert_eq!(py_type(&arr, false, &g).unwrap(), "list[str]");
            let map = Type::Map {
                key: Box::new(Type::Primitive(Prim::String)),
                value: Box::new(Type::Primitive(Prim::String)),
            };
            assert_eq!(py_type(&map, false, &g).unwrap(), "dict[str, str]");
            assert_eq!(py_type(&Type::Any {}, false, &g).unwrap(), "Any");
        }

        #[test]
        fn non_string_map_key_is_a_typed_error() {
            let g = ApiGraph::default();
            let map = Type::Map {
                key: Box::new(Type::Primitive(Prim::Int {
                    bits: 64,
                    signed: true,
                })),
                value: Box::new(Type::Primitive(Prim::String)),
            };
            let err = py_type(&map, false, &g).unwrap_err();
            assert!(
                err.to_string()
                    .contains("cannot be represented as a Python JSON object key"),
                "{err}"
            );
        }

        #[test]
        fn inline_object_is_a_typed_error_parity_with_go() {
            let g = ApiGraph::default();
            let obj = Type::Object(vec![]);
            let err = py_type(&obj, false, &g).unwrap_err();
            assert!(
                err.to_string()
                    .contains("inline object type is unsupported"),
                "{err}"
            );
        }

        #[test]
        fn dangling_named_ref_is_a_typed_error() {
            let g = ApiGraph::default();
            let err = py_type(&Type::Named("dto.Nope".to_string()), false, &g).unwrap_err();
            assert!(err.to_string().contains("dangling $ref"), "{err}");
        }
    }

    mod models {
        use super::{emit_models, emit_models_with_style, sample_graph, PyModelStyle};

        #[test]
        fn named_enum_emits_str_enum_class_with_screaming_snake_members() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(
                out.contains("class BookFormat(str, enum.Enum):"),
                "named enum must be a str enum class:\n{out}"
            );
            assert!(out.contains("    HARDCOVER = \"hardcover\""), "{out}");
            assert!(out.contains("    PAPERBACK = \"paperback\""), "{out}");
            // graph order: hardcover before paperback.
            let h = out.find("HARDCOVER").unwrap();
            let p = out.find("PAPERBACK").unwrap();
            assert!(h < p, "enum members must be in graph order:\n{out}");
        }

        #[test]
        fn dataclass_style_emits_required_fields_before_optional_fields() {
            // BookFilters: genre (required), in_stock (optional), published (required-but-nullable),
            // sort (optional). Alphabetical graph order interleaves defaults; the emitter must put both
            // required fields (genre, published) before both optional ones (in_stock, sort).
            let out = emit_models_with_style(&sample_graph(), "bookstore", PyModelStyle::Dataclass)
                .unwrap();
            let genre = out.find("    genre:").expect("genre field");
            let published = out.find("    published:").expect("published field");
            let in_stock = out.find("    in_stock:").expect("in_stock field");
            let sort = out.find("    sort:").expect("sort field");
            assert!(
                genre < in_stock && genre < sort,
                "required `genre` must precede optionals:\n{out}"
            );
            assert!(
                published < in_stock && published < sort,
                "required-but-nullable `published` must precede optionals:\n{out}"
            );
        }

        #[test]
        fn optional_fields_get_a_none_default_required_do_not() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            // required (no default).
            assert!(
                out.contains("    genre: str\n"),
                "required has no default:\n{out}"
            );
            // optional non-nullable bool → defaulted and widened so generated wrappers can pass None
            // before dumping with exclude_none.
            assert!(
                out.contains("    in_stock: Optional[bool] = Field(default=None)\n"),
                "optional non-nullable field must accept None at wrapper boundaries:\n{out}"
            );
            // required-but-nullable → Optional hint, NO default (it is required/present).
            assert!(
                out.contains("    published: Optional[str]\n"),
                "nullable-required must be Optional without a default:\n{out}"
            );
        }

        #[test]
        fn inline_enum_and_union_fields_use_literal_and_union_hints() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            // BookFilters.sort inline enum, optional + non-nullable → defaulted and widened for
            // wrapper-boundary None values.
            assert!(
                out.contains(
                    "    sort: Optional[Literal[\"asc\", \"desc\"]] = Field(default=None)"
                ),
                "optional inline enum field must accept None at wrapper boundaries:\n{out}"
            );
            // Book.rating inline union, optional+nullable → Optional[Union[..]] = None.
            assert!(
                out.contains("    rating: Optional[Union[int, float]] = Field(default=None)"),
                "inline union field must be Optional[Union[..]] defaulted:\n{out}"
            );
        }

        #[test]
        fn models_header_is_3_9_safe_and_fixed() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(
                out.starts_with("from __future__ import annotations"),
                "every module starts with the lazy-annotation future import:\n{out}"
            );
            assert!(out.contains("import enum"), "{out}");
            // The typing import is COMPUTED per file: only the names this sample actually uses, with
            // `Dict`/`List` gone (they map to the PEP 585 builtins) and no unused `TYPE_CHECKING`.
            assert!(
                out.contains("from typing import Any, Literal, Optional, Union"),
                "{out}"
            );
            assert!(
                !out.contains("Dict[") && !out.contains("List[") && !out.contains("TYPE_CHECKING"),
                "computed header must drop Dict/List/TYPE_CHECKING:\n{out}"
            );
            assert!(
                out.contains("from pydantic import BaseModel, ConfigDict, Field"),
                "{out}"
            );
            assert!(out.contains("class Book(BaseModel):"), "{out}");
            assert!(
                out.contains("model_config = ConfigDict(populate_by_name=True, extra=\"ignore\")"),
                "{out}"
            );
        }

        #[test]
        fn pydantic_models_keep_from_dict_and_to_dict_compat_methods() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(
                out.contains("def from_dict(cls, _data: dict[str, Any]) -> Book:"),
                "Pydantic models should keep legacy decode compatibility:\n{out}"
            );
            assert!(out.contains("return cls.model_validate(_data)"), "{out}");
            assert!(
                out.contains("def to_dict(self) -> dict[str, Any]:"),
                "Pydantic models should keep legacy encode compatibility:\n{out}"
            );
            assert!(
                out.contains(
                    "return self.model_dump(mode=\"json\", by_alias=True, exclude_none=True)"
                ),
                "{out}"
            );
        }

        #[test]
        fn generate_models_is_byte_identical_across_two_runs() {
            let g = sample_graph();
            assert_eq!(
                emit_models(&g, "bookstore").unwrap(),
                emit_models(&g, "bookstore").unwrap(),
                "emit_models must be deterministic"
            );
        }
    }

    /// A facts document with three operations: a body POST returning a model, a templated-path GET with
    /// a path param, and a query-bearing GET — enough to exercise path escaping, query encoding, body
    /// marshalling, success-status comparison, and the typed return decode.
    const OPS_SAMPLE: &[u8] = br#"{
      "module": "app",
      "routes": [
        {
          "method": "POST", "path": "/books", "handler": "createBook",
          "operation_id": "createBook", "params": [],
          "request_body": { "ref_id": "app.models.Book" },
          "responses": [
            { "status": 201, "body": { "ref_id": "app.models.CreatedMessage" } },
            { "status": 409, "body": { "ref_id": "app.models.OutOfStock" } }
          ],
          "span": { "file": "/root/main.py", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/books/{book_id}", "handler": "getBook",
          "operation_id": "getBook",
          "params": [
            { "name": "book_id", "location": "path", "required": true,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "span": { "file": "/root/main.py", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "app.models.Book" } } ],
          "span": { "file": "/root/main.py", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listBooks",
          "operation_id": "listBooks",
          "params": [
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/main.py", "start_line": 3, "end_line": 3 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": null } ],
          "span": { "file": "/root/main.py", "start_line": 3, "end_line": 3 }
        }
      ],
      "schemas": [
        {
          "id": "app.models.Book", "name": "Book",
          "body": { "type": "object", "of": [
            { "json_name": "title", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "app.models.CreatedMessage", "name": "CreatedMessage",
          "body": { "type": "object", "of": [
            { "json_name": "id", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null },
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.OutOfStock", "name": "OutOfStock",
          "body": { "type": "object", "of": [
            { "json_name": "reason", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn ops_graph() -> ApiGraph {
        let facts = serde_json::from_slice(OPS_SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    mod operations {
        use super::{emit_operations, ops_graph, Operation};

        fn ops_for<'g>(graph: &'g ApiGraph, handler: &str) -> Vec<&'g Operation> {
            graph
                .operations
                .iter()
                .filter(|o| o.handler == handler)
                .collect()
        }

        use super::ApiGraph;

        #[test]
        fn body_op_has_snake_method_typed_body_and_typed_return() {
            let g = ops_graph();
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "createBook")).unwrap();
            assert!(
                out.contains("def create_book(")
                    && out.contains("body: Book")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> CreatedMessage:"),
                "snake method, typed body, typed return:\n{out}"
            );
            assert!(
                out.contains("if _status < 200 or _status >= 300:"),
                "rejects only non-2xx statuses:\n{out}"
            );
            assert!(out.contains("if _status == 409:"), "{out}");
            assert!(
                out.contains("self._raise(_status, _headers, _raw, OutOfStock)"),
                "{out}"
            );
            assert!(
                out.contains("self._raise(_status, _headers, _raw)"),
                "{out}"
            );
            assert!(
                out.contains("return CreatedMessage.model_validate(_data)"),
                "{out}"
            );
            assert!(
                out.contains("_status, _headers, _raw = self._do(")
                    && out.contains("\"POST\",")
                    && out.contains("body=body,")
                    && out.contains("content_type=\"application/json\",")
                    && out.contains("body_encoding=\"json\",")
                    && out.contains("operation_id=\"createBook\","),
                "body op passes body to _do:\n{out}"
            );
        }

        #[test]
        fn required_body_precedes_required_query_param() {
            let mut g = ops_graph();
            g.operations[0].params.push(crate::graph::Param {
                name: "tenant".to_string(),
                location: "query".to_string(),
                required: true,
                schema: crate::graph::Type::Primitive(crate::graph::Prim::String),
                default: None,
                style: None,
                explode: None,
                allow_reserved: false,
                openapi_content: None,
                provenance: crate::graph::SourceSpan {
                    file: "main.py".to_string(),
                    start_line: 4,
                    end_line: 4,
                },
            });
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "createBook")).unwrap();
            assert!(
                out.contains("body: Book")
                    && out.contains("tenant")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> CreatedMessage:"),
                "required body must stay before required query params:\n{out}"
            );
            assert!(
                out.contains(
                    "_query.extend(self._parameter_pairs(\"tenant\", tenant, \"form\", True))"
                ),
                "{out}"
            );
        }

        #[test]
        fn typed_success_with_bodyless_alternate_returns_optional_and_decodes_only_body_status() {
            let mut g = ops_graph();
            g.operations[0].responses.push(crate::graph::Response {
                status: 204,
                body: None,
                body_kind: "empty".to_string(),
                content_type: None,
                content_types: Vec::new(),
            });
            g.operations[0]
                .responses
                .sort_by_key(|response| response.status);
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "createBook")).unwrap();
            assert!(
                out.contains("def create_book(")
                    && out.contains("body: Book")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> Optional[CreatedMessage]:"),
                "bodyless alternate success should make the return hint optional:\n{out}"
            );
            assert!(
                out.contains("if _status in (201,):"),
                "only the body-bearing status should decode:\n{out}"
            );
            assert!(out.contains("return None"), "{out}");
        }

        #[test]
        fn templated_path_escapes_each_param_with_urllib_quote() {
            let g = ops_graph();
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "getBook")).unwrap();
            assert!(
                out.contains(
                    "path = f\"/books/{urllib.parse.quote(str(book_id), safe='')}\""
                ),
                "path param must be percent-escaped (V5) with a backslash-free f-string (PYSDK-02):\n{out}"
            );
            assert!(
                out.contains("def get_book(")
                    && out.contains("book_id")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> Book:"),
                "{out}"
            );
        }

        #[test]
        fn query_op_encodes_present_params_and_has_no_body() {
            let g = ops_graph();
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains("def list_books(")
                    && out.contains("cursor: Optional[str] = None")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> Any:"),
                "{out}"
            );
            assert!(out.contains("if cursor is not None:"), "{out}");
            assert!(
                out.contains(
                    "_query.extend(self._parameter_pairs(\"cursor\", cursor, \"form\", True))"
                ),
                "{out}"
            );
            assert!(
                out.contains("path = path + \"?\" + self._encode_query(_query, _allow_reserved)"),
                "{out}"
            );
            // body-less success returns the raw decode (None when empty).
            assert!(
                out.contains("return json.loads(_raw) if _raw else None"),
                "{out}"
            );
            assert!(
                !out.contains(", body=body"),
                "query op has no body arg:\n{out}"
            );
        }

        #[test]
        fn query_api_key_auth_is_appended_to_query_string() {
            let mut g = ops_graph();
            g.security = vec![crate::graph::SecurityScheme {
                id: "QueryAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "query".to_string(),
                name: "api_key".to_string(),
                global: true,
            }];
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains(
                    "_auth_query_api_key = self._api_keys.get(\"api_key\") or self._api_key"
                ),
                "{out}"
            );
            assert!(
                out.contains("_query.append((\"api_key\", _auth_query_api_key))"),
                "{out}"
            );
            assert!(
                out.contains("path = path + \"?\" + self._encode_query(_query, _allow_reserved)"),
                "{out}"
            );
        }

        #[test]
        fn http_auth_marks_operation_dispatch() {
            let mut g = ops_graph();
            g.security = vec![
                crate::graph::SecurityScheme {
                    id: "BearerAuth".to_string(),
                    kind: "http".to_string(),
                    location: String::new(),
                    name: "bearer".to_string(),
                    global: true,
                },
                crate::graph::SecurityScheme {
                    id: "BasicAuth".to_string(),
                    kind: "http".to_string(),
                    location: String::new(),
                    name: "basic".to_string(),
                    global: false,
                },
            ];
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains("_status, _headers, _raw = self._do(")
                    && out.contains("\"GET\",")
                    && out.contains("auth_bearer=True,")
                    && out.contains("operation_id=\"listBooks\","),
                "{out}"
            );
            let op = g
                .operations
                .iter_mut()
                .find(|op| op.id == "listBooks")
                .unwrap();
            op.security = vec!["BasicAuth".to_string()];
            op.security_overrides_global = true;
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(out.contains("auth_basic=True,"), "{out}");
        }

        #[test]
        fn binary_success_returns_bytes_without_json_decode() {
            let mut g = ops_graph();
            let op = g
                .operations
                .iter_mut()
                .find(|op| op.handler == "listBooks")
                .unwrap();
            op.responses[0].body_kind = "binary".to_string();
            op.responses[0].content_type = Some("application/pdf".to_string());
            op.responses[0].content_types = vec!["application/pdf".to_string()];
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains("def list_books(")
                    && out.contains("cursor: Optional[str] = None")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> bytes:"),
                "{out}"
            );
            assert!(out.contains("if _status in (200,):"), "{out}");
            assert!(out.contains("return _raw"), "{out}");
            assert!(
                !out.contains("return json.loads(_raw) if _raw else None"),
                "{out}"
            );
        }

        #[test]
        fn binary_success_with_bodyless_alternate_returns_optional_bytes() {
            let mut g = ops_graph();
            let op = g
                .operations
                .iter_mut()
                .find(|op| op.handler == "listBooks")
                .unwrap();
            op.responses[0].body_kind = "binary".to_string();
            op.responses[0].content_type = Some("application/pdf".to_string());
            op.responses[0].content_types = vec!["application/pdf".to_string()];
            op.responses.push(crate::graph::Response {
                status: 204,
                body: None,
                body_kind: "empty".to_string(),
                content_type: None,
                content_types: Vec::new(),
            });
            op.responses.sort_by_key(|response| response.status);
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains("def list_books(")
                    && out.contains("cursor: Optional[str] = None")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> Optional[bytes]:"),
                "{out}"
            );
            assert!(out.contains("return _raw"), "{out}");
            assert!(out.contains("return None"), "{out}");
        }

        #[test]
        fn mismatched_path_token_and_param_is_a_typed_error() {
            // A path templating {book_id} but a path param named {id} is a typed SdkGen error (WR-03).
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/books/{book_id}", "handler": "getBook",
                  "operation_id": "getBook",
                  "params": [
                    { "name": "id", "location": "path", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [],
              "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let g = ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let err = emit_operations(&g, "bookstore", "/", &ops).unwrap_err();
            assert!(
                err.to_string().contains("do not match its path params"),
                "{err}"
            );
        }
    }

    mod client_errors_init {
        use super::{emit_client, emit_client_with_models, emit_errors, emit_init, ops_graph};

        #[test]
        fn client_has_injectable_opener_and_no_third_party_http_imports() {
            let out = emit_client("bookstore");
            assert!(
                out.contains("opener: Optional[urllib.request.OpenerDirector] = None"),
                "{out}"
            );
            assert!(out.contains("urllib.request.build_opener()"), "{out}");
            assert!(out.contains("except urllib.error.HTTPError as e:"), "{out}");
            // no third-party HTTP libs (PYSDK-01).
            assert!(!out.contains("import requests"), "{out}");
            assert!(!out.contains("import httpx"), "{out}");
        }

        #[test]
        fn client_emits_http_auth_options_when_needed() {
            let out = emit_client_with_models(
                "bookstore",
                "models",
                crate::sdk::model_style::PyModelStyle::default(),
                false,
                true,
                true,
                &[],
                &crate::graph::RuntimePolicy::default(),
                false,
                false,
            );
            assert!(out.contains("import base64"), "{out}");
            assert!(out.contains("bearer_token: Optional[str] = None"), "{out}");
            assert!(
                out.contains("basic_auth: Optional[tuple[str, str]] = None"),
                "{out}"
            );
            assert!(
                out.contains("headers[\"Authorization\"] = f\"Bearer {self._bearer_token}\""),
                "{out}"
            );
            assert!(
                out.contains("base64.b64encode(raw).decode(\"ascii\")"),
                "{out}"
            );
        }

        #[test]
        fn errors_define_typed_apierror_with_is_not_found() {
            let out = emit_errors("bookstore");
            assert!(out.contains("class ApiError(Exception):"), "{out}");
            assert!(out.contains("self.status_code = status_code"), "{out}");
            assert!(out.contains("def is_not_found(self) -> bool:"), "{out}");
            assert!(out.contains("return self.status_code == 404"), "{out}");
        }

        #[test]
        fn init_reexports_client_apierror_and_every_model() {
            let out = emit_init(&ops_graph(), "bookstore");
            assert!(out.contains("from .client import Client"), "{out}");
            assert!(out.contains("from .errors import ApiError"), "{out}");
            assert!(out.contains("    Book,"), "{out}");
            assert!(out.contains("    CreatedMessage,"), "{out}");
            assert!(out.contains("\"Client\","), "{out}");
        }
    }

    /// Regression locks for the four BLOCKERs (CR-01..04) + the hardened warnings (WR-01/02/03/05),
    /// each on an input shape the bookstore fixture does NOT exercise.
    mod regressions {
        use super::super::safe_ident;
        use super::{
            emit_models, emit_models_with_style, emit_operations, ApiGraph, Operation, PyModelStyle,
        };

        fn graph_from(facts: &[u8]) -> ApiGraph {
            let facts = serde_json::from_slice(facts).unwrap();
            ApiGraph::from_facts(facts, "/root")
        }

        // CR-02: a field/param named after a Python keyword must emit a SAFE identifier, never the bare
        // keyword (which is a SyntaxError), while the WIRE key stays the original keyword.
        #[test]
        fn cr02_reserved_word_field_emits_safe_identifier_keeping_wire_key() {
            assert_eq!(safe_ident("from"), "from_");
            assert_eq!(safe_ident("class"), "class_");
            assert_eq!(safe_ident("import"), "import_");
            // a leading-digit name is also unsafe as an identifier.
            assert_eq!(safe_ident("2fast"), "_2fast");
            // a non-keyword (e.g. `id`, `type`) is left untouched (they are builtins, not keywords).
            assert_eq!(safe_ident("id"), "id");
            assert_eq!(safe_ident("type"), "type");

            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "app.models.Reserved", "name": "Reserved",
                  "body": { "type": "object", "of": [
                    { "json_name": "from", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null },
                    { "json_name": "class", "required": false, "optional": true, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null }
                  ] },
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
              ],
              "diagnostics": [] }"#;
            let out = emit_models(&graph_from(facts), "pkg").unwrap();
            // the Python attribute is sanitized ...
            assert!(
                out.contains("    from_: str"),
                "keyword field renamed:\n{out}"
            );
            assert!(
                out.contains("    class_: Optional[str] = Field(default=None, alias=\"class\")"),
                "keyword optional field renamed + defaulted:\n{out}"
            );
            assert!(
                out.contains("    from_: str = Field(..., alias=\"from\")"),
                "required wire key preserved as alias:\n{out}"
            );
            assert!(
                out.contains("alias=\"class\""),
                "optional wire key preserved as alias:\n{out}"
            );
        }

        // CR-03: two wire values normalizing to the same SCREAMING_SNAKE form must emit DISTINCT,
        // collision-free member names (no duplicate class key → no import-time TypeError), with the
        // wire values intact; an empty / numeric value must still produce a valid identifier.
        #[test]
        fn cr03_enum_member_collisions_disambiguate_and_unsafe_values_are_guarded() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "app.models.Status", "name": "Status",
                  "body": { "type": "enum", "of": ["out-of-stock", "out_of_stock", "", "1"] },
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
              ],
              "diagnostics": [] }"#;
            let out =
                emit_models_with_style(&graph_from(facts), "pkg", PyModelStyle::Dataclass).unwrap();
            // first collision keeps the base, the second is suffixed — both wire values intact.
            assert!(out.contains("    OUT_OF_STOCK = \"out-of-stock\""), "{out}");
            assert!(
                out.contains("    OUT_OF_STOCK_2 = \"out_of_stock\""),
                "{out}"
            );
            // empty value → a stable placeholder identifier; numeric → leading-underscore.
            assert!(
                out.contains("    MEMBER = \"\""),
                "empty value guarded:\n{out}"
            );
            assert!(
                out.contains("    _1 = \"1\""),
                "numeric value guarded:\n{out}"
            );
            // no duplicate identifier is emitted.
            assert_eq!(
                out.matches("OUT_OF_STOCK = ").count(),
                1,
                "the base member appears exactly once:\n{out}"
            );
        }

        // CR-04: a typed return decodes via a from_dict that (1) ignores unknown server keys and
        // (2) recurses into nested dataclass fields, rather than the fragile Model(**_data).
        #[test]
        fn cr04_from_dict_is_forward_compatible_and_recurses_into_nested_models() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "app.models.Inner", "name": "Inner",
                  "body": { "type": "object", "of": [
                    { "json_name": "v", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null }
                  ] },
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } },
                { "id": "app.models.Outer", "name": "Outer",
                  "body": { "type": "object", "of": [
                    { "json_name": "inner", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "named", "of": "app.models.Inner" },
                      "description": null, "example": null },
                    { "json_name": "items", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "array", "of": { "type": "named", "of": "app.models.Inner" } },
                      "description": null, "example": null }
                  ] },
                  "span": { "file": "/root/m.py", "start_line": 2, "end_line": 2 } }
              ],
              "diagnostics": [] }"#;
            let out =
                emit_models_with_style(&graph_from(facts), "pkg", PyModelStyle::Dataclass).unwrap();
            // a from_dict classmethod is emitted; nested object field recurses; nested array maps.
            assert!(
                out.contains("def from_dict(cls, _data: dict[str, Any])"),
                "{out}"
            );
            assert!(
                out.contains("inner=Inner.from_dict(_data[\"inner\"])"),
                "nested recurse:\n{out}"
            );
            assert!(
                out.contains("items=[Inner.from_dict(_item) for _item in _data[\"items\"]]"),
                "nested array recurse:\n{out}"
            );
            // ignore-unknown: construction is keyword-by-declared-field, so an extra server key is never
            // forwarded to the constructor (no `**_data`).
            assert!(
                !out.contains("(**_data)"),
                "must not splat the raw dict:\n{out}"
            );
        }

        // WR-01: a REQUIRED query param is a positional arg always written to _query (no None guard);
        // WR-03: two params whose Python identifier collides is a typed SdkGen error, not a SyntaxError.
        #[test]
        fn wr01_required_query_param_is_positional_and_always_sent() {
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/search", "handler": "search",
                  "operation_id": "search",
                  "params": [
                    { "name": "q", "location": "query", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } },
                    { "name": "page", "location": "query", "required": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [], "diagnostics": [] }"#;
            let g = graph_from(facts);
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let out = emit_operations(&g, "pkg", "/", &ops).unwrap();
            // required `q` is positional (no `=None`), optional `page` keeps the default.
            assert!(
                out.contains("def search(")
                    && out.contains("q,")
                    && out.contains("page: Optional[str] = None")
                    && out.contains("request_options: Optional[RequestOptions] = None")
                    && out.contains(") -> Any:"),
                "{out}"
            );
            // required `q` is unconditionally written; optional `page` is guarded.
            assert!(
                out.contains(
                    "        _query.extend(self._parameter_pairs(\"q\", q, \"form\", True))"
                ),
                "required always sent:\n{out}"
            );
            assert!(
                out.contains("        if page is not None:"),
                "optional guarded:\n{out}"
            );
        }

        #[test]
        fn wr03_param_identifier_collision_is_a_typed_error() {
            // a query param literally named `self` collides with the bound receiver → typed error.
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/x", "handler": "x",
                  "operation_id": "x",
                  "params": [
                    { "name": "self", "location": "query", "required": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [], "diagnostics": [] }"#;
            let g = graph_from(facts);
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let err = emit_operations(&g, "pkg", "/", &ops).unwrap_err();
            assert!(err.to_string().contains("collides"), "{err}");
        }

        // WR-05: two distinct schema ids sharing a Python name is a typed error (no silent shadowing).
        #[test]
        fn wr05_duplicate_schema_name_is_a_typed_error() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "a.Book", "name": "Book",
                  "body": { "type": "object", "of": [] },
                  "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 } },
                { "id": "b.Book", "name": "Book",
                  "body": { "type": "object", "of": [] },
                  "span": { "file": "/root/m.py", "start_line": 2, "end_line": 2 } }
              ],
              "diagnostics": [] }"#;
            let g = graph_from(facts);
            let err = emit_models(&g, "pkg").unwrap_err();
            assert!(
                err.to_string().contains("share the Python SDK name"),
                "{err}"
            );
        }
    }
}
