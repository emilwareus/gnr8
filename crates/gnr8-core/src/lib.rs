//! gnr8 — the generation API used by project-local `.gnr8/` crates.
//!
//! The installed `gnr8` CLI scaffolds a small Rust binary crate at `.gnr8/`. That crate depends on
//! this package, imports [`sdk::prelude`], builds a [`sdk::Pipeline`], and hands it to
//! [`runner::run`]. The CLI then compiles and runs the local crate, receives the artifact bundle, and
//! owns writing generated files.
//!
//! A typical `.gnr8/src/main.rs`:
//!
//! ```no_run
//! use gnr8::sdk::prelude::*;
//!
//! fn main() -> std::process::ExitCode {
//!     gnr8::runner::run(
//!         Pipeline::new()
//!             .source(FastApi::new().inputs(["."]))
//!             .transform(SetBasePath::new("/api"))
//!             .transform(SetTitle::new("Public API"))
//!             .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
//!             .target(OpenApi31::new().to("generated/openapi.yaml"))
//!             .target(PySdk::new().module("example.com/public/sdk").to("generated/sdk"))
//!             .post(Header::generated()),
//!     )
//! }
//! ```
//!
//! Supported source stages include [`sdk::builtins::GoGin`], [`sdk::builtins::FastApi`],
//! [`sdk::builtins::Flask`], and [`sdk::builtins::NestJs`]. Supported generation targets include
//! [`sdk::builtins::OpenApi31`], [`sdk::builtins::GoSdk`], [`sdk::builtins::PySdk`], and
//! [`sdk::builtins::TsSdk`].
//!
//! For agent-facing CLI workflows, run `gnr8 guide` or read
//! <https://github.com/emilwareus/gnr8/blob/main/docs/AGENT-USAGE.md>.

// Existing module docs intentionally link some private implementation seams. Keep docs.rs builds
// warning-free while the public crate root and SDK prelude remain the stable entry points.
#![allow(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

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
