//! Language-agnostic emit helpers shared by the Go, Python, and TypeScript SDK emitters.
//!
//! These are the pure, byte-identical pieces of `gosdk::emit`/`pysdk::emit`/`tssdk::emit`: identifier
//! tokenization ([`split_words`]), path joining ([`join_path`]) and templating ([`path_tokens`] +
//! [`path_tokens_match`]), and graph-walking model/response resolvers ([`success_responses_of`],
//! [`request_body_model_of`]).
//! They contain NO per-language formatting — the casers (`exported`/`snake`/`camel`/…) and the type
//! mappers (`go_type`/`py_type`/`ts_type`) stay in each emitter, where they genuinely diverge. One
//! definition per fact (CLAUDE.md rule 3).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::graph::{ApiGraph, Operation, Prim, Schema, Type};
use crate::sdk::layout::SdkFileLayout;
use crate::CoreError;

/// Split an identifier into words on non-alphanumeric separators and lower→upper case boundaries.
///
/// `workflowChainIds` → `["workflow", "Chain", "Ids"]`; `page_size` → `["page", "size"]`;
/// `openai/gpt-image-2` → `["openai", "gpt", "image", "2"]`. The shared tokenizer behind every
/// per-language casing helper.
pub(crate) fn split_words(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    let chars: Vec<char> = name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        let next_is_lower = chars.get(idx + 1).is_some_and(char::is_ascii_lowercase);
        let prev_is_upper = current
            .chars()
            .last()
            .is_some_and(|prev| prev.is_ascii_uppercase());
        if ch.is_ascii_uppercase()
            && !current.is_empty()
            && (prev_lower || (prev_is_upper && next_is_lower))
        {
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

/// Convert an operation/type name into a deterministic lowercase file stem.
///
/// The result is ASCII `[a-z0-9_]+`, never empty, never starts with a digit, and is suitable as the
/// basename portion of generated files (`model_foo.go`, `models/foo.ts`, ...). This is file-structure
/// only; it never changes the public SDK symbol name.
pub(crate) fn file_stem(name: &str) -> String {
    let mut out = split_words(name)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_");
    if out.is_empty() {
        out.push_str("value");
    }
    if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        out.insert_str(0, "value_");
    }
    out
}

/// Put `file_name` under an optional relative directory for configurable split layouts.
///
/// Empty/`None` means the package root. The returned path is still validated by the bundle writer before
/// materialization, so this helper only normalizes harmless leading/trailing slashes.
pub(crate) fn file_in_dir(dir: Option<&str>, file_name: &str) -> String {
    match dir.map(|s| s.trim_matches('/')) {
        Some("") | None => file_name.to_string(),
        Some(dir) => format!("{dir}/{file_name}"),
    }
}

/// Resolve every API-key header the built-in SDK clients may need to send.
pub(crate) fn api_key_header_names(graph: &ApiGraph) -> Result<Vec<String>, CoreError> {
    let schemes = api_key_security_schemes(graph)?;
    let mut headers: Vec<String> = schemes
        .values()
        .filter_map(|scheme| match scheme.location {
            ApiKeyLocation::Header => Some(scheme.name.clone()),
            ApiKeyLocation::Query => None,
        })
        .collect();
    headers.sort();
    headers.dedup();
    Ok(headers)
}

/// Resolve every API-key credential name the built-in SDK clients may need to send.
pub(crate) fn api_key_credential_names(graph: &ApiGraph) -> Result<Vec<String>, CoreError> {
    let schemes = api_key_security_schemes(graph)?;
    let mut names: Vec<String> = schemes.values().map(|scheme| scheme.name.clone()).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Resolve the API-key headers required by one operation, including global schemes.
pub(crate) fn operation_api_key_headers(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Vec<String>, CoreError> {
    let mut headers: Vec<String> = operation_api_key_schemes(graph, op)?
        .into_iter()
        .filter_map(|scheme| match scheme.location {
            ApiKeyLocation::Header => Some(scheme.name),
            ApiKeyLocation::Query => None,
        })
        .collect();
    headers.sort();
    headers.dedup();
    Ok(headers)
}

/// Resolve the API-key query parameter names required by one operation, including global schemes.
pub(crate) fn operation_api_key_queries(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Vec<String>, CoreError> {
    let mut queries: Vec<String> = operation_api_key_schemes(graph, op)?
        .into_iter()
        .filter_map(|scheme| match scheme.location {
            ApiKeyLocation::Header => None,
            ApiKeyLocation::Query => Some(scheme.name),
        })
        .collect();
    queries.sort();
    queries.dedup();
    Ok(queries)
}

/// One operation-scoped API-key scheme after global inheritance and id/header validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationApiKeyScheme {
    /// The OpenAPI security scheme id.
    pub(crate) id: String,
    /// The apiKey credential name.
    pub(crate) name: String,
    /// Where the apiKey credential is sent.
    pub(crate) location: ApiKeyLocation,
}

/// Supported apiKey credential locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiKeyLocation {
    /// HTTP header.
    Header,
    /// Query parameter.
    Query,
}

/// Resolve the API-key schemes required by one operation, including global schemes.
pub(crate) fn operation_api_key_schemes(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Vec<OperationApiKeyScheme>, CoreError> {
    let schemes = supported_security_schemes(graph)?;
    let scheme_ids = operation_security_ids(graph, op);

    let mut out = Vec::new();
    for scheme_id in scheme_ids {
        let Some(scheme) = schemes.get(&scheme_id) else {
            return Err(unknown_security_scheme_error(op, &scheme_id));
        };
        if let SupportedAuthScheme::ApiKey(scheme) = scheme {
            out.push(OperationApiKeyScheme {
                id: scheme_id,
                name: scheme.name.clone(),
                location: scheme.location,
            });
        }
    }
    out.sort_by(|a, b| {
        location_sort_key(a.location)
            .cmp(&location_sort_key(b.location))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    out.dedup();
    Ok(out)
}

/// Supported HTTP security scheme variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HttpAuthScheme {
    /// HTTP bearer token auth.
    Bearer,
    /// HTTP basic auth.
    Basic,
}

/// SDK-wide HTTP auth features required by a graph.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HttpAuthFeatures {
    /// At least one HTTP bearer security scheme is declared.
    pub(crate) bearer: bool,
    /// At least one HTTP basic security scheme is declared.
    pub(crate) basic: bool,
}

/// Resolve which HTTP auth helpers the generated SDK client must expose.
pub(crate) fn http_auth_features(graph: &ApiGraph) -> Result<HttpAuthFeatures, CoreError> {
    let schemes = supported_security_schemes(graph)?;
    let mut features = HttpAuthFeatures::default();
    for scheme in schemes.values() {
        match scheme {
            SupportedAuthScheme::ApiKey(_) => {}
            SupportedAuthScheme::Http(HttpAuthScheme::Bearer) => features.bearer = true,
            SupportedAuthScheme::Http(HttpAuthScheme::Basic) => features.basic = true,
        }
    }
    Ok(features)
}

/// Resolve the HTTP auth schemes required by one operation, including global schemes.
pub(crate) fn operation_http_auth_schemes(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Vec<HttpAuthScheme>, CoreError> {
    let schemes = supported_security_schemes(graph)?;
    let scheme_ids = operation_security_ids(graph, op);
    let mut out = Vec::new();
    for scheme_id in scheme_ids {
        let Some(scheme) = schemes.get(&scheme_id) else {
            return Err(unknown_security_scheme_error(op, &scheme_id));
        };
        if let SupportedAuthScheme::Http(scheme) = scheme {
            out.push(*scheme);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SupportedAuthScheme {
    ApiKey(ApiKeyScheme),
    Http(HttpAuthScheme),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiKeyScheme {
    name: String,
    location: ApiKeyLocation,
}

fn api_key_security_schemes(graph: &ApiGraph) -> Result<BTreeMap<String, ApiKeyScheme>, CoreError> {
    let supported = supported_security_schemes(graph)?;
    let mut schemes = BTreeMap::new();
    for (id, scheme) in supported {
        if let SupportedAuthScheme::ApiKey(scheme) = scheme {
            schemes.insert(id, scheme);
        }
    }
    Ok(schemes)
}

fn supported_security_schemes(
    graph: &ApiGraph,
) -> Result<BTreeMap<String, SupportedAuthScheme>, CoreError> {
    let mut schemes = BTreeMap::new();
    for scheme in &graph.security {
        let auth = match scheme.kind.as_str() {
            "apiKey" => {
                let location = match scheme.location.as_str() {
                    "header" => ApiKeyLocation::Header,
                    "query" => ApiKeyLocation::Query,
                    _ => return Err(unsupported_security_scheme_error(scheme)),
                };
                SupportedAuthScheme::ApiKey(ApiKeyScheme {
                    name: scheme.name.clone(),
                    location,
                })
            }
            "http" if scheme.location.is_empty() => match scheme.name.as_str() {
                "bearer" => SupportedAuthScheme::Http(HttpAuthScheme::Bearer),
                "basic" => SupportedAuthScheme::Http(HttpAuthScheme::Basic),
                _ => return Err(unsupported_security_scheme_error(scheme)),
            },
            _ => return Err(unsupported_security_scheme_error(scheme)),
        };
        if schemes.insert(scheme.id.clone(), auth).is_some() {
            return Err(CoreError::SdkGen {
                message: format!("duplicate security scheme id '{}'", scheme.id),
            });
        }
    }
    Ok(schemes)
}

fn operation_security_ids(graph: &ApiGraph, op: &Operation) -> Vec<String> {
    let mut scheme_ids: Vec<String> = if op.security_overrides_global {
        op.security.clone()
    } else {
        graph
            .security
            .iter()
            .filter(|scheme| scheme.global)
            .map(|scheme| scheme.id.clone())
            .chain(op.security.iter().cloned())
            .collect()
    };
    scheme_ids.sort();
    scheme_ids.dedup();
    scheme_ids
}

fn unsupported_security_scheme_error(scheme: &crate::graph::SecurityScheme) -> CoreError {
    CoreError::SdkGen {
        message: format!(
            "SDK targets support apiKey/header, apiKey/query, http/bearer, and http/basic security only, got scheme '{}' as kind='{}' location='{}' name='{}'",
            scheme.id, scheme.kind, scheme.location, scheme.name
        ),
    }
}

fn unknown_security_scheme_error(op: &Operation, scheme_id: &str) -> CoreError {
    CoreError::SdkGen {
        message: format!(
            "operation '{}' references unknown security scheme '{}'",
            op.id, scheme_id
        ),
    }
}

fn location_sort_key(location: ApiKeyLocation) -> u8 {
    match location {
        ApiKeyLocation::Header => 0,
        ApiKeyLocation::Query => 1,
    }
}

/// Reject duplicate graph schema names before a target turns them into top-level symbols.
///
/// Schema ids can be package-qualified while schema names are local. The local name is what OpenAPI
/// components and SDK model symbols use, so two ids with the same name must be handled before emission.
pub(crate) fn check_unique_schema_names(graph: &ApiGraph, target: &str) -> Result<(), CoreError> {
    let mut seen = BTreeSet::new();
    for schema in &graph.schemas {
        if !seen.insert(schema.name.as_str()) {
            return Err(CoreError::SdkGen {
                message: format!(
                    "two schemas share the {target} name '{}' (distinct ids map to one emitted symbol)",
                    schema.name
                ),
            });
        }
    }
    Ok(())
}

/// Whether a neutral map key can be represented as a JSON/OpenAPI object key.
pub(crate) const fn is_json_object_key(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(Prim::String))
}

/// Escape a Rust string as a double-quoted Go/Python/TypeScript-compatible string literal.
pub(crate) fn quoted_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn kebab_stem(name: &str) -> String {
    file_stem(name).replace('_', "-")
}

fn service_name(op: &Operation) -> &str {
    op.group.as_deref().unwrap_or("default")
}

pub(crate) fn operation_group_name(op: &Operation) -> &str {
    service_name(op)
}

fn render_file_template(template: &str, vars: &[(&str, String)]) -> Result<String, CoreError> {
    let mut out = String::new();
    let mut rest = template;
    loop {
        let Some(open) = rest.find('{') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(CoreError::SdkGen {
                message: format!("file template {template:?} has an unclosed placeholder"),
            });
        };
        let key = &after[..close];
        let Some((_, value)) = vars.iter().find(|(name, _)| *name == key) else {
            return Err(CoreError::SdkGen {
                message: format!("file template {template:?} uses unknown placeholder {{{key}}}"),
            });
        };
        out.push_str(value);
        rest = &after[close + 1..];
    }
    if out.is_empty() {
        return Err(CoreError::SdkGen {
            message: format!("file template {template:?} rendered an empty path"),
        });
    }
    crate::sdk::bundle::safe_frame_name(&out)?;
    Ok(out)
}

/// Resolve the split operation file name for a layout, preserving legacy defaults when no template is
/// configured.
pub(crate) fn operation_file_name(
    layout: &SdkFileLayout,
    op: &Operation,
    default_file_name: &str,
) -> Result<String, CoreError> {
    if let Some(template) = layout.operation_file_template_ref() {
        let service = service_name(op);
        return render_file_template(
            template,
            &[
                ("operation", op.id.clone()),
                ("operation_snake", file_stem(&op.id)),
                ("operation_kebab", kebab_stem(&op.id)),
                ("service", service.to_string()),
                ("service_snake", file_stem(service)),
                ("service_kebab", kebab_stem(service)),
            ],
        );
    }
    Ok(file_in_dir(layout.operation_dir_ref(), default_file_name))
}

/// Resolve the split operation file name for all operations in one tag/group.
pub(crate) fn operation_group_file_name(
    layout: &SdkFileLayout,
    group: &str,
    default_file_name: &str,
) -> Result<String, CoreError> {
    if let Some(template) = layout.operation_file_template_ref() {
        return render_file_template(
            template,
            &[
                ("service", group.to_string()),
                ("service_snake", file_stem(group)),
                ("service_kebab", kebab_stem(group)),
            ],
        );
    }
    Ok(file_in_dir(layout.operation_dir_ref(), default_file_name))
}

/// Resolve the split model file name for a layout, preserving legacy defaults when no template is
/// configured.
pub(crate) fn model_file_name(
    layout: &SdkFileLayout,
    schema: &Schema,
    default_file_name: &str,
) -> Result<String, CoreError> {
    if let Some(template) = layout.model_file_template_ref() {
        return render_file_template(
            template,
            &[
                ("schema", schema.name.clone()),
                ("schema_snake", file_stem(&schema.name)),
                ("schema_kebab", kebab_stem(&schema.name)),
            ],
        );
    }
    Ok(file_in_dir(layout.model_dir_ref(), default_file_name))
}

/// Join the `base_path` prefix with a group-relative operation path (slash-collapsed). `base_path` is
/// the user's `gnr8` config value — the single source of truth for the service prefix shared with the
/// `OpenAPI` lowering (CLAUDE.md rules 3 & 4) — so the SDK URLs and the spec paths agree.
pub(crate) fn join_path(base_path: &str, path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{trimmed}")
    }
}

pub(crate) fn validate_sdk_base_path(base_path: &str) -> Result<(), CoreError> {
    if base_path.is_empty() || base_path == "/" {
        return Ok(());
    }
    if !base_path.starts_with('/') {
        return Err(CoreError::SdkGen {
            message: format!("base path {base_path:?} must be empty, '/', or start with '/'"),
        });
    }
    if base_path.chars().any(|ch| matches!(ch, '?' | '#' | '\\'))
        || base_path.split('/').any(|part| part == "..")
    {
        return Err(CoreError::SdkGen {
            message: format!(
                "base path {base_path:?} must be a clean path prefix without query, fragment, backslash, or '..'"
            ),
        });
    }
    Ok(())
}

/// Extract the set of `{token}` placeholder names from a path template, in first-seen order.
///
/// `"/goal/{uuid}/sub/{kind}"` → `["uuid", "kind"]`. Used to assert the path's templated tokens exactly
/// match the operation's declared path params (WR-03).
pub(crate) fn path_tokens(path: &str) -> Vec<String> {
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

/// Whether the templated path `tokens` are exactly the declared path `params` (order-independent set
/// equality, WR-03). One shared definition so the Go/Python/TypeScript emitters agree; each caller keeps
/// its own typed error construction on a `false` result.
pub(crate) fn path_tokens_match(tokens: &[String], params: &[&str]) -> bool {
    let token_set: BTreeSet<&str> = tokens.iter().map(String::as_str).collect();
    let param_set: BTreeSet<&str> = params.iter().copied().collect();
    token_set == param_set
}

/// The success-response shape an SDK can represent for one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuccessResponses {
    /// Declared successful statuses, sorted by status code. Empty means no explicit 2xx response.
    pub(crate) statuses: Vec<u16>,
    /// The single success body model, when all body-bearing 2xx responses share one model.
    pub(crate) body_model: Option<String>,
    /// The statuses that carry [`Self::body_model`].
    pub(crate) body_statuses: Vec<u16>,
    /// The statuses that carry binary/file content.
    pub(crate) binary_statuses: Vec<u16>,
    /// The media type for binary/file success content.
    pub(crate) binary_content_type: Option<String>,
}

/// One declared non-2xx JSON error response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorResponseBody {
    /// HTTP status for the declared error response.
    pub(crate) status: u16,
    /// Referenced error body model name.
    pub(crate) model: String,
}

/// The request-body shape an SDK operation can accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestBodyModel {
    /// The referenced request schema id.
    pub(crate) schema_id: String,
    /// The referenced request model name.
    pub(crate) model: String,
    /// Whether callers must provide the body.
    pub(crate) required: bool,
    /// Request media type.
    pub(crate) content_type: String,
    /// Runtime body encoder requested by the media type.
    pub(crate) encoding: RequestBodyEncoding,
}

/// Request body media encoding supported by generated SDKs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestBodyEncoding {
    /// JSON request body.
    Json,
    /// Raw UTF-8 `text/plain` request body.
    Text,
    /// `application/x-www-form-urlencoded` request body.
    FormUrlEncoded,
    /// `multipart/form-data` request body.
    Multipart,
    /// Raw binary upload request body.
    Binary,
}

impl SuccessResponses {
    /// Whether at least one declared success has no body while another has a typed body.
    pub(crate) fn has_bodyless_alternative(&self) -> bool {
        (self.body_model.is_some() || !self.binary_statuses.is_empty())
            && self.body_statuses.len() + self.binary_statuses.len() < self.statuses.len()
    }

    /// Whether at least one successful response carries binary/file content.
    pub(crate) fn has_binary_body(&self) -> bool {
        !self.binary_statuses.is_empty()
    }
}

/// Resolve declared non-2xx JSON error body models for one operation.
///
/// The graph currently represents only explicit numeric statuses, not `default` or ranges, so the
/// returned list is sorted by explicit status and used before language fallback behavior.
pub(crate) fn error_response_bodies_of(
    op: &Operation,
    graph: &ApiGraph,
) -> Result<Vec<ErrorResponseBody>, CoreError> {
    let mut out = Vec::new();
    for resp in &op.responses {
        if (200..300).contains(&resp.status) || resp.body_kind != "json" {
            continue;
        }
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
        out.push(ErrorResponseBody {
            status: resp.status,
            model: model.name.clone(),
        });
    }
    out.sort_by_key(|body| body.status);
    out.dedup();
    Ok(out)
}

/// Resolve all 2xx responses for one operation.
///
/// SDK methods have one return type, so multiple body-bearing success responses are accepted only when
/// they point to the same model. Body-less alternate 2xx responses are represented by returning the
/// language's empty/default success value rather than surfacing an API error.
pub(crate) fn success_responses_of(
    op: &Operation,
    graph: &ApiGraph,
) -> Result<SuccessResponses, CoreError> {
    let mut statuses = Vec::new();
    let mut body_statuses = Vec::new();
    let mut binary_statuses = Vec::new();
    let mut body_model: Option<String> = None;
    let mut binary_content_type: Option<String> = None;
    for resp in &op.responses {
        if (200..300).contains(&resp.status) {
            statuses.push(resp.status);
            match resp.body_kind.as_str() {
                "json" => {
                    if let Some(body) = &resp.body {
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
                        match &body_model {
                            Some(existing) if existing != &model.name => {
                                return Err(CoreError::SdkGen {
                                    message: format!(
                                        "operation '{}' has multiple success body models ('{}' and '{}'); \
                                         SDK targets require one return model",
                                        op.id, existing, model.name
                                    ),
                                });
                            }
                            Some(_) => {}
                            None => body_model = Some(model.name.clone()),
                        }
                        body_statuses.push(resp.status);
                    }
                }
                "empty" => {}
                "binary" | "sse" => {
                    if resp.body.is_some() {
                        if resp.body_kind == "sse" {
                            return Err(CoreError::SdkGen {
                                message: format!(
                                    "operation '{}' response {} is text/event-stream with an event \
                                     schema; SDK targets do not yet support typed SSE event streams",
                                    op.id, resp.status
                                ),
                            });
                        }
                        return Err(CoreError::SdkGen {
                            message: format!(
                                "operation '{}' response {} is {} but also has a schema body",
                                op.id, resp.status, resp.body_kind
                            ),
                        });
                    }
                    binary_statuses.push(resp.status);
                    let content_type = resp
                        .content_type
                        .clone()
                        .or_else(|| resp.content_types.first().cloned())
                        .unwrap_or_else(|| "application/octet-stream".to_string());
                    if binary_content_type.is_none() {
                        binary_content_type = Some(content_type);
                    }
                }
                other => {
                    return Err(CoreError::SdkGen {
                        message: format!(
                            "operation '{}' response {} has unsupported body_kind {other:?}",
                            op.id, resp.status
                        ),
                    });
                }
            }
        }
    }
    if body_model.is_some() && !binary_statuses.is_empty() {
        return Err(CoreError::SdkGen {
            message: format!(
                "operation '{}' mixes JSON and binary success responses; SDK targets require one success body kind",
                op.id
            ),
        });
    }
    Ok(SuccessResponses {
        statuses,
        body_model,
        body_statuses,
        binary_statuses,
        binary_content_type,
    })
}

/// Resolve an operation's request-body model and requiredness, if it has a typed body.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the request-body `$ref` is dangling.
pub(crate) fn request_body_model_of(
    op: &Operation,
    graph: &ApiGraph,
) -> Result<Option<RequestBodyModel>, CoreError> {
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
    let content_type = op
        .request_body_content_type
        .clone()
        .unwrap_or_else(|| "application/json".to_string());
    let encoding = request_body_encoding(&content_type).ok_or_else(|| CoreError::SdkGen {
        message: format!(
            "operation '{}' request body content type '{}' is unsupported by generated SDKs; \
             supported request media types are application/json, text/plain, \
             application/x-www-form-urlencoded, multipart/form-data, and application/octet-stream",
            op.id, content_type
        ),
    })?;
    validate_request_body_schema(op, model, encoding)?;
    Ok(Some(RequestBodyModel {
        schema_id: model.id.clone(),
        model: model.name.clone(),
        required: op.request_body_required,
        content_type,
        encoding,
    }))
}

fn request_body_encoding(content_type: &str) -> Option<RequestBodyEncoding> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    match media_type.as_str() {
        "application/json" => Some(RequestBodyEncoding::Json),
        "text/plain" => Some(RequestBodyEncoding::Text),
        "application/x-www-form-urlencoded" => Some(RequestBodyEncoding::FormUrlEncoded),
        "multipart/form-data" => Some(RequestBodyEncoding::Multipart),
        "application/octet-stream" => Some(RequestBodyEncoding::Binary),
        _ => None,
    }
}

fn validate_request_body_schema(
    op: &Operation,
    schema: &Schema,
    encoding: RequestBodyEncoding,
) -> Result<(), CoreError> {
    let ok = match encoding {
        RequestBodyEncoding::Json => true,
        RequestBodyEncoding::Text => matches!(
            &schema.body,
            Type::Primitive(Prim::String) | Type::WellKnown(_) | Type::Enum(_) | Type::Named(_)
        ),
        RequestBodyEncoding::FormUrlEncoded | RequestBodyEncoding::Multipart => {
            matches!(&schema.body, Type::Object(_))
        }
        RequestBodyEncoding::Binary => matches!(&schema.body, Type::Primitive(Prim::Bytes)),
    };
    if ok {
        return Ok(());
    }
    Err(CoreError::SdkGen {
        message: format!(
            "operation '{}' request body schema '{}' cannot be encoded as {:?}",
            op.id, schema.name, encoding
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        file_stem, http_auth_features, operation_api_key_headers, operation_api_key_queries,
        operation_http_auth_schemes, success_responses_of, HttpAuthScheme,
    };
    use crate::graph::{ApiGraph, Operation, Response, SecurityScheme, SourceSpan};

    #[test]
    fn file_stem_splits_acronym_before_capitalized_word() {
        assert_eq!(
            file_stem("PosthogQueryHogQLOutput"),
            "posthog_query_hog_ql_output"
        );
        assert_eq!(
            file_stem("SupabaseCreateSignedURLOutput"),
            "supabase_create_signed_url_output"
        );
    }

    #[test]
    fn binary_successes_allow_multiple_media_types() -> Result<(), crate::CoreError> {
        let graph = ApiGraph::default();
        let op = Operation {
            id: "download".to_string(),
            method: "GET".to_string(),
            path: "/download".to_string(),
            handler: "download".to_string(),
            group: None,
            middleware: Vec::new(),
            params: vec![],
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: vec![
                Response {
                    status: 200,
                    body: None,
                    body_kind: "binary".to_string(),
                    content_type: Some("application/pdf".to_string()),
                    content_types: vec!["application/pdf".to_string()],
                },
                Response {
                    status: 206,
                    body: None,
                    body_kind: "binary".to_string(),
                    content_type: Some("application/octet-stream".to_string()),
                    content_types: vec!["application/octet-stream".to_string()],
                },
            ],
            security: Vec::new(),
            security_overrides_global: false,
            provenance: SourceSpan {
                file: "http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        };
        let success = success_responses_of(&op, &graph)?;
        assert_eq!(success.binary_statuses, vec![200, 206]);
        assert_eq!(
            success.binary_content_type.as_deref(),
            Some("application/pdf")
        );
        assert!(success.has_binary_body());
        assert!(!success.has_bodyless_alternative());
        Ok(())
    }

    #[test]
    fn operation_api_key_headers_honor_override_security() -> Result<(), crate::CoreError> {
        let graph = ApiGraph {
            security: vec![
                SecurityScheme {
                    id: "ApiKeyAuth".to_string(),
                    kind: "apiKey".to_string(),
                    location: "header".to_string(),
                    name: "X-API-Key".to_string(),
                    global: true,
                },
                SecurityScheme {
                    id: "CSRFAuth".to_string(),
                    kind: "apiKey".to_string(),
                    location: "header".to_string(),
                    name: "X-CSRF-Token".to_string(),
                    global: false,
                },
            ],
            ..ApiGraph::default()
        };
        let op = Operation {
            id: "write".to_string(),
            method: "POST".to_string(),
            path: "/write".to_string(),
            handler: "write".to_string(),
            group: None,
            middleware: Vec::new(),
            params: vec![],
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: vec![],
            security: vec!["CSRFAuth".to_string()],
            security_overrides_global: true,
            provenance: SourceSpan {
                file: "http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        };
        assert_eq!(
            operation_api_key_headers(&graph, &op)?,
            vec!["X-CSRF-Token"]
        );
        Ok(())
    }

    #[test]
    fn operation_api_key_queries_honor_global_and_public_override() -> Result<(), crate::CoreError>
    {
        let graph = ApiGraph {
            security: vec![SecurityScheme {
                id: "QueryAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "query".to_string(),
                name: "api_key".to_string(),
                global: true,
            }],
            ..ApiGraph::default()
        };
        let mut op = Operation {
            id: "list".to_string(),
            method: "GET".to_string(),
            path: "/items".to_string(),
            handler: "list".to_string(),
            group: None,
            middleware: Vec::new(),
            params: vec![],
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: vec![],
            security: vec![],
            security_overrides_global: false,
            provenance: SourceSpan {
                file: "http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        };
        assert_eq!(operation_api_key_queries(&graph, &op)?, vec!["api_key"]);
        op.security_overrides_global = true;
        assert!(operation_api_key_queries(&graph, &op)?.is_empty());
        Ok(())
    }

    #[test]
    fn operation_http_auth_schemes_honor_global_and_override() -> Result<(), crate::CoreError> {
        let graph = ApiGraph {
            security: vec![
                SecurityScheme {
                    id: "BearerAuth".to_string(),
                    kind: "http".to_string(),
                    location: String::new(),
                    name: "bearer".to_string(),
                    global: true,
                },
                SecurityScheme {
                    id: "BasicAuth".to_string(),
                    kind: "http".to_string(),
                    location: String::new(),
                    name: "basic".to_string(),
                    global: false,
                },
                SecurityScheme {
                    id: "HeaderAuth".to_string(),
                    kind: "apiKey".to_string(),
                    location: "header".to_string(),
                    name: "X-API-Key".to_string(),
                    global: true,
                },
            ],
            ..ApiGraph::default()
        };
        let features = http_auth_features(&graph)?;
        assert!(features.bearer);
        assert!(features.basic);

        let mut op = Operation {
            id: "write".to_string(),
            method: "POST".to_string(),
            path: "/write".to_string(),
            handler: "write".to_string(),
            group: None,
            middleware: Vec::new(),
            params: vec![],
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: vec![],
            security: vec![],
            security_overrides_global: false,
            provenance: SourceSpan {
                file: "http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        };
        assert_eq!(
            operation_http_auth_schemes(&graph, &op)?,
            vec![HttpAuthScheme::Bearer]
        );
        assert_eq!(operation_api_key_headers(&graph, &op)?, vec!["X-API-Key"]);

        op.security = vec!["BasicAuth".to_string()];
        op.security_overrides_global = true;
        assert_eq!(
            operation_http_auth_schemes(&graph, &op)?,
            vec![HttpAuthScheme::Basic]
        );
        assert!(operation_api_key_headers(&graph, &op)?.is_empty());
        Ok(())
    }
}
