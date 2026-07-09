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
//! exercises a 4xx path — a `DeleteGoal` against a stub returning the declared 400 `HttpError` must
//! surface a `*APIError` with a typed `Body` (SDK-04 typed error). A `go build`/`go test` non-zero exit
//! maps to a captured
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

fn query_api_key_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "github.com/acme/svc",
          "operations": [
            {
              "id": "listItems",
              "method": "GET",
              "path": "/items",
              "handler": "listItems",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "http.go", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": [
            {
              "id": "QueryAuth",
              "kind": "apiKey",
              "location": "query",
              "name": "api_key"
            }
          ]
        }"#,
    )
    .expect("query api-key graph json")
}

fn http_auth_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "github.com/acme/svc",
          "operations": [
            {
              "id": "getBearer",
              "method": "GET",
              "path": "/bearer",
              "handler": "getBearer",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "security": ["BearerAuth"],
              "security_overrides_global": true,
              "provenance": { "file": "http.go", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "getBasic",
              "method": "GET",
              "path": "/basic",
              "handler": "getBasic",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "security": ["BasicAuth"],
              "security_overrides_global": true,
              "provenance": { "file": "http.go", "start_line": 2, "end_line": 2 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": [
            {
              "id": "BearerAuth",
              "kind": "http",
              "location": "",
              "name": "bearer",
              "global": false
            },
            {
              "id": "BasicAuth",
              "kind": "http",
              "location": "",
              "name": "basic",
              "global": false
            }
          ]
        }"#,
    )
    .expect("http auth graph json")
}

fn runtime_graph() -> gnr8::graph::ApiGraph {
    let mut graph: gnr8::graph::ApiGraph = serde_json::from_str(
        r#"{
          "module": "github.com/acme/svc",
          "operations": [
            {
              "id": "listItems",
              "method": "GET",
              "path": "/items",
              "handler": "listItems",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "http.go", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "createUnsafe",
              "method": "POST",
              "path": "/unsafe",
              "handler": "createUnsafe",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "http.go", "start_line": 2, "end_line": 2 }
            },
            {
              "id": "createIdempotent",
              "method": "POST",
              "path": "/idempotent",
              "handler": "createIdempotent",
              "params": [],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "http.go", "start_line": 3, "end_line": 3 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("runtime graph json");
    graph.runtime = gnr8::graph::RuntimePolicy {
        default_timeout_ms: Some(5_000),
        max_retries: 0,
        retry_statuses: vec![408, 429],
        retry_unsafe_methods: false,
        hooks: Vec::new(),
    };
    graph.operation_runtime = vec![gnr8::graph::OperationRuntimePolicy {
        operation_id: "createIdempotent".to_string(),
        idempotent: true,
        idempotency_key_header: Some("Idempotency-Key".to_string()),
    }];
    graph
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

const HTTPTEST_SMOKE_TEMPLATE: &str = r#"package __PKG__

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
func TestCreateGoalSmoke(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/goal/" {
			t.Errorf("path = %s, want /goal/", r.URL.Path)
		}
		body, _ := io.ReadAll(r.Body)
		if !strings.Contains(string(body), "\"name\":\"my-goal\"") {
			t.Errorf("request body = %s, want it to contain name=my-goal", string(body))
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_ = json.NewEncoder(w).Encode(CommandMessageWithUUID{Message: "ok", UUID: "goal-123"})
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	out, err := c.CreateGoal(context.Background(), CreateGoalInput{Name: "my-goal"})
	if err != nil {
		t.Fatalf("CreateGoal returned error: %v", err)
	}
	if out.UUID != "goal-123" {
		t.Fatalf("out.UUID = %q, want goal-123", out.UUID)
	}
	if out.Message != "ok" {
		t.Fatalf("out.Message = %q, want ok", out.Message)
	}
}

// SDK-04: a declared 400 with an HttpError body must surface a *APIError with a typed Body.
func TestDeleteGoalBadRequestAPIError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodDelete {
			t.Errorf("method = %s, want DELETE", r.Method)
		}
		if r.URL.Path != "/goal/missing-uuid" {
			t.Errorf("path = %s, want /goal/missing-uuid", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Request-ID", "req-400")
		w.WriteHeader(http.StatusBadRequest)
		_ = json.NewEncoder(w).Encode(HttpError{Message: "bad request", Slug: "bad_request"})
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	_, err := c.DeleteGoal(context.Background(), "missing-uuid")
	if err == nil {
		t.Fatalf("DeleteGoal on a 400 must return an error")
	}
	apiErr, ok := err.(*APIError)
	if !ok {
		t.Fatalf("error type = %T, want *APIError", err)
	}
	if apiErr.StatusCode != 400 {
		t.Fatalf("StatusCode = %d, want 400", apiErr.StatusCode)
	}
	if apiErr.IsNotFound() {
		t.Fatalf("IsNotFound() = true, want false for a 400")
	}
	if apiErr.RequestID != "req-400" {
		t.Fatalf("RequestID = %q, want req-400", apiErr.RequestID)
	}
	if got := apiErr.Headers.Get("X-Request-ID"); got != "req-400" {
		t.Fatalf("Headers.Get(X-Request-ID) = %q, want req-400", got)
	}
	if !strings.Contains(string(apiErr.RawBody), "bad_request") {
		t.Fatalf("RawBody = %s, want bad_request", string(apiErr.RawBody))
	}
	if apiErr.JSONBody == nil {
		t.Fatalf("JSONBody = nil, want parsed JSON")
	}
	typed, ok := apiErr.Body.(HttpError)
	if !ok {
		t.Fatalf("Body type = %T, want HttpError", apiErr.Body)
	}
	if typed.Slug != "bad_request" {
		t.Fatalf("Body.Slug = %q, want bad_request", typed.Slug)
	}
	if apiErr.Message != "bad request" {
		t.Fatalf("Message = %q, want bad request", apiErr.Message)
	}
	if apiErr.Slug != "bad_request" {
		t.Fatalf("Slug = %q, want bad_request", apiErr.Slug)
	}
}
"#;

/// SDK-05 + SDK-04: a fixed httptest smoke test constructs the Client, calls `CreateGoal` (POST /goal/)
/// asserting method/path/body + the decoded response, and exercises a declared 4xx `DeleteGoal` path
/// that must surface a `*APIError` with a typed error body. `go test ./...` must pass.
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
    let smoke = HTTPTEST_SMOKE_TEMPLATE.replace("__PKG__", &pkg);
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

#[test]
fn generated_sdk_sends_query_api_key() {
    if !go_available() {
        eprintln!("skipping sdk_compile query auth smoke: go toolchain unavailable");
        return;
    }
    let graph = query_api_key_graph();
    let dir = materialize_sdk_from_graph("query-api-key", &graph, "/api");
    let pkg = package_clause(&dir);
    let smoke = format!(
        r#"package {pkg}

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestQueryAPIKeyIsSent(t *testing.T) {{
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		if r.Method != http.MethodGet {{
			t.Errorf("method = %s, want GET", r.Method)
		}}
		if r.URL.Path != "/api/items" {{
			t.Errorf("path = %s, want /api/items", r.URL.Path)
		}}
		if got := r.URL.Query().Get("api_key"); got != "secret" {{
			t.Errorf("api_key query = %q, want secret", got)
		}}
		w.WriteHeader(http.StatusNoContent)
	}}))
	defer srv.Close()

	c := NewClient(srv.URL, WithAPIKey("secret"))
	if _, err := c.ListItems(context.Background()); err != nil {{
		t.Fatalf("ListItems returned error: %v", err)
	}}
}}
"#
    );
    std::fs::write(dir.join("query_auth_test.go"), smoke).expect("write query_auth_test.go");

    let test = run_go(&["test", "./..."], &dir);
    assert!(
        test.is_ok(),
        "go test ./... (query auth smoke) must pass: {test:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
fn generated_sdk_sends_bearer_and_basic_auth() {
    if !go_available() {
        eprintln!("skipping sdk_compile http auth smoke: go toolchain unavailable");
        return;
    }
    let graph = http_auth_graph();
    let dir = materialize_sdk_from_graph("http-auth", &graph, "/api");
    let pkg = package_clause(&dir);
    let smoke = format!(
        r#"package {pkg}

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestBearerAndBasicAuthAreSent(t *testing.T) {{
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		switch r.URL.Path {{
		case "/api/bearer":
			if got := r.Header.Get("Authorization"); got != "Bearer secret-token" {{
				t.Errorf("bearer Authorization = %q, want Bearer secret-token", got)
			}}
		case "/api/basic":
			username, password, ok := r.BasicAuth()
			if !ok || username != "user" || password != "pass" {{
				t.Errorf("basic auth = (%q, %q, %v), want (user, pass, true)", username, password, ok)
			}}
		default:
			t.Errorf("path = %s, want /api/bearer or /api/basic", r.URL.Path)
		}}
		w.WriteHeader(http.StatusNoContent)
	}}))
	defer srv.Close()

	c := NewClient(srv.URL, WithBearerToken("secret-token"), WithBasicAuth("user", "pass"))
	if _, err := c.GetBearer(context.Background()); err != nil {{
		t.Fatalf("GetBearer returned error: %v", err)
	}}
	if _, err := c.GetBasic(context.Background()); err != nil {{
		t.Fatalf("GetBasic returned error: %v", err)
	}}
}}
"#
    );
    std::fs::write(dir.join("http_auth_test.go"), smoke).expect("write http_auth_test.go");

    let test = run_go(&["test", "./..."], &dir);
    assert!(
        test.is_ok(),
        "go test ./... (http auth smoke) must pass: {test:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the test writes one fixed generated-SDK Go program so failures show the exact smoke source"
)]
fn generated_sdk_runtime_retries_idempotency_and_hooks_work_against_httptest() {
    if !go_available() {
        eprintln!("skipping sdk_compile runtime smoke: go toolchain unavailable");
        return;
    }
    let graph = runtime_graph();
    let dir = materialize_sdk_from_graph("runtime", &graph, "/api");
    let pkg = package_clause(&dir);
    let smoke = format!(
        r#"package {pkg}

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

type runtimeEvent struct {{
	Kind string
	OperationID string
	Method string
	PathTemplate string
	Trace string
	StatusCode int
}}

func hasRuntimeEvent(events []runtimeEvent, want runtimeEvent) bool {{
	for _, event := range events {{
		if event == want {{
			return true
		}}
	}}
	return false
}}

func TestRuntimeRetriesIdempotencyAndHooks(t *testing.T) {{
	counts := map[string]int{{}}
	idempotencyKeys := []string{{}}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		key := r.Method + " " + r.URL.Path
		counts[key]++
		switch r.URL.Path {{
		case "/api/items":
			if counts[key] == 1 {{
				w.WriteHeader(http.StatusInternalServerError)
				return
			}}
			w.WriteHeader(http.StatusNoContent)
		case "/api/unsafe":
			w.WriteHeader(http.StatusInternalServerError)
		case "/api/idempotent":
			idempotencyKeys = append(idempotencyKeys, r.Header.Get("Idempotency-Key"))
			if counts[key] == 1 {{
				w.WriteHeader(http.StatusInternalServerError)
				return
			}}
			w.WriteHeader(http.StatusNoContent)
		default:
			t.Errorf("unexpected path %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}}
	}}))
	defer srv.Close()

	events := []runtimeEvent{{}}
	c := NewClient(
		srv.URL,
		WithTimeout(5*time.Second),
		WithMaxRetries(0),
		WithRequestHook(func(_ context.Context, ctx RequestContext, _ *http.Request) error {{
			events = append(events, runtimeEvent{{
				Kind: "request",
				OperationID: ctx.OperationID,
				Method: ctx.Method,
				PathTemplate: ctx.PathTemplate,
				Trace: ctx.RequestMetadata["trace"],
			}})
			return nil
		}}),
		WithResponseHook(func(_ context.Context, ctx RequestContext, _ *http.Response) error {{
			events = append(events, runtimeEvent{{
				Kind: "response",
				OperationID: ctx.OperationID,
				StatusCode: ctx.StatusCode,
			}})
			return nil
		}}),
		WithErrorHook(func(_ context.Context, ctx RequestContext, _ error) {{
			events = append(events, runtimeEvent{{
				Kind: "error",
				OperationID: ctx.OperationID,
				StatusCode: ctx.StatusCode,
			}})
		}}),
	)

	_, err := c.ListItems(
		context.Background(),
		WithRequestMaxRetries(1),
		WithRequestTimeout(5*time.Second),
		WithRequestMetadata(map[string]string{{"trace": "runtime"}}),
	)
	if err != nil {{
		t.Fatalf("ListItems returned error: %v", err)
	}}
	if counts["GET /api/items"] != 2 {{
		t.Fatalf("GET /api/items count = %d, want 2", counts["GET /api/items"])
	}}

	_, err = c.CreateUnsafe(context.Background(), WithRequestMaxRetries(1))
	if err == nil {{
		t.Fatalf("CreateUnsafe must return an APIError")
	}}
	apiErr, ok := err.(*APIError)
	if !ok || apiErr.StatusCode != http.StatusInternalServerError {{
		t.Fatalf("CreateUnsafe error = %#v, want *APIError status 500", err)
	}}
	if counts["POST /api/unsafe"] != 1 {{
		t.Fatalf("POST /api/unsafe count = %d, want 1", counts["POST /api/unsafe"])
	}}

	_, err = c.CreateIdempotent(
		context.Background(),
		WithRequestMaxRetries(1),
		WithIdempotencyKey("idem-1"),
	)
	if err != nil {{
		t.Fatalf("CreateIdempotent returned error: %v", err)
	}}
	if counts["POST /api/idempotent"] != 2 {{
		t.Fatalf("POST /api/idempotent count = %d, want 2", counts["POST /api/idempotent"])
	}}
	if len(idempotencyKeys) != 2 || idempotencyKeys[0] != "idem-1" || idempotencyKeys[1] != "idem-1" {{
		t.Fatalf("idempotency keys = %#v, want two idem-1 values", idempotencyKeys)
	}}

	if !hasRuntimeEvent(events, runtimeEvent{{Kind: "request", OperationID: "listItems", Method: "GET", PathTemplate: "/items", Trace: "runtime"}}) {{
		t.Fatalf("missing listItems request event in %#v", events)
	}}
	if !hasRuntimeEvent(events, runtimeEvent{{Kind: "response", OperationID: "listItems", StatusCode: 500}}) {{
		t.Fatalf("missing listItems 500 response event in %#v", events)
	}}
	if !hasRuntimeEvent(events, runtimeEvent{{Kind: "response", OperationID: "listItems", StatusCode: 204}}) {{
		t.Fatalf("missing listItems 204 response event in %#v", events)
	}}
	if !hasRuntimeEvent(events, runtimeEvent{{Kind: "error", OperationID: "createUnsafe", StatusCode: 500}}) {{
		t.Fatalf("missing createUnsafe error event in %#v", events)
	}}
}}
"#
    );
    std::fs::write(dir.join("runtime_test.go"), smoke).expect("write runtime_test.go");

    let test = run_go(&["test", "./..."], &dir);
    assert!(
        test.is_ok(),
        "go test ./... (runtime smoke) must pass: {test:?}"
    );

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
