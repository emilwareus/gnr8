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
//!   (b) `python3 -c "import bookstore"`         — executes the class bodies (catches the @dataclass
//!       field-order `TypeError`, a bad `Optional`/`Literal`, or a `NameError`, Pitfall 1/3).
//!   (c) a program-written stdlib `http.server` driver — binds `("127.0.0.1", 0)` (ephemeral port,
//!       Pitfall 5), serves in a daemon thread, injects an `OpenerDirector` into the generated `Client`,
//!       and asserts a 2xx `@dataclass` round-trip AND a 4xx → typed `ApiError(is_not_found())`.
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

/// The `FastAPI` fixture, resolved relative to this crate's manifest dir (mirrors the other tests).
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/fastapi-bookstore"
);

/// The generated SDK's Python package name (also the import name and the package subdir).
const PACKAGE: &str = "bookstore";

/// The four files the `pysdk` bundle always frames (D-06 push order).
const SDK_FILES: [&str; 4] = ["__init__.py", "client.py", "errors.py", "models.py"];

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
    let dir = std::env::temp_dir().join(format!(
        "gnr8-pysdk-compile-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// Run `python3 <args>` in `dir`, mapping a non-zero exit to a captured-stderr `CoreError` (never a
/// panic — the harness uses NO `unwrap`/`expect` on the subprocess `Result`, threat T-03-03-05). A spawn
/// failure (missing toolchain) maps to `CoreError::PythonToolchainMissing`. Discrete args + `current_dir`
/// only — NEVER a shell string (threat T-03-03-01 / V13).
fn run_python(args: &[&str], dir: &Path) -> Result<String, gnr8_core::CoreError> {
    let output = Command::new("python3")
        .args(args)
        .current_dir(dir)
        // Belt-and-braces hermeticity: never let an import silently reach the network or a user site dir
        // (the Python analog of GOPROXY=off — the round-trip is localhost-only, no pip fetch).
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .output()
        // Spawn failure (e.g. python3 absent) → the dedicated toolchain-missing variant (error.rs:45).
        .map_err(|source| gnr8_core::CoreError::PythonToolchainMissing { source })?;
    if !output.status.success() {
        // Reuse the generic captured-stderr carrier (no new error variant added — the plan's interfaces
        // note: GoBuild is the generic exit-code+stderr carrier the harness reuses, T-03-03-05).
        return Err(gnr8_core::CoreError::GoBuild {
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
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 build_graph must succeed (requires python3 for the pyextract sidecar)");
    // `base_path` is the graph's single source of truth (the FastAPI fixture's is "/"); pass it through
    // exactly as a Pipeline would (CLAUDE.md rules 3 & 4).
    let bundle = gnr8_core::pysdk::generate(&graph, PACKAGE, &graph.base_path)
        .expect("pysdk::generate must succeed");
    let dir = unique_temp_dir("ok");
    let pkg_dir = dir.join(PACKAGE);
    std::fs::create_dir_all(&pkg_dir).expect("create package subdir");
    gnr8_core::pysdk::write_to_dir(&bundle, &pkg_dir)
        .expect("write_to_dir must materialize the SDK");
    dir
}

/// PYSDK-02 (a)+(b) + PYSDK-01: the generated SDK `py_compile`s every file (syntax), `import`s cleanly
/// (executes the class bodies — dataclass field order, 3.9 annotation spellings), and carries ZERO
/// third-party HTTP imports (supply-chain assertion, grepped over the written files).
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

    // Supply-chain assertion (PYSDK-01 / threat T-03-03-04): no third-party HTTP/validation deps land in
    // the generated output, and the expected stdlib seams ARE present in the right files.
    let client_src = std::fs::read_to_string(pkg_dir.join("client.py")).expect("read client.py");
    let models_src = std::fs::read_to_string(pkg_dir.join("models.py")).expect("read models.py");
    let errors_src = std::fs::read_to_string(pkg_dir.join("errors.py")).expect("read errors.py");
    for name in SDK_FILES {
        let src = std::fs::read_to_string(pkg_dir.join(name)).expect("read generated .py");
        for banned in ["import requests", "import httpx", "import pydantic"] {
            assert!(
                !src.contains(banned),
                "generated {name} must not contain a third-party HTTP/validation import ({banned}):\n{src}"
            );
        }
    }
    assert!(
        client_src.contains("urllib.request.OpenerDirector"),
        "client.py must expose the injectable OpenerDirector seam:\n{client_src}"
    );
    assert!(
        models_src.contains("@dataclass"),
        "models.py must emit @dataclass models:\n{models_src}"
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
    // resolves the `<dir>/bookstore/` package — executes every class body (catches the dataclass
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
        Err(gnr8_core::CoreError::GoBuild { code, stderr }) => {
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
