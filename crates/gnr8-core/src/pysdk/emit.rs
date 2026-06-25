//! `format!`-based Python SDK emitters (D-05: no template engine; small internal templating only).
//!
//! Each emitter turns the router-agnostic [`crate::graph::ApiGraph`] into one idiomatic, dependency-free
//! Python source file. Unlike [`crate::gosdk::emit`], there is NO `gofmt` normalization step (Python has
//! no stdlib formatter; `black`/`autopep8` are third-party — CLAUDE.md rule 2): every emitter produces
//! already-correct, significant-whitespace Python directly.
//!
//! - [`emit_models`]   — one `@dataclass` per object [`Schema`] (fields partitioned required-first so the
//!   class imports without a `TypeError` on Python 3.9), one `class X(str, enum.Enum)` per named enum
//!   [`Schema`]; Python types follow [`py_type`].
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

use std::fmt::Write as _;

use crate::graph::{ApiGraph, Field, Prim, Type};
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

/// The fixed, deterministic import header at the top of `models.py`.
///
/// Python tolerates unused imports at runtime (unlike `go build`), so a FIXED header is emitted rather
/// than a computed set — deterministic by construction (no `BTreeSet` to iterate). `from __future__
/// import annotations` makes every annotation a lazy string, sidestepping Python-3.9 generic-subscription
/// concerns (`List[..]`/`Optional[..]`) and forward-reference ordering between models.
const MODELS_HEADER: &str = "\
from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Any, Dict, List, Literal, Optional, Union
";

/// Split an identifier into words on `_`/`-`/space separators and lower→upper case boundaries.
///
/// `workflowChainIds` → `["workflow", "Chain", "Ids"]`; `next_cursor` → `["next", "cursor"]`. The shared
/// tokenizer behind every Python casing helper (twin of `gosdk::emit::split_words`).
fn split_words(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch);
        prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Convert an identifier to CamelCase (Python class name): `book_format` → `BookFormat`.
///
/// Used for the rare case a Python class name must be derived from a json identifier. Graph schema
/// `.name`s are already PascalCase symbols and are used verbatim; this exists for symmetry with the Go
/// twin's `exported` and for deriving class-shaped names where needed.
pub(crate) fn class_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for word in split_words(name) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    out
}

/// Convert an identifier to snake_case (Python method/attribute name): `createBook` → `create_book`.
pub(crate) fn snake(name: &str) -> String {
    split_words(name)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert an enum member value to a SCREAMING_SNAKE identifier: `out-of-stock` → `OUT_OF_STOCK`.
pub(crate) fn screaming_snake(value: &str) -> String {
    split_words(value)
        .iter()
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
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
pub(crate) fn py_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(prim) => py_primitive(prim).to_string(),
        // Every well-known scalar carries on the wire as a string in this dependency-free SDK (a
        // date-time is an RFC-3339 `str`; a uuid/email/uri is a `str`) — A7. No `datetime` import, so
        // the @dataclass marshals cleanly through `json`.
        Type::WellKnown(_) => "str".to_string(),
        Type::Array(items) => format!("List[{}]", py_type(items, false, graph)?),
        // A keyed map and a free-form value map to `Dict[str, Any]` / `Any`.
        Type::Map { .. } => "Dict[str, Any]".to_string(),
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

/// Emit `models.py`: one `@dataclass` per object schema + one `class X(str, enum.Enum)` per named enum.
///
/// Schemas are consumed in the graph's id-sorted order (determinism). A schema whose body is
/// [`Type::Enum`] becomes a named enum class; a [`Type::Object`] becomes a `@dataclass`; every other
/// body is a typed [`CoreError::SdkGen`] (mirror of the Go twin's non-object/non-enum arm).
///
/// `package` is currently unused in the body (the file carries no package clause in Python) but is kept
/// in the signature to mirror the Go twin and the `generate` call site.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if a field's schema cannot be mapped or a schema body is unsupported.
pub(crate) fn emit_models(graph: &ApiGraph, _package: &str) -> Result<String, CoreError> {
    let mut out = String::new();
    out.push_str(MODELS_HEADER);

    for schema in &graph.schemas {
        out.push('\n');
        match &schema.body {
            // A named enum (top-level Schema body) → a `class X(str, enum.Enum)`. The `str` mixin makes
            // `json.dumps` serialize the member value as its string — the twin of Go's `type X string`.
            Type::Enum(members) => emit_enum_class(&mut out, &schema.name, members)?,
            // A named object → a `@dataclass`.
            Type::Object(fields) => emit_dataclass(&mut out, &schema.name, fields, graph)?,
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
                let alias = py_type(&schema.body, false, graph)?;
                writeln!(out, "{} = {alias}", schema.name).map_err(sink)?;
            }
        }
    }
    Ok(out)
}

/// Emit a named enum class: `class {name}(str, enum.Enum)` with `MEMBER = "value"` lines.
///
/// Members are emitted in graph order (already lexically sorted, GRAPH-02). The member identifier is the
/// SCREAMING_SNAKE form of the value; the value itself is the wire string.
fn emit_enum_class(out: &mut String, name: &str, members: &[String]) -> Result<(), CoreError> {
    writeln!(out, "class {name}(str, enum.Enum):").map_err(sink)?;
    if members.is_empty() {
        // An empty enum still needs a body to be valid Python.
        writeln!(out, "    pass").map_err(sink)?;
        return Ok(());
    }
    for value in members {
        let member = screaming_snake(value);
        writeln!(out, "    {member} = \"{value}\"").map_err(sink)?;
    }
    Ok(())
}

/// Emit a `@dataclass` for an object schema, partitioning fields required-first / optional-last.
///
/// PITFALL 1 (RESEARCH): `@dataclass` raises `TypeError: non-default argument follows default argument`
/// at class-definition (import) time if a no-default field follows a defaulted one. The graph sorts
/// fields alphabetically by `json_name`, which interleaves the two, so this partitions the fields —
/// required (no default) first, optional (default `= None`) last — before emitting. `kw_only=True` is
/// Python 3.10+ and unavailable on 3.9, so partitioning is the 3.9-safe fix. The reorder is a
/// presentation concern only: json keys are name-addressed, so wire behavior is unchanged.
fn emit_dataclass(
    out: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
) -> Result<(), CoreError> {
    writeln!(out, "@dataclass").map_err(sink)?;
    writeln!(out, "class {name}:").map_err(sink)?;
    if fields.is_empty() {
        writeln!(out, "    pass").map_err(sink)?;
        return Ok(());
    }
    // Partition preserving each group's (already-sorted) relative order: required (no default) first,
    // optional (defaulted) last — so defaulted fields are contiguous at the end (PITFALL 1).
    let (required, optional): (Vec<&Field>, Vec<&Field>) =
        fields.iter().partition(|f| !f.optional);
    for field in required {
        let hint = py_type(&field.schema, field.nullable, graph)?;
        writeln!(out, "    {}: {hint}", field.json_name).map_err(sink)?;
    }
    for field in optional {
        let hint = py_type(&field.schema, field.nullable, graph)?;
        // An optional field (the key may be absent) defaults to None so the class imports and a caller
        // may omit it. The type hint already carries Optional[..] when the value is also nullable.
        writeln!(out, "    {}: {hint} = None", field.json_name).map_err(sink)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{class_name, emit_models, py_type, screaming_snake, snake};
    use crate::graph::{ApiGraph, Prim, Type};

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
        use super::{class_name, screaming_snake, snake};

        #[test]
        fn helpers_produce_python_casings() {
            assert_eq!(class_name("book_format"), "BookFormat");
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
            assert_eq!(py_type(&sort, false, &g).unwrap(), "Literal[\"asc\", \"desc\"]");
        }

        #[test]
        fn named_ref_resolves_to_the_schema_name() {
            let g = sample_graph();
            let named = Type::Named("app.models.BookFormat".to_string());
            assert_eq!(py_type(&named, false, &g).unwrap(), "BookFormat");
            assert_eq!(
                py_type(&named, true, &g).unwrap(),
                "Optional[BookFormat]"
            );
        }

        #[test]
        fn named_union_resolves_each_variant_to_its_class_name() {
            // BookOrError = Union[Book, OutOfStock].
            let g = sample_graph();
            let body = g
                .schemas
                .iter()
                .find(|s| s.name == "BookOrError")
                .unwrap();
            assert_eq!(
                py_type(&body.body, false, &g).unwrap(),
                "Union[Book, OutOfStock]"
            );
        }

        #[test]
        fn array_and_map_and_any_map_to_typing_generics() {
            let g = ApiGraph::default();
            let arr = Type::Array(Box::new(Type::Primitive(Prim::String)));
            assert_eq!(py_type(&arr, false, &g).unwrap(), "List[str]");
            let map = Type::Map {
                key: Box::new(Type::Primitive(Prim::String)),
                value: Box::new(Type::Primitive(Prim::String)),
            };
            assert_eq!(py_type(&map, false, &g).unwrap(), "Dict[str, Any]");
            assert_eq!(py_type(&Type::Any {}, false, &g).unwrap(), "Any");
        }

        #[test]
        fn inline_object_is_a_typed_error_parity_with_go() {
            let g = ApiGraph::default();
            let obj = Type::Object(vec![]);
            let err = py_type(&obj, false, &g).unwrap_err();
            assert!(
                err.to_string().contains("inline object type is unsupported"),
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
        use super::{emit_models, sample_graph};

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
        fn dataclass_emits_required_fields_before_optional_fields() {
            // BookFilters: genre (required), in_stock (optional), published (required-but-nullable),
            // sort (optional). Alphabetical graph order interleaves defaults; the emitter must put both
            // required fields (genre, published) before both optional ones (in_stock, sort).
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
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
            assert!(out.contains("    genre: str\n"), "required has no default:\n{out}");
            // optional non-nullable bool → defaulted, hint NOT Optional.
            assert!(
                out.contains("    in_stock: bool = None\n"),
                "optional field must default to None:\n{out}"
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
            // BookFilters.sort inline enum → Literal, defaulted (optional).
            assert!(
                out.contains("    sort: Literal[\"asc\", \"desc\"] = None"),
                "inline enum field must be a defaulted Literal:\n{out}"
            );
            // Book.rating inline union, optional+nullable → Optional[Union[..]] = None.
            assert!(
                out.contains("    rating: Optional[Union[int, float]] = None"),
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
            assert!(out.contains("from dataclasses import dataclass"), "{out}");
            assert!(out.contains("import enum"), "{out}");
            assert!(
                out.contains("from typing import Any, Dict, List, Literal, Optional, Union"),
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
}
