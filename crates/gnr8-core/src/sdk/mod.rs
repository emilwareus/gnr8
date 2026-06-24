//! Go SDK generation seam (Phase 3): generates a Go SDK from the API graph.

/// Generate the Go SDK (serialized output) from the [`crate::graph::ApiGraph`].
///
/// Stubbed in Phase 1 — returns [`crate::CoreError::NotYetImplemented`]. Implemented in Phase 3.
///
/// # Errors
///
/// Returns [`crate::CoreError::NotYetImplemented`] until Phase 3 implements SDK generation.
pub fn generate(_graph: &crate::graph::ApiGraph) -> Result<String, crate::CoreError> {
    crate::not_yet("sdk::generate", 3)
}
