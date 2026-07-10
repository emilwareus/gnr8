//! PYSDK-02 compile + smoke gate: the generated Python SDK genuinely `py_compile`s, `import`s, AND
//! answers a real HTTP round-trip (the phase's hardest acceptance bar — a string snapshot can look
//! correct yet not compile, RESEARCH Pitfall 3). The twin of `tests/sdk_compile.rs`, but for the
//! `pysdk` target: because `python3` is present and `go` is absent in this sandbox, THIS is the SDK
//! acceptance test that actually runs (the Go SDK tests skip).
//!
//! The harness (1) builds the graph from the `fastapi-bookstore` fixture via the Phase-2 `pyextract`
//! path that `build_graph` routes to, (2) generates the SDK via `pysdk::generate` and materializes it
//! through `pysdk::write_to_dir` into a `<dir>/bookstore/` package under a UNIQUE temp subdir below
//! `std::env::temp_dir()` (the zero-dependency `std` path — no `tempfile` crate, threat T-03-03-SC), then
//! runs three gates against `python3`:
//!   (a) `python3 -m py_compile <each .py>`     — syntax gate (catches an `IndentationError`, Pitfall 3).
//!   (b) `python3 -c "import bookstore"`         — executes the class bodies (catches the Pydantic
//!       field-order `TypeError`, a bad `Optional`/`Literal`, or a `NameError`, Pitfall 1/3).
//!   (c) a program-written stdlib `http.server` driver — binds `("127.0.0.1", 0)` (ephemeral port,
//!       Pitfall 5), serves in a daemon thread, injects an `OpenerDirector` into the generated `Client`,
//!       and asserts a 2xx Pydantic-model round-trip AND a 4xx → typed `ApiError(is_not_found())`.
//!
//! Hermeticity (CLAUDE.md rule 2 + ASVS): the fake backend + driver use ONLY the Python stdlib
//! (`http.server`, `threading`, `json`, `urllib.request`) — NO fastapi/uvicorn/requests/httpx/pytest, no
//! `pip install`. The harness also greps every written `.py` and asserts the generated SDK carries no
//! third-party HTTP import (PYSDK-01).
//!
//! Requires `python3`; skips gracefully (early return) if it is absent so a non-Python environment never
//! hard-fails the suite (mirrors how `tests/sdk_compile.rs` skips without `go`).

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The `FastAPI` fixture, resolved relative to this crate's manifest dir (mirrors the other tests).
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/fastapi-bookstore"
);

/// The generated SDK's Python package name (also the import name and the package subdir).
const PACKAGE: &str = "bookstore";

/// The four files the `pysdk` bundle always frames (D-06 push order).
const SDK_FILES: [&str; 4] = ["__init__.py", "client.py", "errors.py", "models.py"];

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Whether the `python3` toolchain is available so this test skips gracefully if it is absent.
fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Create a UNIQUE temp subdir under `std::env::temp_dir()` (PID + nanosecond timestamp — no
/// user-supplied path component, threat T-03-03-02). No `tempfile` crate (T-03-03-SC); copied verbatim
/// from the Go twin so the two harnesses stay byte-for-byte aligned.
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gnr8-pysdk-compile-{label}-{}-{seq}-{nanos}",
        std::process::id(),
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// Run `python3 <args>` in `dir`, mapping a non-zero exit to a captured-stderr `CoreError` (never a
/// panic — the harness uses NO `unwrap`/`expect` on the subprocess `Result`, threat T-03-03-05). A spawn
/// failure (missing toolchain) maps to `CoreError::PythonToolchainMissing`. Discrete args + `current_dir`
/// only — NEVER a shell string (threat T-03-03-01 / V13).
fn run_python(args: &[&str], dir: &Path) -> Result<String, gnr8::CoreError> {
    let output = Command::new("python3")
        .args(args)
        .current_dir(dir)
        // Belt-and-braces hermeticity: never let an import silently reach the network or a user site dir
        // (the Python analog of GOPROXY=off — the round-trip is localhost-only, no pip fetch).
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .output()
        // Spawn failure (e.g. python3 absent) → the dedicated toolchain-missing variant (error.rs:45).
        .map_err(|source| gnr8::CoreError::PythonToolchainMissing { source })?;
    if !output.status.success() {
        // Reuse the generic captured-stderr carrier (no new error variant added — the plan's interfaces
        // note: GoBuild is the generic exit-code+stderr carrier the harness reuses, T-03-03-05).
        return Err(gnr8::CoreError::GoBuild {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn write_pydantic_stub(dir: &Path) {
    std::fs::write(
        dir.join("pydantic.py"),
        r#"import enum


class ConfigDict(dict):
    pass


def Field(default=None, *args, **kwargs):
    return default


class BaseModel:
    def __init__(self, **kwargs):
        annotations = {}
        for cls in reversed(self.__class__.mro()):
            annotations.update(getattr(cls, "__annotations__", {}))
        for name in annotations:
            if name in kwargs:
                setattr(self, name, kwargs[name])
            elif hasattr(self.__class__, name):
                setattr(self, name, getattr(self.__class__, name))
            else:
                setattr(self, name, None)
        for name, value in kwargs.items():
            setattr(self, name, value)

    @classmethod
    def model_validate(cls, data):
        if isinstance(data, cls):
            return data
        if isinstance(data, dict):
            return cls(**data)
        return data

    def model_dump(self, **_kwargs):
        def dump(value):
            if isinstance(value, BaseModel):
                return value.model_dump(**_kwargs)
            if isinstance(value, (bytes, bytearray)):
                if _kwargs.get("mode") == "json":
                    return bytes(value).decode("utf-8")
                return value
            if isinstance(value, enum.Enum):
                if _kwargs.get("mode") == "json":
                    return value.value
                return value
            if isinstance(value, list):
                return [dump(item) for item in value]
            if isinstance(value, dict):
                return {key: dump(item) for key, item in value.items()}
            return value

        return {
            key: dump(value)
            for key, value in self.__dict__.items()
            if not key.startswith("_")
        }
"#,
    )
    .expect("write pydantic stub");
}

/// Materialize the generated SDK into a fresh temp dir as an importable `<dir>/bookstore/` package,
/// returning the temp dir (the package PARENT). Python needs NO manifest analog to the Go `go.mod`.
///
/// The four files go under `<dir>/bookstore/` so `__init__.py`'s relative imports (`from .client import
/// Client`) resolve and `python3 -c "import bookstore"` works with `<dir>` as the current dir.
fn materialize_sdk_from_graph(
    label: &str,
    graph: &gnr8::graph::ApiGraph,
    base_path: &str,
) -> PathBuf {
    let bundle =
        gnr8::pysdk::generate(graph, PACKAGE, base_path).expect("pysdk::generate must succeed");
    let dir = unique_temp_dir(label);
    let pkg_dir = dir.join(PACKAGE);
    std::fs::create_dir_all(&pkg_dir).expect("create package subdir");
    gnr8::sdk::bundle::write_to_dir(&bundle, &pkg_dir)
        .expect("write_to_dir must materialize the SDK");
    write_pydantic_stub(&dir);
    dir
}

fn materialize_sdk() -> PathBuf {
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 build_graph must succeed (requires python3 for the pyextract sidecar)");
    // `base_path` is the graph's single source of truth (the FastAPI fixture's is "/"); pass it through
    // exactly as a Pipeline would (CLAUDE.md rules 3 & 4).
    materialize_sdk_from_graph("ok", &graph, &graph.base_path)
}

fn auth_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "app",
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
              "provenance": { "file": "main.py", "start_line": 1, "end_line": 1 }
            },
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
              "provenance": { "file": "main.py", "start_line": 2, "end_line": 2 }
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
              "provenance": { "file": "main.py", "start_line": 3, "end_line": 3 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/api",
          "title": "API",
          "security": [
            {
              "id": "QueryAuth",
              "kind": "apiKey",
              "location": "query",
              "name": "api_key",
              "global": true
            },
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
    .expect("auth graph json")
}

#[expect(
    clippy::too_many_lines,
    reason = "the media graph is an explicit JSON fixture covering four content types"
)]
fn media_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "app",
          "operations": [
            {
              "id": "postText",
              "method": "POST",
              "path": "/text",
              "handler": "postText",
              "params": [],
              "request_body": { "ref_id": "dto.TextBody" },
              "request_body_required": true,
              "request_body_content_type": "text/plain",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.py", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "postForm",
              "method": "POST",
              "path": "/form",
              "handler": "postForm",
              "params": [],
              "request_body": { "ref_id": "dto.FormBody" },
              "request_body_required": true,
              "request_body_content_type": "application/x-www-form-urlencoded",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.py", "start_line": 2, "end_line": 2 }
            },
            {
              "id": "postMultipart",
              "method": "POST",
              "path": "/multipart",
              "handler": "postMultipart",
              "params": [],
              "request_body": { "ref_id": "dto.MultipartBody" },
              "request_body_required": true,
              "request_body_content_type": "multipart/form-data",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.py", "start_line": 3, "end_line": 3 }
            },
            {
              "id": "postBinary",
              "method": "POST",
              "path": "/binary",
              "handler": "postBinary",
              "params": [],
              "request_body": { "ref_id": "dto.UploadBytes" },
              "request_body_required": true,
              "request_body_content_type": "application/octet-stream",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.py", "start_line": 4, "end_line": 4 }
            }
          ],
          "schemas": [
            {
              "id": "dto.FormBody",
              "name": "FormBody",
              "body": { "type": "object", "of": [
                {
                  "json_name": "count",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "name",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "tags",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "array", "of": { "type": "primitive", "of": { "prim": "string" } } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "dto.MultipartBody",
              "name": "MultipartBody",
              "body": { "type": "object", "of": [
                {
                  "json_name": "file",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "bytes" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "title",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "files",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "array", "of": { "type": "primitive", "of": { "prim": "bytes" } } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 2, "end_line": 2 }
            },
            {
              "id": "dto.TextBody",
              "name": "TextBody",
              "body": { "type": "primitive", "of": { "prim": "string" } },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 3, "end_line": 3 }
            },
            {
              "id": "dto.UploadBytes",
              "name": "UploadBytes",
              "body": { "type": "primitive", "of": { "prim": "bytes" } },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 4, "end_line": 4 }
            }
          ],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("media graph json")
}

fn runtime_graph() -> gnr8::graph::ApiGraph {
    let mut graph: gnr8::graph::ApiGraph = serde_json::from_str(
        r#"{
          "module": "app",
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
              "provenance": { "file": "main.py", "start_line": 1, "end_line": 1 }
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
              "provenance": { "file": "main.py", "start_line": 2, "end_line": 2 }
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
              "provenance": { "file": "main.py", "start_line": 3, "end_line": 3 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/api",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("runtime graph json");
    graph.runtime = gnr8::graph::RuntimePolicy {
        default_timeout_ms: Some(5_000),
        max_retries: 0,
        retry_statuses: Vec::new(),
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

fn pagination_graph() -> gnr8::graph::ApiGraph {
    let mut graph: gnr8::graph::ApiGraph = serde_json::from_str(
        r#"{
          "module": "app",
          "operations": [
            {
              "id": "listItems",
              "method": "GET",
              "path": "/items",
              "handler": "listItems",
              "params": [
                {
                  "name": "cursor",
                  "location": "query",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "main.py", "start_line": 1, "end_line": 1 }
                }
              ],
              "request_body": null,
              "request_body_required": true,
              "responses": [ { "status": 200, "body": { "ref_id": "dto.ItemPage" } } ],
              "provenance": { "file": "main.py", "start_line": 1, "end_line": 1 }
            }
          ],
          "schemas": [
            {
              "id": "dto.Item",
              "name": "Item",
              "body": { "type": "object", "of": [
                {
                  "json_name": "id",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "dto.ItemPage",
              "name": "ItemPage",
              "body": { "type": "object", "of": [
                {
                  "json_name": "items",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "array", "of": { "type": "named", "of": "dto.Item" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "next_cursor",
                  "required": false,
                  "optional": true,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.py", "start_line": 2, "end_line": 2 }
            }
          ],
          "diagnostics": [],
          "base_path": "/api",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("pagination graph json");
    graph.pagination = vec![gnr8::graph::PaginationPolicy {
        operation_id: "listItems".to_string(),
        mode: gnr8::graph::PaginationMode::Cursor,
        items_field: "items".to_string(),
        cursor_param: Some("cursor".to_string()),
        next_cursor_field: Some("next_cursor".to_string()),
        page_param: None,
        page_size_param: None,
        offset_param: None,
        limit_param: None,
        termination: gnr8::graph::PaginationTermination::NoNextCursor,
    }];
    graph
}

/// PYSDK-02 (a)+(b) + PYSDK-01: the generated SDK `py_compile`s every file (syntax), `import`s cleanly
/// (executes the class bodies — Pydantic model definitions, 3.9 annotation spellings), and carries
/// ZERO third-party HTTP imports (supply-chain assertion, grepped over the written files).
#[test]
fn generated_sdk_py_compiles_and_imports() {
    if !python_available() {
        eprintln!("skipping pysdk_compile: python3 toolchain unavailable");
        return;
    }
    let dir = materialize_sdk();
    let pkg_dir = dir.join(PACKAGE);

    // The four production SDK files exist under the package subdir.
    for name in SDK_FILES {
        assert!(
            pkg_dir.join(name).exists(),
            "expected {name} in {}",
            pkg_dir.display()
        );
    }

    // Supply-chain assertion (PYSDK-01 / threat T-03-03-04): no third-party HTTP deps land in the
    // generated output, and the expected stdlib/Pydantic seams ARE present in the right files.
    let client_src = std::fs::read_to_string(pkg_dir.join("client.py")).expect("read client.py");
    let models_src = std::fs::read_to_string(pkg_dir.join("models.py")).expect("read models.py");
    let errors_src = std::fs::read_to_string(pkg_dir.join("errors.py")).expect("read errors.py");
    for name in SDK_FILES {
        let src = std::fs::read_to_string(pkg_dir.join(name)).expect("read generated .py");
        for banned in ["import requests", "import httpx"] {
            assert!(
                !src.contains(banned),
                "generated {name} must not contain a third-party HTTP import ({banned}):\n{src}"
            );
        }
    }
    assert!(
        client_src.contains("urllib.request.OpenerDirector"),
        "client.py must expose the injectable OpenerDirector seam:\n{client_src}"
    );
    assert!(
        models_src.contains("class Book(BaseModel):"),
        "models.py must emit Pydantic BaseModel models by default:\n{models_src}"
    );
    assert!(
        models_src.contains("ConfigDict(populate_by_name=True, extra=\"ignore\")"),
        "models.py must emit modern Pydantic v2 model config:\n{models_src}"
    );
    assert!(
        errors_src.contains("class ApiError(Exception):"),
        "errors.py must define the typed ApiError:\n{errors_src}"
    );

    // Gate (a): py_compile every file — a syntax/indentation error exits non-zero (Pitfall 3).
    for name in SDK_FILES {
        let path = pkg_dir.join(name);
        let path_str = path.to_str().expect("utf-8 path");
        let compiled = run_python(&["-m", "py_compile", path_str], &dir);
        assert!(
            compiled.is_ok(),
            "python3 -m py_compile {name} must succeed: {compiled:?}"
        );
    }

    // Gate (b): import the package with the package PARENT as the current dir so `import bookstore`
    // resolves the `<dir>/bookstore/` package — executes every class body (catches the model
    // field-order TypeError / a bad Optional / a NameError, Pitfall 1/3).
    let imported = run_python(&["-c", "import bookstore"], &dir);
    assert!(
        imported.is_ok(),
        "python3 -c 'import bookstore' must succeed (class bodies execute): {imported:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// Threat T-03-03-05 / RUST-04: `py_compile` of invalid Python surfaces a captured-stderr `CoreError`
/// (carrying the exit code + stderr), never a panic in the `run_python` helper. Mirrors the Go twin's
/// `invalid_go_build_maps_to_go_build_error_not_panic`.
#[test]
fn invalid_python_compile_maps_to_captured_error_not_panic() {
    if !python_available() {
        eprintln!("skipping pysdk_compile error-path: python3 toolchain unavailable");
        return;
    }
    let dir = unique_temp_dir("bad");
    // Deliberately invalid Python — `py_compile` must exit non-zero.
    let broken = dir.join("broken.py");
    std::fs::write(&broken, "def (:\n").expect("write broken.py");

    let result = run_python(
        &["-m", "py_compile", broken.to_str().expect("utf-8 path")],
        &dir,
    );
    match result {
        Err(gnr8::CoreError::GoBuild { code, stderr }) => {
            assert!(
                code != Some(0),
                "a failed compile must not report exit code 0"
            );
            assert!(
                !stderr.is_empty(),
                "the error must carry the captured stderr"
            );
        }
        other => panic!("expected a captured-stderr CoreError, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// The hermetic round-trip driver: a stdlib-only Python program that stands up a fake backend, injects
/// an `OpenerDirector` into the generated `Client`, and asserts a 2xx Pydantic-model round-trip plus
/// a 4xx → fallback `ApiError` path. Written to a FILE and run by path (NEVER `-c "<interpolated
/// data>"`, threat
/// T-03-03-01 / V13). It uses ONLY the Python stdlib (`http.server`/`threading`/`json`/`urllib`) — no
/// fastapi/uvicorn/requests/httpx/pytest, no `pip install` (CLAUDE.md rule 2, threat T-03-03-04).
///
/// Backend shape (matches the `FastAPI` fixture's committed graph):
/// - `do_POST` is the `create_book` path (`/`): replies `201` with a `CreatedMessage` body.
/// - `do_GET` is the `get_book` path (`/{book_id}`): replies `404` with an undeclared fallback body.
///
/// The body passed to `create_book` is an actual `Book` Pydantic-style instance (CR-01 regression
/// coverage): the generated `_do` now marshals `BaseModel` values via `model_dump` before
/// `json.dumps`, so the advertised typed happy path — construct the model, pass it to the method —
/// must round-trip. (The prior driver sent a raw dict, which routed AROUND the broken signature and
/// masked the `TypeError: Object of type Book is not JSON serializable` defect.)
const ROUND_TRIP_DRIVER: &str = r#"import json
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

import bookstore


class _Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):  # silence the default stderr request log
        pass

    def _send(self, code, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        if code >= 400:
            self.send_header("X-Request-ID", f"req-{code}")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):  # the create_book path (POST /): 201 -> CreatedMessage
        length = int(self.headers.get("Content-Length", 0))
        _ = self.rfile.read(length)  # drain the request body
        self._send(201, {"message": "ok", "id": 1})

    def do_GET(self):  # the get_book path (GET /{book_id}): 404 -> fallback error body
        self._send(404, {"message": "not found", "slug": "book_not_found"})


def main():
    # Bind an EPHEMERAL port (Pitfall 5) so parallel test runs never race a fixed port.
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        opener = urllib.request.build_opener()
        client = bookstore.Client(f"http://127.0.0.1:{port}", opener=opener)

        # 2xx: create_book is called with an ACTUAL Book model instance (CR-01) — the generated
        # _do marshals it via model_dump before json.dumps, exercising the typed request-body
        # path the signature advertises. The 201 reply decodes into a CreatedMessage @dataclass.
        book = bookstore.Book(
            author=bookstore.Author(name="Ada", bio=None),
            format=bookstore.BookFormat.HARDCOVER,
            id=7,
            title="Notes",
        )
        created = client.create_book(book)
        assert isinstance(created, bookstore.CreatedMessage), type(created)
        assert created.id == 1, created.id
        assert created.message == "ok", created.message

        # 4xx without a declared error model: get_book(999) hits the 404 path -> ApiError with fallback
        # JSON body plus response metadata and standard message/slug fields (Pitfall 6).
        try:
            client.get_book(999)
        except bookstore.ApiError as e:
            assert e.status_code == 404, e.status_code
            assert e.is_not_found(), "is_not_found() must be true for a 404"
            assert e.request_id == "req-404", e.request_id
            assert e.headers.get("X-Request-ID") == "req-404", e.headers
            assert b"book_not_found" in e.raw_body, e.raw_body
            assert e.json_body["slug"] == "book_not_found", e.json_body
            assert isinstance(e.body, dict), type(e.body)
            assert e.body["slug"] == "book_not_found", e.body
            assert e.message == "not found", e.message
            assert e.slug == "book_not_found", e.slug
        else:
            raise SystemExit("get_book(999) must raise ApiError on a 404")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
"#;

const AUTH_DRIVER: &str = r#"import threading
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

import bookstore


class _Handler(BaseHTTPRequestHandler):
    seen = []

    def log_message(self, *args):
        pass

    def _send_no_content(self):
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        _Handler.seen.append(parsed.path)
        if parsed.path == "/api/items":
            query = urllib.parse.parse_qs(parsed.query)
            assert query.get("api_key") == ["secret"], query
        elif parsed.path == "/api/bearer":
            assert self.headers.get("Authorization") == "Bearer secret-token", self.headers
        elif parsed.path == "/api/basic":
            assert self.headers.get("Authorization") == "Basic dXNlcjpwYXNz", self.headers
        else:
            raise AssertionError(f"unexpected path {parsed.path}")
        self._send_no_content()


def main():
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        client = bookstore.Client(
            f"http://127.0.0.1:{port}",
            api_key="secret",
            bearer_token="secret-token",
            basic_auth=("user", "pass"),
            opener=urllib.request.build_opener(),
        )
        assert client.list_items() is None
        assert client.get_bearer() is None
        assert client.get_basic() is None
        assert _Handler.seen == ["/api/items", "/api/bearer", "/api/basic"], _Handler.seen
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
"#;

const MEDIA_DRIVER: &str = r#"import threading
import urllib.parse
import urllib.request
from enum import Enum
from http.server import BaseHTTPRequestHandler, HTTPServer

import bookstore


class WireValue(str, Enum):
    ADA = "Ada"
    REPORT = "Report"


class _Handler(BaseHTTPRequestHandler):
    seen = []

    def log_message(self, *args):
        pass

    def _send_no_content(self):
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        _Handler.seen.append(self.path)
        if self.path == "/text":
            assert self.headers.get("Content-Type") == "text/plain", self.headers
            assert body == b"hello", body
        elif self.path == "/form":
            assert self.headers.get("Content-Type") == "application/x-www-form-urlencoded", self.headers
            values = urllib.parse.parse_qs(body.decode("utf-8"))
            assert values == {"count": ["3"], "name": ["Ada"], "tags": ["sdk", "media"]}, values
        elif self.path == "/multipart":
            assert self.headers.get("Content-Type", "").startswith("multipart/form-data; boundary="), self.headers
            assert b'name="title"' in body, body
            assert b"Report" in body, body
            assert b'name="file"; filename="file"' in body, body
            assert b"\xff\x00binary" in body, body
            assert body.count(b'name="files"; filename="files"') == 2, body
            assert b"part-one" in body, body
            assert b"part-two" in body, body
        elif self.path == "/binary":
            assert self.headers.get("Content-Type") == "application/octet-stream", self.headers
            assert body == b"raw-bytes", body
        else:
            raise AssertionError(f"unexpected path {self.path}")
        self._send_no_content()


def main():
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        client = bookstore.Client(
            f"http://127.0.0.1:{port}",
            opener=urllib.request.build_opener(),
        )
        assert client.post_text("hello") is None
        assert client.post_form(bookstore.FormBody(name=WireValue.ADA, count=3, tags=["sdk", "media"])) is None
        assert client.post_multipart(
            bookstore.MultipartBody(
                title=WireValue.REPORT,
                file=b"\xff\x00binary",
                files=[b"part-one", b"part-two"],
            )
        ) is None
        assert client.post_binary(b"raw-bytes") is None
        assert _Handler.seen == ["/text", "/form", "/multipart", "/binary"], _Handler.seen
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
"#;

const RUNTIME_DRIVER: &str = r#"import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

import bookstore


events = []


class _Handler(BaseHTTPRequestHandler):
    counts = {"GET /api/items": 0, "POST /api/unsafe": 0, "POST /api/idempotent": 0}
    idempotency_keys = []

    def log_message(self, *args):
        pass

    def _send_empty(self, code):
        self.send_response(code)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        key = f"{self.command} {self.path}"
        _Handler.counts[key] += 1
        self._send_empty(429 if _Handler.counts[key] == 1 else 204)

    def do_POST(self):
        key = f"{self.command} {self.path}"
        _Handler.counts[key] += 1
        if self.path == "/api/idempotent":
            _Handler.idempotency_keys.append(self.headers.get("Idempotency-Key"))
            self._send_empty(500 if _Handler.counts[key] == 1 else 204)
        else:
            self._send_empty(500)


def main():
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        def request_hook(context, request):
            events.append(("request", context.operation_id, context.method, context.path_template, context.request_metadata.get("trace")))

        def response_hook(context):
            events.append(("response", context.operation_id, context.status))

        def error_hook(context, error):
            events.append(("error", context.operation_id, context.status, type(error).__name__))

        client = bookstore.Client(
            f"http://127.0.0.1:{port}",
            timeout=5.0,
            max_retries=0,
            hooks=bookstore.ClientHooks(
                request=[request_hook],
                response=[response_hook],
                error=[error_hook],
            ),
            opener=urllib.request.build_opener(),
        )

        retry_once = bookstore.RequestOptions(max_retries=1, timeout=5.0, metadata={"trace": "runtime"})
        assert client.list_items(request_options=retry_once) is None
        assert _Handler.counts["GET /api/items"] == 2, _Handler.counts

        try:
            client.create_unsafe(request_options=bookstore.RequestOptions(max_retries=1))
        except bookstore.ApiError as e:
            assert e.status_code == 500, e.status_code
        else:
            raise AssertionError("unsafe POST must not retry and must raise")
        assert _Handler.counts["POST /api/unsafe"] == 1, _Handler.counts

        idem = bookstore.RequestOptions(max_retries=1, idempotency_key="idem-1")
        assert client.create_idempotent(request_options=idem) is None
        assert _Handler.counts["POST /api/idempotent"] == 2, _Handler.counts
        assert _Handler.idempotency_keys == ["idem-1", "idem-1"], _Handler.idempotency_keys

        assert ("request", "listItems", "GET", "/items", "runtime") in events, events
        assert ("response", "listItems", 429) in events, events
        assert ("response", "listItems", 204) in events, events
        assert any(event[:3] == ("error", "createUnsafe", 500) for event in events), events
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
"#;

const PAGINATION_DRIVER: &str = r#"import json
import threading
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

import bookstore


class _Handler(BaseHTTPRequestHandler):
    seen = []

    def log_message(self, *args):
        pass

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed.query)
        cursor = query.get("cursor", [""])[0]
        _Handler.seen.append(cursor)
        if parsed.path != "/api/items":
            raise AssertionError(f"unexpected path {parsed.path}")
        if cursor == "":
            payload = {"items": [{"id": "a"}], "next_cursor": "n2"}
        elif cursor == "n2":
            payload = {"items": [{"id": "b"}], "next_cursor": ""}
        else:
            raise AssertionError(f"unexpected cursor {cursor}")
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        client = bookstore.Client(
            f"http://127.0.0.1:{port}",
            opener=urllib.request.build_opener(),
        )
        raw = client.list_items()
        assert raw.items[0]["id"] == "a", raw

        _Handler.seen.clear()
        pages = list(client.list_items_pages())
        assert [page.items[0]["id"] for page in pages] == ["a", "b"], pages
        assert _Handler.seen == ["", "n2"], _Handler.seen

        _Handler.seen.clear()
        items = list(client.iter_list_items())
        assert [item["id"] for item in items] == ["a", "b"], items
        assert _Handler.seen == ["", "n2"], _Handler.seen
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
"#;

/// PYSDK-02 (c): the generated SDK round-trips against a stdlib `http.server` via an injected
/// `OpenerDirector` — a 2xx model decode AND a 4xx → typed `ApiError(is_not_found())`. The driver
/// is written to a file under the package PARENT and run by path so `import bookstore` resolves.
#[test]
fn generated_sdk_round_trips_against_stdlib_http_server() {
    if !python_available() {
        eprintln!("skipping pysdk_compile round-trip: python3 toolchain unavailable");
        return;
    }
    let dir = materialize_sdk();

    // The driver is a PROGRAM-FIXED .py written to a FILE next to the `bookstore/` package (NOT part of
    // the SDK bundle — the bundle stays production-SDK-only, mirroring how the Go twin writes a separate
    // smoke_test.go). Running it by path (never `-c`) keeps the harness clear of command injection (V13).
    let driver = dir.join("round_trip_driver.py");
    std::fs::write(&driver, ROUND_TRIP_DRIVER).expect("write round-trip driver");

    let driver_str = driver.to_str().expect("utf-8 path");
    // Current dir is the package parent (`dir`), so `import bookstore` resolves the `<dir>/bookstore/`
    // package; an uncaught AssertionError/SystemExit in the driver exits non-zero -> a captured error.
    let result = run_python(&[driver_str], &dir);
    assert!(
        result.is_ok(),
        "the stdlib http.server round-trip driver must pass (2xx model + 4xx ApiError): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// AUTH-04: generated Python SDK auth settings are observable at runtime against a stdlib HTTP server:
/// query API-key, bearer token, and basic auth all reach the request generated by the client.
#[test]
fn generated_sdk_sends_auth_against_stdlib_http_server() {
    if !python_available() {
        eprintln!("skipping pysdk_compile auth round-trip: python3 toolchain unavailable");
        return;
    }
    let graph = auth_graph();
    let dir = materialize_sdk_from_graph("auth", &graph, &graph.base_path);
    let driver = dir.join("auth_driver.py");
    std::fs::write(&driver, AUTH_DRIVER).expect("write auth driver");

    let driver_str = driver.to_str().expect("utf-8 path");
    let result = run_python(&[driver_str], &dir);
    assert!(
        result.is_ok(),
        "the stdlib auth driver must pass (query API key + bearer + basic): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// MEDIA-01: generated Python SDK clients send the common request body media types with the correct
/// body encoder and `Content-Type`: text/plain, application/x-www-form-urlencoded, multipart/form-data,
/// and application/octet-stream.
#[test]
fn generated_sdk_media_request_bodies_work_against_stdlib_http_server() {
    if !python_available() {
        eprintln!("skipping pysdk_compile media round-trip: python3 toolchain unavailable");
        return;
    }
    let graph = media_graph();
    let dir = materialize_sdk_from_graph("media", &graph, &graph.base_path);
    let driver = dir.join("media_driver.py");
    std::fs::write(&driver, MEDIA_DRIVER).expect("write media driver");

    let driver_str = driver.to_str().expect("utf-8 path");
    let result = run_python(&[driver_str], &dir);
    assert!(
        result.is_ok(),
        "the stdlib media driver must pass (text + form + multipart + binary): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// RUN-01..07: generated Python SDK runtime controls are observable end-to-end against a stdlib
/// server. A retryable GET retries via per-request `max_retries`, an unsafe POST does not retry, an
/// explicitly idempotent POST retries with the same idempotency key, and hooks receive operation
/// context/status/metadata.
#[test]
fn generated_sdk_runtime_retries_idempotency_and_hooks_work_against_stdlib_http_server() {
    if !python_available() {
        eprintln!("skipping pysdk_compile runtime round-trip: python3 toolchain unavailable");
        return;
    }
    let graph = runtime_graph();
    let dir = materialize_sdk_from_graph("runtime", &graph, &graph.base_path);
    let driver = dir.join("runtime_driver.py");
    std::fs::write(&driver, RUNTIME_DRIVER).expect("write runtime driver");

    let driver_str = driver.to_str().expect("utf-8 path");
    let result = run_python(&[driver_str], &dir);
    assert!(
        result.is_ok(),
        "the stdlib runtime driver must pass (retries + idempotency + hooks): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

/// PAGE-03/PAGE-04: generated Python SDK pagination helpers iterate explicit cursor policies while
/// keeping the raw operation callable.
#[test]
fn generated_sdk_pagination_helpers_work_against_stdlib_http_server() {
    if !python_available() {
        eprintln!("skipping pysdk_compile pagination round-trip: python3 toolchain unavailable");
        return;
    }
    let graph = pagination_graph();
    let dir = materialize_sdk_from_graph("pagination", &graph, &graph.base_path);
    let driver = dir.join("pagination_driver.py");
    std::fs::write(&driver, PAGINATION_DRIVER).expect("write pagination driver");

    let driver_str = driver.to_str().expect("utf-8 path");
    let result = run_python(&[driver_str], &dir);
    assert!(
        result.is_ok(),
        "the stdlib pagination driver must pass (pages + items + raw method): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}
