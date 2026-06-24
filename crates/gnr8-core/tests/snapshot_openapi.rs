//! Red-by-design contract test (FIX-03 / FIX-04): the expected `OpenAPI` 3.1.0 document for goalservice.
//!
//! Builds the graph via the Phase-2 `analyze::build_graph` seam, then lowers it via the Phase-3
//! `lower::to_openapi` seam. Today `build_graph` already returns `CoreError::NotYetImplemented`, so
//! the first `.expect()` panics BEFORE the snapshot assertion → the test FAILS CLEARLY
//! (red-by-design). It is never marked ignored (FIX-04), and there is no pre-authored `.snap`.
//! `to_openapi` returns the serialized `OpenAPI` text, so the snapshot uses `assert_snapshot!`
//! (plain text). Phases 2-3 implement both seams → the snapshot is reviewed/accepted → it turns green.

// Tests legitimately use unwrap/expect (skill ch.4 + ch.5); scoped allow keeps RUST-04 intact
// for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn openapi_matches_expected_for_goalservice() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 must implement analyze::build_graph; red-by-design until then");
    let openapi = gnr8_core::lower::to_openapi(&graph)
        .expect("Phase 3 must implement lower::to_openapi; red-by-design until then");
    insta::assert_snapshot!("goalservice_openapi", openapi);
}
