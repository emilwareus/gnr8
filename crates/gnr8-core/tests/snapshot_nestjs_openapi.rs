//! RED-BY-DESIGN contract test (IR-04): the INTENDED OpenAPI 3.1 document lowered from the NestJS
//! bookstore fixture.
//!
//! Intentionally RED in Phase 1: there is NO `tsextract` sidecar yet, so `analyze::build_graph`
//! panics honestly before lowering ever runs. The committed
//! `snapshots/snapshot_nestjs_openapi__nestjs_openapi.snap` is authored BY HAND to the INTENDED
//! neutral OpenAPI the Phase-4 extractor MUST produce (objects, arrays, string enums, `oneOf` unions,
//! `type: [T, "null"]` nullability) — the acceptance contract that flips green when `tsextract` lands.
//!
//! Marked `#[ignore]` (red-by-design) so `cargo test`/`make check` SKIP it while it stays visible:
//!   `cargo test -p gnr8-core --test snapshot_nestjs_openapi -- --ignored`  (FAILS at the .expect()).

// Tests legitimately use unwrap/expect; scoped allow keeps RUST-04 intact for production code.
// `doc_markdown` is allowed too: these test-target doc comments are prose that names many
// proper nouns (NestJS, OpenAPI, tsextract, ...) where backtick-per-noun hurts readability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::doc_markdown)]

/// The static NestJS bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/nestjs-bookstore"
);

/// The fixture's security schemes — code-as-config (CLAUDE.md rule 4); security is supplied, never
/// scraped. One `ApiKeyAuth` / `X-API-Key` scheme.
fn fixture_security() -> Vec<gnr8_core::graph::SecurityScheme> {
    vec![gnr8_core::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(),
        kind: "apiKey".to_string(),
        location: "header".to_string(),
        name: "X-API-Key".to_string(),
    }]
}

#[test]
#[ignore = "red-by-design: tsextract lands in Phase 4; intended-green snapshot is the acceptance contract"]
fn openapi_matches_expected_for_nestjs() {
    // RED BY DESIGN (Phase 1): no tsextract yet — build_graph panics before lowering runs.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("tsextract lands in Phase 4 — intentionally red until then");
    let openapi = gnr8_core::lower::to_openapi(&graph, "bookstore", "/books", &fixture_security())
        .expect("lower::to_openapi must succeed");
    insta::assert_snapshot!("nestjs_openapi", openapi);
}
