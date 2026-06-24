//! gnr8-core — extraction, graph, lowering, SDK generation, and diagnostics live here as
//! module seams. Phase 1 only stubs the seams; each returns a typed `CoreError::NotYetImplemented`
//! until the owning phase fills it in.

pub mod error;
pub use error::CoreError;

pub mod analyze;
pub mod config;
pub mod diagnostics;
pub mod graph;
pub mod lifecycle;
pub mod lower;
pub mod manifest;
pub mod sdk;
pub mod workspace;

/// Stub used by Phase-1 CLI arms and unimplemented seams.
///
/// Returns a typed, non-panicking error naming the command and the phase that will implement it.
///
/// # Errors
///
/// Always returns [`CoreError::NotYetImplemented`] carrying `command` and `phase` — this is a
/// scaffolding helper, so it never succeeds.
pub fn not_yet<T>(command: &str, phase: u8) -> Result<T, CoreError> {
    Err(CoreError::NotYetImplemented {
        command: command.to_string(),
        phase,
    })
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{not_yet, CoreError};

    #[test]
    fn core_error_not_yet_implemented_display() {
        let err = CoreError::NotYetImplemented {
            command: "generate".into(),
            phase: 3,
        };
        assert_eq!(
            err.to_string(),
            "'generate' is not yet implemented (arrives in phase 3)"
        );
    }

    #[test]
    fn not_yet_returns_typed_error() {
        let result = not_yet::<()>("init", 4);
        let err = result.unwrap_err();
        let CoreError::NotYetImplemented { command, phase } = err else {
            unreachable!("not_yet must return NotYetImplemented, got {err:?}");
        };
        assert_eq!(command, "init");
        assert_eq!(phase, 4);
    }

    #[test]
    fn build_graph_no_longer_returns_not_yet_implemented() {
        // 02-03 implemented build_graph: it now runs the goextract helper rather than returning the
        // NotYetImplemented stub. Against a bad target it surfaces a typed subprocess error (toolchain
        // missing / helper exit / parse) — never NotYetImplemented and never a panic (GO-06).
        let result = crate::analyze::build_graph("/gnr8-nonexistent-target-dir-xyz");
        let err = result.unwrap_err();
        assert!(
            !matches!(err, CoreError::NotYetImplemented { .. }),
            "build_graph is implemented now; got {err:?}"
        );
        assert!(
            matches!(
                err,
                CoreError::GoToolchainMissing { .. }
                    | CoreError::HelperExit { .. }
                    | CoreError::FactsParse { .. }
            ),
            "expected a typed subprocess error, got {err:?}"
        );
    }
}
