//! The internal API graph — the source of truth from which `OpenAPI` and the Go SDK are lowered.
//!
//! The graph is deliberately **router-agnostic** (D-03): it stores HTTP route facts (method, path
//! template, params, request type, response type + status), NOT framework internals. Gin is the only
//! recognized router in this proof-of-concept, but no Gin-specific field belongs here — that keeps
//! `chi`/`echo`/`net-http` addable later without reshaping the graph. The real shape lands in Phase 2.

/// Placeholder API graph. HTTP route facts land in Phase 2; empty for now.
///
/// MUST NOT carry router-framework-specific fields (no handler-context handles, etc.) — see D-03.
#[derive(Debug, Default, serde::Serialize)]
pub struct ApiGraph {
    // HTTP route facts (method / path template / params / request + response types) land in Phase 2.
}
