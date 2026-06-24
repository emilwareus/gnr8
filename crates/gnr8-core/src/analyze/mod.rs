//! Go source analysis seam (Phase 2): reads a Go module and extracts HTTP route facts.
//!
//! Wave 1 (02-01) landed the Rust↔Go contract surface:
//! - [`facts`] — the serde mirror of the `goextract` JSON facts document.
//! - [`helper`] — the `std::process::Command` subprocess driver with typed errors.
//!
//! Wave 3 (02-03) wires them together: [`build_graph`] runs the helper, deserializes the facts, and
//! assembles the router-agnostic [`crate::graph::ApiGraph`] (stable ids, sorted serialization,
//! provenance on every node — GRAPH-01/02, D-07/D-08).

pub(crate) mod facts;
pub(crate) mod helper;

/// Build the router-agnostic [`crate::graph::ApiGraph`] from a Go fixture/module directory.
///
/// Runs the `goextract` helper against `fixture_dir` (via [`helper::run_goextract`]), deserializes
/// the JSON facts, and maps them into the graph ([`crate::graph::ApiGraph::from_facts`]). Operation
/// ids are stable (the `@ID` annotation else the handler symbol), schema ids are package-qualified,
/// and every collection is sorted so two runs over unchanged source are byte-identical (GRAPH-02).
///
/// # Errors
///
/// Propagates the typed subprocess errors from [`helper::run_goextract`] — never a panic (GO-06):
/// - [`crate::CoreError::GoToolchainMissing`] if `go` cannot be spawned.
/// - [`crate::CoreError::HelperExit`] if the helper exits non-zero.
/// - [`crate::CoreError::FactsParse`] if the helper's stdout is not the expected JSON.
pub fn build_graph(fixture_dir: &str) -> Result<crate::graph::ApiGraph, crate::CoreError> {
    let facts = helper::run_goextract(fixture_dir)?;
    Ok(crate::graph::ApiGraph::from_facts(facts, fixture_dir))
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use crate::CoreError;

    /// `build_graph` against a non-existent fixture dir must still go through the helper and return a
    /// typed `CoreError` (toolchain-missing if `go` is absent, else `HelperExit`/`FactsParse`) — never
    /// a panic, never `NotYetImplemented` (GO-06). On a dev machine with the Go toolchain the helper
    /// runs and exits non-zero for a bad path → `HelperExit`; without `go` it is `GoToolchainMissing`.
    #[test]
    fn build_graph_surfaces_typed_error_for_bad_target() {
        let result = super::build_graph("/gnr8-nonexistent-target-dir-xyz");
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::GoToolchainMissing { .. }
                    | CoreError::HelperExit { .. }
                    | CoreError::FactsParse { .. }
            ),
            "expected a typed subprocess error, got {err:?}"
        );
        // It must NOT be the old NotYetImplemented stub.
        assert!(
            !matches!(err, CoreError::NotYetImplemented { .. }),
            "build_graph is implemented in 02-03; must not return NotYetImplemented"
        );
    }
}
