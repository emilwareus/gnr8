//! RED-BY-DESIGN contract test (IR-04): the INTENDED neutral API graph for the FastAPI bookstore
//! fixture.
//!
//! This test is intentionally RED in Phase 1: there is NO `pyextract` sidecar yet, so
//! `analyze::build_graph` cannot produce a graph for a Python service and the `.expect()` below
//! panics honestly. The committed `snapshots/snapshot_fastapi_graph__fastapi_graph.snap` is authored
//! BY HAND to the INTENDED neutral graph the Phase-2 extractor MUST produce — it is the acceptance
//! contract that flips this test green with ZERO snapshot edits the moment `pyextract` lands.
//!
//! Because it is red by design it is marked `#[ignore]` so `cargo test`/`make check` SKIP it (the
//! green gate stays green) while it remains visible and runnable on demand:
//!   `cargo test -p gnr8-core --test snapshot_fastapi_graph -- --ignored`  (FAILS at the .expect()).
//! `CI=true` keeps insta in `INSTA_UPDATE=no`, so the committed snapshot never auto-flips green.

// Tests legitimately use unwrap/expect; scope the allow to this test target so the workspace-wide
// RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The static FastAPI bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fastapi-bookstore");

#[test]
#[ignore = "red-by-design: pyextract lands in Phase 2; intended-green snapshot is the acceptance contract"]
fn graph_matches_expected_for_fastapi() {
    // RED BY DESIGN (Phase 1): no pyextract yet, so build_graph cannot produce this graph.
    // The committed .snap encodes the INTENDED neutral graph the Phase-2 extractor must produce.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — intentionally red until then");
    insta::assert_yaml_snapshot!("fastapi_graph", graph);
}
