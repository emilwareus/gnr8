//! End-to-end determinism contract (GRAPH-02 / D-08): two runs over the unchanged goalservice fixture
//! must serialize byte-identically — for the graph AND for both downstream artifacts (`OpenAPI` + SDK).
//!
//! This is the integration-level proof that the whole pipeline is deterministic — the Go helper sorts
//! before marshalling, `ApiGraph::from_facts` sorts every collection and relativizes file paths, and
//! lowering/SDK emission preserve that order with `Vec<(K,V)>` (never a `HashMap`), so unchanged source
//! ⇒ identical output (RESEARCH Pitfall 4 / TARGET-API §5.6 idempotent generation). It complements the
//! per-rule unit tests in `graph::tests` and the locked `snapshot_*` snapshots.
//!
//! Requires the Go toolchain (the tests invoke the helper via `go run`, and `gosdk::generate` pipes each
//! file through `gofmt`). They skip gracefully — return early rather than failing — if the toolchain is
//! unavailable, but on dev + CI (go 1.26) they run.

// Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code (Pitfall 2).
// `doc_markdown` is allowed too: these test-target doc comments name many proper nouns (NestJS,
// FastAPI, OpenAPI, ...) where backtick-per-noun hurts readability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::doc_markdown)]

mod nestjs_toolchain;

/// The Go Gin fixture, resolved relative to this crate's manifest dir (mirrors the snapshot tests).
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// The NestJS (TypeScript) fixture — the determinism twin proves the tsextract sidecar path
/// (route recognition + transitive schema collection) is byte-identical across runs, exactly like
/// the Go + Python helper paths.
const NESTJS_FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/nestjs-bookstore"
);

/// The `FastAPI` (Python) fixture — the determinism twin proves the pyextract sidecar path is
/// byte-identical across runs, exactly like the Go helper path.
const FASTAPI_FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/fastapi-bookstore"
);

/// The Flask (Python) fixture — the determinism twin proves the Flask recognizer path (typed
/// envelope + diagnostics) is byte-identical across runs, exactly like the `FastAPI` path.
const FLASK_FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/flask-bookstore"
);

/// The fixture's security schemes — the single source of truth for security (CLAUDE.md rule 4): one
/// `ApiKeyAuth` / `X-API-Key` scheme. Security is no longer scraped from the source, so the contract
/// tests supply it here to drive lowering (graph-owned `SecurityScheme`s).
fn fixture_security() -> Vec<gnr8_core::graph::SecurityScheme> {
    vec![gnr8_core::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(),
        kind: "apiKey".to_string(),
        location: "header".to_string(),
        name: "X-API-Key".to_string(),
    }]
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
    let a = gnr8_core::gosdk::generate(&first, "goalservice", "/goal")
        .expect("first sdk::generate must succeed (requires gofmt)");
    let b = gnr8_core::gosdk::generate(&second, "goalservice", "/goal")
        .expect("second sdk::generate must succeed (requires gofmt)");

    assert_eq!(
        a, b,
        "two sdk::generate runs over unchanged source must be byte-identical (idempotent SDK gen)"
    );
}

#[test]
fn fastapi_build_graph_is_byte_identical_across_two_runs() {
    // Skip gracefully if the python3 toolchain is absent so the test never fails for a missing dep.
    let Ok(first) = gnr8_core::analyze::build_graph(FASTAPI_FIXTURE_DIR) else {
        eprintln!(
            "skipping FastAPI determinism test: python3 toolchain unavailable for {FASTAPI_FIXTURE_DIR}"
        );
        return;
    };
    let second = gnr8_core::analyze::build_graph(FASTAPI_FIXTURE_DIR)
        .expect("second FastAPI build_graph run must also succeed");

    let a = serde_json::to_string(&first).expect("serialize first FastAPI graph");
    let b = serde_json::to_string(&second).expect("serialize second FastAPI graph");

    assert_eq!(
        a, b,
        "two pyextract build_graph runs over unchanged source must serialize byte-identically (GRAPH-02)"
    );
}

#[test]
fn fastapi_to_openapi_is_byte_identical_across_two_runs() {
    // Skip gracefully if the python3 toolchain is absent.
    let Ok(first) = gnr8_core::analyze::build_graph(FASTAPI_FIXTURE_DIR) else {
        eprintln!(
            "skipping FastAPI OpenAPI determinism test: python3 toolchain unavailable for {FASTAPI_FIXTURE_DIR}"
        );
        return;
    };
    let second = gnr8_core::analyze::build_graph(FASTAPI_FIXTURE_DIR)
        .expect("second FastAPI build_graph run must also succeed");

    // Build twice AND lower twice — proving both the upstream graph and the reused lowering are
    // deterministic end-to-end for the Python path (idempotent OpenAPI generation).
    let security = fixture_security();
    let a = gnr8_core::lower::to_openapi(&first, "bookstore", "/books", &security)
        .expect("first FastAPI to_openapi must succeed");
    let b = gnr8_core::lower::to_openapi(&second, "bookstore", "/books", &security)
        .expect("second FastAPI to_openapi must succeed");

    assert_eq!(
        a, b,
        "two FastAPI to_openapi runs over unchanged source must be byte-identical (idempotent lowering)"
    );
}

#[test]
fn flask_build_graph_is_byte_identical_across_two_runs() {
    // Skip gracefully if the python3 toolchain is absent so the test never fails for a missing dep.
    let Ok(first) = gnr8_core::analyze::build_graph(FLASK_FIXTURE_DIR) else {
        eprintln!(
            "skipping Flask determinism test: python3 toolchain unavailable for {FLASK_FIXTURE_DIR}"
        );
        return;
    };
    let second = gnr8_core::analyze::build_graph(FLASK_FIXTURE_DIR)
        .expect("second Flask build_graph run must also succeed");

    let a = serde_json::to_string(&first).expect("serialize first Flask graph");
    let b = serde_json::to_string(&second).expect("serialize second Flask graph");

    assert_eq!(
        a, b,
        "two pyextract build_graph runs over unchanged Flask source must serialize byte-identically (GRAPH-02)"
    );
}

#[test]
fn flask_to_openapi_is_byte_identical_across_two_runs() {
    // Skip gracefully if the python3 toolchain is absent.
    let Ok(first) = gnr8_core::analyze::build_graph(FLASK_FIXTURE_DIR) else {
        eprintln!(
            "skipping Flask OpenAPI determinism test: python3 toolchain unavailable for {FLASK_FIXTURE_DIR}"
        );
        return;
    };
    let second = gnr8_core::analyze::build_graph(FLASK_FIXTURE_DIR)
        .expect("second Flask build_graph run must also succeed");

    // Build twice AND lower twice — proving both the upstream graph and the reused lowering are
    // deterministic end-to-end for the Flask path (idempotent OpenAPI generation).
    let security = fixture_security();
    let a = gnr8_core::lower::to_openapi(&first, "bookstore", "/orders", &security)
        .expect("first Flask to_openapi must succeed");
    let b = gnr8_core::lower::to_openapi(&second, "bookstore", "/orders", &security)
        .expect("second Flask to_openapi must succeed");

    assert_eq!(
        a, b,
        "two Flask to_openapi runs over unchanged source must be byte-identical (idempotent lowering)"
    );
}

#[test]
fn nestjs_build_graph_is_byte_identical_across_two_runs() {
    // Skip gracefully if the node/typescript toolchain is absent so the test never fails for a
    // missing dependency (mirrors the go/python skips above).
    if !nestjs_toolchain::available() {
        eprintln!(
            "skipping NestJS determinism test: node/typescript toolchain unavailable for {NESTJS_FIXTURE_DIR}"
        );
        return;
    }
    let first = gnr8_core::analyze::build_graph(NESTJS_FIXTURE_DIR)
        .expect("first NestJS build_graph run must succeed (requires node + vendored typescript)");
    let second = gnr8_core::analyze::build_graph(NESTJS_FIXTURE_DIR)
        .expect("second NestJS build_graph run must also succeed");

    let a = serde_json::to_string(&first).expect("serialize first NestJS graph");
    let b = serde_json::to_string(&second).expect("serialize second NestJS graph");

    assert_eq!(
        a, b,
        "two tsextract build_graph runs over unchanged source must serialize byte-identically (GRAPH-02)"
    );
}

#[test]
fn nestjs_to_openapi_is_byte_identical_across_two_runs() {
    // Skip gracefully if the node/typescript toolchain is absent.
    if !nestjs_toolchain::available() {
        eprintln!(
            "skipping NestJS OpenAPI determinism test: node/typescript toolchain unavailable for {NESTJS_FIXTURE_DIR}"
        );
        return;
    }
    let first = gnr8_core::analyze::build_graph(NESTJS_FIXTURE_DIR)
        .expect("first NestJS build_graph run must succeed (requires node + vendored typescript)");
    let second = gnr8_core::analyze::build_graph(NESTJS_FIXTURE_DIR)
        .expect("second NestJS build_graph run must also succeed");

    // Build twice AND lower twice — proving both the upstream graph and the reused lowering are
    // deterministic end-to-end for the TypeScript path (idempotent OpenAPI generation).
    let security = fixture_security();
    let a = gnr8_core::lower::to_openapi(&first, "bookstore", "/books", &security)
        .expect("first NestJS to_openapi must succeed");
    let b = gnr8_core::lower::to_openapi(&second, "bookstore", "/books", &security)
        .expect("second NestJS to_openapi must succeed");

    assert_eq!(
        a, b,
        "two NestJS to_openapi runs over unchanged source must be byte-identical (idempotent lowering)"
    );
}
