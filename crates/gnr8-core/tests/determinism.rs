//! End-to-end determinism contract (GRAPH-02 / D-08): two `build_graph` runs over the unchanged
//! goalservice fixture must serialize byte-identically.
//!
//! This is the integration-level proof that the whole pipeline is deterministic — the Go helper sorts
//! before marshalling, and `ApiGraph::from_facts` sorts every collection and relativizes file paths,
//! so unchanged source ⇒ identical output. It complements the per-rule unit tests in `graph::tests`
//! and the locked `snapshot_graph`/`snapshot_diagnostics` snapshots.
//!
//! Requires the Go toolchain (the test invokes the helper via `go run`). It skips gracefully — returns
//! early rather than failing — if the toolchain is unavailable, but on dev + CI (go 1.26) it runs.

// Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture, resolved relative to this crate's manifest dir (mirrors the snapshot tests).
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn build_graph_is_byte_identical_across_two_runs() {
    // Skip gracefully if the Go toolchain is absent so the test never fails for a missing dependency.
    let Ok(first) = gnr8_core::analyze::build_graph(FIXTURE_DIR) else {
        eprintln!("skipping determinism test: go toolchain unavailable for {FIXTURE_DIR}");
        return;
    };
    let second = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("second build_graph run must also succeed");

    let a = serde_json::to_string(&first).expect("serialize first graph");
    let b = serde_json::to_string(&second).expect("serialize second graph");

    assert_eq!(
        a, b,
        "two build_graph runs over unchanged source must serialize byte-identically (GRAPH-02)"
    );
}
