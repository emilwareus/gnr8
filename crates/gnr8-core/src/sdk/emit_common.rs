//! Language-agnostic emit helpers shared by the Go, Python, and TypeScript SDK emitters.
//!
//! These are the pure, byte-identical pieces of `gosdk::emit`/`pysdk::emit`/`tssdk::emit`: identifier
//! tokenization ([`split_words`]), path joining ([`join_path`]) and templating ([`path_tokens`] +
//! [`path_tokens_match`]), and the graph-walking model resolvers ([`success_of`], [`body_model_of`]).
//! They contain NO per-language formatting — the casers (`exported`/`snake`/`camel`/…) and the type
//! mappers (`go_type`/`py_type`/`ts_type`) stay in each emitter, where they genuinely diverge. One
//! definition per fact (CLAUDE.md rule 3).

use std::collections::BTreeSet;

use crate::graph::{ApiGraph, Operation};
use crate::CoreError;

/// Split an identifier into words on `_`/`-`/space separators and lower→upper case boundaries.
///
/// `workflowChainIds` → `["workflow", "Chain", "Ids"]`; `page_size` → `["page", "size"]`. The shared
/// tokenizer behind every per-language casing helper.
pub(crate) fn split_words(name: &str) -> Vec<String> {
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

/// Resolve an operation's primary success (lowest 2xx) response status + model name.
///
/// Returns the first 2xx response's status regardless of whether it carries a body (WR-01); the model is
/// `Some` only when that response has a typed body. The graph sorts each operation's responses by status
/// at build time, so "first 2xx in order" IS "lowest 2xx" (rule 3: one source of truth, no second sort).
/// An operation with no 2xx response at all yields `None`.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the success body `$ref` is dangling.
pub(crate) fn success_of(
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

/// Resolve an operation's request-body model name, if it has a typed body.
///
/// # Errors
///
/// Returns [`CoreError::SdkGen`] if the request-body `$ref` is dangling.
pub(crate) fn body_model_of(op: &Operation, graph: &ApiGraph) -> Result<Option<String>, CoreError> {
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
