//! Go SDK generation seam (Phase 3): generates a Go SDK from the API graph.

mod bundle;
mod emit;
mod gofmt;

// `generate` (Task 3) consumes `SdkBundle` + `write_to_dir`; until then they are unused. Allow so the
// `-D warnings` gate stays green this task. Removed once `generate` is wired.
#[allow(unused_imports)]
use bundle::SdkBundle;

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

/// Materialize an [`SdkBundle`]'s framed files to `dir/<name>` (consumed by 03-03's compile test).
///
/// File names are program-controlled (the fixed `client.go`/`errors.go`/`<tag>.go`/`models.go` set —
/// threat T-03-03 temp-dir hygiene; no untrusted path is joined). The bundle is re-parsed through the
/// shared [`bundle::parse`] framing so the on-disk files match the snapshot byte-for-byte.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] if any file cannot be written.
#[allow(dead_code)] // consumed by 03-03's compile test; wired here for that plan.
pub(crate) fn write_to_dir(
    bundle: &SdkBundle,
    dir: &std::path::Path,
) -> Result<(), crate::CoreError> {
    let framed = bundle.to_string();
    for (name, contents) in bundle::parse(&framed) {
        let path = dir.join(&name);
        std::fs::write(&path, contents).map_err(|err| crate::CoreError::SdkGen {
            message: format!("failed to write SDK file {}: {err}", path.display()),
        })?;
    }
    Ok(())
}
