# Phase 3: Python Target — `PySdk` - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous) — decisions grounded in locked PROJECT/REQUIREMENTS; recommended defaults auto-accepted

<domain>
## Phase Boundary

Generate a **dependency-free Python SDK** from the neutral `ApiGraph` IR — a pure IR→string TWIN of the
Go SDK (`gosdk/`) — and prove it works against the live FastAPI fixture (extracted in Phase 2) via a
hermetic generate-and-run test. This is "pure codegen — no parser problem": the whole upstream pipeline
(extraction → IR → lowering) already exists; this phase adds one new `Target`.

**In scope:**
- A new `crates/gnr8-core/src/pysdk/` module (mirrors `gosdk/`): `emit` (IR→Python via `format!`, NO
  template engine), `bundle` (deterministic multi-file framing à la `gosdk::bundle`), `mod` (`generate`
  + `write_to_dir`). The generated SDK: stdlib `urllib.request` HTTP, `@dataclass` request/response
  models, string-literal-union or `enum`-class enums (match the IR enum shape; pick one, document it),
  a typed `ApiError`, and an injectable `OpenerDirector` (so the HTTP transport is swappable for tests).
- A `PySdk` `Target` built-in in `crates/gnr8-core/src/sdk/builtins.rs` (mirrors `GoSdk`): single source
  of truth for the SDK package/module name; wires into the `Pipeline`.
- A hermetic acceptance test (mirrors `tests/sdk_compile.rs`): build the IR from `fixtures/fastapi-bookstore`
  (via the Phase-2 `pyextract` path) → generate the Python SDK → `write_to_dir` into a unique stdlib
  temp dir → import + compile-check + a real round-trip against a **stdlib `http.server`** fake server
  (NO fastapi/uvicorn/requests/httpx — those are third-party), injecting an `OpenerDirector` pointed at
  the stub. Assert a 2xx dataclass round-trip AND a 4xx → typed `ApiError`. Skip gracefully if `python3`
  is absent (mirror the Go test's toolchain-skip).
- A determinism guarantee: generating twice over the same IR is byte-identical (mirror the Go SDK
  determinism test / `snapshot_sdk` analog).

**Out of scope:** any TS work (Phases 4/5); changing the IR/lowering/extraction (frozen); a Python SDK
snapshot is OPTIONAL (the hermetic run + determinism are the load-bearing acceptance — add a `snapshot_pysdk`
only if it mirrors the Go `snapshot_sdk.rs` pattern cleanly).
</domain>

<decisions>
## Implementation Decisions

### Locked (from PROJECT.md / REQUIREMENTS / STATE — non-negotiable)
- **Dependency-free generated SDK:** stdlib `urllib` (`urllib.request`, `OpenerDirector`), `@dataclass`
  models, typed `ApiError`, injectable opener — exactly like the v1 Go SDK's `net/http`. NO third-party
  HTTP deps (axios/requests/httpx forbidden — REQUIREMENTS Out of Scope).
- **gnr8-core takes ZERO new OSS crates** (rule 2). The `pysdk/` module is pure Rust stdlib + the
  existing in-repo SDK seam. `format!`-based emission, no template engine (mirror gosdk — D-05).
- **IR is the single source of truth** (rule 3/4): the SDK base path + package name come from the
  `PySdk` target config / the graph's `base_path` (the SAME source the OpenAPI lowering uses), never
  re-derived. Pure IR→string twin: no per-language branches added to lowering.
- **Deterministic, byte-identical output** across runs (PYSDK-03).
- **Config is code:** enablement via the `.gnr8/` `PySdk` `Target` built-in, not a data file (rule 4).

### Recommended defaults (auto-accepted; Claude's discretion at plan/exec, guided by RESEARCH)
- **No formatter pass:** the Go SDK pipes through real `gofmt`; Python has no stdlib auto-formatter, so
  `pysdk::emit` produces deterministic, correctly-indented Python DIRECTLY (careful `format!`), with no
  external normalization step. (Do NOT add `black`/`autopep8` — third-party, rule 2.)
- **"Type-checks" acceptance without mypy:** mypy is third-party (rule 2, not available). Prove the SDK
  is sound via: `python3 -m py_compile` of every generated file (syntax) + `import`-clean (`python3 -c
  "import <sdk>"`) + the round-trip test exercising the typed surface. If a deeper static check is
  wanted, use only stdlib (`ast`/`compileall`). Document that "type-checks" = compiles + imports +
  round-trips, given the stdlib-only constraint. (Research to confirm the strongest stdlib-only check.)
- **Enum shape:** match the neutral IR's enum representation to idiomatic Python — prefer `enum.Enum`
  (or `str, Enum`) classes for named enums and `Literal[...]` for inline; mirror whatever the Phase-2
  extractor/lowering already treats as named-vs-inline so the SDK and OpenAPI agree.
- **Module layout of generated SDK:** mirror the Go SDK's file split (client / errors / operations /
  models) adapted to Python (e.g. a single package or a small multi-file bundle), package/module name
  from the `PySdk` target — the single source of truth. Exact split at Claude's discretion.
- **Hermetic server:** Python stdlib `http.server.BaseHTTPRequestHandler` on an ephemeral localhost port
  is the fake backend; the SDK's `OpenerDirector` is injected to hit it. No real FastAPI run.

### Claude's Discretion
Exact `pysdk/` module split, the generated SDK's file layout + class/method shape, the precise
stdlib-only type-check command, and the `PySdk` target's surface — all at Claude's discretion, guided by
the `gosdk/` twin, the `GoSdk` target, and the `tests/sdk_compile.rs` hermetic-test pattern.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets / Analogs (the twin to mirror)
- `crates/gnr8-core/src/gosdk/` (`mod.rs` generate/write_to_dir, `emit.rs` format!-based, `bundle.rs`
  SdkBundle framing, `gofmt.rs` normalize) — `pysdk/` is its structural twin (drop the gofmt analog).
- `crates/gnr8-core/src/sdk/builtins.rs` `GoSdk` `Target` (lines ~410-460) — clone to `PySdk`.
- `crates/gnr8-core/src/sdk/mod.rs` `trait Target` (`generate(&self, ir, out, cx)`) — the seam PySdk impls.
- `crates/gnr8-core/tests/sdk_compile.rs` — the hermetic generate→write→build→smoke pattern to mirror
  (stdlib temp dir, zero-dependency hermetic build, toolchain-skip-if-absent). For Python: py_compile +
  import + stdlib http.server round-trip instead of go build + httptest.
- `crates/gnr8-core/tests/sdk_pipeline.rs` + `snapshot_sdk.rs` — determinism + (optional) SDK snapshot analogs.
- `fixtures/fastapi-bookstore/` + Phase-2 `pyextract` — the IR source for the hermetic round-trip (PYSDK-02).

### Established Patterns
- IR → `Target::generate` → `Artifacts` → `.gnr8/` Pipeline writes files (lifecycle/manifest ownership).
- Generation is byte-identical across runs (determinism test); typed errors, no production panic.

### Integration Points
- New `pysdk/` module + `PySdk` Target in builtins.rs (+ prelude export) + a hermetic test target.
- Python toolchain: sandbox `python3` 3.9.25 (stdlib `urllib`, `http.server`, `dataclasses`, `py_compile`
  all present). NOTE the generated SDK should target a reasonable Python (3.9+) so it imports on the sandbox.

</code_context>

<specifics>
## Specific Ideas

- PySdk is a pure IR→string twin of the Go SDK — same seam, same determinism bar, dependency-free runtime.
- The hermetic test's fake server is Python stdlib `http.server`; the transport is an injected
  `OpenerDirector` — this is how "round-trips, no third-party HTTP deps" is satisfied.
- "type-checks" is bounded by the stdlib-only constraint (no mypy): compiles + imports + round-trips.

</specifics>

<deferred>
## Deferred Ideas

- TypeScript SDK target (`TsSdk`) — Phase 5.
- Cross-language examples + `docs/USAGE.md` envelope — Phase 6.
- SDKs with third-party HTTP deps — permanently out of scope (dependency-free mandate).

</deferred>
