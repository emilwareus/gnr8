//! The crate-level typed error for gnr8-core (RUST-04 / D-09).
//!
//! Library code returns `Result<_, CoreError>` and never panics in production paths; the
//! binary's `anyhow` boundary lives only in `gnr8/src/main.rs`.

/// Errors produced by gnr8-core.
///
/// Phase 1 ships only the `NotYetImplemented` variant — every module seam returns it until the
/// owning phase fills the seam in. Real variants (e.g. Go package-load failures) are added as
/// Phases 2–3 land:
///
/// ```ignore
/// // #[error("failed to load Go packages at {path}: {source}")]
/// // PackageLoad { path: String, #[source] source: std::io::Error },
/// ```
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A command or seam recognized by the CLI but not yet built; names the implementing phase.
    #[error("'{command}' is not yet implemented (arrives in phase {phase})")]
    NotYetImplemented {
        /// The command or seam name (e.g. `"generate"`, `"analyze::build_graph"`).
        command: String,
        /// The roadmap phase that will implement it.
        phase: u8,
    },
}
