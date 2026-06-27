//! Contract test (IR-04): the neutral API graph for the NestJS bookstore fixture — now GREEN.
//!
//! Plan 04-03 landed NestJS route recognition in `tsextract`, so `analyze::build_graph` (TypeScript
//! dispatch → `run_tsextract`) produces the real router-agnostic graph for the bookstore service and
//! this test asserts it against the COMMITTED `snapshots/snapshot_nestjs_graph__nestjs_graph.snap`.
//! That snapshot was authored BY HAND to the intended neutral graph and the extractor + reconciled
//! fixture reproduce it byte-for-byte with ZERO snapshot edits. `CI=true` keeps insta in
//! `INSTA_UPDATE=no`, so a mismatch hard-fails — it never auto-accepts.
//!
//! Requires the node toolchain + the vendored `typescript` (the test invokes the helper via
//! `node tsextract/index.js`). It SKIPS gracefully — returns early rather than failing — when node
//! or the vendored typescript is absent, mirroring the Go fixture tests' skip when `go` is absent, so
//! `make check` stays green on a node-less box.

// Tests legitimately use unwrap/expect; scope the allow to this test target so the workspace-wide
// RUST-04 deny stays intact for production code (Pitfall 2).
// `doc_markdown` is allowed too: these test-target doc comments are prose that names many
// proper nouns (NestJS, OpenAPI, tsextract, ...) where backtick-per-noun hurts readability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::doc_markdown)]

mod nestjs_toolchain;

/// The static NestJS bookstore fixture, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/nestjs-bookstore"
);

#[test]
fn graph_matches_expected_for_nestjs() {
    // Skip (not fail) when node or the vendored typescript is absent, so a toolchain-less box never
    // hard-fails make check (mirrors the determinism.rs go-toolchain skip).
    if !nestjs_toolchain::available() {
        eprintln!("skipping nestjs graph snapshot: node/typescript toolchain unavailable");
        return;
    }
    // 04-03: build_graph runs the tsextract helper and returns the real router-agnostic graph; the
    // committed .snap locks its reviewed YAML shape (byte-identical against the reconciled fixture).
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires node + the vendored typescript)");
    insta::assert_yaml_snapshot!("nestjs_graph", graph);
}
