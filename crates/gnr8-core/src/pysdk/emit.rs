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

use crate::graph::{ApiGraph, Field, Operation, Param, Prim, Type};
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

/// Convert an identifier to `snake_case` (Python method/attribute name): `createBook` → `create_book`.
pub(crate) fn snake(name: &str) -> String {
    split_words(name)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert an enum member value to a `SCREAMING_SNAKE` identifier: `out-of-stock` → `OUT_OF_STOCK`.
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
pub(crate) fn py_type(
    schema: &Type,
    nullable: bool,
    graph: &ApiGraph,
) -> Result<String, CoreError> {
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
                // A module-level alias assignment is evaluated EAGERLY at import time (unlike a
                // @dataclass annotation, which `from __future__ import annotations` keeps lazy). The
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
    Ok(out)
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
    let (required, optional): (Vec<&Field>, Vec<&Field>) = fields.iter().partition(|f| !f.optional);
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

/// Emit `errors.py`: the typed `ApiError(Exception)` with status/message/slug/hints + `is_not_found()`.
///
/// `package` is unused in the body (no package clause in Python) but kept for call-site symmetry with
/// the Go twin's `emit_errors`. The `from __future__ import annotations` header keeps annotations lazy.
pub(crate) fn emit_errors(_package: &str) -> String {
    "\
from __future__ import annotations

from typing import Any, List, Optional


class ApiError(Exception):
    \"\"\"Raised by operation methods on a non-success response.

    Carries the HTTP status and the decoded error body (message/slug/hints).
    \"\"\"

    def __init__(
        self,
        status_code: int,
        message: str = \"\",
        slug: str = \"\",
        hints: Optional[List[Any]] = None,
    ) -> None:
        super().__init__(f\"{status_code} {message} ({slug})\")
        self.status_code = status_code
        self.message = message
        self.slug = slug
        self.hints = hints if hints is not None else []

    def is_not_found(self) -> bool:
        return self.status_code == 404
"
    .to_string()
}

/// Emit `client.py`: the dependency-free `Client` backed by an injectable `urllib` `OpenerDirector`.
///
/// The operation methods (one per graph operation) are appended to this same file by [`emit_operations`]
/// and re-frame into `client.py`. The `Client` holds a `base_url`, an optional `api_key`, and an
/// `OpenerDirector` defaulting to `urllib.request.build_opener()` — the swappable transport seam the
/// hermetic test injects (RESEARCH Pattern 3). `_do` builds a `urllib.request.Request`, sets the
/// `Content-Type`/`X-API-Key` headers, opens via the injected opener, and catches
/// `urllib.error.HTTPError` so 4xx/5xx return a `(code, body)` pair instead of raising (Pitfall 6).
pub(crate) fn emit_client(_package: &str) -> String {
    "\
from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Optional

from .errors import ApiError
from .models import *  # noqa: F401,F403  (re-export models for return-type annotations)


class Client:
    \"\"\"Dependency-free SDK client over urllib (no requests/httpx).\"\"\"

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
        opener: Optional[urllib.request.OpenerDirector] = None,
    ) -> None:
        self._base_url = base_url.rstrip(\"/\")
        self._api_key = api_key
        self._opener = opener or urllib.request.build_opener()

    def _do(self, method: str, path: str, *, body: Optional[Any] = None) -> tuple:
        data = json.dumps(body).encode(\"utf-8\") if body is not None else None
        req = urllib.request.Request(self._base_url + path, data=data, method=method)
        if data is not None:
            req.add_header(\"Content-Type\", \"application/json\")
        if self._api_key:
            req.add_header(\"X-API-Key\", self._api_key)
        try:
            with self._opener.open(req) as resp:
                return resp.status, resp.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    @staticmethod
    def _raise(status: int, raw: bytes) -> None:
        try:
            decoded = json.loads(raw) if raw else {}
        except ValueError:
            decoded = {}
        if not isinstance(decoded, dict):
            decoded = {}
        raise ApiError(
            status,
            decoded.get(\"message\", \"\"),
            decoded.get(\"slug\", \"\"),
            decoded.get(\"hints\"),
        )
"
    .to_string()
}

/// Join the `base_path` prefix with a group-relative operation path (slash-collapsed). Twin of
/// `gosdk::emit::join_path` — the SAME single source of truth (`ir.base_path`) the `OpenAPI` lowering uses.
fn join_path(base_path: &str, path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{trimmed}")
    }
}

/// Resolve an operation's primary success (lowest 2xx) response status + model name.
///
/// Returns the first 2xx response's status regardless of whether it carries a body; the model is `Some`
/// only when that response has a typed body. An operation with no 2xx response yields `None`. Twin of
/// `gosdk::emit::success_of`.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the success body `$ref` is dangling.
fn success_of(
    op: &Operation,
    graph: &ApiGraph,
) -> Result<Option<(u16, Option<String>)>, CoreError> {
    for resp in &op.responses {
        if (200..300).contains(&resp.status) {
            let model = match &resp.body {
                Some(body) => {
                    let model = graph
                        .schemas
                        .iter()
                        .find(|s| s.id == body.ref_id)
                        .ok_or_else(|| CoreError::SdkGen {
                            message: format!(
                                "operation '{}' success response references dangling $ref '{}'",
                                op.id, body.ref_id
                            ),
                        })?;
                    Some(model.name.clone())
                }
                None => None,
            };
            return Ok(Some((resp.status, model)));
        }
    }
    Ok(None)
}

/// Resolve an operation's request-body model name, if it has a typed body. Twin of
/// `gosdk::emit::body_model_of`.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the request-body `$ref` is dangling.
fn body_model_of(op: &Operation, graph: &ApiGraph) -> Result<Option<String>, CoreError> {
    let Some(body) = &op.request_body else {
        return Ok(None);
    };
    let model = graph
        .schemas
        .iter()
        .find(|s| s.id == body.ref_id)
        .ok_or_else(|| CoreError::SdkGen {
            message: format!(
                "operation '{}' request body references dangling $ref '{}'",
                op.id, body.ref_id
            ),
        })?;
    Ok(Some(model.name.clone()))
}

/// Extract the `{token}` placeholder names from a path template, in first-seen order. Twin of
/// `gosdk::emit::path_tokens`.
fn path_tokens(path: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            tokens.push(after[..close].to_string());
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    tokens
}

/// Emit `client.py`'s operation methods (appended to the client file by [`generate`]).
///
/// `ops` are all of the graph's operations, in graph order. Each method:
/// - takes `self`, then path params as positional args, then a typed `body` arg for body-bearing ops,
///   then optional query params (each defaulting to `None`);
/// - interpolates each path param through `urllib.parse.quote(str(value), safe="")` (V5 path-injection
///   mitigation — twin of Go `url.PathEscape`); builds the query with `urllib.parse.urlencode` over the
///   present optional params; joins `base_path` + `op.path`;
/// - calls `self._do`, and on a status != the operation's real success status raises `ApiError` via
///   `self._raise`; on success decodes JSON into the response dataclass (or returns the raw dict).
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
) -> Result<String, CoreError> {
    let mut out = String::new();
    for op in ops {
        out.push('\n');
        emit_operation(&mut out, op, graph, base_path)?;
    }
    Ok(out)
}

/// Emit a single operation method (4-space indented as a `Client` method body).
fn emit_operation(
    out: &mut String,
    op: &Operation,
    graph: &ApiGraph,
    base_path: &str,
) -> Result<(), CoreError> {
    let method_name = snake(&op.handler);
    let abs = join_path(base_path, &op.path);
    let tokens = path_tokens(&abs);

    let path_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "path").collect();
    let query_params: Vec<&Param> = op.params.iter().filter(|p| p.location == "query").collect();

    // The templated path tokens must be exactly the declared path params (set equality), so neither a
    // dangling token (a KeyError at runtime) nor an unused arg can slip through (twin of WR-03).
    let mut token_set: Vec<&str> = tokens.iter().map(String::as_str).collect();
    token_set.sort_unstable();
    let mut param_set: Vec<&str> = path_params.iter().map(|p| p.name.as_str()).collect();
    param_set.sort_unstable();
    if token_set != param_set {
        return Err(CoreError::SdkGen {
            message: format!(
                "operation '{}' path '{}' templated tokens {:?} do not match its path params {:?}",
                op.id, abs, tokens, param_set
            ),
        });
    }

    let body_model = body_model_of(op, graph)?;
    let success = success_of(op, graph)?;
    let success_status = success.as_ref().map_or(200, |(s, _)| *s);
    let return_model = success.as_ref().and_then(|(_, m)| m.clone());
    let return_hint = return_model.clone().unwrap_or_else(|| "Any".to_string());

    // Signature: self, path params (positional), body (typed), then optional query params (= None).
    let mut args: Vec<String> = vec!["self".to_string()];
    for p in &path_params {
        args.push(snake(&p.name));
    }
    if let Some(model) = &body_model {
        args.push(format!("body: {model}"));
    }
    for p in &query_params {
        args.push(format!("{}=None", snake(&p.name)));
    }

    writeln!(
        out,
        "    def {method_name}({}) -> {return_hint}:",
        args.join(", ")
    )
    .map_err(sink)?;

    // Build the path: f-string interpolation with each path param percent-escaped (V5).
    if tokens.is_empty() {
        writeln!(out, "        path = \"{abs}\"").map_err(sink)?;
    } else {
        let mut fstring = abs.clone();
        for token in &tokens {
            let placeholder = format!("{{{token}}}");
            // `safe=''` uses SINGLE quotes inside the double-quoted f-string: a backslash in an
            // f-string expression part is a `SyntaxError` on Python 3.9-3.11 ("f-string expression
            // part cannot include a backslash"), so escaped double-quotes (`safe=\"\"`) would not
            // compile. Single quotes need no escape and are valid on every Python 3.x (PYSDK-02).
            let escaped = format!("{{urllib.parse.quote(str({}), safe='')}}", snake(token));
            fstring = fstring.replace(&placeholder, &escaped);
        }
        writeln!(out, "        path = f\"{fstring}\"").map_err(sink)?;
    }

    // Query encoding: collect present (non-None) optional params, urlencode, append to the path.
    if !query_params.is_empty() {
        writeln!(out, "        _query = {{}}").map_err(sink)?;
        for p in &query_params {
            let arg = snake(&p.name);
            writeln!(out, "        if {arg} is not None:").map_err(sink)?;
            writeln!(out, "            _query[\"{}\"] = {arg}", p.name).map_err(sink)?;
        }
        writeln!(out, "        if _query:").map_err(sink)?;
        writeln!(
            out,
            "            path = path + \"?\" + urllib.parse.urlencode(_query)"
        )
        .map_err(sink)?;
    }

    // Dispatch: call _do, compare the real success status, raise ApiError otherwise, decode on success.
    let body_arg = if body_model.is_some() {
        ", body=body"
    } else {
        ""
    };
    writeln!(
        out,
        "        _status, _raw = self._do(\"{}\", path{body_arg})",
        op.method
    )
    .map_err(sink)?;
    writeln!(out, "        if _status != {success_status}:").map_err(sink)?;
    writeln!(out, "            self._raise(_status, _raw)").map_err(sink)?;
    if let Some(model) = &return_model {
        writeln!(out, "        _data = json.loads(_raw) if _raw else {{}}").map_err(sink)?;
        writeln!(out, "        return {model}(**_data)").map_err(sink)?;
    } else {
        writeln!(out, "        return json.loads(_raw) if _raw else None").map_err(sink)?;
    }
    Ok(())
}

/// Emit `__init__.py`: re-export `Client`, `ApiError`, and every model/enum class so `import <pkg>`
/// exposes the whole surface. Class names are emitted in graph order (deterministic). Twin of the Go
/// twin's single-package surface (Go has no `__init__`, so this is Python-specific but deterministic).
pub(crate) fn emit_init(graph: &ApiGraph, _package: &str) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n\n");
    out.push_str("from .client import Client\n");
    out.push_str("from .errors import ApiError\n");

    // Every named schema becomes a top-level symbol in models.py (class or alias) — re-export them all.
    let names: Vec<&str> = graph.schemas.iter().map(|s| s.name.as_str()).collect();
    if !names.is_empty() {
        out.push_str("from .models import (\n");
        for name in &names {
            let _ = writeln!(out, "    {name},");
        }
        out.push_str(")\n");
    }

    out.push_str("\n__all__ = [\n");
    out.push_str("    \"Client\",\n");
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
        emit_client, emit_errors, emit_init, emit_models, emit_operations, py_type,
        screaming_snake, snake,
    };
    use crate::graph::{ApiGraph, Operation, Prim, Type};

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
            assert!(
                out.contains("    genre: str\n"),
                "required has no default:\n{out}"
            );
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
                out.contains("def create_book(self, body: Book) -> CreatedMessage:"),
                "snake method, typed body, typed return:\n{out}"
            );
            // success status is the real 201, not a default 200.
            assert!(
                out.contains("if _status != 201:"),
                "compares real 201:\n{out}"
            );
            assert!(out.contains("self._raise(_status, _raw)"), "{out}");
            assert!(out.contains("return CreatedMessage(**_data)"), "{out}");
            assert!(
                out.contains("self._do(\"POST\", path, body=body)"),
                "body op passes body to _do:\n{out}"
            );
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
                out.contains("def get_book(self, book_id) -> Book:"),
                "{out}"
            );
        }

        #[test]
        fn query_op_encodes_present_params_and_has_no_body() {
            let g = ops_graph();
            let out = emit_operations(&g, "bookstore", "/", &ops_for(&g, "listBooks")).unwrap();
            assert!(
                out.contains("def list_books(self, cursor=None) -> Any:"),
                "{out}"
            );
            assert!(out.contains("if cursor is not None:"), "{out}");
            assert!(out.contains("_query[\"cursor\"] = cursor"), "{out}");
            assert!(
                out.contains("path = path + \"?\" + urllib.parse.urlencode(_query)"),
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
        use super::{emit_client, emit_errors, emit_init, ops_graph};

        #[test]
        fn client_has_injectable_opener_and_no_third_party_imports() {
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
}
