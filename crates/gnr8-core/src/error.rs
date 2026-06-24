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

    /// The Go toolchain could not be spawned (e.g. `go` is not installed or not on `PATH`).
    ///
    /// Wraps the [`std::io::Error`] from spawning the `goextract` subprocess so the cause is
    /// preserved without panicking (GO-06).
    #[error("Go toolchain not available (is `go` installed and on PATH?): {source}")]
    GoToolchainMissing {
        /// The underlying spawn error from [`std::process::Command::output`].
        #[source]
        source: std::io::Error,
    },

    /// The `goextract` helper ran but exited with a non-zero status.
    ///
    /// Carries the exit `code` (absent if the process was signal-terminated) and the captured
    /// `stderr` text so callers can diagnose the failure (GO-06).
    #[error("goextract helper exited with status {code:?}:\n{stderr}")]
    HelperExit {
        /// The process exit code, or `None` if terminated by a signal.
        code: Option<i32>,
        /// The captured standard-error output of the helper.
        stderr: String,
    },

    /// The helper's stdout could not be parsed as the expected JSON facts document.
    ///
    /// Wraps the [`serde_json::Error`] (including position info) so malformed or
    /// forward-incompatible JSON is a typed error, never a panic (GO-06 / Security V5).
    #[error("failed to parse goextract JSON facts: {source}")]
    FactsParse {
        /// The underlying deserialization error.
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5);
    // scope the allow so the workspace-wide RUST-04 deny stays intact for prod code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::CoreError;

    mod display {
        use super::CoreError;

        #[test]
        fn go_toolchain_missing_renders_with_source() {
            let err = CoreError::GoToolchainMissing {
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
            };
            let msg = err.to_string();
            assert!(msg.contains("Go toolchain not available"), "{msg}");
            assert!(msg.contains("no such file"), "{msg}");
        }

        #[test]
        fn helper_exit_renders_code_and_stderr() {
            let err = CoreError::HelperExit {
                code: Some(2),
                stderr: "go: cannot find module".to_string(),
            };
            let msg = err.to_string();
            assert!(msg.contains("exited with status"), "{msg}");
            assert!(msg.contains("Some(2)"), "{msg}");
            assert!(msg.contains("go: cannot find module"), "{msg}");
        }

        #[test]
        fn facts_parse_renders_underlying_serde_error() {
            // Force a real serde_json::Error by parsing invalid JSON.
            let parse_err = serde_json::from_str::<serde_json::Value>("{ not json }").unwrap_err();
            let err = CoreError::FactsParse { source: parse_err };
            let msg = err.to_string();
            assert!(
                msg.contains("failed to parse goextract JSON facts"),
                "{msg}"
            );
        }
    }
}
