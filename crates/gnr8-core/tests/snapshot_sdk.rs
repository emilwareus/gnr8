//! GREEN contract test (FIX-03 / FIX-04): the expected Go SDK output for goalservice.
//!
//! Builds the graph via the Phase-2 `analyze::build_graph` seam, then generates the Go SDK via the
//! Phase-3 `sdk::generate` seam (03-02). Both seams are now implemented: `generate` returns a
//! deterministic, gofmt-clean multi-file Go SDK bundle String (functional-options `Client`, tag-grouped
//! ctx-first operations, model structs + enum newtypes, typed `APIError`), framed with stable
//! `// ==== gnr8:file <name> ====` markers. The committed `.snap` was authored from the real generated
//! output and reviewed against `fixtures/goalservice/expected/sdk/*.go` for semantic equivalence
//! (Pitfall 2 — reconcile, not byte-copy). The snapshot uses `assert_snapshot!` (plain text).
//!
//! This test runs the Go toolchain (it builds the graph AND pipes each file through `gofmt`), present
//! on dev + CI (go 1.26).

// Tests legitimately use unwrap/expect (skill ch.4 + ch.5); scoped allow keeps RUST-04 intact
// for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn sdk_matches_expected_for_goalservice() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 build_graph must succeed (requires the Go toolchain)");
    let sdk = gnr8_core::sdk::generate(&graph)
        .expect("Phase 3 sdk::generate must succeed (requires gofmt)");
    insta::assert_snapshot!("goalservice_sdk", sdk);
}
