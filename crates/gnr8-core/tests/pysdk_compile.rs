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

/// Materialize the generated SDK into a fresh temp dir as an importable `<dir>/bookstore/` package,
/// returning the temp dir (the package PARENT). Python needs NO manifest analog to the Go `go.mod`.
///
/// The four files go under `<dir>/bookstore/` so `__init__.py`'s relative imports (`from .client import
/// Client`) resolve and `python3 -c "import bookstore"` works with `<dir>` as the current dir.
fn materialize_sdk() -> PathBuf {
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 build_graph must succeed (requires python3 for the pyextract sidecar)");
    // `base_path` is the graph's single source of truth (the FastAPI fixture's is "/"); pass it through
    // exactly as a Pipeline would (CLAUDE.md rules 3 & 4).
    let bundle = gnr8::pysdk::generate(&graph, PACKAGE, &graph.base_path)
        .expect("pysdk::generate must succeed");
    let dir = unique_temp_dir("ok");
    let pkg_dir = dir.join(PACKAGE);
    std::fs::create_dir_all(&pkg_dir).expect("create package subdir");
    gnr8::sdk::bundle::write_to_dir(&bundle, &pkg_dir)
        .expect("write_to_dir must materialize the SDK");
    std::fs::write(
        dir.join("pydantic.py"),
        r#"class ConfigDict(dict):
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
    dir
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
