//! Contract test (FIX-03 / FIX-04): the expected API graph for the goalservice fixture — now GREEN.
//!
//! 02-03 implemented the `analyze::build_graph` seam, so the `.expect()` now succeeds and the test
//! asserts the real router-agnostic graph against the reviewed
//! `snapshots/snapshot_graph__goalservice_graph.snap` (4 operations with stable ids — all the handler
//! symbol, purely code-derived — request/response schema refs, the `aggregation`/`cursor`/`page_size`
//! query params, relativized provenance spans, all object schemas + the code-defined `TargetDirection`
//! enum). The snapshot was authored from REAL output and reviewed (not hand-written). CI runs insta
//! in `INSTA_UPDATE=no` (`CI=true`), so a mismatch hard-fails — it never auto-accepts (FIX-04).
//!
//! Requires the Go toolchain (the test invokes the helper via `go run`).

// Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn graph_matches_expected_for_goalservice() {
    // 02-03: build_graph runs the goextract helper and returns the real router-agnostic graph; the
    // snapshot below locks its reviewed YAML shape (byte-identical across unchanged source, GRAPH-02).
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires the Go toolchain)");
    insta::assert_yaml_snapshot!("goalservice_graph", graph);
}
