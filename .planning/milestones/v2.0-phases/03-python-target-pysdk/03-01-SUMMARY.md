---
phase: 03-python-target-pysdk
plan: 01
subsystem: pysdk
tags: [codegen, python, sdk, ir-lowering, determinism]
requires:
  - "crate::graph::ApiGraph (the neutral IR + Type/Field/Operation/Schema vocabulary)"
  - "crate::CoreError::SdkGen (typed error for un-representable facts)"
provides:
  - "pysdk::generate(graph, package, base_path) -> Result<String, CoreError> (deterministic four-file Python SDK bundle)"
  - "pysdk::split_bundle / pysdk::write_to_dir (framing + on-disk materialization with the path-traversal name guard)"
  - "pysdk::emit::py_type (exhaustive IR->Python type hint, incl. Union/inline-Enum the Go target rejects)"
affects:
  - "crates/gnr8-core/src/lib.rs (pub mod pysdk; registration)"
tech-stack:
  added: []
  patterns:
    - "format!-based significant-whitespace Python emission (NO formatter; no gofmt analog)"
    - "exhaustive match over Type with no _ => arm (rule 3)"
    - "fixed deterministic import headers (no computed set, no HashMap iteration)"
    - "required-first @dataclass field partition (3.9-safe, kw_only is 3.10+)"
key-files:
  created:
    - crates/gnr8-core/src/pysdk/bundle.rs
    - crates/gnr8-core/src/pysdk/emit.rs
    - crates/gnr8-core/src/pysdk/mod.rs
  modified:
    - crates/gnr8-core/src/lib.rs
decisions:
  - "Named non-object/non-enum schema body (e.g. BookOrError union) -> module-level type alias (Name = Union[...]), NOT a typed error like the Go twin: Python has sum types and the FastAPI fixture has a named union schema."
  - "Inline Type::Object stays a typed SdkGen error (parity with Go) — every object in the IR is a named $ref."
  - "WellKnown::DateTime -> str (RFC-3339 wire string), no datetime import, so @dataclass marshals cleanly through json (A7)."
  - "Fixed import header per file + from __future__ import annotations (lazy annotations) — deterministic by construction; sidesteps 3.9 generic-subscription + forward-ref ordering."
metrics:
  duration: ~20m
  completed: 2026-06-25
  tasks: 3
  files: 4
  unit_tests: 32
---

# Phase 3 Plan 01: pysdk module (IR→Python SDK emitter) Summary

Dependency-free Python SDK generator — the pure IR→string structural twin of `gosdk/` minus the `gofmt`
step — emitting a deterministic four-file bundle (`__init__.py`, `client.py`, `errors.py`, `models.py`)
with an exhaustive `py_type` that handles the `Union`/inline-`Enum` cases the Go target rejects.

## What was built

- **`pysdk/bundle.rs`** — `SdkFile`/`SdkBundle` marker framing copied verbatim from `gosdk/bundle.rs`
  (the `// ==== gnr8:file <name> ====` marker is stripped before files are written, so it never lands in
  emitted Python), with round-trip / determinism / marker-isolation tests.
- **`pysdk/emit.rs`** — the `format!`-based emitters:
  - `py_type` — EXHAUSTIVE over all nine `Type` variants, NO `_ =>` arm. The load-bearing divergence
    from `go_type`: `Union(variants)` → `Union[..]`, inline `Enum(members)` → `Literal[..]` (graph
    order), `Named(ref)` → the resolved schema name, `Array(T)` → `List[..]`, `Map`/`Any` →
    `Dict[str, Any]`/`Any`, `WellKnown` → `str`; inline `Object` → typed `SdkGen` error (parity with
    Go). `nullable` wraps the hint in `Optional[..]`.
  - `emit_models` — named `Enum` body → `class X(str, enum.Enum)` (SCREAMING_SNAKE members); `Object`
    body → `@dataclass` with fields **partitioned required-first / optional-last** (the 3.9-safe fix for
    "non-default argument follows default argument", since `kw_only` is 3.10+); a named union/scalar/array
    body → a module-level type alias (`BookOrError = Union[Book, OutOfStock]`).
  - `emit_client` — dependency-free `Client` over `urllib.request`, with an injectable
    `OpenerDirector` (`opener or urllib.request.build_opener()`) and `_do` catching
    `urllib.error.HTTPError` so 4xx/5xx return `(code, body)`; `_raise` builds the typed `ApiError`.
  - `emit_errors` — `ApiError(Exception)` with `status_code`/`message`/`slug`/`hints` + `is_not_found()`.
  - `emit_operations` — `snake`-named methods; path params escaped via `urllib.parse.quote(str(v),
    safe="")` (V5 path-injection); query via `urllib.parse.urlencode`; compares the operation's REAL
    success status (`success_of` twin) and raises `ApiError` otherwise; decodes JSON into the response
    dataclass. A path whose templated tokens don't match its declared path params is a typed `SdkGen`
    error (WR-03 twin).
  - `emit_init` — re-exports `Client`/`ApiError`/every model in graph order + `__all__`.
- **`pysdk/mod.rs`** — `generate` (fixed-order four-file bundle, no `go_file`/`gofmt` wrapper),
  `split_bundle`, and `write_to_dir` with the path-traversal name guard (reject empty/`/`/`\`/`..`).
- **`lib.rs`** — `pub mod pysdk;` registered in alpha order (after `manifest`, before `runner`).

## Verification results

- `cargo test -p gnr8-core --lib pysdk` — 32 unit tests green.
- `cargo test -p gnr8-core` — full crate green (162 lib + all integration suites).
- `cargo clippy -p gnr8-core --all-targets -- -D warnings` — clean.
- `git diff HEAD -- Cargo.toml crates/gnr8-core/Cargo.toml` — empty (zero new Rust crates, rule 2).
- `py_type` match lists all nine variants with no `_ =>`/`other =>` arm; `Type::Union` and `Literal`
  both emitted (the cases Go rejects).
- **3.9-safety proven out-of-band:** a temporary smoke test generated the SDK and ran
  `python3 -m py_compile` on each file plus `python3 -c "import bookstore"` on Python 3.9.25 — all
  passed (compiles AND imports → significant-whitespace correct, dataclass ordering import-safe,
  `from __future__` + typing spellings valid). The temp test was removed afterward (the durable
  hermetic test is plan 03-03's deliverable).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Named non-object/non-enum schema body must emit a type alias, not a typed error**
- **Found during:** Task 2 (the `named_enum_emits_str_enum_class` test failed because `emit_models`
  rejected the named-union schema `BookOrError` as "unsupported non-object/non-enum body").
- **Issue:** The plan's `emit_models` sketch mirrored the Go twin and errored on any non-object/non-enum
  schema body. But the FastAPI bookstore fixture (and the plan's own must-haves) require named unions
  (`BookOrError = Union[Book, OutOfStock]`) to be representable — Python has sum types. Erroring would
  make the bookstore graph fail to generate, defeating the whole point of the Python target.
- **Fix:** A named schema whose body is not `Object`/`Enum` now emits a module-level type alias via
  `py_type` (`{name} = {hint}`). The match stays exhaustive (the scalar/array/map/named/union/any arms
  share one branch). This is the documented load-bearing divergence from the Go twin.
- **Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
- **Commit:** 392dd98

### Scope adjustments (not deviations)

- **`class_name` helper dropped.** The plan listed `class_name`/`exported` casing helpers, but graph
  schema `.name`s are already PascalCase symbols used verbatim in production — `class_name` had no
  production caller and would have been dead code under `-D warnings`. Kept `snake` (method names) and
  `screaming_snake` (enum members), both of which ARE used. The shared `split_words` tokenizer remains.
- **`split_bundle` carries a scoped `#[allow(dead_code)]`** with a note: it is consumed by the `PySdk`
  target in plan 03-02; the allow is removed the moment that target lands. (Mirrors how the Go
  `split_bundle` is used by `builtins.rs`.)

## Threat surface

All Wave-1 threat-register mitigations applied: `write_to_dir` reuses the path-traversal name guard
(T-03-01-01); generated `client.py` percent-escapes every path param (T-03-01-02); the generated SDK
imports only Python stdlib (`urllib`/`json`/`dataclasses`/`enum`/`typing`) with a test asserting no
`requests`/`httpx` (T-03-01-03); every `fmt::Error` folds to `SdkGen` via `sink`, no production
`unwrap`/`expect`/`panic` (T-03-01-04); fixed import headers + graph-sorted iteration give byte-identical
output, asserted by a determinism unit test (T-03-01-05); zero packages added (T-03-01-SC).

No new threat surface beyond the plan's `<threat_model>`.

## Known Stubs

None. `pysdk::generate` produces a complete, compilable, importable four-file SDK. (`split_bundle` is
fully implemented and is wired by the `PySdk` target in the next plan — it is not a stub.)

## Self-Check: PASSED

- Files: `crates/gnr8-core/src/pysdk/{bundle,emit,mod}.rs` all FOUND; `pub mod pysdk;` in `lib.rs`.
- Commits: `dca6f70`, `392dd98`, `60f70db` all FOUND in git log.
