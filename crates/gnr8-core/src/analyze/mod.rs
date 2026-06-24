//! Go source analysis seam (Phase 2): reads a Go module and extracts HTTP route facts.

/// Build the router-agnostic [`crate::graph::ApiGraph`] from a Go fixture/module directory.
///
/// Stubbed in Phase 1 — returns [`crate::CoreError::NotYetImplemented`]. Implemented in Phase 2.
///
/// # Errors
///
/// Returns [`crate::CoreError::NotYetImplemented`] until Phase 2 implements Go analysis.
pub fn build_graph(_fixture_dir: &str) -> Result<crate::graph::ApiGraph, crate::CoreError> {
    crate::not_yet("analyze::build_graph", 2)
}
