//! Red-by-design contract test (FIX-03 / FIX-04): the expected API graph for the goalservice fixture.
//!
//! This test calls the Phase-2 `analyze::build_graph` seam, which today returns
//! `CoreError::NotYetImplemented`, so the `.expect()` panics BEFORE the snapshot assertion runs →
//! the test FAILS CLEARLY (red-by-design, per RESEARCH Pattern 4 mechanism 1). It is never marked
//! ignored (silent skip would violate FIX-04), and there is no pre-authored `.snap` (the redness
//! comes from the stubbed seam, not a missing-file race). Phase 2 implements `build_graph` → the
//! snapshot is reviewed/accepted → the test turns green.

// Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn graph_matches_expected_for_goalservice() {
    // Phase 1: build_graph(..) returns Err(NotYetImplemented) → .expect() panics → test FAILS (red).
    // Phase 2: build_graph(..) returns the real graph → snapshot compared → turns green on review.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 must implement analyze::build_graph; red-by-design until then");
    insta::assert_yaml_snapshot!("goalservice_graph", graph);
}
