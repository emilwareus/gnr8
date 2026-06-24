//! `OpenAPI` lowering seam (Phase 3): lowers the API graph to an `OpenAPI` 3.1.0 document.
//!
//! The graph is the source of truth; the `OpenAPI` document is an artifact serialized from typed
//! structs (PROJECT constraint / D-01). [`to_openapi`] is a pure graph→typed-doc transform (no
//! re-analysis — D-02): it builds a [`model::OpenApiDoc`] from the [`crate::graph::ApiGraph`] and
//! serializes it with the deterministic key-ordered writer in [`yaml`].
//!
//! ## Resolved Open Question A3 — the absolute `/goal` base-path prefix
//!
//! The Phase-2 graph stores **group-relative** operation paths (`/`, `/list`, `/{uuid}`) and carries
//! NO explicit service base path; 02-03 deferred joining the dynamic `"/" + basePath` prefix to
//! Phase-3 lowering (see `graph::Operation::path`). Per RESEARCH recommendation (a) — lower from a
//! known constant for the single-group `PoC` rather than reshaping the Phase-2 graph (which would be
//! an out-of-scope Phase-2 change) — this module defines a private [`BASE_PATH`] and joins it to each
//! operation's group-relative path with slash-collapse, yielding `/goal/`, `/goal/list`,
//! `/goal/{uuid}` (never `/goal//list` and never a dropped prefix). A multi-group generalization is
//! deferred (D-02).

// The typed model + writer are exercised by their own unit tests in this task; `to_openapi` wires
// them into a production path in Task 3, at which point this allow is removed. Scoped to these two
// submodules so the rest of the crate keeps the default dead-code deny.
#[allow(dead_code)]
mod model;
#[allow(dead_code)]
mod yaml;

/// The absolute service base-path prefix joined to every group-relative operation path (Open Q A3).
///
/// The fixture is a single route group mounted at `/goal`; the graph cannot constant-fold the dynamic
/// `basePath` prefix, so lowering supplies it deterministically here. See the module doc for the full
/// rationale.
#[allow(dead_code)] // wired by `to_openapi` in Task 3.
const BASE_PATH: &str = "/goal";

/// Lower the [`crate::graph::ApiGraph`] to an `OpenAPI` 3.1.0 document (serialized).
///
/// Stubbed in Phase 1 — returns [`crate::CoreError::NotYetImplemented`]. Implemented in Phase 3.
///
/// # Errors
///
/// Returns [`crate::CoreError::NotYetImplemented`] until Phase 3 implements `OpenAPI` lowering.
pub fn to_openapi(_graph: &crate::graph::ApiGraph) -> Result<String, crate::CoreError> {
    crate::not_yet("lower::to_openapi", 3)
}
