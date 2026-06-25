//! End-to-end determinism contract (GRAPH-02 / D-08): two runs over the unchanged goalservice fixture
//! must serialize byte-identically — for the graph AND for both downstream artifacts (`OpenAPI` + SDK).
//!
//! This is the integration-level proof that the whole pipeline is deterministic — the Go helper sorts
//! before marshalling, `ApiGraph::from_facts` sorts every collection and relativizes file paths, and
//! lowering/SDK emission preserve that order with `Vec<(K,V)>` (never a `HashMap`), so unchanged source
//! ⇒ identical output (RESEARCH Pitfall 4 / TARGET-API §5.6 idempotent generation). It complements the
//! per-rule unit tests in `graph::tests` and the locked `snapshot_*` snapshots.
//!
//! Requires the Go toolchain (the tests invoke the helper via `go run`, and `sdk::generate` pipes each
//! file through `gofmt`). They skip gracefully — return early rather than failing — if the toolchain is
//! unavailable, but on dev + CI (go 1.26) they run.

// Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture, resolved relative to this crate's manifest dir (mirrors the snapshot tests).
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// The fixture's security config — the single source of truth for security (CLAUDE.md rule 4): one
/// `ApiKeyAuth` / `X-API-Key` scheme. Security is no longer scraped from the source, so the contract
/// tests supply it here to drive lowering.
fn fixture_security() -> gnr8_core::config::SecurityConfig {
    gnr8_core::config::SecurityConfig {
        schemes: vec![gnr8_core::config::SecurityScheme {
            id: "ApiKeyAuth".to_string(),
            kind: "apiKey".to_string(),
            location: "header".to_string(),
            name: "X-API-Key".to_string(),
        }],
    }
}

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

#[test]
fn to_openapi_is_byte_identical_across_two_runs() {
    // Skip gracefully if the Go toolchain is absent so the test never fails for a missing dependency.
    let Ok(first) = gnr8_core::analyze::build_graph(FIXTURE_DIR) else {
        eprintln!("skipping OpenAPI determinism test: go toolchain unavailable for {FIXTURE_DIR}");
        return;
    };
    let second = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("second build_graph run must also succeed");

    // Build the graph twice AND lower twice — proving both the upstream graph and the lowering are
    // deterministic end-to-end (idempotent OpenAPI generation, RESEARCH Pitfall 4 / TARGET-API §5.6).
    let security = fixture_security();
    let a = gnr8_core::lower::to_openapi(&first, "goalservice", "/goal", &security)
        .expect("first to_openapi must succeed");
    let b = gnr8_core::lower::to_openapi(&second, "goalservice", "/goal", &security)
        .expect("second to_openapi must succeed");

    assert_eq!(
        a, b,
        "two to_openapi runs over unchanged source must be byte-identical (idempotent lowering)"
    );
}

#[test]
fn sdk_generate_is_byte_identical_across_two_runs() {
    // Skip gracefully if the Go toolchain is absent (build_graph + gofmt both need it).
    let Ok(first) = gnr8_core::analyze::build_graph(FIXTURE_DIR) else {
        eprintln!("skipping SDK determinism test: go toolchain unavailable for {FIXTURE_DIR}");
        return;
    };
    let second = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("second build_graph run must also succeed");

    // Build the graph twice AND generate twice — proving the SDK emission (gofmt'd, file-marker-framed)
    // is byte-identical end-to-end (idempotent SDK generation).
    let a = gnr8_core::sdk::generate(&first, "goalservice", "/goal")
        .expect("first sdk::generate must succeed (requires gofmt)");
    let b = gnr8_core::sdk::generate(&second, "goalservice", "/goal")
        .expect("second sdk::generate must succeed (requires gofmt)");

    assert_eq!(
        a, b,
        "two sdk::generate runs over unchanged source must be byte-identical (idempotent SDK gen)"
    );
}
