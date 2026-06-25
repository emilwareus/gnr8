//! RED-BY-DESIGN contract test (IR-04): the INTENDED neutral API graph for the NestJS bookstore
//! fixture.
//!
//! Intentionally RED in Phase 1: there is NO `tsextract` sidecar yet, so `analyze::build_graph`
//! cannot produce a graph for a TypeScript service and the `.expect()` below panics honestly. The
//! committed `snapshots/snapshot_nestjs_graph__nestjs_graph.snap` is authored BY HAND to the INTENDED
//! neutral graph the Phase-4 extractor MUST produce — the acceptance contract that flips this test
//! green with ZERO snapshot edits when `tsextract` lands.
//!
//! Marked `#[ignore]` (red-by-design) so `cargo test`/`make check` SKIP it while it stays visible:
//!   `cargo test -p gnr8-core --test snapshot_nestjs_graph -- --ignored`  (FAILS at the .expect()).

// Tests legitimately use unwrap/expect; scoped allow keeps RUST-04 intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The static NestJS bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/nestjs-bookstore");

#[test]
#[ignore = "red-by-design: tsextract lands in Phase 4; intended-green snapshot is the acceptance contract"]
fn graph_matches_expected_for_nestjs() {
    // RED BY DESIGN (Phase 1): no tsextract yet, so build_graph cannot produce this graph.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("tsextract lands in Phase 4 — intentionally red until then");
    insta::assert_yaml_snapshot!("nestjs_graph", graph);
}
