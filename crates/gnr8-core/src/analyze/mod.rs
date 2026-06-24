//! Go source analysis seam (Phase 2): reads a Go module and extracts HTTP route facts.
//!
//! Wave 1 (02-01) lands the Rust↔Go contract surface:
//! - [`facts`] — the serde mirror of the `goextract` JSON facts document.
//! - [`helper`] — the `std::process::Command` subprocess driver with typed errors.
//!
//! [`build_graph`] still returns [`crate::CoreError::NotYetImplemented`]; 02-03 builds the
//! [`crate::graph::ApiGraph`] from [`facts::GoFacts`] and flips it green.

pub(crate) mod facts;
pub(crate) mod helper;

/// Build the router-agnostic [`crate::graph::ApiGraph`] from a Go fixture/module directory.
///
/// Stubbed until 02-03 — returns [`crate::CoreError::NotYetImplemented`]. The contract surface
/// it will consume ([`facts::GoFacts`] via [`helper::run_goextract`]) lands in 02-01.
///
/// # Errors
///
/// Returns [`crate::CoreError::NotYetImplemented`] until 02-03 implements graph construction.
pub fn build_graph(_fixture_dir: &str) -> Result<crate::graph::ApiGraph, crate::CoreError> {
    crate::not_yet("analyze::build_graph", 2)
}
