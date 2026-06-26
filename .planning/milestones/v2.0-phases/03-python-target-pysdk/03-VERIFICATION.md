---
phase: 03-python-target-pysdk
verified: 2026-06-25T22:40:00Z
status: passed
score: 3/3 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
---

# Phase 3: Python Target — `PySdk` Verification Report

**Phase Goal:** A developer can generate a dependency-free Python SDK from the neutral IR and prove it works against the live FastAPI fixture, establishing the second SDK target as a pure IR→string twin of the Go SDK.
**Verified:** 2026-06-25T22:40:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth (ROADMAP Success Criterion + PLAN must-haves) | Status | Evidence |
| --- | --- | --- | --- |
| 1 | **PYSDK-01** — A developer can generate a dependency-free Python SDK from the IR (stdlib `urllib`, `@dataclass` models, typed `ApiError`, injectable `OpenerDirector`); deterministic four-file bundle; exhaustive `py_type` incl. Union/inline-Enum | ✓ VERIFIED | `crates/gnr8-core/src/pysdk/{bundle,emit,mod}.rs` exist (178/1731/262 LOC); `pub mod pysdk` in lib.rs:15. `py_type` exhaustive match over all 9 `Type` variants (emit.rs:155-191), no `_=>` arm; `Type::Union`×5, `Literal`×13. Emitted client imports only `__future__/json/urllib.*/typing/dataclasses/enum` (emit.rs:48-52,470-472,510-517). Determinism tests `generate_is_byte_identical_across_two_runs`, `generate_models_is_byte_identical_across_two_runs` PASS. 39 pysdk lib tests green. |
| 2 | **PYSDK-02** — Generated SDK imports/type-checks and round-trips against the FastAPI fixture in a hermetic test (no third-party HTTP deps) | ✓ VERIFIED | `cargo test -p gnr8-core --test pysdk_compile` → **3 passed, 0 failed** (ran in 0.62s, NO skip). `generated_sdk_py_compiles_and_imports` (py_compile + `import bookstore` gates), `generated_sdk_round_trips_against_stdlib_http_server` (stdlib `http.server` on port 0, injected `OpenerDirector`, typed `Book` dataclass POST → 201 `CreatedMessage` dataclass assert + `get_book(999)` → 404 typed `ApiError.is_not_found()`), `invalid_python_compile_maps_to_captured_error_not_panic`. python3 3.9.25 present — the gate ACTUALLY ran. Supply-chain assertion (pysdk_compile.rs:145) asserts absence of `import requests/httpx/pydantic` in generated output. |
| 3 | **PYSDK-03** — A developer adds the Python SDK via a `PySdk` `Target` built-in; output byte-identical across runs | ✓ VERIFIED | `pub struct PySdk` in sdk/builtins.rs (×1); exported in prelude (mod.rs:338). `crate::pysdk::generate` called (×2), `ir.base_path` used (×8, single source), `sdk_package` single definition (builtins.rs:650, reused by GoSdk+PySdk — no second derivation). Tests `pysdk_target_writes_under_the_output_dir_and_is_deterministic` and `targets_error_when_unconfigured` (PySdk arms) PASS. |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| --- | --- | --- | --- |
| `crates/gnr8-core/src/pysdk/bundle.rs` | SdkBundle/SdkFile marker framing + round-trip | ✓ VERIFIED | 178 LOC; `MARKER_PREFIX` present; framing/round-trip/determinism tests green |
| `crates/gnr8-core/src/pysdk/emit.rs` | IR→Python emitters; exhaustive `py_type` | ✓ VERIFIED | 1731 LOC; exhaustive match (no `_=>` in py_type/emit_models), Union/Literal/enum-class; CR-01..WR-05 hardening (safe_ident, from_dict, asdict body) |
| `crates/gnr8-core/src/pysdk/mod.rs` | generate/split_bundle/write_to_dir | ✓ VERIFIED | 262 LOC; `pub fn generate`; four-file push order; name-safety guard; determinism test |
| `crates/gnr8-core/src/lib.rs` | `pub mod pysdk;` registration | ✓ VERIFIED | line 15 |
| `crates/gnr8-core/src/sdk/builtins.rs` | PySdk Target struct + impl + tests | ✓ VERIFIED | `pub struct PySdk`; drives `crate::pysdk::generate`; reuses `sdk_package`; unsafe-name guard |
| `crates/gnr8-core/src/sdk/mod.rs` | PySdk in prelude | ✓ VERIFIED | line 338 prelude re-export |
| `crates/gnr8-core/tests/pysdk_compile.rs` | Hermetic generate→compile→import→round-trip | ✓ VERIFIED | 336 LOC; `python_available`/`run_python`; 3 tests pass |

### Key Link Verification

| From | To | Via | Status |
| --- | --- | --- | --- |
| pysdk/mod.rs | pysdk/emit.rs | generate() calls emit::emit_* | ✓ WIRED |
| pysdk/emit.rs | crate::graph::Type | exhaustive match | ✓ WIRED |
| sdk/builtins.rs | crate::pysdk::generate | PySdk::generate(ir, &package, &ir.base_path) | ✓ WIRED |
| sdk/builtins.rs | sdk_package | reuses single derivation (rule 3) | ✓ WIRED |
| tests/pysdk_compile.rs | pysdk::generate + write_to_dir | build_graph(fixture)→generate→write_to_dir | ✓ WIRED |
| tests/pysdk_compile.rs | python3 subprocess | discrete-arg Command | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Produces Real Data | Status |
| --- | --- | --- | --- | --- |
| Generated `bookstore` SDK | 2xx `CreatedMessage` dataclass | `build_graph(fastapi-bookstore)` → IR → generate → live round-trip vs stdlib http.server | Yes — `created.id==1`, `created.message=="ok"` asserted | ✓ FLOWING |
| Generated `Client._do` | typed `Book` request body | `dataclasses.asdict(book)` → json.dumps (CR-01) | Yes — real Book(Author, BookFormat, id, title) sent | ✓ FLOWING |
| Generated `ApiError` | 4xx error path | urllib HTTPError → `_raise` → ApiError | Yes — `status_code==404`, `is_not_found()` asserted | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| --- | --- | --- | --- |
| Hermetic SDK compile/import/round-trip | `cargo test -p gnr8-core --test pysdk_compile` | 3 passed, 0 failed (0.62s, no skip) | ✓ PASS |
| Full core suite | `cargo test -p gnr8-core` | all binaries 0 failed; 169+ lib + 3 pysdk_compile green; 3 ignored (pre-existing Go-toolchain) | ✓ PASS |
| Green gate | `make check` | exit 0 (fmt + clippy -D warnings + Rust + Go build/vet/test) | ✓ PASS |
| python3 toolchain | `python3 --version` | 3.9.25 (round-trip actually runs) | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| --- | --- | --- | --- | --- |
| PYSDK-01 | 03-01 | Dependency-free Python SDK from IR | ✓ SATISFIED | Truth 1 |
| PYSDK-02 | 03-03 | Imports/type-checks + hermetic round-trip | ✓ SATISFIED | Truth 2 |
| PYSDK-03 | 03-02 | PySdk Target built-in; byte-identical | ✓ SATISFIED | Truth 3 |

All three phase requirement IDs are declared in PLAN frontmatter, present in REQUIREMENTS.md (lines 33-35, mapped to Phase 3 at lines 101-103), and satisfied. No orphaned requirements.

### CLAUDE.md Invariant Checks

| Invariant | Status | Evidence |
| --- | --- | --- |
| Rule 2 — no new OSS deps | ✓ PASS | `git diff b4a5f77..HEAD -- Cargo.lock **/Cargo.toml Cargo.toml` is EMPTY across the entire phase |
| Generated SDK + hermetic test stdlib-Python only | ✓ PASS | Emit imports only `__future__/typing/enum/dataclasses/json/urllib.*`; test backend/driver use `http.server/threading/json/urllib` only; supply-chain absence asserted |
| Rule 3 — single source of truth / no fallback | ✓ PASS | `fn sdk_package` defined once (builtins.rs:650); `ir.base_path` never re-derived; `py_type`/`emit_models` exhaustive (no `_=>` catch-all on the Type→string mapping) |
| RUST-04 — no production unwrap/expect/panic | ✓ PASS | All `.unwrap()`/`.expect()` in pysdk lib files are inside `#[cfg(test)]`; errors via typed `CoreError::SdkGen`/`Config` |

### Documented Limitation (not a regression)

Union-typed **2xx success** return decode (e.g. `BookOrError = Union[Book, OutOfStock]`) cannot dispatch to a single `from_dict`. Documented in 03-REVIEW-FIX.md as a future-phase item. Confirmed NOT a regression: it was non-functional pre-fix, and no acceptance test exercises it — the `get_book` round-trip only hits the 404/error path (pysdk_compile.rs:291-299). Acceptable.

### Anti-Patterns Found

None. No `TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER` markers in any phase-modified file. The only `requests/httpx/pydantic` strings in the codebase are negative assertions / docstrings asserting their absence.

### Human Verification Required

None. python3 3.9.25 is present, so the load-bearing PYSDK-02 hermetic round-trip executed in-process and passed; `make check` (incl. Go via PATH) exits 0. No visual/UX/external-service items.

### Gaps Summary

No gaps. All 3 ROADMAP success criteria are observably true in the codebase: the `pysdk` module generates a deterministic dependency-free four-file Python SDK with an exhaustive IR type mapping (Union/inline-Enum/named-Enum); the `PySdk` Target built-in drives it from a `.gnr8/` Pipeline with byte-identical output; and the hermetic `pysdk_compile` test proves generate → py_compile → import → live round-trip (2xx typed dataclass + 4xx typed ApiError) against the FastAPI-fixture-derived IR using only Python stdlib. All CLAUDE.md invariants hold (zero new crates, stdlib-only generated/test code, single source of truth, no production panics).

---

_Verified: 2026-06-25T22:40:00Z_
_Verifier: Claude (gsd-verifier)_
