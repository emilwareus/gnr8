//! Contract test (IR-04): the OpenAPI 3.1 document lowered from the Flask bookstore fixture — GREEN.
//!
//! Plan 02-04 landed Flask route recognition in `pyextract`, so `analyze::build_graph` produces the
//! real graph and the reused lowering renders the OpenAPI document asserted against the COMMITTED
//! `snapshots/snapshot_flask_openapi__flask_openapi.snap` (objects, arrays, string enums, `oneOf`
//! unions, `type: [T, "null"]` nullability). The extractor + reconciled fixture reproduce it
//! byte-for-byte with ZERO snapshot edits. `CI=true` keeps insta in `INSTA_UPDATE=no`, so a mismatch
//! hard-fails — it never auto-accepts.
//!
//! Requires the python3 toolchain (the test invokes the helper via `python3 -m pyextract`).

// Tests legitimately use unwrap/expect; scoped allow keeps RUST-04 intact for production code.
// `doc_markdown` is allowed too: these test-target doc comments are prose that names many
// proper nouns (Flask, OpenAPI, pyextract, ...) where backtick-per-noun hurts readability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::doc_markdown)]

/// The static Flask bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/flask-bookstore"
);

/// The fixture's security schemes — code-as-config (CLAUDE.md rule 4); security is supplied, never
/// scraped. One `ApiKeyAuth` / `X-API-Key` scheme.
fn fixture_security() -> Vec<gnr8::graph::SecurityScheme> {
    vec![gnr8::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(),
        kind: "apiKey".to_string(),
        location: "header".to_string(),
        name: "X-API-Key".to_string(),
        global: true,
    }]
}

#[test]
fn openapi_matches_expected_for_flask() {
    // 02-04: build_graph runs pyextract and the reused lowering renders the OpenAPI; the committed
    // .snap locks its reviewed shape (byte-identical against the reconciled fixture).
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires the python3 toolchain)");
    let openapi = gnr8::lower::to_openapi(&graph, "bookstore", "/orders", &fixture_security())
        .expect("lower::to_openapi must succeed");
    insta::assert_snapshot!("flask_openapi", openapi);
}
