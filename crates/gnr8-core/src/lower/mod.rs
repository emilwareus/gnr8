//! `OpenAPI` lowering seam (Phase 3): lowers the API graph to an `OpenAPI` 3.1.0 document.

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
