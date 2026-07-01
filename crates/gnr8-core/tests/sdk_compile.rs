//! SDK-05 compile + smoke gate: the generated Go SDK genuinely `go build`s and answers a real HTTP
//! round-trip (the phase's hardest acceptance bar — a string snapshot can look correct yet not compile,
//! RESEARCH Pitfall 3).
//!
//! The test (1) builds the graph from the goalservice fixture, (2) generates the SDK via `gosdk::generate`
//! and materializes it through `gosdk::write_to_dir` into a UNIQUE temp subdir under
//! `std::env::temp_dir()` (the zero-dependency `std` path — no `tempfile` crate, threat T-03-03-SC),
//! (3) writes a generated `go.mod` with `module gnr8sdktest` + `go 1.26` and ZERO `require`s so the
//! build is hermetic and never reaches the module proxy (RESEARCH Pitfall 5 — GOPROXY=off-safe), then
//! (4) runs `go build ./...` AND a fixed `httptest`-based `smoke_test.go` via `go test ./...`.
//!
//! The smoke test constructs the `Client` via `NewClient(srv.URL)`, calls `CreateGoal` (POST `/goal/`)
//! and asserts method/path/body + the decoded `CommandMessageWithUUID.UUID` (SDK-05 exercised), and
//! exercises a 4xx path — a `DeleteGoal` against a stub returning 404 must surface a `*APIError` with
//! `StatusCode == 404` (SDK-04 typed error). A `go build`/`go test` non-zero exit maps to a captured
//! stderr failure (or `CoreError::GoBuild` in the harness helper), never a panic (threat T-03-03-04).
//!
//! Requires the Go toolchain (present on dev + CI, go 1.26); skips gracefully (early return) if it is
//! absent so a non-Go environment never hard-fails the suite (mirrors `tests/determinism.rs`).

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The Go Gin fixture, resolved relative to this crate's manifest dir (mirrors the other tests).
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// Whether the `go` toolchain is available so this test skips gracefully if it is absent.
fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Create a UNIQUE temp subdir under `std::env::temp_dir()` (PID + nanosecond timestamp — no
/// user-supplied path component, threat T-03-03-03). No `tempfile` crate (T-03-03-SC).
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-sdk-compile-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// Run `go <args>` in `dir`, mapping a non-zero exit to `CoreError::GoBuild` (never a panic — the
/// harness uses NO `unwrap`/`expect` on the subprocess `Result`, threat T-03-03-04). A spawn failure
/// (missing toolchain) maps to `CoreError::GoToolchainMissing`.
fn run_go(args: &[&str], dir: &Path) -> Result<String, gnr8::CoreError> {
    let output = Command::new("go")
        // Discrete args + `current_dir` — never a shell string (threat T-03-03-01).
        .args(args)
        .current_dir(dir)
        // Hermetic: zero-require go.mod means nothing is fetched; force the proxy off as belt-and-braces
        // so a stray import can never silently reach the network in CI (RESEARCH Pitfall 5).
        .env("GOPROXY", "off")
        .env("GOFLAGS", "-mod=mod")
        .output()
        .map_err(|source| gnr8::CoreError::GoToolchainMissing { source })?;
    if !output.status.success() {
        return Err(gnr8::CoreError::GoBuild {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The package clause from a written SDK file is the source of truth for the smoke test's package
/// (the generated SDK package is `goalservice` for this fixture, but read it rather than hardcode it).
fn package_clause(dir: &Path) -> String {
    let models = std::fs::read_to_string(dir.join("models.go")).expect("read models.go");
    for line in models.lines() {
        if let Some(pkg) = line.trim().strip_prefix("package ") {
            return pkg.trim().to_string();
        }
    }
    panic!("no package clause found in generated models.go:\n{models}");
}

/// Write a hermetic, stdlib-only `go.mod`: `module gnr8sdktest` + `go 1.26`, ZERO `require`s
/// (RESEARCH Pitfall 5). No `go.sum` is needed because nothing is fetched.
fn write_go_mod(dir: &Path) {
    std::fs::write(dir.join("go.mod"), "module gnr8sdktest\n\ngo 1.26\n").expect("write go.mod");
}

/// Materialize the generated SDK + a hermetic go.mod into a fresh temp dir, returning the dir.
fn materialize_sdk_from_graph(
    label: &str,
    graph: &gnr8::graph::ApiGraph,
    base_path: &str,
) -> PathBuf {
    let bundle = gnr8::gosdk::generate(graph, "goalservice", base_path)
        .expect("sdk::generate must succeed (requires gofmt)");
    let dir = unique_temp_dir(label);
    gnr8::sdk::bundle::write_to_dir(&bundle, &dir)
        .expect("write_to_dir must materialize the SDK files");
    write_go_mod(&dir);
    dir
}

/// Materialize the generated SDK + a hermetic go.mod into a fresh temp dir, returning the dir.
fn materialize_sdk() -> PathBuf {
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 build_graph must succeed (requires the Go toolchain)");
    materialize_sdk_from_graph("ok", &graph, "/goal")
}

fn optional_body_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "github.com/acme/svc",
          "operations": [
            {
              "id": "markRead",
              "method": "PATCH",
              "path": "/read",
              "handler": "markRead",
              "params": [],
              "request_body": { "ref_id": "dto.MarkReadRequest" },
              "request_body_required": false,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.MarkReadRequest",
              "name": "MarkReadRequest",
              "body": { "type": "object", "of": [
                {
                  "json_name": "lastId",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("optional body graph json")
}

/// SDK-05: the generated SDK materializes to a hermetic stdlib-only temp module and `go build ./...`
/// exits 0 (it genuinely compiles).
#[test]
fn generated_sdk_go_builds_clean() {
    if !go_available() {
        eprintln!("skipping sdk_compile: go toolchain unavailable");
        return;
    }
    let dir = materialize_sdk();

    // The four production SDK files plus the hermetic go.mod exist; smoke_test.go is added below. The
    // operations file is the generic `operations.go` — there are no per-tag files since tags were a
    // doc-comment-annotation fact and have been removed (CLAUDE.md rules 1 & 3).
    for name in [
        "client.go",
        "errors.go",
        "operations.go",
        "models.go",
        "go.mod",
    ] {
        assert!(
            dir.join(name).exists(),
            "expected {name} in {}",
            dir.display()
        );
    }

    let build = run_go(&["build", "./..."], &dir);
    assert!(build.is_ok(), "go build ./... must succeed: {build:?}");

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// SDK-05 + SDK-04: a fixed httptest smoke test constructs the Client, calls `CreateGoal` (POST /goal/)
/// asserting method/path/body + the decoded response, and exercises a 4xx `DeleteGoal` path that must
/// surface a `*APIError` with `StatusCode` == 404. `go test ./...` must pass.
#[test]
fn generated_sdk_passes_httptest_smoke() {
    if !go_available() {
        eprintln!("skipping sdk_compile smoke: go toolchain unavailable");
        return;
    }
    let dir = materialize_sdk();
    let pkg = package_clause(&dir);

    // A FIXED smoke *_test.go written by the harness (NOT part of the snapshot-ed SDK bundle — the
    // bundle stays production-SDK-only, RESEARCH Open Q2 recommendation b). It shares the SDK's package
    // (read from the written files) so it can call unexported helpers and the package types directly.
    let smoke = format!(
        r#"package {pkg}

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// SDK-05: CreateGoal sends POST /goal/ with the marshaled body and decodes the 201 response.
func TestCreateGoalSmoke(t *testing.T) {{
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		if r.Method != http.MethodPost {{
			t.Errorf("method = %s, want POST", r.Method)
		}}
		if r.URL.Path != "/goal/" {{
			t.Errorf("path = %s, want /goal/", r.URL.Path)
		}}
		body, _ := io.ReadAll(r.Body)
		if !strings.Contains(string(body), "\"name\":\"my-goal\"") {{
			t.Errorf("request body = %s, want it to contain name=my-goal", string(body))
		}}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_ = json.NewEncoder(w).Encode(CommandMessageWithUUID{{Message: "ok", UUID: "goal-123"}})
	}}))
	defer srv.Close()

	c := NewClient(srv.URL)
	out, err := c.CreateGoal(context.Background(), CreateGoalInput{{Name: "my-goal"}})
	if err != nil {{
		t.Fatalf("CreateGoal returned error: %v", err)
	}}
	if out.UUID != "goal-123" {{
		t.Fatalf("out.UUID = %q, want goal-123", out.UUID)
	}}
	if out.Message != "ok" {{
		t.Fatalf("out.Message = %q, want ok", out.Message)
	}}
}}

// SDK-04: a 404 with an HttpError body must surface a *APIError carrying StatusCode == 404.
func TestDeleteGoalNotFoundAPIError(t *testing.T) {{
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		if r.Method != http.MethodDelete {{
			t.Errorf("method = %s, want DELETE", r.Method)
		}}
		if r.URL.Path != "/goal/missing-uuid" {{
			t.Errorf("path = %s, want /goal/missing-uuid", r.URL.Path)
		}}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		_ = json.NewEncoder(w).Encode(HttpError{{Message: "not found", Slug: "goal_not_found"}})
	}}))
	defer srv.Close()

	c := NewClient(srv.URL)
	_, err := c.DeleteGoal(context.Background(), "missing-uuid")
	if err == nil {{
		t.Fatalf("DeleteGoal on a 404 must return an error")
	}}
	apiErr, ok := err.(*APIError)
	if !ok {{
		t.Fatalf("error type = %T, want *APIError", err)
	}}
	if apiErr.StatusCode != 404 {{
		t.Fatalf("StatusCode = %d, want 404", apiErr.StatusCode)
	}}
	if !apiErr.IsNotFound() {{
		t.Fatalf("IsNotFound() = false, want true for a 404")
	}}
}}
"#
    );
    std::fs::write(dir.join("smoke_test.go"), smoke).expect("write smoke_test.go");

    let test = run_go(&["test", "./..."], &dir);
    assert!(
        test.is_ok(),
        "go test ./... (httptest smoke) must pass: {test:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
fn generated_sdk_optional_body_nil_sends_no_body() {
    if !go_available() {
        eprintln!("skipping sdk_compile optional body smoke: go toolchain unavailable");
        return;
    }
    let graph = optional_body_graph();
    let dir = materialize_sdk_from_graph("optional-body", &graph, "/api");
    let pkg = package_clause(&dir);
    let smoke = format!(
        r#"package {pkg}

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestOptionalBodyNilDoesNotSendJSON(t *testing.T) {{
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		if r.Method != http.MethodPatch {{
			t.Errorf("method = %s, want PATCH", r.Method)
		}}
		if r.URL.Path != "/api/read" {{
			t.Errorf("path = %s, want /api/read", r.URL.Path)
		}}
		body, _ := io.ReadAll(r.Body)
		if len(body) != 0 {{
			t.Errorf("body = %q, want empty", string(body))
		}}
		if got := r.Header.Get("Content-Type"); got != "" {{
			t.Errorf("Content-Type = %q, want empty", got)
		}}
		w.WriteHeader(http.StatusNoContent)
	}}))
	defer srv.Close()

	c := NewClient(srv.URL)
	if _, err := c.MarkRead(context.Background(), nil); err != nil {{
		t.Fatalf("MarkRead nil body returned error: %v", err)
	}}
}}
"#
    );
    std::fs::write(dir.join("optional_body_test.go"), smoke).expect("write optional smoke");
    let test = run_go(&["test", "./..."], &dir);
    assert!(test.is_ok(), "go test ./... must succeed: {test:?}");

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// Threat T-03-03-04 / RUST-04: a `go build` of invalid Go surfaces `CoreError::GoBuild` (carrying the
/// captured stderr), never a panic in the harness helper.
#[test]
fn invalid_go_build_maps_to_go_build_error_not_panic() {
    if !go_available() {
        eprintln!("skipping sdk_compile error-path: go toolchain unavailable");
        return;
    }
    let dir = unique_temp_dir("bad");
    write_go_mod(&dir);
    // Deliberately invalid Go — `go build` must exit non-zero.
    std::fs::write(dir.join("broken.go"), "package gnr8sdktest\n\nfunc {\n")
        .expect("write broken.go");

    let result = run_go(&["build", "./..."], &dir);
    match result {
        Err(gnr8::CoreError::GoBuild { code, stderr }) => {
            assert!(
                code != Some(0),
                "a failed build must not report exit code 0"
            );
            assert!(!stderr.is_empty(), "GoBuild must carry the captured stderr");
        }
        other => panic!("expected CoreError::GoBuild, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}
