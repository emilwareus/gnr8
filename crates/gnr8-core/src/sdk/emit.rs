//! `format!`-based Go SDK emitters (D-05: no template engine; small internal templating only).
//!
//! Each emitter turns the router-agnostic [`crate::graph::ApiGraph`] into one idiomatic Go source file
//! matching the `fixtures/goalservice/expected/sdk/{client,models,errors,<tag>}.go` shape:
//!
//! - [`emit_models`]   — one struct per object [`Schema`], one `type X string` newtype + const block
//!   per enum [`Schema`]; Go field names are exported-CamelCase of the json tag (with Go initialisms),
//!   json tags carry `,omitempty` for optional fields, types follow TARGET-API.md §4.
//! - [`emit_client`]   — the functional-options `Client` (`NewClient`, `WithHTTPClient`, `WithAPIKey`).
//! - [`emit_operations`] — one file per tag: tag-grouped typed methods on `*Client`, `context.Context`
//!   first, path params as positional string args, a params struct for query-bearing ops, a typed body
//!   input; each method marshals the body, builds the request, sets `X-API-Key`, decodes 2xx into the
//!   success model and non-2xx into an [`APIError`].
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

use crate::graph::{ApiGraph, Field, Operation, Schema, SchemaType};
use crate::CoreError;

/// The fixed Go package name for the generated SDK (matches the fixture's `package goalservice`).
pub(crate) const PACKAGE: &str = "goalservice";

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
    out
}

/// Split an identifier into words on `_`/`-` separators and lower→upper case boundaries.
///
/// `workflowChainIds` → `["workflow", "Chain", "Ids"]`; `page_size` → `["page", "size"]`.
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

/// Map a graph [`SchemaType`] to its Go SDK type (TARGET-API.md §4), resolving refs to model names.
///
/// `optional` controls pointer wrapping for value types (`number`→`*float32`, `boolean`→`*bool`, etc.).
/// Strings, slices, and maps are already nilable in Go so they are never pointer-wrapped (matches
/// `expected/sdk/models.go`, where optional strings stay `string` with omitempty and only value types
/// like `NextCursor` become `*string`).
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on an unknown `kind` or a `ref`/`array` missing its target.
fn go_type(schema: &SchemaType, optional: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema.kind.as_str() {
        // uuid → string, plain string → string (format is a doc hint only for strings).
        "string" => match schema.format.as_deref() {
            Some("date-time") => "time.Time".to_string(),
            _ => "string".to_string(),
        },
        "boolean" => "bool".to_string(),
        "integer" => "int64".to_string(),
        // TARGET-API §4: number → float32 (the generator narrows; the diagnostic is already in graph).
        "number" => "float32".to_string(),
        "array" => {
            let items = schema.items.as_deref().ok_or_else(|| CoreError::SdkGen {
                message: "array schema is missing its `items` element type".to_string(),
            })?;
            // Slice elements are never optional-pointer-wrapped.
            return Ok(format!("[]{}", go_type(items, false, graph)?));
        }
        "object" => {
            // Free-form map (additionalProperties) → map[string]any; a bare object is unexpected here.
            if schema.additional_properties == Some(true) {
                "map[string]any".to_string()
            } else {
                return Err(CoreError::SdkGen {
                    message: "inline object schema without additionalProperties is unsupported \
                              (expected a named $ref)"
                        .to_string(),
                });
            }
        }
        "ref" => {
            let ref_id = schema.ref_id.as_deref().ok_or_else(|| CoreError::SdkGen {
                message: "ref schema is missing its `ref_id`".to_string(),
            })?;
            let target = graph
                .schemas
                .iter()
                .find(|s| s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            // Both objects and enum newtypes are referenced by their exported Go name.
            return Ok(maybe_pointer(
                target.name.clone(),
                optional,
                is_value_ref(target),
            ));
        }
        other => {
            return Err(CoreError::SdkGen {
                message: format!("unknown SchemaType kind '{other}'"),
            });
        }
    };
    // Strings/maps are nilable already; value types get a pointer when optional.
    let is_value = matches!(base.as_str(), "bool" | "int64" | "float32" | "time.Time");
    Ok(maybe_pointer(base, optional, is_value))
}

/// Whether a referenced schema lowers to a Go *value* type that needs a pointer to be optional.
///
/// Enum newtypes are string-backed value types (`*TargetDirection` when optional, per `expected/sdk`);
/// object refs are structs and are pointer-wrapped only when optional too.
fn is_value_ref(target: &Schema) -> bool {
    // Both enums and structs are value types in Go; an optional field is a pointer either way.
    matches!(target.kind.as_str(), "enum" | "object")
}

/// Wrap `base` in a Go pointer when the field is optional and the underlying Go type is a value type.
fn maybe_pointer(base: String, optional: bool, is_value: bool) -> String {
    if optional && is_value {
        format!("*{base}")
    } else {
        base
    }
}

/// Build the Go json struct tag for a field, adding the omitempty option when the field is optional.
fn json_tag(json_name: &str, optional: bool) -> String {
    if optional {
        format!("`json:\"{json_name},omitempty\"`")
    } else {
        format!("`json:\"{json_name}\"`")
    }
}

/// Whether emitting `field`'s Go type requires the `time` import (a `time.Time` value anywhere).
fn field_needs_time(schema: &SchemaType) -> bool {
    match schema.kind.as_str() {
        "string" => schema.format.as_deref() == Some("date-time"),
        "array" => schema.items.as_deref().is_some_and(field_needs_time),
        _ => false,
    }
}

/// Emit `models.go`: one struct per object schema + one `type X string` newtype + const block per enum.
///
/// Schemas are consumed in the graph's id-sorted order; fields in their json-name-sorted order — both
/// already guaranteed by the graph (GRAPH-02), so the output is deterministic without re-sorting here.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if any field's schema cannot be mapped to a Go type.
pub(crate) fn emit_models(graph: &ApiGraph) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut needs_time = false;

    let mut first = true;
    for schema in &graph.schemas {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        match schema.kind.as_str() {
            "enum" => emit_enum(&mut body, schema)?,
            "object" => {
                for field in &schema.fields {
                    if field_needs_time(&field.schema) {
                        needs_time = true;
                    }
                }
                emit_struct(&mut body, schema, graph)?;
            }
            other => {
                return Err(CoreError::SdkGen {
                    message: format!(
                        "schema '{}' has unsupported kind '{other}' (expected object|enum)",
                        schema.id
                    ),
                });
            }
        }
    }

    let imports = if needs_time { vec!["time"] } else { Vec::new() };
    Ok(file(&imports, &body))
}

/// Emit a single object struct: one exported field per graph field with its Go type and json tag.
fn emit_struct(body: &mut String, schema: &Schema, graph: &ApiGraph) -> Result<(), CoreError> {
    writeln!(body, "type {} struct {{", schema.name).map_err(sink)?;
    for field in &schema.fields {
        emit_struct_field(body, field, graph)?;
    }
    writeln!(body, "}}").map_err(sink)?;
    Ok(())
}

/// Emit one struct field line: the exported Go name, its Go type, and the json struct tag.
fn emit_struct_field(body: &mut String, field: &Field, graph: &ApiGraph) -> Result<(), CoreError> {
    let go_name = exported(&field.json_name);
    let go_ty = go_type(&field.schema, field.optional, graph)?;
    let tag = json_tag(&field.json_name, field.optional);
    writeln!(body, "{go_name} {go_ty} {tag}").map_err(sink)?;
    Ok(())
}

/// Emit a string-enum newtype + a const block of `NameValue Name = "value"` (values in graph order).
fn emit_enum(body: &mut String, schema: &Schema) -> Result<(), CoreError> {
    writeln!(body, "type {} string", schema.name).map_err(sink)?;
    writeln!(body).map_err(sink)?;
    writeln!(body, "const (").map_err(sink)?;
    for value in &schema.enum_values {
        let const_name = format!("{}{}", schema.name, exported(value));
        writeln!(body, "{const_name} {} = \"{value}\"", schema.name).map_err(sink)?;
    }
    writeln!(body, ")").map_err(sink)?;
    Ok(())
}

/// Emit `client.go`: the functional-options `Client` + `Option` + `WithHTTPClient`/`WithAPIKey`/`NewClient`.
///
/// `net/http` + `time` are always needed (the default client carries a `30 * time.Second` timeout).
pub(crate) fn emit_client() -> String {
    let body = "\
// Client is the goalservice SDK entrypoint. Tag-grouped operation methods hang
// off this type; it is constructed with functional options.
type Client struct {
baseURL string
httpClient *http.Client
apiKey string
}

// Option mutates a Client during construction (functional-options pattern).
type Option func(*Client)

// WithHTTPClient overrides the default *http.Client (timeouts, transport, etc.).
func WithHTTPClient(hc *http.Client) Option {
return func(c *Client) { c.httpClient = hc }
}

// WithAPIKey sets the API key sent to satisfy the ApiKeyAuth security scheme.
func WithAPIKey(key string) Option {
return func(c *Client) { c.apiKey = key }
}

// NewClient builds a Client for the given base URL, applying any options. A
// sensible default *http.Client is used unless WithHTTPClient overrides it.
func NewClient(baseURL string, opts ...Option) *Client {
c := &Client{
baseURL: baseURL,
httpClient: &http.Client{Timeout: 30 * time.Second},
}
for _, opt := range opts {
opt(c)
}
return c
}
";
    file(&["net/http", "time"], body)
}

/// Emit `errors.go`: the typed `APIError` (status + decoded body) + `Error()` + `IsNotFound()`.
pub(crate) fn emit_errors() -> String {
    let body = "\
// APIError is returned by operation methods on non-2xx responses. It exposes the
// HTTP status and the decoded error body (message/slug/hints).
type APIError struct {
StatusCode int
Message string
Slug string
Hints []string
}

// Error implements the error interface.
func (e *APIError) Error() string {
return fmt.Sprintf(\"goalservice: %d %s (%s)\", e.StatusCode, e.Message, e.Slug)
}

// IsNotFound reports whether the error is a 404.
func (e *APIError) IsNotFound() bool {
return e.StatusCode == 404
}
";
    file(&["fmt"], body)
}

/// The success status + (optional) model of an operation's first 2xx response.
///
/// WR-01: `model` is `Option` because a 2xx response can carry no body (e.g. a `204 No Content` or a
/// body-less `201`); the `status` is always the response's real status so the dispatch compares
/// against the actual success code rather than a hard-coded `200`.
struct Success {
    status: u16,
    model: Option<String>,
}

/// Resolve an operation's primary success (lowest 2xx) response status + model name.
///
/// Returns the first 2xx response's status regardless of whether it carries a body (WR-01); the
/// model is `Some` only when that response has a typed body. An operation with no 2xx response at all
/// yields `None`.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the success body `$ref` is dangling.
fn success_of(op: &Operation, graph: &ApiGraph) -> Result<Option<Success>, CoreError> {
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
            return Ok(Some(Success {
                status: resp.status,
                model,
            }));
        }
    }
    Ok(None)
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

/// Resolve an operation's request-body model name, if it has a typed body.
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

/// The absolute base path joined to each operation's group-relative path (mirrors lowering Pitfall 1).
const BASE_PATH: &str = "/goal";

/// Join the service base prefix with a group-relative operation path (slash-collapsed).
fn join_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        format!("{BASE_PATH}/")
    } else {
        format!("{BASE_PATH}/{trimmed}")
    }
}

/// Emit one operations file for `tag`: tag-grouped, ctx-first typed methods on `*Client`.
///
/// `ops` are the operations assigned to this tag, in graph order. Each method:
/// - takes `ctx context.Context` first, then path params as positional `string` args, then a generated
///   `<Method>Params` struct for query-bearing ops, then a typed body input;
/// - marshals the body with `encoding/json`, builds the request to `baseURL + <absolute path>`, sets
///   `X-API-Key` when the client's apiKey is non-empty, and decodes a 2xx body into the success model
///   or a non-2xx body into an [`APIError`].
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] on a dangling body/response `$ref` for any op in the group.
pub(crate) fn emit_operations(
    graph: &ApiGraph,
    tag: &str,
    ops: &[&Operation],
) -> Result<String, CoreError> {
    let mut body = String::new();
    let mut first = true;
    for op in ops {
        if !first {
            writeln!(body).map_err(sink)?;
        }
        first = false;
        emit_operation(&mut body, op, graph)?;
    }
    // Operation methods always touch context/net-http/encoding-json/bytes/fmt (request build + decode).
    // WR-02: non-string query params additionally need `strconv`/`time` to URL-encode; `file` sorts +
    // de-duplicates the union.
    let mut imports: Vec<&str> = vec!["bytes", "context", "encoding/json", "fmt", "net/http"];
    imports.extend(query_imports(ops, graph)?);
    // WR-04: any op with a templated path interpolates `url.PathEscape(...)`, which needs `net/url`.
    if ops
        .iter()
        .any(|op| op.params.iter().any(|p| p.location == "path"))
    {
        imports.push("net/url");
    }
    let _ = tag; // the tag names the FILE (handled by the bundle), not the imports.
    Ok(file(&imports, &body))
}

/// Emit a single operation method, including its `<Method>Params` struct when the op has query params.
fn emit_operation(body: &mut String, op: &Operation, graph: &ApiGraph) -> Result<(), CoreError> {
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
        .and_then(|s| s.model.as_deref())
        .unwrap_or("struct{}")
        .to_string();
    // WR-01: compare against the operation's REAL success status (e.g. 201/204), not a default 200.
    // An op with no 2xx response declared falls back to 200 (the conventional default).
    let success_status = success.as_ref().map_or(200, |s| s.status);

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
        join_path(&op.path)
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
    let has_decode = success.as_ref().is_some_and(|s| s.model.is_some());
    emit_request_dispatch(
        body,
        op,
        graph,
        &path_params,
        &query_params,
        has_body,
        success_status,
        has_decode,
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
    path_params: &[&str],
    query_params: &[&crate::graph::Param],
    has_body: bool,
    success_status: u16,
    has_decode: bool,
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
    emit_url(body, op, path_params)?;

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
    writeln!(body, "if c.apiKey != \"\" {{").map_err(sink)?;
    writeln!(body, "req.Header.Set(\"X-API-Key\", c.apiKey)").map_err(sink)?;
    writeln!(body, "}}").map_err(sink)?;

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
    // Which APIError fields the resolved error struct can supply (by its declared json fields).
    let error_model = error_model_of(op, graph)?;
    let usable: Vec<(&str, &str)> = error_model.map_or_else(Vec::new, |model| {
        API_ERROR_FIELDS
            .iter()
            .copied()
            .filter(|(_, json_name)| model.fields.iter().any(|f| f.json_name == *json_name))
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
            writeln!(body, "{go_field}: apiErr.{src},").map_err(sink)?;
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

/// Extract the set of `{token}` placeholder names from a path template, in first-seen order.
///
/// `"/goal/{uuid}/sub/{kind}"` → `["uuid", "kind"]`. Used by [`emit_url`] to assert the path's
/// templated tokens exactly match the operation's declared path params (WR-03).
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

/// Emit the `url :=` line, interpolating path params via `fmt.Sprintf` when the path is templated.
///
/// WR-03: the set of `{token}`s in the absolute path is asserted to equal the set of declared path
/// params before emitting, so a token with no matching arg (a runtime `%!s(MISSING)` in the URL) or
/// an arg with no matching token becomes a typed [`CoreError::SdkGen`] at generation time instead.
///
/// WR-04: each interpolated path value is wrapped in `url.PathEscape(...)` so a value containing
/// `/`, `?`, `#`, `%`, or `..` can never restructure the request URL.
fn emit_url(body: &mut String, op: &Operation, path_params: &[&str]) -> Result<(), CoreError> {
    let abs = join_path(&op.path);
    let tokens = path_tokens(&abs);

    // WR-03: the templated tokens must be exactly the declared path params (order-independent set
    // equality), so neither a dangling token nor an unused arg can slip through.
    let token_set: BTreeSet<&str> = tokens.iter().map(String::as_str).collect();
    let param_set: BTreeSet<&str> = path_params.iter().copied().collect();
    if token_set != param_set {
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
    out
}

/// Frame a Go file: `package <PACKAGE>`, a computed import block, then the body.
///
/// Imports are sorted + de-duplicated (a `BTreeSet`) so the block is deterministic and `gofmt`-stable.
/// A single import emits the one-line form; multiple imports emit the parenthesized block — `gofmt`
/// canonicalizes either, so this is just to keep the pre-format text tidy.
fn file(imports: &[&str], body: &str) -> String {
    // `write!` into a String is infallible in practice; the trait is fallible, so swallow the unit
    // error with `let _ =` rather than `unwrap` (RUST-04) — there is no failure mode to surface.
    let mut out = String::new();
    let _ = writeln!(out, "package {PACKAGE}");
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
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } },
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.GoalResponse" } } ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        }
      ],
      "schemas": [
        {
          "id": "dto.CommandMessage", "name": "CommandMessage", "kind": "object",
          "fields": [
            { "json_name": "message", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/c.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.CreateGoalInput", "name": "CreateGoalInput", "kind": "object",
          "fields": [
            { "json_name": "analyticsQuery", "required": true, "optional": false,
              "schema": { "kind": "ref", "format": null, "items": null, "ref_id": "dto.GoalAnalyticsQuery", "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "createdAt", "required": false, "optional": false,
              "schema": { "kind": "string", "format": "date-time", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "name", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "targetDirection", "required": false, "optional": true,
              "schema": { "kind": "ref", "format": null, "items": null, "ref_id": "dto.TargetDirection", "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "targetValue", "required": false, "optional": true,
              "schema": { "kind": "number", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "workflowChainIds", "required": false, "optional": true,
              "schema": { "kind": "array", "format": null, "items": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null }, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.GoalAnalyticsQuery", "name": "GoalAnalyticsQuery", "kind": "object",
          "fields": [
            { "json_name": "windowDays", "required": false, "optional": false,
              "schema": { "kind": "integer", "format": "int64", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.GoalResponse", "name": "GoalResponse", "kind": "object",
          "fields": [
            { "json_name": "metadata", "required": false, "optional": true,
              "schema": { "kind": "object", "format": null, "items": null, "ref_id": null, "additional_properties": true },
              "description": null, "example": null },
            { "json_name": "uuid", "required": true, "optional": false,
              "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "dto.HttpError", "name": "HttpError", "kind": "object",
          "fields": [
            { "json_name": "message", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/c.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.TargetDirection", "name": "TargetDirection", "kind": "enum",
          "fields": [], "enum_values": ["gte","lte"],
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
                  "id": "dto.GoalResponse", "name": "GoalResponse", "kind": "object",
                  "fields": [
                    {{ "json_name": "uuid", "required": true, "optional": false,
                      "schema": {{ "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null }},
                      "description": null, "example": null }}
                  ],
                  "enum_values": [], "span": {{ "file": "/root/g.go", "start_line": 1, "end_line": 1 }}
                }},
                {{
                  "id": "dto.{error_name}", "name": "{error_name}", "kind": "object",
                  "fields": [
                    {{ "json_name": "message", "required": true, "optional": false,
                      "schema": {{ "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null }},
                      "description": null, "example": null }},
                    {{ "json_name": "slug", "required": false, "optional": true,
                      "schema": {{ "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null }},
                      "description": null, "example": null }}
                  ],
                  "enum_values": [], "span": {{ "file": "/root/e.go", "start_line": 1, "end_line": 1 }}
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
                  "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
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
                  "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
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
                  "schema": { "kind": "integer", "format": "int64", "items": null, "ref_id": null, "additional_properties": null },
                  "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } },
                { "name": "active", "location": "query", "required": false,
                  "schema": { "kind": "boolean", "format": null, "items": null, "ref_id": null, "additional_properties": null },
                  "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
              ],
              "request_body": null,
              "responses": [ { "status": 200, "body": { "ref_id": "dto.GoalResponse" } } ],
              "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.GoalResponse", "name": "GoalResponse", "kind": "object",
              "fields": [
                { "json_name": "uuid", "required": true, "optional": false,
                  "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
                  "description": null, "example": null }
              ],
              "enum_values": [], "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
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
              "id": "dto.GoalResponse", "name": "GoalResponse", "kind": "object",
              "fields": [
                { "json_name": "uuid", "required": true, "optional": false,
                  "schema": { "kind": "string", "format": "uuid", "items": null, "ref_id": null, "additional_properties": null },
                  "description": null, "example": null }
              ],
              "enum_values": [], "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
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
        use super::{emit_models, sample_graph};

        #[test]
        fn optional_field_is_pointer_with_omitempty_required_is_plain() {
            let out = emit_models(&sample_graph()).unwrap();
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
        fn enum_emits_newtype_and_sorted_const_block() {
            let out = emit_models(&sample_graph()).unwrap();
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
            let out = emit_models(&sample_graph()).unwrap();
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
            // free-form map → map[string]any.
            assert!(
                out.contains("Metadata map[string]any `json:\"metadata,omitempty\"`"),
                "{out}"
            );
        }

        #[test]
        fn imports_time_only_when_a_time_field_exists() {
            let out = emit_models(&sample_graph()).unwrap();
            // GoalResponse.createdAt is a date-time, so `time` must be imported.
            assert!(out.contains("import \"time\""), "{out}");
        }

        #[test]
        fn nested_ref_uses_referenced_model_name() {
            let out = emit_models(&sample_graph()).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
            for imp in ["bytes", "context", "encoding/json", "fmt", "net/http"] {
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
            assert!(
                out.contains("var apiErr ApiError"),
                "error decode must use the graph's error model name `ApiError`:\n{out}"
            );
            assert!(
                !out.contains("var apiErr HttpError"),
                "error decode must NOT reference a hard-coded `HttpError`:\n{out}"
            );
            // It still populates the APIError from the resolved struct's fields.
            assert!(out.contains("Message: apiErr.Message,"), "{out}");
            assert!(out.contains("Slug: apiErr.Slug,"), "{out}");
        }

        #[test]
        fn error_decode_falls_back_to_an_anonymous_struct_when_no_error_response_exists() {
            // An operation with no typed non-2xx response has no graph error model; the SDK must NOT
            // fabricate a dependency on a named type — it decodes into a generator-owned anonymous
            // struct exposing exactly the fields APIError consumes, so it always compiles.
            let graph = super::no_error_response_graph();
            let ops: Vec<&crate::graph::Operation> = graph.operations.iter().collect();
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let err = emit_operations(&graph, "Goals", &ops).unwrap_err();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
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
            let out = emit_operations(&graph, "Goals", &ops).unwrap();
            // The synthetic error model in error_model_graph declares message + slug only.
            assert!(out.contains("Message: apiErr.Message,"), "{out}");
            assert!(out.contains("Slug: apiErr.Slug,"), "{out}");
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
            let out = emit_client();
            assert!(
                out.contains("func NewClient(baseURL string, opts ...Option) *Client"),
                "{out}"
            );
            assert!(
                out.contains("func WithHTTPClient(hc *http.Client) Option"),
                "{out}"
            );
            assert!(out.contains("func WithAPIKey(key string) Option"), "{out}");
            // computed imports.
            assert!(out.contains("\"net/http\""), "{out}");
            assert!(out.contains("\"time\""), "{out}");
        }

        #[test]
        fn errors_emit_apierror_with_error_method() {
            let out = emit_errors();
            assert!(out.contains("type APIError struct"), "{out}");
            assert!(out.contains("StatusCode int"), "{out}");
            assert!(out.contains("func (e *APIError) Error() string"), "{out}");
            assert!(out.contains("import \"fmt\""), "{out}");
        }
    }

    mod type_mapping {
        use super::{go_type, join_path, sample_graph};
        use crate::graph::SchemaType;

        fn st(kind: &str, format: Option<&str>) -> SchemaType {
            SchemaType {
                kind: kind.to_string(),
                format: format.map(str::to_string),
                items: None,
                ref_id: None,
                additional_properties: None,
            }
        }

        #[test]
        fn value_types_get_a_pointer_only_when_optional() {
            let graph = sample_graph();
            assert_eq!(
                go_type(&st("number", None), true, &graph).unwrap(),
                "*float32"
            );
            assert_eq!(
                go_type(&st("number", None), false, &graph).unwrap(),
                "float32"
            );
            assert_eq!(
                go_type(&st("boolean", None), true, &graph).unwrap(),
                "*bool"
            );
            assert_eq!(
                go_type(&st("integer", Some("int64")), false, &graph).unwrap(),
                "int64"
            );
            // strings are nilable already → never pointer-wrapped.
            assert_eq!(
                go_type(&st("string", None), true, &graph).unwrap(),
                "string"
            );
            assert_eq!(
                go_type(&st("string", Some("date-time")), false, &graph).unwrap(),
                "time.Time"
            );
        }

        #[test]
        fn join_path_prefixes_the_service_base() {
            assert_eq!(join_path("/"), "/goal/");
            assert_eq!(join_path("/list"), "/goal/list");
            assert_eq!(join_path("/{uuid}"), "/goal/{uuid}");
        }
    }
}
