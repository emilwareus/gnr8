---
phase: 03-python-target-pysdk
plan: 03
subsystem: pysdk
tags: [test, acceptance, python, hermetic, compile-gate, round-trip]
requires:
  - "pysdk::generate / pysdk::write_to_dir (03-01: the deterministic four-file Python SDK bundle + materialization)"
  - "crate::analyze::build_graph (Phase 2: fastapi-bookstore dir -> pyextract -> IR; runs because python3 is present)"
  - "crate::CoreError::{PythonToolchainMissing, GoBuild} (typed spawn-failure + captured-stderr carriers reused by run_python)"
provides:
  - "tests/pysdk_compile.rs: the hermetic generate->write->py_compile->import->round-trip acceptance gate (twin of sdk_compile.rs)"
  - "the PYSDK-02 load-bearing proof: the generated SDK compiles, imports, and round-trips (2xx dataclass + 4xx typed ApiError)"
affects:
  - "Makefile (pysdk_compile added to the blocking `gates:` line + comment)"
  - "crates/gnr8-core/src/pysdk/emit.rs (two generated-SDK compile bugs fixed: f-string backslash, eager alias forward-ref)"
tech-stack:
  added: []
  patterns:
    - "hermetic stdlib-Python test: http.server on 127.0.0.1:0 (ephemeral) in a daemon thread, shutdown()/server_close() in finally"
    - "injected urllib OpenerDirector seam for a transport-swappable round-trip (no requests/httpx)"
    - "program-fixed driver written to a FILE and run by PATH (never -c with interpolated data — V13 command-injection mitigation)"
    - "unique_temp_dir via std::env::temp_dir() + PID + nanos (no tempfile crate)"
    - "subprocess non-zero exit -> captured-stderr CoreError, spawn failure -> PythonToolchainMissing (no unwrap on the Result)"
key-files:
  created:
    - crates/gnr8-core/tests/pysdk_compile.rs
  modified:
    - crates/gnr8-core/src/pysdk/emit.rs
    - Makefile
    - crates/gnr8-core/src/pysdk/mod.rs
    - crates/gnr8-core/src/sdk/builtins.rs
decisions:
  - "The generated templated-path f-string emitted `safe=\\\"\\\"` (escaped double quotes) -> SyntaxError on Python 3.9-3.11 ('f-string expression part cannot include a backslash'). Fixed the emitter to `safe=''` (single quotes, backslash-free, valid on every Python 3.x). A real compile bug the string-only unit tests could not catch."
  - "A named type alias (BookOrError = Union[Book, OutOfStock]) was an EAGER module-level assignment that referenced a class defined LATER in the id-sorted file -> NameError at import. Fixed the emitter to emit a PEP-484 string forward reference (Name = \"Union[...]\") so the assignment binds a plain str without evaluating forward names."
  - "The 2xx body passed to create_book is a Book-shaped dict, not a Book dataclass instance: the generated _do does json.dumps(body), and a @dataclass is not json-serializable; the `body: Book` hint is a lazy annotation, unenforced at runtime. A dict is the correct stdlib-only payload."
  - "run_python reuses CoreError::GoBuild as the generic exit-code+stderr carrier (no new error variant), exactly as the plan's interfaces directed; spawn failure maps to the existing PythonToolchainMissing."
metrics:
  duration: ~35m
  completed: 2026-06-25
  tasks: 2
  files: 1
  integration_tests: 3
---

# Phase 3 Plan 03: pysdk hermetic acceptance test (compile + round-trip) Summary

`tests/pysdk_compile.rs` — the load-bearing PYSDK-02 acceptance gate and the twin of `tests/sdk_compile.rs`:
it builds the IR from `fixtures/fastapi-bookstore` (the Phase-2 `pyextract` path, which runs because
`python3` is present), generates the Python SDK, writes it as an importable `bookstore/` package under a
unique stdlib temp dir, then proves it `py_compile`s, `import`s, and round-trips against a stdlib
`http.server` (2xx `@dataclass` decode + 4xx typed `ApiError`) — all with zero third-party imports. Because
`python3` is present and `go` is absent here, this is the SDK acceptance test that actually RUNS.

## What was built

- **`tests/pysdk_compile.rs`** (3 integration tests, `#![allow(clippy::unwrap_used, expect_used, panic)]`
  scoped to the target):
  - `generated_sdk_py_compiles_and_imports` — materializes the SDK into `<temp>/bookstore/`, asserts the
    four files exist, **greps every written `.py` for the supply-chain claim** (no `requests`/`httpx`/
    `pydantic`; `OpenerDirector`/`@dataclass`/`ApiError` present in the right files), then runs gate (a)
    `python3 -m py_compile <each file>` (syntax) and gate (b) `python3 -c "import bookstore"` with the
    package parent as `current_dir` (executes every class body — dataclass field order, 3.9 spellings).
  - `generated_sdk_round_trips_against_stdlib_http_server` — gate (c): a program-fixed driver (`const
    ROUND_TRIP_DRIVER`) written to a FILE and run by path stands up a stdlib `http.server` on
    `("127.0.0.1", 0)` (ephemeral port) in a daemon thread, injects an `OpenerDirector` into the generated
    `Client`, asserts `create_book({...dict...})` decodes a 201 `CreatedMessage` dataclass (`id==1`,
    `message=="ok"`) AND `get_book(999)` raises a typed `ApiError(status_code==404, is_not_found())`;
    `shutdown()`/`server_close()` run in a `finally`.
  - `invalid_python_compile_maps_to_captured_error_not_panic` — the error-path twin: invalid Python makes
    `py_compile` exit non-zero and `run_python` returns a captured-stderr `CoreError`, never a panic.
  - Harness helpers translated from the Go twin: `python_available()`, `unique_temp_dir()` (verbatim,
    `std::env::temp_dir()` + PID + nanos, no `tempfile` crate), `run_python()` (discrete args +
    `current_dir`, NO shell; non-zero exit -> `CoreError::GoBuild`, spawn failure ->
    `PythonToolchainMissing`), `materialize_sdk()` (no `go.mod` analog — Python needs no manifest).
- **`Makefile`** — `--test pysdk_compile` added to the blocking `gates:` line alongside `sdk_compile`, with
  the comment updated; `make check` (and `make gates`) stays green.

## Verification results

- `cargo test -p gnr8-core --test pysdk_compile` — 3 tests green; the gates RUN (no skip — `python3` present).
- `cargo test -p gnr8-core` — full crate green (incl. the now-green `snapshot_fastapi_*` from Phase 2;
  `snapshot_nestjs_*` remain `#[ignore]`d as designed).
- `cargo clippy -p gnr8-core --tests --all-features --locked -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- `make check` (with Go on PATH) — green end-to-end: fmt-check, clippy, full test suite, Go fixture
  build/vet, goextract build/vet/test.
- Supply-chain greps over the WRITTEN SDK files assert no `requests`/`httpx`/`pydantic`; the driver imports
  only `json`/`threading`/`urllib.request`/`http.server`/`bookstore` (no fastapi/uvicorn/pytest, no pip).
- `git diff HEAD~2 -- Cargo.toml Cargo.lock` — empty (zero new crates, rule 2).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Generated templated-path f-string did not compile (SyntaxError) on Python 3.9-3.11**
- **Found during:** Task 1 (gate (a) `py_compile client.py` exited non-zero).
- **Issue:** `emit_operation` emitted `path = f"/{urllib.parse.quote(str(book_id), safe=\"\")}"` — a
  backslash inside an f-string expression part is a `SyntaxError` ("f-string expression part cannot include
  a backslash") on every Python 3.9-3.11. The 32 string-only emit unit tests asserted the substring but
  never compiled it, so the defect was invisible until the real `py_compile` gate. EVERY templated-path
  operation (`get_book`, `update_book`) was affected — the generated SDK did not compile.
- **Fix:** Emit `safe=''` (single quotes inside the double-quoted f-string) — backslash-free, valid on all
  Python 3.x. Updated the `templated_path_escapes_each_param_with_urllib_quote` unit test to match.
- **Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
- **Commit:** 19f9eab

**2. [Rule 1 - Bug] Named type alias was an eager forward reference -> NameError at import**
- **Found during:** Task 1 (gate (b) `import bookstore` exited non-zero).
- **Issue:** `emit_models` emitted `BookOrError = Union[Book, OutOfStock]` as a module-level assignment.
  Unlike a `@dataclass` annotation (kept lazy by `from __future__ import annotations`), an alias assignment
  is evaluated EAGERLY at import; schemas are id-sorted, so `BookOrError` precedes `OutOfStock` in the file
  and the eager RHS raised `NameError: name 'OutOfStock' is not defined`. The import gate (and any real
  consumer) failed.
- **Fix:** Emit the alias RHS as a PEP-484 string forward reference: `BookOrError = "Union[Book, OutOfStock]"`.
  The assignment binds a plain `str` (importable, re-exportable) without evaluating any forward name; it
  remains a valid type alias in annotation position. Single deterministic path (no fallback — rule 3).
- **Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
- **Commit:** 19f9eab

### Scope adjustments (not deviations)

- **`cargo fmt` reflowed two pre-existing test assertions** in `pysdk/mod.rs` and `sdk/builtins.rs` to
  canonical form (no logic change). Included so `make check`'s `fmt-check` stays green.
- **Acceptance-criterion grep `fastapi|uvicorn == 0` is satisfied in substance, not literally.** The coarse
  grep matches the *fixture name* (`fastapi-bookstore`) and hermeticity prose in comments; the actual
  `ROUND_TRIP_DRIVER` imports only Python stdlib + `bookstore` (verified: no `import fastapi`/`uvicorn`/
  `requests`/`httpx`/`pytest`). The backend is provably stdlib-only.

## Threat surface

All Wave-3 threat-register mitigations applied: `run_python` uses discrete `Command::args` + `current_dir`
with NO shell string and the driver is written to a FILE and run by path (T-03-03-01); `unique_temp_dir`
is `temp_dir()` + PID + nanos with best-effort cleanup (T-03-03-02); the fake backend binds an ephemeral
port in a daemon thread and `shutdown()`/`server_close()` in `finally` (T-03-03-03); the test greps every
written `.py` asserting no third-party deps and the driver+backend are stdlib-only (T-03-03-04); subprocess
non-zero exit -> captured-stderr `CoreError`, spawn failure -> `PythonToolchainMissing`, no `unwrap` on the
Result (T-03-03-05); nothing is installed — `python3` + stdlib are pre-present (T-03-03-SC).

No new threat surface beyond the plan's `<threat_model>`.

## Known Stubs

None. The test exercises the full generate -> write -> compile -> import -> round-trip path against the
real fixture; the two emitter fixes make the generated SDK genuinely compile, import, and round-trip.

## Self-Check: PASSED

- File: `crates/gnr8-core/tests/pysdk_compile.rs` FOUND.
- Commits: `19f9eab`, `f4004ba` both FOUND in git log.
- `cargo test -p gnr8-core --test pysdk_compile` runs 3 tests, all green (no skip — python3 present).
