//! Contract test (IR-04): the neutral API graph for the Flask bookstore fixture — now GREEN.
//!
//! Plan 02-04 landed Flask route recognition (the HONEST typed envelope) in `pyextract`, so
//! `analyze::build_graph` (Python dispatch → `run_pyextract`) produces the real router-agnostic graph
//! for the Flask bookstore service and this test asserts it against the COMMITTED
//! `snapshots/snapshot_flask_graph__flask_graph.snap`. That snapshot was authored BY HAND to the
//! intended neutral graph — including the three untyped-surface diagnostics — and the extractor +
//! reconciled fixture reproduce it byte-for-byte with ZERO snapshot edits. `CI=true` keeps insta in
//! `INSTA_UPDATE=no`, so a mismatch hard-fails — it never auto-accepts.
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

#[test]
fn graph_matches_expected_for_flask() {
    // 02-04: build_graph runs the pyextract helper and returns the real router-agnostic graph for the
    // Flask typed envelope; the committed .snap locks its reviewed YAML shape (byte-identical against
    // the reconciled fixture, diagnostics anchored at lines 42/69/78).
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires the python3 toolchain)");
    insta::assert_yaml_snapshot!("flask_graph", graph);
}
