//! gnr8-core — extraction, graph construction, `OpenAPI` lowering, SDK generation, lifecycle planning,
//! and diagnostics for the `gnr8` CLI.
//!
//! The crate is the typed library boundary: production paths return [`CoreError`], generated artifacts
//! are deterministic, and language-specific extractors feed one neutral API graph that every target
//! lowers from.

pub mod error;
pub use error::CoreError;

pub mod analyze;
pub mod diagnostics;
pub mod gosdk;
pub mod graph;
pub mod lifecycle;
pub mod lower;
pub mod manifest;
pub mod pysdk;
pub mod resource;
pub mod runner;
pub mod sdk;
pub mod tssdk;
pub mod workspace;

/// Convenience re-export of the code-as-config composition surface (the four traits, `Pipeline`,
/// `Cx`, `Artifacts`, every built-in, and `SecurityScheme`). The user's `.gnr8` lifecycle imports
/// `use gnr8::sdk::prelude::*;` (or this alias) and composes a [`sdk::Pipeline`].
pub use sdk::prelude;

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
        // build_graph is implemented: it detects the target language and runs the matching sidecar
        // rather than returning the NotYetImplemented stub. With language dispatch (02-01) a
        // non-existent target classifies as ambiguous (no Go/Python markers) and surfaces `Config`
        // BEFORE any spawn; a real-but-bad target would surface `HelperExit`/`FactsParse`, and a
        // missing toolchain `GoToolchainMissing`/`PythonToolchainMissing`. Never NotYetImplemented,
        // never a panic (GO-06 / rule 3).
        let result = crate::analyze::build_graph("/gnr8-nonexistent-target-dir-xyz");
        let err = result.unwrap_err();
        assert!(
            !matches!(err, CoreError::NotYetImplemented { .. }),
            "build_graph is implemented now; got {err:?}"
        );
        assert!(
            matches!(
                err,
                CoreError::Config { .. }
                    | CoreError::GoToolchainMissing { .. }
                    | CoreError::PythonToolchainMissing { .. }
                    | CoreError::HelperExit { .. }
                    | CoreError::FactsParse { .. }
            ),
            "expected a typed dispatch/subprocess error, got {err:?}"
        );
    }
}
