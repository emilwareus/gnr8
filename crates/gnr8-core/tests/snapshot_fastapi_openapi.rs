//! RED-BY-DESIGN contract test (IR-04): the INTENDED OpenAPI 3.1 document lowered from the FastAPI
//! bookstore fixture.
//!
//! This test is intentionally RED in Phase 1: there is NO `pyextract` sidecar yet, so
//! `analyze::build_graph` cannot produce a graph for a Python service and the `.expect()` below
//! panics honestly before lowering ever runs. The committed
//! `snapshots/snapshot_fastapi_openapi__fastapi_openapi.snap` is authored BY HAND to the INTENDED
//! neutral OpenAPI the Phase-2 extractor MUST produce (objects, arrays, string enums, `oneOf` unions,
//! `type: [T, "null"]` nullability) — the acceptance contract that flips this test green with ZERO
//! snapshot edits when `pyextract` lands.
//!
//! It is marked `#[ignore]` (red-by-design) so `cargo test`/`make check` SKIP it while it stays
//! visible and runnable on demand:
//!   `cargo test -p gnr8-core --test snapshot_fastapi_openapi -- --ignored`  (FAILS at the .expect()).

// Tests legitimately use unwrap/expect; scoped allow keeps RUST-04 intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The static FastAPI bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fastapi-bookstore");

/// The fixture's security schemes — the single source of truth for security (CLAUDE.md rule 4):
/// security is SUPPLIED by code-as-config, never scraped from the source. One `ApiKeyAuth` /
/// `X-API-Key` scheme, mirroring the goalservice OpenAPI contract test.
fn fixture_security() -> Vec<gnr8_core::graph::SecurityScheme> {
    vec![gnr8_core::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(),
        kind: "apiKey".to_string(),
        location: "header".to_string(),
        name: "X-API-Key".to_string(),
    }]
}

#[test]
#[ignore = "red-by-design: pyextract lands in Phase 2; intended-green snapshot is the acceptance contract"]
fn openapi_matches_expected_for_fastapi() {
    // RED BY DESIGN (Phase 1): no pyextract yet — build_graph panics before lowering runs.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — intentionally red until then");
    let openapi = gnr8_core::lower::to_openapi(&graph, "bookstore", "/books", &fixture_security())
        .expect("lower::to_openapi must succeed");
    insta::assert_snapshot!("fastapi_openapi", openapi);
}
