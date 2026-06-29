//! `format!`-based TypeScript SDK emitters (D-05: no template engine; small internal templating only).
//!
//! Each emitter turns the router-agnostic [`crate::graph::ApiGraph`] into one idiomatic, dependency-free
//! TypeScript source file. Like [`crate::pysdk::emit`] (and unlike [`crate::gosdk::emit`]) there is NO
//! `gofmt`-style normalization step (the only stdlib TS formatter would be `tsc` itself, which does not
//! reformat; `prettier` is third-party — CLAUDE.md rule 2): every emitter produces already-correct
//! TypeScript directly.
//!
//! - [`emit_models`]   — one `export interface X` per object [`Schema`], one
//!   `export type X = "a" | "b"` per named enum [`Schema`], and one `export type X = …` per named
//!   union/alias [`Schema`]; TypeScript types follow [`ts_type`].
//! - [`emit_client`]   — the injectable platform-`fetch`-backed `Client` (Task 2).
//! - [`emit_errors`]   — the typed `ApiError extends Error` (Task 2).
//! - [`emit_operations`] / [`emit_index`] — the per-operation methods + the re-export surface (Task 2).
//!
//! THE LOAD-BEARING DIVERGENCE from the Go twin lives in [`ts_type`]: where `go_type` returns an error
//! for [`Type::Union`], inline [`Type::Enum`], and inline [`Type::Object`] (Go has no sum types and the
//! Go target only emits named DTOs), TypeScript *can* express sum/inline types, so `ts_type` maps a
//! union to `A | B` and an inline enum to a string-literal union `"a" | "b"`. The match over [`Type`]
//! stays exhaustive — no `_ =>` arm — so a future IR variant fails to compile until handled (rule 3).
//! Inline [`Type::Object`] keeps parity with the Go/Python targets: an EXPLICIT typed error arm.
//!
//! Determinism (TSSDK-03): every collection is consumed in the graph's already-sorted order, and each
//! file's preamble is a FIXED string (TS interfaces and `fetch` need no computed import set, no
//! [`std::collections::HashMap`] iteration). Every un-representable fact (a dangling `$ref`, an inline
//! object) returns [`crate::CoreError::SdkGen`]; there is no production `unwrap`/`expect`/`panic`
//! (RUST-04).

use std::fmt::Write as _;

use crate::graph::{ApiGraph, Field, Operation, Param, Prim, Type};
use crate::sdk::emit_common::{
    body_model_of, check_unique_schema_names, file_stem, is_json_object_key, join_path,
    path_tokens, path_tokens_match, quoted_string_literal, split_words, success_responses_of,
};
use crate::sdk::surface::ResolvedTypeAlias;
use crate::sdk::typescript::{TsModelPropertyPolicy, TsNullablePolicy};
use crate::CoreError;

/// Fold an indentation/`format!` write error into a typed [`CoreError::SdkGen`].
///
/// `write!`/`writeln!` into a `String` is infallible in practice, but the `fmt::Write` trait is
/// fallible; mapping the error keeps the path `unwrap`/`expect`-free (RUST-04).
fn sink(err: std::fmt::Error) -> CoreError {
    CoreError::SdkGen {
        message: format!("failed to format TypeScript source: {err}"),
    }
}

/// Convert an identifier to `camelCase` (TypeScript method/property name): `createBook` → `createBook`,
/// `create_book` → `createBook`. The first word is lowercased; each subsequent word is capitalized.
pub(crate) fn camel(name: &str) -> String {
    let words = split_words(name);
    let mut out = String::new();
    for (i, word) in words.iter().enumerate() {
        if i == 0 {
            out.push_str(&word.to_ascii_lowercase());
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(&chars.as_str().to_ascii_lowercase());
            }
        }
    }
    out
}

/// Escape an arbitrary wire string into a TypeScript double-quoted string literal (the quotes
/// included).
///
/// This is the SINGLE deterministic helper used everywhere a wire value is emitted as a TS string
/// literal — inline string-literal-union members ([`ts_type`]), named-enum alias members
/// ([`emit_enum_alias`]), and quoted non-identifier interface property names ([`emit_interface`]).
/// One path, no fallback (rule 3).
///
/// Enum/property values flow from arbitrary source-language string constants and are NOT constrained
/// to identifier-safe ASCII (JSON keys/enum values routinely carry `-`, `/`, `.`, spaces, and
/// occasionally `"`/`\`). Interpolating such a value raw into a `"…"` literal either breaks `tsc`
/// (an embedded `"`) or SILENTLY corrupts the literal type (an embedded `\b` becomes a backspace),
/// so the SDK's compile-time contract would no longer match the wire value. Escaping `\` and `"`
/// (plus newline/CR/tab and other C0 control chars) preserves the wire value EXACTLY while keeping
/// the emitted literal valid TS (CR-01).
fn ts_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Any other C0 control char → a `\uXXXX` escape (deterministic lower-hex).
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Is `name` a plain (bare) TypeScript identifier — usable UNQUOTED as an interface member name?
///
/// Non-empty, first char `A-Za-z_$`, every later char `A-Za-z0-9_$`. A wire key that fails this
/// (kebab-case, leading digit, spaces, empty) must be emitted as a QUOTED string-literal member via
/// [`ts_string_literal`] (CR-02). Reserved words are intentionally NOT rejected: TypeScript accepts
/// reserved words as object/interface member names, so treating them as bare identifiers is valid.
fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Map a neutral graph [`Type`] to its TypeScript type, resolving named refs to model names.
///
/// ALL TypeScript-specific type mapping lives HERE (per-target mapping, IR-03). The match over [`Type`]
/// is exhaustive — NO `_ =>` / `other =>` arm — so a future variant fails to compile until handled
/// (rule 3).
///
/// This is the load-bearing divergence from `gosdk::emit::go_type`: a [`Type::Union`] becomes `A | B`
/// and an inline [`Type::Enum`] becomes a string-literal union `"a" | "b"` (TypeScript has sum/literal
/// types; the Go target rejected both). An inline [`Type::Object`] keeps parity with the Go/Python
/// targets — a typed [`CoreError::SdkGen`] — because every object in the neutral IR is a named `$ref`.
///
/// `nullable` wraps the resulting type as `base | null` (the value may be explicitly `null`). The
/// `optional` axis is NOT read here — it drives the field declaration's `?:` in [`emit_interface`], not
/// the type (the two are distinct, independent axes).
///
/// `ns` is the namespace prefix prepended to every resolved [`Type::Named`] symbol so the SAME mapping
/// serves both files with one path (rule 3): `models.ts` references its sibling symbols BARE (`ns = ""`),
/// while `client.ts` reaches them through its `import * as models` namespace (`ns = "models."`). Without
/// this, a named enum/model used as a param type emits a bare symbol that is out of scope in `client.ts`
/// (`error TS2304: Cannot find name 'BookFormat'`). The prefix is the caller's context, NOT a fallback.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling `Named` ref or an inline [`Type::Object`].
pub(crate) fn ts_type(
    schema: &Type,
    nullable: bool,
    graph: &ApiGraph,
    ns: &str,
) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(prim) => ts_primitive(prim).to_string(),
        // Every well-known scalar carries on the wire as a string in this dependency-free SDK (a
        // date-time is an RFC-3339 `string`; a uuid/email/uri is a `string`) — A7. No `Date` import.
        Type::WellKnown(_) => "string".to_string(),
        Type::Array(items) => format!("{}[]", ts_type(items, false, graph, ns)?),
        Type::Map { key, value } => {
            if !is_json_object_key(key) {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "map key type {key:?} cannot be represented as a TypeScript JSON object key"
                    ),
                });
            }
            format!("Record<string, {}>", ts_type(value, false, graph, ns)?)
        }
        Type::Any {} => "unknown".to_string(),
        Type::Named(ref_id) => {
            let target = graph
                .schemas
                .iter()
                .find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            // Prefix the resolved symbol with the caller's namespace (`""` in models.ts, `"models."`
            // in client.ts) so the reference resolves in BOTH files (rule 3, one path).
            format!("{ns}{}", target.name)
        }
        // An inline enum stays inline as a string-literal union (members in graph-sorted order) — the
        // case the Go target rejects. A named enum (a top-level Schema body) is instead a `type` alias;
        // see emit_models.
        Type::Enum(members) => members
            .iter()
            .map(|m| ts_string_literal(m))
            .collect::<Vec<_>>()
            .join(" | "),
        // A sum type becomes a native `A | B` union (the case the Go target rejects — Go has no sum
        // types).
        Type::Union(variants) => {
            let mut parts: Vec<String> = Vec::with_capacity(variants.len());
            for variant in variants {
                parts.push(ts_type(variant, false, graph, ns)?);
            }
            parts.join(" | ")
        }
        // An inline (anonymous) object is not emitted as a TypeScript type — every object in the IR is a
        // named `$ref`. Keep parity with the Go/Python targets: an EXPLICIT typed error arm, NOT a
        // catch-all (a future IR variant must fail to compile here, rule 3).
        Type::Object(_) => {
            return Err(CoreError::SdkGen {
                message: "inline object type is unsupported by the TypeScript SDK target \
                          (expected a named $ref)"
                    .to_string(),
            });
        }
    };
    if nullable {
        Ok(format!("{base} | null"))
    } else {
        Ok(base)
    }
}

/// Map a neutral [`Prim`] to its TypeScript type. There is a single numeric type (`number`), so integer
/// width and float width are irrelevant; a byte string carries base64 on the wire as a `string`.
///
/// The arm-per-variant match is deliberate even though several arms share a body (Int/Float → `number`,
/// String/Bytes → `string`): an exhaustive, one-arm-per-`Prim` match means a future `Prim` variant fails
/// to compile here until its TS mapping is chosen (rule 3) — the same exhaustiveness discipline as
/// `ts_type`. `match_same_arms` is allowed locally to preserve that property.
#[allow(clippy::match_same_arms)]
fn ts_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String => "string",
        Prim::Bool => "boolean",
        Prim::Int { .. } => "number",
        Prim::Float { .. } => "number",
        Prim::Bytes => "string",
    }
}

/// Emit `models.ts`: one `export interface X` per object schema, one `export type X = "a" | "b"` per
/// named enum, one `export type X = …` per named union/scalar/array alias.
///
/// Schemas are consumed in the graph's id-sorted order (determinism). A schema whose body is
/// [`Type::Enum`] becomes a string-literal-union `type` alias; a [`Type::Object`] becomes an
/// `interface`; every other named body (`Union`/`Array`/scalar/`Named`) becomes a plain `type` alias.
/// Unlike the Python twin, TypeScript `type` aliases are order-independent, so there is NO forward-ref
/// hack and NO fixed import header.
///
/// `package` is currently unused in the body (the file carries no package clause) but is kept in the
/// signature to mirror the twin and the `generate` call site.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if a field's schema cannot be mapped or two schemas collide on a name.
#[cfg(test)]
pub(crate) fn emit_models(graph: &ApiGraph, package: &str) -> Result<String, CoreError> {
    emit_models_with_aliases(graph, package, &[])
}

#[cfg(test)]
pub(crate) fn emit_models_with_aliases(
    graph: &ApiGraph,
    _package: &str,
    aliases: &[ResolvedTypeAlias],
) -> Result<String, CoreError> {
    emit_models_with_aliases_and_policies(
        graph,
        aliases,
        TsModelPropertyPolicy::Strict,
        TsNullablePolicy::ExplicitNull,
    )
}

pub(crate) fn emit_models_with_aliases_and_policies(
    graph: &ApiGraph,
    aliases: &[ResolvedTypeAlias],
    model_property_policy: TsModelPropertyPolicy,
    nullable_policy: TsNullablePolicy,
) -> Result<String, CoreError> {
    let mut out = String::new();

    check_unique_schema_names(graph, "TypeScript SDK")?;

    for (i, schema) in graph.schemas.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match &schema.body {
            // A named enum (top-level Schema body) → a string-literal-union `type` alias. The twin of
            // Python's `class X(str, enum.Enum)` and Go's `type X string`.
            Type::Enum(members) => emit_enum_alias(&mut out, &schema.name, members)?,
            // A named object → an `interface`.
            Type::Object(fields) => emit_interface_with_policies(
                &mut out,
                &schema.name,
                fields,
                graph,
                "",
                model_property_policy,
                nullable_policy,
            )?,
            // A named NON-object/NON-enum schema (e.g. `BookOrError = Book | OutOfStock`, or a
            // scalar/array/map alias) → a plain `type` alias. This is the load-bearing divergence from
            // the Go twin, which rejected named unions outright (Go has no sum types). `ts_type` maps
            // the body exhaustively, so a named union/array/scalar alias emits a valid TS type. TS `type`
            // aliases are order-free — NO PEP-484-style forward-ref hack is needed (RESEARCH Pitfall 6).
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Union(_)
            | Type::Any {} => {
                // models.ts references its sibling symbols BARE (no namespace prefix).
                let alias = ts_type(&schema.body, false, graph, "")?;
                writeln!(out, "export type {} = {alias};", schema.name).map_err(sink)?;
            }
        }
    }
    for alias in aliases {
        if !out.is_empty() {
            out.push('\n');
        }
        writeln!(out, "export type {} = {};", alias.alias, alias.canonical).map_err(sink)?;
    }
    Ok(out)
}

/// Emit OpenAPI-generator-compatible `models.ts`, including runtime enum const objects plus matching
/// type aliases.
pub(crate) fn emit_models_openapi_generator_compat(
    graph: &ApiGraph,
    _package: &str,
    aliases: &[ResolvedTypeAlias],
    model_property_policy: TsModelPropertyPolicy,
    nullable_policy: TsNullablePolicy,
) -> Result<String, CoreError> {
    let mut out = String::new();

    check_unique_schema_names(graph, "TypeScript SDK")?;

    for (i, schema) in graph.schemas.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match &schema.body {
            Type::Enum(members) => emit_runtime_enum_alias(&mut out, &schema.name, members)?,
            Type::Object(fields) => emit_interface_with_policies(
                &mut out,
                &schema.name,
                fields,
                graph,
                "",
                model_property_policy,
                nullable_policy,
            )?,
            Type::Primitive(_)
            | Type::WellKnown(_)
            | Type::Array(_)
            | Type::Map { .. }
            | Type::Named(_)
            | Type::Union(_)
            | Type::Any {} => {
                let alias = ts_type(&schema.body, false, graph, "")?;
                writeln!(out, "export type {} = {alias};", schema.name).map_err(sink)?;
            }
        }
    }
    for alias in aliases {
        if !out.is_empty() {
            out.push('\n');
        }
        writeln!(out, "export type {} = {};", alias.alias, alias.canonical).map_err(sink)?;
    }
    Ok(out)
}

/// Emit one model schema into its own TypeScript file.
pub(crate) fn emit_model_schema_with_policies(
    graph: &ApiGraph,
    schema: &crate::graph::Schema,
    model_property_policy: TsModelPropertyPolicy,
    nullable_policy: TsNullablePolicy,
) -> Result<String, CoreError> {
    check_unique_schema_names(graph, "TypeScript SDK")?;
    let mut out = String::new();
    out.push_str("import type * as models from \"./index\";\n\n");
    match &schema.body {
        Type::Enum(members) => emit_enum_alias(&mut out, &schema.name, members)?,
        Type::Object(fields) => emit_interface_with_policies(
            &mut out,
            &schema.name,
            fields,
            graph,
            "models.",
            model_property_policy,
            nullable_policy,
        )?,
        Type::Primitive(_)
        | Type::WellKnown(_)
        | Type::Array(_)
        | Type::Map { .. }
        | Type::Named(_)
        | Type::Union(_)
        | Type::Any {} => {
            let alias = ts_type(&schema.body, false, graph, "models.")?;
            writeln!(out, "export type {} = {alias};", schema.name).map_err(sink)?;
        }
    }
    Ok(out)
}

/// Emit a split-model compatibility alias shim.
pub(crate) fn emit_model_alias(alias: &ResolvedTypeAlias) -> String {
    format!(
        "import type {{ {} }} from \"./{}\";\n\nexport type {} = {};\n",
        alias.canonical,
        file_stem(&alias.canonical),
        alias.alias,
        alias.canonical
    )
}

/// Emit a named enum alias: `export type {name} = "a" | "b";` (members in graph order). The wire string
/// IS the literal — a string-literal union has no member *identifier* to sanitize (so the Python
/// `SCREAMING_SNAKE`/keyword machinery has no analog here; RESEARCH Pitfall 6) — but the literal VALUE
/// still must be escaped for a TS double-quoted string literal so an embedded `"`/`\`/control char does
/// not break `tsc` or silently corrupt the literal type (CR-01); [`ts_string_literal`] does both.
fn emit_enum_alias(out: &mut String, name: &str, members: &[String]) -> Result<(), CoreError> {
    if members.is_empty() {
        // An empty closed set has no inhabitants — `never` is the precise TS type.
        writeln!(out, "export type {name} = never;").map_err(sink)?;
        return Ok(());
    }
    let lits: Vec<String> = members.iter().map(|m| ts_string_literal(m)).collect();
    writeln!(out, "export type {name} = {};", lits.join(" | ")).map_err(sink)?;
    Ok(())
}

fn emit_runtime_enum_alias(
    out: &mut String,
    name: &str,
    members: &[String],
) -> Result<(), CoreError> {
    if members.is_empty() {
        writeln!(out, "export const {name} = {{}} as const;").map_err(sink)?;
        writeln!(out, "export type {name} = never;").map_err(sink)?;
        return Ok(());
    }
    writeln!(out, "export const {name} = {{").map_err(sink)?;
    let keys = enum_member_keys(members);
    for (key, value) in keys.iter().zip(members.iter()) {
        writeln!(out, "  {key}: {},", ts_string_literal(value)).map_err(sink)?;
    }
    writeln!(out, "}} as const;").map_err(sink)?;
    writeln!(
        out,
        "export type {name} = typeof {name}[keyof typeof {name}];"
    )
    .map_err(sink)?;
    Ok(())
}

fn enum_member_keys(members: &[String]) -> Vec<String> {
    let mut used = Vec::new();
    members
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let mut candidate = pascal_identifier(value);
            if candidate.is_empty() {
                candidate = format!("Value{index}");
            }
            if used.contains(&candidate) {
                let base = candidate.clone();
                let mut suffix = 2;
                while used.contains(&candidate) {
                    candidate = format!("{base}{suffix}");
                    suffix += 1;
                }
            }
            used.push(candidate.clone());
            candidate
        })
        .collect()
}

fn pascal_identifier(value: &str) -> String {
    let mut out = String::new();
    for word in split_words(value) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    out
}

/// Emit an `export interface` for an object schema, one field line per field in GRAPH ORDER.
///
/// Unlike the Python `@dataclass` twin there is NO required-first partition (TS `?:` is order-free —
/// RESEARCH Pitfall 6) and NO runtime decoder: an `interface` is zero-runtime and `await res.json() as X`
/// suffices (Assumption A1). The two independent axes are: `optional` → `?:` at the field site;
/// `nullable` → `| null` inside [`ts_type`]. All four combinations are representable.
fn emit_interface_with_policies(
    out: &mut String,
    name: &str,
    fields: &[Field],
    graph: &ApiGraph,
    ns: &str,
    model_property_policy: TsModelPropertyPolicy,
    nullable_policy: TsNullablePolicy,
) -> Result<(), CoreError> {
    if fields.is_empty() {
        // eslint/tsc dislikes `{}` as a type; an empty record is the precise zero-field shape.
        writeln!(out, "export interface {name} {{}}").map_err(sink)?;
        return Ok(());
    }
    writeln!(out, "export interface {name} {{").map_err(sink)?;
    for field in fields {
        // The wire key (`json_name`) is the property name. A plain identifier is emitted bare; any
        // other wire key (kebab-case, leading digit, spaces, empty) is NOT a legal bare member name,
        // so it is emitted as a QUOTED + escaped string-literal member — TS accepts any string literal
        // as a member name, and the quoted form keeps the wire key EXACT (CR-02). One deterministic
        // rule, no fallback (rule 3). An interface field references its sibling model/enum symbols
        // BARE — same file (models.ts).
        let key = if is_ident(&field.json_name) {
            field.json_name.clone()
        } else {
            ts_string_literal(&field.json_name)
        };
        let effective_optional =
            model_property_policy.field_optional(field.required, field.optional);
        let effective_nullable = nullable_policy.field_nullable(effective_optional, field.nullable);
        let hint = ts_type(&field.schema, effective_nullable, graph, ns)?;
        let opt = if effective_optional { "?" } else { "" };
        writeln!(out, "  {key}{opt}: {hint};").map_err(sink)?;
    }
    writeln!(out, "}}").map_err(sink)?;
    Ok(())
}

/// Emit `errors.ts`: the typed `ApiError extends Error` carrying the HTTP status + decoded body, with an
/// `isNotFound()` helper. `package` is unused in the body but kept for call-site symmetry with the twin.
pub(crate) fn emit_errors(_package: &str) -> String {
    "\
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly body: unknown,
  ) {
    super(`HTTP ${status}`);
    this.name = \"ApiError\";
  }

  isNotFound(): boolean {
    return this.status === 404;
  }
}
"
    .to_string()
}

/// Emit `client.ts`'s header + the dependency-free `Client` backed by the platform `fetch` global.
///
/// The operation methods (one per graph operation) are appended to this same file by [`emit_operations`]
/// and re-frame into `client.ts`. The `Client` holds a normalized `baseUrl` and a `fetchFn` defaulting to
/// the platform `fetch` — the swappable injectable transport seam (RESEARCH Pattern 3). No `axios` /
/// `node-fetch` import; `typeof fetch` needs the DOM lib at typecheck time (handled by the test's
/// `--lib es2022,dom`).
#[cfg(test)]
pub(crate) fn emit_client(package: &str) -> String {
    emit_client_with_models(package, "models", None)
}

/// Emit `client.ts` with a configurable model-barrel import path.
pub(crate) fn emit_client_with_models(
    _package: &str,
    model_module: &str,
    auth_header: Option<&str>,
) -> String {
    let api_key_option = if auth_header.is_some() {
        "  apiKey?: string;\n"
    } else {
        ""
    };
    let api_key_field = if auth_header.is_some() {
        "  private readonly apiKey?: string;\n"
    } else {
        ""
    };
    let api_key_assign = if auth_header.is_some() {
        "    this.apiKey = opts.apiKey;\n"
    } else {
        ""
    };
    format!(
        "\
import {{ ApiError }} from \"./errors\";
import * as models from \"./{model_module}\";

export interface ClientOptions {{
  baseUrl: string;
  fetch?: typeof fetch;
{api_key_option}}}

export class Client {{
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;
{api_key_field}

  constructor(opts: ClientOptions) {{
    this.baseUrl = opts.baseUrl.replace(/\\/+$/, \"\");
    this.fetchFn = opts.fetch ?? fetch;
{api_key_assign}  }}
"
    )
}

/// Emit `client.ts`'s operation methods (appended to the client file by [`generate`]).
///
/// `ops` are all of the graph's operations, in graph order. Each method:
/// - takes path params as positional args, then a typed `body` arg for body-bearing ops, then required
///   query params (positional), then optional query params (each defaulting to `undefined`);
/// - interpolates each path param through `encodeURIComponent(String(value))` (V5 path-injection
///   mitigation — twin of Go `url.PathEscape` / Python `urllib.quote(safe='')`); builds the query with a
///   `URLSearchParams`; joins `base_path` + `op.path`;
/// - `await`s `this.fetchFn`, throws `ApiError` for non-2xx responses, and returns decoded JSON only
///   for success statuses that declare a body model.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling body/response `$ref`, or a path whose templated tokens do
/// not match its declared path params.
pub(crate) fn emit_operations(
    graph: &ApiGraph,
    _package: &str,
    base_path: &str,
    ops: &[&Operation],
    auth_header: Option<&str>,
) -> Result<String, CoreError> {
    let mut out = String::new();
    for op in ops {
        out.push('\n');
        emit_operation(&mut out, op, graph, base_path, auth_header)?;
    }
    // Close the `class Client {` opened by emit_client.
    out.push_str("}\n");
    Ok(out)
}

/// The collision-checked TypeScript identifiers for one operation's arguments.
///
/// Each `*_idents` vector aligns positionally with its params vector. Path params and required query
/// params are positional (no default); optional query params take a `?: T` default.
struct ResolvedArgs<'op> {
    path_idents: Vec<String>,
    required_query: Vec<&'op Param>,
    required_query_idents: Vec<String>,
    optional_query: Vec<&'op Param>,
    optional_query_idents: Vec<String>,
}

/// Reserved argument name a generated method already binds (`body`), which a path/query param must not
/// collide with (it would shadow the typed body argument — WR-03 analog).
const RESERVED_ARGS: &[&str] = &["body"];

/// Resolve + collision-check every operation argument's TypeScript identifier (WR-03 / WR-01 analog).
///
/// Each identifier is the `camelCase` form of the param name; the set is tracked as it grows so a
/// collision (two params whose identifier matches, or a param colliding with the bound `body`) is a
/// typed [`CoreError::SdkGen`] rather than a TS "duplicate parameter" error. Query params are split
/// required-first (positional) / optional-last (`?: T`) so all non-defaulted args precede optional ones.
/// One deterministic pass, no fallback (rule 3).
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
    let mut used_args: Vec<String> = if has_body {
        RESERVED_ARGS.iter().map(|s| (*s).to_string()).collect()
    } else {
        Vec::new()
    };
    let mut reserve = |name: &str| -> Result<String, CoreError> {
        let ident = camel(name);
        // A param name that tokenizes to nothing (e.g. `"_"`, `"-"`, or empty) would emit `: T` with
        // no binding name → invalid TS. Reject it with a typed error rather than emit broken code
        // (WR-01). One deterministic check, no fallback (rule 3).
        if ident.is_empty() {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter named '{name}' that yields an empty TypeScript \
                     identifier (no usable binding name)",
                    op.id
                ),
            });
        }
        if used_args.contains(&ident) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter whose TypeScript identifier '{ident}' collides \
                     with another argument (body or another param)",
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
    // ones take the `?: T` default.
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

/// Emit a single operation method (2-space indented as a `Client` method body).
fn emit_operation(
    out: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
    auth_header: Option<&str>,
) -> Result<(), CoreError> {
    let method_name = camel(&op.handler);
    let abs = join_path(base_path, &op.path);
    let tokens = path_tokens(&abs);

    let path_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "path").collect();
    let query_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "query").collect();

    // The templated path tokens must be exactly the declared path params (order-independent set
    // equality), so neither a dangling token nor an unused arg can slip through (twin of WR-03).
    // `param_set` is built sorted for a stable error message.
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

    let body_model = body_model_of(op, graph)?;
    let success = success_responses_of(op, graph)?;
    let return_model = success.body_model.clone();
    // A typed body/response references a model symbol re-exported from ./models; reference it through the
    // `models` namespace import so client.ts has no per-name import to compute (determinism).
    let return_ty = return_model.as_ref().map_or_else(
        || "void".to_string(),
        |m| {
            if success.has_bodyless_alternative() {
                format!("models.{m} | undefined")
            } else {
                format!("models.{m}")
            }
        },
    );

    let ResolvedArgs {
        path_idents,
        required_query,
        required_query_idents,
        optional_query,
        optional_query_idents,
    } = resolve_op_args(op, &path_params, &query_params, body_model.is_some())?;

    // Signature: path params (positional), body (typed), required query (positional), then optional
    // query params (`?: T`). All non-optional args precede optional ones (TS requirement).
    let mut args: Vec<String> = Vec::new();
    // A param type emitted into client.ts must reach a named model/enum through the `models` namespace
    // import (the symbols live in models.ts, not in scope here) — pass the `"models."` prefix so a named
    // enum param (e.g. `format: models.BookFormat`) resolves instead of emitting a bare TS2304 name.
    for (p, ident) in path_params.iter().zip(path_idents.iter()) {
        let ty = ts_type(&p.schema, false, graph, "models.")?;
        args.push(format!("{ident}: {ty}"));
    }
    if let Some(model) = &body_model {
        args.push(format!("body: models.{model}"));
    }
    for (p, ident) in required_query.iter().zip(required_query_idents.iter()) {
        let ty = ts_type(&p.schema, false, graph, "models.")?;
        args.push(format!("{ident}: {ty}"));
    }
    for (p, ident) in optional_query.iter().zip(optional_query_idents.iter()) {
        let ty = ts_type(&p.schema, false, graph, "models.")?;
        args.push(format!("{ident}?: {ty}"));
    }

    let ret_promise = if return_model.is_some() {
        format!("Promise<{return_ty}>")
    } else {
        "Promise<void>".to_string()
    };
    writeln!(
        out,
        "\n  async {method_name}({}): {ret_promise} {{",
        args.join(", ")
    )
    .map_err(sink)?;

    emit_op_path(out, &abs, &tokens, &path_params, &path_idents)?;
    emit_op_query(
        out,
        &query_params,
        &required_query,
        &required_query_idents,
        &optional_query,
        &optional_query_idents,
    )?;
    emit_op_dispatch(
        out,
        &op.method,
        &success.body_statuses,
        success.has_bodyless_alternative(),
        body_model.is_some(),
        return_model.as_deref(),
        auth_header,
    )?;
    writeln!(out, "  }}").map_err(sink)?;
    Ok(())
}

/// Emit the `let path = …` line for one operation: a template literal with each path param
/// percent-escaped (V5). TS template literals have no backslash restriction, so no Python-style `safe=''`
/// workaround is needed (Pitfall 6).
fn emit_op_path(
    out: &mut String,
    abs: &str,
    tokens: &[String],
    path_params: &[&Param],
    path_idents: &[String],
) -> Result<(), CoreError> {
    if tokens.is_empty() {
        writeln!(out, "    let path = `{abs}`;").map_err(sink)?;
        return Ok(());
    }
    let mut tmpl = abs.to_string();
    for token in tokens {
        let ident = path_params
            .iter()
            .zip(path_idents.iter())
            .find(|(pp, _)| &pp.name == token)
            .map_or_else(|| camel(token), |(_, id)| id.clone());
        let placeholder = format!("{{{token}}}");
        let interp = format!("${{encodeURIComponent(String({ident}))}}");
        tmpl = tmpl.replace(&placeholder, &interp);
    }
    writeln!(out, "    let path = `{tmpl}`;").map_err(sink)?;
    Ok(())
}

/// Emit the `URLSearchParams` query-building block for one operation (WR-01 analog): a REQUIRED query
/// param is always appended (positional arg, no guard); an OPTIONAL one is appended only when defined.
/// The wire key stays the ORIGINAL `p.name`.
fn emit_op_query(
    out: &mut String,
    query_params: &[&Param],
    required_query: &[&Param],
    required_query_idents: &[String],
    optional_query: &[&Param],
    optional_query_idents: &[String],
) -> Result<(), CoreError> {
    if query_params.is_empty() {
        return Ok(());
    }
    writeln!(out, "    const searchParams = new URLSearchParams();").map_err(sink)?;
    for (p, ident) in required_query.iter().zip(required_query_idents.iter()) {
        writeln!(
            out,
            "    searchParams.set(\"{}\", String({ident}));",
            p.name
        )
        .map_err(sink)?;
    }
    for (p, ident) in optional_query.iter().zip(optional_query_idents.iter()) {
        writeln!(out, "    if ({ident} !== undefined) {{").map_err(sink)?;
        writeln!(
            out,
            "      searchParams.set(\"{}\", String({ident}));",
            p.name
        )
        .map_err(sink)?;
        writeln!(out, "    }}").map_err(sink)?;
    }
    writeln!(out, "    const qs = searchParams.toString();").map_err(sink)?;
    writeln!(out, "    if (qs) {{").map_err(sink)?;
    writeln!(out, "      path = path + \"?\" + qs;").map_err(sink)?;
    writeln!(out, "    }}").map_err(sink)?;
    Ok(())
}

/// Emit the fetch dispatch block: await fetch, reject non-2xx responses, and cast decoded JSON only for
/// body-bearing success statuses. The request carries a JSON body only for body-bearing ops.
fn emit_op_dispatch(
    out: &mut String,
    method: &str,
    body_statuses: &[u16],
    has_bodyless_success: bool,
    has_body: bool,
    return_model: Option<&str>,
    auth_header: Option<&str>,
) -> Result<(), CoreError> {
    writeln!(out, "    const headers: Record<string, string> = {{}};").map_err(sink)?;
    if let Some(header) = auth_header {
        writeln!(out, "    if (this.apiKey) {{").map_err(sink)?;
        writeln!(
            out,
            "      headers[{}] = this.apiKey;",
            quoted_string_literal(header)
        )
        .map_err(sink)?;
        writeln!(out, "    }}").map_err(sink)?;
    }
    if has_body {
        writeln!(out, "    headers[\"Content-Type\"] = \"application/json\";").map_err(sink)?;
    }
    writeln!(
        out,
        "    const res = await this.fetchFn(`${{this.baseUrl}}${{path}}`, {{"
    )
    .map_err(sink)?;
    writeln!(out, "      method: \"{method}\",").map_err(sink)?;
    writeln!(out, "      headers,").map_err(sink)?;
    if has_body {
        writeln!(out, "      body: JSON.stringify(body),").map_err(sink)?;
    }
    writeln!(out, "    }});").map_err(sink)?;
    writeln!(out, "    if (res.status < 200 || res.status >= 300) {{").map_err(sink)?;
    writeln!(
        out,
        "      throw new ApiError(res.status, await res.json().catch(() => null));"
    )
    .map_err(sink)?;
    writeln!(out, "    }}").map_err(sink)?;
    if let Some(model) = return_model {
        writeln!(
            out,
            "    if ({}) {{",
            ts_status_match("res.status", body_statuses)
        )
        .map_err(sink)?;
        writeln!(out, "      return (await res.json()) as models.{model};").map_err(sink)?;
        writeln!(out, "    }}").map_err(sink)?;
        if !has_bodyless_success {
            writeln!(
                out,
                "    throw new ApiError(res.status, await res.json().catch(() => null));"
            )
            .map_err(sink)?;
        }
    }
    Ok(())
}

fn ts_status_match(expr: &str, statuses: &[u16]) -> String {
    statuses
        .iter()
        .map(|status| format!("{expr} === {status}"))
        .collect::<Vec<_>>()
        .join(" || ")
}

/// Emit `index.ts`: re-export `Client`, `ApiError`, and every named model/enum symbol so a consumer can
/// `import { Client, Book } from "<pkg>"`. Symbols are emitted in graph order (deterministic). Twin of
/// `pysdk::emit::emit_init`.
///
/// Shares the SINGLE [`check_unique_schema_names`] validator with [`emit_models`] so the re-export
/// surface can never drift from the model definitions: a duplicate schema name is rejected here too
/// (WR-05), regardless of the `generate` call order (this runs before `emit_models`). One source of
/// truth, no second rule (rule 3).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] when two schemas map to the same TypeScript symbol name.
#[cfg(test)]
pub(crate) fn emit_index(graph: &ApiGraph, package: &str) -> Result<String, CoreError> {
    emit_index_with_models(graph, package, "models", &[])
}

/// Emit `index.ts` with a configurable model-barrel export path.
pub(crate) fn emit_index_with_models(
    graph: &ApiGraph,
    _package: &str,
    model_module: &str,
    aliases: &[ResolvedTypeAlias],
) -> Result<String, CoreError> {
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let mut out = String::new();
    out.push_str("export { Client } from \"./client\";\n");
    out.push_str("export type { ClientOptions } from \"./client\";\n");
    out.push_str("export { ApiError } from \"./errors\";\n");

    // Every named schema becomes a top-level symbol in models.ts (interface or type) — re-export them
    // all as types (interfaces and `type` aliases are type-only re-exports).
    let mut names: Vec<&str> = graph.schemas.iter().map(|s| s.name.as_str()).collect();
    names.extend(aliases.iter().map(|alias| alias.alias.as_str()));
    if !names.is_empty() {
        out.push_str("export type {\n");
        for name in &names {
            writeln!(out, "  {name},").map_err(sink)?;
        }
        writeln!(out, "}} from \"./{model_module}\";").map_err(sink)?;
    }
    Ok(out)
}

/// Emit `models/index.ts` for split-model layout.
pub(crate) fn emit_models_index(
    graph: &ApiGraph,
    aliases: &[ResolvedTypeAlias],
) -> Result<String, CoreError> {
    check_unique_schema_names(graph, "TypeScript SDK")?;
    let mut out = String::new();
    for schema in &graph.schemas {
        writeln!(out, "export * from \"./{}\";", file_stem(&schema.name)).map_err(sink)?;
    }
    for alias in aliases {
        writeln!(out, "export * from \"./{}\";", file_stem(&alias.alias)).map_err(sink)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        camel, emit_client, emit_errors, emit_index, emit_models, emit_operations, ts_type,
    };
    use crate::graph::{ApiGraph, Operation, Prim, Type};

    /// A facts document covering the bookstore shapes that diverge from the Go target: a named enum
    /// (`BookFormat`), a named union (`BookOrError`), an inline union field (`Book.rating: number |
    /// number`), an inline enum field (`BookFilters.sort: "asc" | "desc"`), plus required / optional /
    /// nullable mixes to prove the two independent field axes.
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
          "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 }
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
          "span": { "file": "/root/m.ts", "start_line": 2, "end_line": 2 }
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
          "span": { "file": "/root/m.ts", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "app.models.BookFormat", "name": "BookFormat",
          "body": { "type": "enum", "of": ["hardcover", "paperback"] },
          "span": { "file": "/root/m.ts", "start_line": 4, "end_line": 4 }
        },
        {
          "id": "app.models.BookOrError", "name": "BookOrError",
          "body": { "type": "union", "of": [
            { "type": "named", "of": "app.models.Book" },
            { "type": "named", "of": "app.models.OutOfStock" }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 5, "end_line": 5 }
        },
        {
          "id": "app.models.OutOfStock", "name": "OutOfStock",
          "body": { "type": "object", "of": [
            { "json_name": "reason", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 6, "end_line": 6 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    mod casing {
        use super::camel;

        #[test]
        fn helper_produces_typescript_camel_case() {
            assert_eq!(camel("createBook"), "createBook");
            assert_eq!(camel("list_books"), "listBooks");
            assert_eq!(camel("get-book"), "getBook");
            assert_eq!(camel("HTTPServer"), "httpServer");
        }
    }

    mod type_mapping {
        use super::{sample_graph, ts_type, ApiGraph, Prim, Type};

        #[test]
        fn primitives_and_wellknown_map_to_typescript_scalars() {
            let g = ApiGraph::default();
            assert_eq!(
                ts_type(&Type::Primitive(Prim::String), false, &g, "").unwrap(),
                "string"
            );
            assert_eq!(
                ts_type(&Type::Primitive(Prim::Bool), false, &g, "").unwrap(),
                "boolean"
            );
            assert_eq!(
                ts_type(
                    &Type::Primitive(Prim::Int {
                        bits: 64,
                        signed: true
                    }),
                    false,
                    &g,
                    "",
                )
                .unwrap(),
                "number"
            );
            assert_eq!(
                ts_type(&Type::Primitive(Prim::Float { bits: 64 }), false, &g, "").unwrap(),
                "number"
            );
            // a bytes primitive carries base64 as a string.
            assert_eq!(
                ts_type(&Type::Primitive(Prim::Bytes), false, &g, "").unwrap(),
                "string"
            );
            // a date-time well-known carries as a string (A7).
            assert_eq!(
                ts_type(
                    &Type::WellKnown(crate::graph::WellKnown::DateTime),
                    false,
                    &g,
                    "",
                )
                .unwrap(),
                "string"
            );
        }

        #[test]
        fn nullable_wraps_the_type_with_pipe_null() {
            let g = ApiGraph::default();
            assert_eq!(
                ts_type(&Type::Primitive(Prim::String), true, &g, "").unwrap(),
                "string | null"
            );
        }

        #[test]
        fn inline_union_becomes_pipe_union_the_go_target_rejects() {
            // Book.rating-shaped inline union: the case go_type errors on. TS emits number | number.
            let g = ApiGraph::default();
            let rating = Type::Union(vec![
                Type::Primitive(Prim::Int {
                    bits: 64,
                    signed: true,
                }),
                Type::Primitive(Prim::Float { bits: 64 }),
            ]);
            assert_eq!(ts_type(&rating, false, &g, "").unwrap(), "number | number");
            // nullable wraps the whole union.
            assert_eq!(
                ts_type(&rating, true, &g, "").unwrap(),
                "number | number | null"
            );
        }

        #[test]
        fn inline_enum_becomes_string_literal_union_the_go_target_rejects() {
            // BookFilters.sort-shaped inline enum: go_type errors; TS emits a literal union, graph order.
            let g = ApiGraph::default();
            let sort = Type::Enum(vec!["asc".to_string(), "desc".to_string()]);
            assert_eq!(ts_type(&sort, false, &g, "").unwrap(), "\"asc\" | \"desc\"");
        }

        #[test]
        fn named_ref_resolves_to_the_schema_name() {
            let g = sample_graph();
            let named = Type::Named("app.models.BookFormat".to_string());
            assert_eq!(ts_type(&named, false, &g, "").unwrap(), "BookFormat");
            assert_eq!(ts_type(&named, true, &g, "").unwrap(), "BookFormat | null");
        }

        #[test]
        fn named_ref_is_namespace_qualified_for_client_ts_context() {
            // In client.ts a named enum/model param must reach models.ts through the `models` namespace
            // import — passing `ns = "models."` qualifies the symbol so it is in scope (regression for
            // the TS2304 `Cannot find name 'BookFormat'` codegen bug the tssdk_compile gate caught).
            let g = sample_graph();
            let named = Type::Named("app.models.BookFormat".to_string());
            assert_eq!(
                ts_type(&named, false, &g, "models.").unwrap(),
                "models.BookFormat"
            );
            assert_eq!(
                ts_type(&named, true, &g, "models.").unwrap(),
                "models.BookFormat | null"
            );
            // An array of a named ref carries the prefix through the element type.
            let arr = Type::Array(Box::new(named));
            assert_eq!(
                ts_type(&arr, false, &g, "models.").unwrap(),
                "models.BookFormat[]"
            );
        }

        #[test]
        fn named_union_resolves_each_variant_to_its_symbol_name() {
            // BookOrError = Book | OutOfStock.
            let g = sample_graph();
            let body = g.schemas.iter().find(|s| s.name == "BookOrError").unwrap();
            assert_eq!(
                ts_type(&body.body, false, &g, "").unwrap(),
                "Book | OutOfStock"
            );
        }

        #[test]
        fn array_and_map_and_any_map_to_typescript_generics() {
            let g = ApiGraph::default();
            let arr = Type::Array(Box::new(Type::Primitive(Prim::String)));
            assert_eq!(ts_type(&arr, false, &g, "").unwrap(), "string[]");
            let map = Type::Map {
                key: Box::new(Type::Primitive(Prim::String)),
                value: Box::new(Type::Primitive(Prim::Float { bits: 64 })),
            };
            // Open Q1: the typed Record<string, V> (value preserved, not widened to unknown).
            assert_eq!(
                ts_type(&map, false, &g, "").unwrap(),
                "Record<string, number>"
            );
            assert_eq!(ts_type(&Type::Any {}, false, &g, "").unwrap(), "unknown");
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
            let err = ts_type(&map, false, &g, "").unwrap_err();
            assert!(
                err.to_string()
                    .contains("cannot be represented as a TypeScript JSON object key"),
                "{err}"
            );
        }

        #[test]
        fn inline_object_is_a_typed_error_parity_with_go_and_python() {
            let g = ApiGraph::default();
            let obj = Type::Object(vec![]);
            let err = ts_type(&obj, false, &g, "").unwrap_err();
            assert!(
                err.to_string()
                    .contains("inline object type is unsupported"),
                "{err}"
            );
        }

        #[test]
        fn dangling_named_ref_is_a_typed_error() {
            let g = ApiGraph::default();
            let err = ts_type(&Type::Named("dto.Nope".to_string()), false, &g, "").unwrap_err();
            assert!(err.to_string().contains("dangling $ref"), "{err}");
        }
    }

    mod models {
        use super::{emit_models, sample_graph, Type};
        use crate::sdk::typescript::{TsModelPropertyPolicy, TsNullablePolicy};
        use crate::tssdk::emit::emit_models_with_aliases_and_policies;

        #[test]
        fn named_enum_emits_string_literal_union_type_alias_in_graph_order() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(
                out.contains("export type BookFormat = \"hardcover\" | \"paperback\";"),
                "named enum must be a string-literal-union type alias in graph order:\n{out}"
            );
        }

        #[test]
        fn named_union_emits_a_plain_type_alias_no_forward_ref_hack() {
            // BookOrError = Book | OutOfStock. TS type aliases are order-free — emit directly, NO
            // PEP-484-style string forward reference (RESEARCH Pitfall 6).
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(
                out.contains("export type BookOrError = Book | OutOfStock;"),
                "named union must be a plain order-free type alias:\n{out}"
            );
            // the forward-ref hack (a quoted RHS) must NOT appear.
            assert!(
                !out.contains("export type BookOrError = \"Book"),
                "no forward-ref string hack:\n{out}"
            );
        }

        #[test]
        fn object_emits_an_interface_with_the_two_independent_field_axes() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            assert!(out.contains("export interface BookFilters {"), "{out}");
            // required, non-nullable.
            assert!(
                out.contains("  genre: string;"),
                "required non-nullable:\n{out}"
            );
            // optional (`?:`), non-nullable.
            assert!(
                out.contains("  in_stock?: boolean;"),
                "optional non-nullable `?:`:\n{out}"
            );
            // required, nullable (`| null`, no `?`).
            assert!(
                out.contains("  published: string | null;"),
                "required nullable `| null`:\n{out}"
            );
            // optional inline enum field (`?:` + literal union).
            assert!(
                out.contains("  sort?: \"asc\" | \"desc\";"),
                "optional inline enum field:\n{out}"
            );
        }

        #[test]
        fn object_emits_fields_in_graph_order_no_required_first_partition() {
            // The graph sorts fields alphabetically: genre, in_stock, published, sort. TS `?:` is
            // order-free, so the emitter must NOT reorder required-first (RESEARCH Pitfall 6).
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            let genre = out.find("  genre").unwrap();
            let in_stock = out.find("  in_stock").unwrap();
            let published = out.find("  published").unwrap();
            let sort = out.find("  sort").unwrap();
            assert!(
                genre < in_stock && in_stock < published && published < sort,
                "fields must be in graph order, not required-first:\n{out}"
            );
        }

        #[test]
        fn inline_union_field_uses_a_pipe_union_with_nullable() {
            let out = emit_models(&sample_graph(), "bookstore").unwrap();
            // Book.rating inline union, optional + nullable → `rating?: number | number | null;`.
            assert!(
                out.contains("  rating?: number | number | null;"),
                "inline union field optional+nullable:\n{out}"
            );
        }

        #[test]
        fn openapi_required_policy_uses_schema_required_not_source_optional_axis() {
            let mut graph = sample_graph();
            let Type::Object(fields) = &mut graph.schemas[1].body else {
                panic!("Book must be an object");
            };
            let title = fields
                .iter_mut()
                .find(|field| field.json_name == "title")
                .expect("Book.title field");
            title.required = false;
            title.optional = false;

            let out = emit_models_with_aliases_and_policies(
                &graph,
                &[],
                TsModelPropertyPolicy::OpenApiRequired,
                TsNullablePolicy::ExplicitNull,
            )
            .unwrap();

            assert!(
                out.contains("  title?: string;"),
                "schema-non-required field should emit `?` even when source optional axis is false:\n{out}"
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
    /// marshalling, success-status comparison, and the typed return cast.
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
          "span": { "file": "/root/main.ts", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/books/{book_id}", "handler": "getBook",
          "operation_id": "getBook",
          "params": [
            { "name": "book_id", "location": "path", "required": true,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "app.models.Book" } } ],
          "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listBooks",
          "operation_id": "listBooks",
          "params": [
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/main.ts", "start_line": 3, "end_line": 3 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": null } ],
          "span": { "file": "/root/main.ts", "start_line": 3, "end_line": 3 }
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
          "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 }
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
          "span": { "file": "/root/m.ts", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.OutOfStock", "name": "OutOfStock",
          "body": { "type": "object", "of": [
            { "json_name": "reason", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn ops_graph() -> ApiGraph {
        let facts = serde_json::from_slice(OPS_SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    mod operations {
        use super::{emit_operations, ops_graph, ApiGraph, Operation};

        fn ops_for<'g>(graph: &'g ApiGraph, handler: &str) -> Vec<&'g Operation> {
            graph
                .operations
                .iter()
                .filter(|o| o.handler == handler)
                .collect()
        }

        #[test]
        fn body_op_has_camel_method_typed_body_and_typed_return() {
            let g = ops_graph();
            let out =
                emit_operations(&g, "bookstore", "/", &ops_for(&g, "createBook"), None).unwrap();
            assert!(
                out.contains(
                    "async createBook(body: models.Book): Promise<models.CreatedMessage> {"
                ),
                "camel method, typed body, typed return:\n{out}"
            );
            assert!(
                out.contains("if (res.status < 200 || res.status >= 300) {"),
                "rejects only non-2xx statuses:\n{out}"
            );
            assert!(
                out.contains("throw new ApiError(res.status, await res.json().catch(() => null));"),
                "{out}"
            );
            // typed return casts the decoded JSON to the response interface.
            assert!(
                out.contains("return (await res.json()) as models.CreatedMessage;"),
                "{out}"
            );
            assert!(
                out.contains("body: JSON.stringify(body),"),
                "body op serializes the body:\n{out}"
            );
            assert!(out.contains("method: \"POST\","), "{out}");
        }

        #[test]
        fn typed_success_with_bodyless_alternate_returns_undefined_union_and_decodes_only_body_status(
        ) {
            let mut g = ops_graph();
            g.operations[0].responses.push(crate::graph::Response {
                status: 204,
                body: None,
            });
            g.operations[0]
                .responses
                .sort_by_key(|response| response.status);
            let out =
                emit_operations(&g, "bookstore", "/", &ops_for(&g, "createBook"), None).unwrap();
            assert!(
                out.contains(
                    "async createBook(body: models.Book): Promise<models.CreatedMessage | undefined> {"
                ),
                "bodyless alternate success should make the return type optional:\n{out}"
            );
            assert!(
                out.contains("if (res.status === 201) {"),
                "only the body-bearing status should decode:\n{out}"
            );
        }

        #[test]
        fn templated_path_escapes_each_param_with_encode_uri_component() {
            let g = ops_graph();
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "getBook"), None).unwrap();
            assert!(
                out.contains("let path = `/books/${encodeURIComponent(String(bookId))}`;"),
                "path param must be percent-escaped (V5) via a backslash-free template literal:\n{out}"
            );
            assert!(
                out.contains("async getBook(bookId: number): Promise<models.Book> {"),
                "{out}"
            );
        }

        #[test]
        fn query_op_encodes_present_params_and_has_no_body() {
            let g = ops_graph();
            let out =
                emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks"), None).unwrap();
            assert!(
                out.contains("async listBooks(cursor?: string): Promise<void> {"),
                "{out}"
            );
            assert!(out.contains("if (cursor !== undefined) {"), "{out}");
            assert!(
                out.contains("searchParams.set(\"cursor\", String(cursor));"),
                "{out}"
            );
            assert!(out.contains("path = path + \"?\" + qs;"), "{out}");
            // no body for a body-less op.
            assert!(
                !out.contains("JSON.stringify(body)"),
                "query op has no body:\n{out}"
            );
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
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [],
              "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let g = ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let err = emit_operations(&g, "bookstore", "/", &ops, None).unwrap_err();
            assert!(
                err.to_string().contains("do not match its path params"),
                "{err}"
            );
        }

        #[test]
        fn required_query_param_is_positional_and_always_sent() {
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/search", "handler": "search",
                  "operation_id": "search",
                  "params": [
                    { "name": "q", "location": "query", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } },
                    { "name": "page", "location": "query", "required": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [], "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let g = ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let out = emit_operations(&g, "pkg", "/", &ops, None).unwrap();
            // required `q` is positional (no `?:`), optional `page` keeps the `?:`.
            assert!(
                out.contains("async search(q: string, page?: string): Promise<void> {"),
                "{out}"
            );
            // required `q` unconditionally set; optional `page` guarded.
            assert!(out.contains("searchParams.set(\"q\", String(q));"), "{out}");
            assert!(out.contains("if (page !== undefined) {"), "{out}");
        }

        #[test]
        fn wr01_empty_camel_param_identifier_is_a_typed_error() {
            // A param name that tokenizes to nothing (`"_"`) yields an empty camel identifier → would
            // emit `: T` with no binding name. Reject with a typed error (WR-01).
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/x", "handler": "x",
                  "operation_id": "x",
                  "params": [
                    { "name": "_", "location": "query", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [], "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let g = ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let err = emit_operations(&g, "pkg", "/", &ops, None).unwrap_err();
            assert!(
                err.to_string().contains("empty TypeScript identifier"),
                "{err}"
            );
        }

        #[test]
        fn param_identifier_collision_is_a_typed_error() {
            // two query params whose camelCase identifier collides → typed error.
            let facts = br#"{
              "module": "app",
              "routes": [
                { "method": "GET", "path": "/x", "handler": "x",
                  "operation_id": "x",
                  "params": [
                    { "name": "foo_bar", "location": "query", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } },
                    { "name": "fooBar", "location": "query", "required": true,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
                  ],
                  "request_body": null,
                  "responses": [ { "status": 200, "body": null } ],
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "schemas": [], "diagnostics": []
            }"#;
            let facts = serde_json::from_slice(facts).unwrap();
            let g = ApiGraph::from_facts(facts, "/root");
            let ops: Vec<&Operation> = g.operations.iter().collect();
            let err = emit_operations(&g, "pkg", "/", &ops, None).unwrap_err();
            assert!(err.to_string().contains("collides"), "{err}");
        }
    }

    mod client_errors_index {
        use super::{emit_client, emit_errors, emit_index, ops_graph};

        #[test]
        fn client_uses_fetch_with_an_injectable_transport_and_no_third_party_imports() {
            let out = emit_client("bookstore");
            assert!(out.contains("fetch?: typeof fetch;"), "{out}");
            assert!(out.contains("this.fetchFn = opts.fetch ?? fetch;"), "{out}");
            assert!(
                out.contains("import { ApiError } from \"./errors\";"),
                "{out}"
            );
            // no third-party HTTP libs (TSSDK-01/02).
            assert!(!out.contains("axios"), "{out}");
            assert!(!out.contains("node-fetch"), "{out}");
            assert!(!out.contains("@types"), "{out}");
        }

        #[test]
        fn errors_define_typed_apierror_extends_error_with_is_not_found() {
            let out = emit_errors("bookstore");
            assert!(
                out.contains("export class ApiError extends Error {"),
                "{out}"
            );
            assert!(out.contains("public readonly status: number,"), "{out}");
            assert!(out.contains("public readonly body: unknown,"), "{out}");
            assert!(out.contains("isNotFound(): boolean {"), "{out}");
            assert!(out.contains("return this.status === 404;"), "{out}");
        }

        #[test]
        fn index_reexports_client_apierror_and_every_model() {
            let out = emit_index(&ops_graph(), "bookstore").unwrap();
            assert!(
                out.contains("export { Client } from \"./client\";"),
                "{out}"
            );
            assert!(
                out.contains("export { ApiError } from \"./errors\";"),
                "{out}"
            );
            assert!(out.contains("  Book,"), "{out}");
            assert!(out.contains("  CreatedMessage,"), "{out}");
            assert!(out.contains("} from \"./models\";"), "{out}");
        }
    }

    /// Regression locks for the WR-05 schema-name collision on a shape the fixtures do NOT exercise.
    mod regressions {
        use super::{emit_models, ApiGraph};

        fn graph_from(facts: &[u8]) -> ApiGraph {
            let facts = serde_json::from_slice(facts).unwrap();
            ApiGraph::from_facts(facts, "/root")
        }

        // CR-01: an enum member carrying a `"`/`\`/control char must be ESCAPED into a valid TS
        // string literal — an embedded `"` would break tsc, an embedded `\b` would silently corrupt
        // the literal type. The wire value must be preserved exactly (escaped, not stripped).
        #[test]
        fn cr01_named_enum_member_with_special_chars_is_escaped() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "a.Weird", "name": "Weird",
                  "body": { "type": "enum", "of": ["a\"b", "c\\d", "e\nf", "plain"] },
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "diagnostics": [] }"#;
            let out = emit_models(&graph_from(facts), "pkg").unwrap();
            // `"` is backslash-escaped (not a bare quote that would terminate the literal early).
            assert!(
                out.contains(r#""a\"b""#),
                "embedded quote must be escaped:\n{out}"
            );
            // `\` is doubled (so `\d` cannot become an escape sequence).
            assert!(
                out.contains(r#""c\\d""#),
                "backslash must be doubled:\n{out}"
            );
            // newline becomes `\n` (not a raw line break inside the literal).
            assert!(out.contains(r#""e\nf""#), "newline must be escaped:\n{out}");
            // an identifier-safe member is untouched (happy path unchanged).
            assert!(out.contains(r#""plain""#), "plain member unchanged:\n{out}");
            // the raw (unescaped) embedded quote must NOT appear as a bare `"a"b"`.
            assert!(
                !out.contains(r#""a"b""#),
                "raw unescaped quote must not leak:\n{out}"
            );
        }

        // CR-01 (inline-enum site): the SAME escaping must apply to an inline string-literal union.
        #[test]
        fn cr01_inline_enum_field_member_with_special_chars_is_escaped() {
            use super::ts_type;
            let g = ApiGraph::default();
            let e = crate::graph::Type::Enum(vec!["a\"b".to_string(), "c\\d".to_string()]);
            assert_eq!(ts_type(&e, false, &g, "").unwrap(), r#""a\"b" | "c\\d""#);
        }

        // CR-02: a non-identifier wire key (kebab-case, leading digit, spaces) must be emitted as a
        // QUOTED member name — a bare `content-type?:` is a tsc parse error. The wire key stays exact.
        #[test]
        fn cr02_non_identifier_field_name_is_quoted() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "a.Headers", "name": "Headers",
                  "body": { "type": "object", "of": [
                    { "json_name": "content-type", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null },
                    { "json_name": "123abc", "required": false, "optional": true, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                      "description": null, "example": null },
                    { "json_name": "user name", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "string" } },
                      "description": null, "example": null },
                    { "json_name": "plainKey", "required": true, "optional": false, "nullable": false,
                      "schema": { "type": "primitive", "of": { "prim": "bool" } },
                      "description": null, "example": null }
                  ] },
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } }
              ],
              "diagnostics": [] }"#;
            let out = emit_models(&graph_from(facts), "pkg").unwrap();
            // kebab-case key → quoted (the `?`/`:` stay OUTSIDE the quotes).
            assert!(
                out.contains(r#"  "content-type": string;"#),
                "kebab-case key must be quoted:\n{out}"
            );
            // leading-digit key → quoted, and the optional `?` stays outside the quotes.
            assert!(
                out.contains(r#"  "123abc"?: number;"#),
                "leading-digit optional key must be quoted with `?` outside:\n{out}"
            );
            // key with a space → quoted.
            assert!(
                out.contains(r#"  "user name": string;"#),
                "spaced key must be quoted:\n{out}"
            );
            // an identifier-safe key stays UNQUOTED (happy path unchanged — snapshot-safe).
            assert!(
                out.contains("  plainKey: boolean;"),
                "identifier-safe key stays bare:\n{out}"
            );
        }

        // WR-05: two distinct schema ids sharing a TS name is a typed error (no silent redeclaration).
        #[test]
        fn wr05_duplicate_schema_name_is_a_typed_error() {
            let facts = br#"{
              "module": "app", "routes": [],
              "schemas": [
                { "id": "a.Book", "name": "Book",
                  "body": { "type": "object", "of": [] },
                  "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 } },
                { "id": "b.Book", "name": "Book",
                  "body": { "type": "object", "of": [] },
                  "span": { "file": "/root/m.ts", "start_line": 2, "end_line": 2 } }
              ],
              "diagnostics": [] }"#;
            let g = graph_from(facts);
            let err = emit_models(&g, "pkg").unwrap_err();
            assert!(
                err.to_string().contains("share the TypeScript SDK name"),
                "{err}"
            );
        }
    }
}
