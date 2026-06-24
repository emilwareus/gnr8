//! Diagnostics seam (Phase 2+): collects warnings for unsupported / lossy source patterns.

/// Collect diagnostics (serialized output) for a Go fixture/module directory.
///
/// Stubbed in Phase 1 — returns [`crate::CoreError::NotYetImplemented`]. Implemented in Phase 2+.
///
/// # Errors
///
/// Returns [`crate::CoreError::NotYetImplemented`] until Phase 2+ implements diagnostics.
pub fn collect(_fixture_dir: &str) -> Result<String, crate::CoreError> {
    crate::not_yet("diagnostics::collect", 2)
}
