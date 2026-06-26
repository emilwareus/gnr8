# Phase 3: Python Target — `PySdk` - Research

**Researched:** 2026-06-25
**Domain:** IR→Python source codegen (pure `format!` emission, no template engine, no formatter), dependency-free stdlib `urllib` SDK, hermetic stdlib-`http.server` round-trip test
**Confidence:** HIGH (the entire upstream pipeline — extraction, IR, the GoSdk twin, the hermetic-test pattern — exists and was read in full; the only genuinely new ground is Python-specific type mapping + the inline-Union/Enum cases the Go target rejects)

## Summary

This phase is **pure codegen, no parser problem** (CONTEXT line 12). Everything upstream already exists: `analyze::build_graph` already detects Python and runs `pyextract` (verified — `analyze/mod.rs` has a `Lang::Python` arm calling `helper::run_pyextract`), the neutral `ApiGraph` IR is frozen, and the `gosdk/` module is a complete, readable structural template. The job is to clone `gosdk/` → `pysdk/`, clone the `GoSdk` Target → `PySdk` Target, and clone `tests/sdk_compile.rs` → a Python hermetic test — translating Go idioms to Python stdlib idioms at each step.

The single most important finding, which dominates the whole phase: **the FastAPI fixture exercises `Type` variants the Go SDK target explicitly errors on.** `gosdk/emit.rs::go_type` returns `CoreError::SdkGen` for `Type::Union` ("Go has no sum types"), for inline `Type::Object`, and for inline `Type::Enum`. But the bookstore graph (verified in the committed snapshot `snapshot_fastapi_graph__fastapi_graph.snap`) contains a **named union** (`BookOrError = Union[Book, OutOfStock]`), an **inline union field** (`Book.rating: Optional[Union[int, float]]`), and an **inline enum field** (`BookFilters.sort: Literal["asc","desc"]`). The Python SDK MUST handle all of these (Python *does* have sum types via `typing.Union` / `Optional`), so `pysdk/emit.rs` is NOT a mechanical port of `go_type` — it must implement the variants the Go target was allowed to reject. This is the central planning risk and the reason the exhaustive `match Type` (no `_ =>`) discipline matters most here.

Second finding: there is **no Python equivalent of the `gofmt` normalization step**, and that is fine and intended (CONTEXT line 53-55). `gosdk` emits sloppy Go and lets `gofmt` canonicalize indentation; `pysdk` must emit **already-correct, deterministic, significant-whitespace Python directly** from `format!`. Python's indentation is semantic, so emission cannot be lazy about it. This is the second major delta from the twin.

**Primary recommendation:** Build `pysdk/{mod,emit,bundle}.rs` as a structural twin of `gosdk/` minus `gofmt.rs`; emit a single-package multi-file Python SDK (`__init__.py`, `models.py`, `errors.py`, `client.py`) targeting **Python 3.9** (use `Optional[X]` / `Union[A,B]`, NEVER PEP-604 `X | None` — 3.9 in the sandbox); represent named enums as `class X(str, enum.Enum)` and inline enums + unions as `typing` aliases in type hints; clone `GoSdk` → `PySdk` in `builtins.rs`; and write a hermetic test mirroring `sdk_compile.rs` that uses `python3 -m py_compile` + import + a stdlib `http.server` round-trip with an injected `OpenerDirector`, skipping if `python3` is absent.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| IR→Python source emission | `crates/gnr8-core/src/pysdk/emit.rs` (new) | — | Per-target language mapping lives in the target's emitter, exactly as `go_type` lives in `gosdk/emit.rs` (IR-03 / rule 1). Never in lowering. |
| Multi-file framing / determinism | `crates/gnr8-core/src/pysdk/bundle.rs` (new) | `pysdk/mod.rs` | Twin of `gosdk::bundle::SdkBundle`; the marker framing is reused verbatim (comment marker works in Python too). |
| `generate` + `write_to_dir` orchestration | `crates/gnr8-core/src/pysdk/mod.rs` (new) | — | Twin of `gosdk/mod.rs`; pushes files in fixed sorted order, frames into bundle. |
| Pipeline enablement (config-as-code) | `crates/gnr8-core/src/sdk/builtins.rs` `PySdk` (new) | `sdk::prelude` export | Twin of `GoSdk` Target; single source of truth for module/package name + output dir (rule 4). |
| Package/module name derivation | `PySdk` target (`builtins.rs`) | — | Single source of truth (rule 3), same shape as `sdk_package()`. |
| Base path → request URLs | IR `base_path` (read by `pysdk::emit`) | — | SAME `ir.base_path` the OpenAPI lowering + GoSdk use (rule 3/4); never re-derived. |
| Runtime HTTP transport (generated SDK) | generated `client.py` (`urllib.request.OpenerDirector`) | — | Dependency-free; injectable opener is the test seam (twin of Go's `*http.Client`). |
| Hermetic acceptance (compile + import + round-trip) | `crates/gnr8-core/tests/sdk_compile_py.rs` (new) | stdlib `http.server` | Twin of `tests/sdk_compile.rs`; Python toolchain skip-if-absent. |

## Standard Stack

This phase adds **zero** packages to anything (CLAUDE.md rule 2). The "stack" is entirely the existing in-repo seam plus host/target stdlibs.

### Core (already present — verified by reading the source)
| Component | Location | Purpose | Why it's the path |
|-----------|----------|---------|-------------------|
| `gosdk/` module | `crates/gnr8-core/src/gosdk/{mod,emit,bundle,gofmt}.rs` | The structural twin to clone | Complete, idiomatic, exhaustive-match reference (read in full) |
| `GoSdk` Target | `crates/gnr8-core/src/sdk/builtins.rs:403-490` | The Target to clone → `PySdk` | Single-source-of-truth name derivation + `output_anchors` + safe-name guard |
| `trait Target` | `crates/gnr8-core/src/sdk/mod.rs:157-176` | The seam `PySdk` implements | `generate(&self, ir, out, cx)` + `output_anchors()` |
| `Artifacts` | `crates/gnr8-core/src/sdk/mod.rs:78-121` | Sorted output sink targets write to | `out.write(path, text)`, binary-searched sorted insert (determinism) |
| `analyze::build_graph` | `crates/gnr8-core/src/analyze/mod.rs:125` | IR from a Python dir (already routes to `run_pyextract`) | Language auto-detect; the FastApi Source already wraps it |
| `ApiGraph` / `Type` | `crates/gnr8-core/src/graph/mod.rs`, re-exporting `analyze::facts::{Type,Prim,WellKnown,Field}` | The neutral IR `pysdk` consumes | Closed `Type` enum → exhaustive match (rule 3) |
| `CoreError::SdkGen` | `crates/gnr8-core/src/error.rs:92-96` | The typed error for un-representable facts | **Reuse as-is** — owned `message` shape. No `error.rs` edit needed for emission. |

### Supporting (host stdlib — Rust)
| Component | Purpose | When to use |
|-----------|---------|-------------|
| `std::fmt::Write` + `format!`/`writeln!` | Source emission | Every emitter (mirror `gosdk/emit.rs`; fold `fmt::Error` to `SdkGen` via a `sink()` helper) |
| `std::collections::BTreeSet` | Sorted/deduped import sets | If `pysdk` computes imports (see Pitfall 4 — likely a small fixed set) |
| `std::process::Command` | Spawning `python3` in the test | Hermetic test only (mirror `run_go`) |
| `std::env::temp_dir` + PID + nanos | Unique hermetic temp dir | Test only (mirror `unique_temp_dir`, threat T-03-03) — NO `tempfile` crate |

### Supporting (target stdlib — generated Python, all verified present on sandbox 3.9.25)
| Module | Purpose | Notes |
|--------|---------|-------|
| `urllib.request` (`Request`, `OpenerDirector`, `build_opener`, `HTTPError`) | HTTP transport | The Go `net/http` twin; `OpenerDirector` is the injectable transport |
| `urllib.parse` (`urlencode`, `quote`) | Query encoding + path-segment escaping | Twin of Go `url.Values` / `url.PathEscape` |
| `json` | Marshal request body / decode response | Twin of Go `encoding/json` |
| `dataclasses` (`@dataclass`, `field`, `asdict`) | Request/response models | Twin of Go structs |
| `enum` (`class X(str, Enum)`) | Named enums | Twin of Go `type X string` newtype + consts |
| `typing` (`Optional`, `Union`, `List`, `Dict`, `Any`, `Literal`) | Type hints | **3.9-safe spellings only** (see Open Q1) |
| `py_compile` / `compileall` | Syntax verification in the test | The "type-check" proxy (see Open Q2) |
| `http.server` (`BaseHTTPRequestHandler`, `HTTPServer`) + `threading` | Hermetic fake backend | Twin of Go `httptest.NewServer` |

**Installation:** None. `npm install` / `pip install` / `cargo add` are all forbidden (rule 2). The sandbox `python3` 3.9.25 already has every module above (verified: `python3 -c "import urllib.request, http.server, dataclasses, enum, json, py_compile, compileall, typing"` succeeds).

### Alternatives Considered
| Instead of | Could Use | Tradeoff / Why rejected |
|------------|-----------|-------------------------|
| `class X(str, enum.Enum)` for named enums | `Literal["a","b"]` alias for ALL enums | Rejected for *named* enums: a named graph schema must become a named Python symbol (it is `$ref`'d). Use `Literal` only for *inline* enums (which have no name). See Pitfall 2. |
| `@dataclass` models | `TypedDict` / plain dict | `@dataclass` matches CONTEXT decision (line 41-42) and gives a real typed surface to assert in the round-trip; `TypedDict` is weaker and not requested. |
| `urllib.request` | `http.client` | Both stdlib; `urllib`'s `OpenerDirector` is the explicit CONTEXT-decided injection seam (line 20, 41). |
| Computed import block (like `gosdk`) | Fixed import header per file | Python tolerates unused imports at runtime (unlike `go build`). A fixed header is simpler and deterministic. See Pitfall 4. |
| `__init__.py` re-export package | Single flat `sdk.py` module | A package mirrors the Go multi-file split (CONTEXT line 64) and makes `import <pkg>` clean for PYSDK-02. Both are valid; package recommended. |

**Version verification:** Not applicable — no external packages. Host Rust toolchain and target `python3` are the only dependencies; `python3 --version` = 3.9.25 (verified), `go` is NOT on PATH in this sandbox (verified — so the existing Go SDK tests will skip here, and the new Python test will actually run).

## Package Legitimacy Audit

> Not applicable. This phase installs **no external packages** in any ecosystem (CLAUDE.md rule 2 forbids it; the generated SDK is dependency-free by mandate). slopcheck/registry verification is moot — there is nothing to verify. All "dependencies" are Rust `std`, Go-was-the-twin, and Python stdlib modules shipped with the interpreter.

## Architecture Patterns

### System Architecture Diagram

```
                          ┌──────────────────────────────────────────────┐
  fixtures/fastapi-       │  analyze::build_graph(dir)                     │
  bookstore/ (*.py) ─────►│   detects Lang::Python → helper::run_pyextract│  (EXISTS — Phase 2)
                          │   → facts JSON → ApiGraph::from_facts          │
                          └───────────────────────┬──────────────────────┘
                                                   │ &ApiGraph (frozen, sorted IR)
                                                   │ + ir.base_path (single source of truth)
                                                   ▼
   .gnr8/ Pipeline ──► PySdk::generate(ir, out, cx) ──┐         (NEW — builtins.rs twin of GoSdk)
   (config-as-code)     │ derive package name (1 src) │
                        ▼                             ▼
              ┌─────────────────────────────────────────────────┐
              │  pysdk::generate(ir, package, base_path)         │  (NEW — twin of gosdk::generate)
              │   ┌───────────────────────────────────────────┐ │
              │   │ emit_models  → models.py  (@dataclass +    │ │  ◄── EXHAUSTIVE match Type:
              │   │                            enum.Enum)      │ │       handles Union + inline
              │   │ emit_errors  → errors.py  (ApiError)       │ │       Enum/Object that GoSdk
              │   │ emit_client  → client.py  (OpenerDirector) │ │       REJECTS
              │   │ emit_ops     → client.py methods           │ │
              │   │ emit_init    → __init__.py (re-exports)    │ │
              │   └───────────────────────────────────────────┘ │
              │   NO gofmt step — emit() produces correct        │
              │   significant-whitespace Python directly         │
              │   → SdkBundle (marker-framed, deterministic)     │
              └───────────────────────┬─────────────────────────┘
                                      │ split_bundle / write_to_dir
                                      ▼
                         out.write("<dir>/<name>.py", contents)   → Artifacts (sorted)

  ── HERMETIC TEST (tests/sdk_compile_py.rs, twin of sdk_compile.rs) ──────────────
   build_graph(fixture) → pysdk::generate → write_to_dir(unique temp dir)
      → python3 -m py_compile <each .py>        (a) SYNTAX gate
      → python3 -c "import <pkg>"               (b) IMPORT gate
      → spawn stdlib http.server in a thread, build OpenerDirector → inject into
        generated Client(base_url, opener=...) → call an op:
          2xx → assert decoded @dataclass round-trip
          4xx → assert raised ApiError(status_code=...)   (c) ROUND-TRIP gate
   skip-if python3 absent (early return), mirror Go test's go_available()
```

### Recommended Project Structure

```
crates/gnr8-core/src/pysdk/        # NEW — structural twin of gosdk/ (minus gofmt)
├── mod.rs                         # generate() + write_to_dir() + split_bundle()  (twin of gosdk/mod.rs)
├── emit.rs                        # format!-based IR→Python emitters             (twin of gosdk/emit.rs)
└── bundle.rs                      # SdkBundle marker framing                      (twin of gosdk/bundle.rs)
#  NO gofmt.rs — Python has no stdlib formatter; emit produces correct whitespace directly

crates/gnr8-core/src/sdk/builtins.rs   # ADD `PySdk` Target struct (twin of GoSdk ~line 403-490)
crates/gnr8-core/src/sdk/mod.rs        # ADD `PySdk` to prelude re-export (line 337-340)
crates/gnr8-core/src/lib.rs            # ADD `pub mod pysdk;` (mirror `pub mod gosdk;` line 10)

crates/gnr8-core/tests/sdk_compile_py.rs   # NEW — hermetic test (twin of sdk_compile.rs)
crates/gnr8-core/tests/snapshot_pysdk.rs   # OPTIONAL — only if it cleanly mirrors snapshot_sdk.rs
```

Generated SDK on disk (the artifact, written into a temp dir / `generated/sdk-py/`):
```
<dir>/
├── __init__.py     # re-exports Client, ApiError, every model + enum (clean `import <pkg>`)
├── models.py       # @dataclass per object schema; class X(str, Enum) per named enum
├── errors.py       # ApiError(Exception) — status_code/message/slug/hints + is_not_found()
└── client.py       # Client (OpenerDirector-injectable) + one method per operation
```

### Pattern 1: Exhaustive `Type` match that handles MORE than the Go target
**What:** `pysdk::emit` must map every `Type` variant to a Python type hint. Unlike `go_type`, it must NOT error on `Union`, inline `Enum`, or (where reachable) inline `Object`.
**When to use:** the `py_type(&Type, optional, nullable, graph) -> Result<String, CoreError>` function (twin of `go_type`).
**Mapping (the load-bearing table):**

| `Type` variant | Go target (`go_type`) | Python target (`py_type`) — REQUIRED |
|----------------|-----------------------|--------------------------------------|
| `Primitive(String)` | `string` | `str` |
| `Primitive(Bool)` | `bool` | `bool` |
| `Primitive(Int{..})` | `int64` | `int` |
| `Primitive(Float{..})` | `float32` | `float` |
| `Primitive(Bytes)` | `[]byte` | `bytes` |
| `WellKnown(DateTime)` | `time.Time` | `str` (RFC-3339 wire string; do NOT import `datetime` for dataclass simplicity unless planned) |
| `WellKnown(Uuid/Date/Duration/Decimal/Email/Uri)` | `string` | `str` |
| `Array(T)` | `[]T` | `List[<py_type(T)>]` (3.9: `List`, not `list[...]` in annotations is fine either way at runtime, but `List` is safest for forward-ref/`from __future__`) |
| `Map{..}` | `map[string]any` | `Dict[str, Any]` |
| `Named(ref)` | resolve → exported name, `*T` if nullable value | the schema's `name`; wrap `Optional[Name]` if nullable |
| `Object(fields)` | **ERROR** (inline unsupported) | reachable? In the fixture, inline objects do not appear as fields — but handle defensively: either a typed error (matches Go) OR `Dict[str, Any]`. **Recommend: typed error**, identical to Go, unless plan finds a reachable inline object. |
| `Enum(members)` | **ERROR** (inline unsupported) | **MUST handle** — `BookFilters.sort` is an inline enum. → `Literal["asc", "desc"]` (members in graph-sorted order). |
| `Union(variants)` | **ERROR** (Go has no sum types) | **MUST handle** — `Book.rating` (inline) and `BookOrError` (named) are unions. → `Union[<py_type(v) for v in variants>]` |
| `Any{}` | `map[string]any` | `Any` |

**Optional vs nullable (carry the two axes independently — same discipline as `gosdk`):**
- `nullable` (value may be `None`) → wrap the hint in `Optional[...]`.
- `optional` (key may be absent) → give the dataclass field a default (`= None` or `field(default=...)`). In a `@dataclass`, fields with defaults must come AFTER fields without — see Pitfall 1.
- These are distinct (graph test `field_nullable_axis_is_carried_distinctly_from_optional` proves the IR carries both). A field can be optional-not-nullable (`tags`, `in_stock`), nullable-not-optional (`Author.bio`, `published`, `next_cursor`), both (`rating`, `sort`), or neither.

### Pattern 2: Named enum → `class X(str, enum.Enum)`; inline enum → `Literal[...]`
**What:** Mirror the OpenAPI/extractor named-vs-inline distinction (CONTEXT line 61-63).
**When to use:** `emit_models` enum arm + `py_type` Enum arm.
**Example (named enum, in `models.py`):**
```python
# Source pattern: graph Schema{ name: "BookFormat", body: Type::Enum(["hardcover","paperback"]) }
# Members are graph-sorted (verified: snapshot shows hardcover before paperback).
class BookFormat(str, enum.Enum):
    HARDCOVER = "hardcover"
    PAPERBACK = "paperback"
```
The `str` mixin makes `json.dumps` serialize the value as the string (no custom encoder needed) — the Python twin of Go's `type X string` newtype. The member identifier is the `exported()`-style upper-snake of the value; reuse the `exported`/`split_words` logic conceptually but emit `SCREAMING_SNAKE` for enum members.

**Example (inline enum, in a type hint — `BookFilters.sort`):**
```python
sort: Optional[Literal["asc", "desc"]] = "asc"   # inline enum stays inline (no named class)
```

### Pattern 3: Dependency-free client with injectable `OpenerDirector`
**What:** The generated `Client` holds a `base_url`, an optional API key, and an `OpenerDirector` (defaulting to `urllib.request.build_opener()`), so the test can inject an opener pointed at the stub.
**When to use:** `emit_client` + the per-operation method emitter.
**Example (the transport skeleton — `client.py`):**
```python
# Twin of Go's NewClient(baseURL, WithHTTPClient(...)). The opener is the swappable transport.
class Client:
    def __init__(self, base_url: str, *, api_key: Optional[str] = None,
                 opener: Optional[urllib.request.OpenerDirector] = None) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._opener = opener or urllib.request.build_opener()

    def _do(self, method: str, path: str, *, body: Optional[Any] = None) -> tuple[int, bytes]:
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(self._base_url + path, data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        if self._api_key:
            req.add_header("X-API-Key", self._api_key)
        try:
            with self._opener.open(req) as resp:        # injected transport
                return resp.status, resp.read()
        except urllib.error.HTTPError as e:             # 4xx/5xx land here
            return e.code, e.read()
```
Each operation method then: builds the path (escaping path params with `urllib.parse.quote`), encodes query params (`urllib.parse.urlencode`), calls `_do`, and on non-success raises `ApiError` (decoding the body) — twin of `emit_request_dispatch`.

> **NOTE — 3.9 caveat in the example above:** `tuple[int, bytes]` as an *annotation* works at runtime under `from __future__ import annotations` (annotations become strings, never evaluated). To be maximally safe, emit `from __future__ import annotations` at the top of every generated module OR use `Tuple[int, bytes]` from `typing`. Recommend `from __future__ import annotations` in every file — it makes all annotations lazy strings, sidestepping every 3.9 generic-subscription concern. See Open Q1.

### Pattern 4: Marker framing reused verbatim
The `// ==== gnr8:file <name> ====` marker is a comment in Go. In Python a `#`-comment differs, but the marker does NOT need to be a valid Python comment — `bundle::parse` only splits on the literal marker line and the marker never appears inside emitted Python (just as it never appears in gofmt'd Go). **Recommend: reuse the exact `gosdk::bundle` framing unchanged** (copy `bundle.rs`), keeping the `// ====` marker so the framing code is byte-identical to the proven twin. The marker line lives only in the bundle String, not in the files `write_to_dir` materializes (verified: `bundle::parse` strips marker lines; only inter-marker content is written).

### Anti-Patterns to Avoid
- **Porting `go_type`'s Union/Enum/Object error arms verbatim.** That would make the bookstore fixture fail to generate (`BookOrError`, `rating`, `sort`). The Python target's whole point is that the IR is language-neutral and Python *can* express these. Handle them.
- **PEP-604 `X | None` / `list[int]` bare builtins in annotations.** `X | None` needs Python 3.10; the sandbox is 3.9.25. Use `Optional[X]` / `Union[...]` / `List[...]` from `typing`, and/or `from __future__ import annotations`. (Open Q1.)
- **A `gofmt`-style post-format pass.** No stdlib formatter exists; `black`/`autopep8` are third-party (rule 2). Emit correct indentation directly.
- **Iterating a `HashMap`** anywhere (non-determinism). Consume the graph's already-sorted Vecs in order; use `BTreeSet` if a set is needed (mirror `gosdk`).
- **A second source / fallback for the package name** (rule 3). One derivation in the `PySdk` target.
- **`unwrap`/`expect`/`panic` in `src/`** (RUST-04). Fold `fmt::Error` to `SdkGen` via a `sink()` helper exactly like `gosdk/emit.rs:33`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP in the generated SDK | A socket client | `urllib.request` + `OpenerDirector` | stdlib, dependency-free, injectable for tests (CONTEXT decision) |
| Query/path encoding | Manual string concat | `urllib.parse.urlencode` / `quote` | Correct escaping (twin of Go `url.PathEscape`, threat parity) |
| JSON | A serializer | `json` (stdlib) | dependency-free |
| Models | Hand-written `__init__` | `@dataclass` | stdlib, typed, `asdict` for marshalling |
| Enum classes | Constants | `enum.Enum` with `str` mixin | json-serializable, twin of Go newtype |
| Multi-file framing | New bundle format | Copy `gosdk::bundle` verbatim | proven, deterministic, round-trippable |
| Target seam | New trait | Implement existing `trait Target` | the pipeline already drives it |
| Temp dir in test | `tempfile` crate | `std::env::temp_dir()` + PID + nanos | rule 2 (no crate); mirror `unique_temp_dir` |
| Fake HTTP backend in test | A real FastAPI/uvicorn run | stdlib `http.server.BaseHTTPRequestHandler` | rule 2 + hermetic (no `pip install`, no network) |
| "Type-check" the SDK | Bundle a type checker | `py_compile` + import + round-trip | mypy is third-party + unavailable; this is the strongest stdlib-only proof (Open Q2) |

**Key insight:** This phase is a *translation* exercise against a fully-built, fully-readable Go twin. Almost nothing is genuinely novel except (1) Python type mapping including the sum-type/inline cases Go rejected, and (2) significant-whitespace emission without a formatter. Resist re-deriving anything the twin already solved (framing, determinism, name-safety guard, temp-dir hygiene, skip-if-toolchain-absent).

## Common Pitfalls

### Pitfall 1: `@dataclass` field ordering — non-default fields cannot follow default fields
**What goes wrong:** `@dataclass` raises `TypeError: non-default argument follows default argument` at *class definition time* (i.e. at import) if a field without a default is declared after a field with one. The graph sorts object fields **alphabetically by `json_name`** (verified `normalize_fields` sorts by name), so a model like `BookFilters` (`genre` required, `in_stock` default, `published` required-but-nullable, `sort` default) will interleave defaulted and non-defaulted fields in alphabetical order → guaranteed `TypeError` on import.
**Why it happens:** Python dataclass semantics + alphabetical field order = defaults are NOT contiguous at the end.
**How to avoid:** Either (a) give EVERY field a default (required fields get no semantic default but emitting `field()` with no default still counts as no-default — does NOT help), OR (b) **give every field a default value** so ordering never violates the rule — required-non-nullable fields can default to a sentinel, but that weakens the type. **Recommended:** use `@dataclass` with `kw_only=True`... but `kw_only` is Python **3.10+** (NOT available on 3.9). So on 3.9 the robust fix is: **emit every field with a default** (`= None` for nullable, and for required fields either reorder so non-defaulted come first OR default them). The cleanest 3.9-safe approach: **emit non-defaulted (required) fields first, then defaulted (optional) fields**, regardless of the graph's alphabetical order — i.e. the emitter partitions fields by `optional` before writing them. This is a Python-target presentation concern, not an IR change.
**Warning signs:** import step (gate b) fails with `TypeError: non-default argument follows default argument`. **This will be caught by the import gate — design for it up front.**
**Plan action:** `emit_struct` for Python must sort fields into (required-first, optional-last) order within each class. Document that this reorders relative to the JSON-name sort but does not affect wire behavior (json keys are name-addressed).

### Pitfall 2: Inline enum vs named enum representation drift
**What goes wrong:** Emitting an inline `Literal` as a named class (or vice versa) makes the SDK disagree with the OpenAPI spec and the extractor's named-vs-inline model.
**Why it happens:** Both `BookFormat` (named) and `BookFilters.sort` (inline `Literal`) arrive as `Type::Enum(["..."])` — the variant alone doesn't say named-vs-inline; the *position* does (a top-level `Schema.body` is named; a `Field.schema` / `Param.schema` is inline).
**How to avoid:** In `emit_models`, a `Schema` whose `body` is `Type::Enum` → emit a `class X(str, Enum)`. In `py_type` (used for fields/params/returns), a `Type::Enum` → emit `Literal[...]`. Never the reverse. (This is the exact named-vs-inline split CONTEXT line 61-63 asks for.)
**Warning signs:** the SDK references a `SortOrder` class that the OpenAPI spec has no schema for (the FastAPI OpenAPI snapshot has NO `SortOrder` component — verified the fixture comment says "a `Literal` alias is inline-only and is NEVER a standalone schema").

### Pitfall 3: A string snapshot can look right yet not import/compile (the SDK-05 lesson)
**What goes wrong:** Generated Python looks plausible but fails `py_compile` (IndentationError) or import (the dataclass TypeError, a bad `Optional` spelling, a forward reference to a not-yet-defined class).
**Why it happens:** Significant whitespace + import-time class evaluation make Python *stricter at "compile"* than Go in some ways. `sdk_compile.rs`'s docstring calls this out for Go ("a string snapshot can look correct yet not compile, RESEARCH Pitfall 3").
**How to avoid:** The hermetic test's `py_compile` + import gates ARE the defense. Emit `from __future__ import annotations` to make all annotations lazy strings (kills forward-reference ordering problems between `Author`/`Book`, and makes 3.9 generic spellings irrelevant). Order class definitions so referenced classes precede referencing ones where possible (e.g. `Author` before `Book`); `from __future__ import annotations` makes this non-fatal anyway.
**Warning signs:** gate (a) `py_compile` non-zero exit (syntax/indent), gate (b) import `TypeError`/`NameError`.

### Pitfall 4: Non-determinism from import ordering or set iteration
**What goes wrong:** Two `generate` runs differ byte-for-byte → PYSDK-03 fails.
**Why it happens:** iterating a `HashMap`, or computing an import set in unstable order.
**How to avoid:** Consume graph Vecs in their sorted order (already guaranteed). If computing imports, use `BTreeSet` (mirror `gosdk::query_imports`). **Simplest:** emit a fixed import header per file (Python tolerates unused imports — unlike `go build`), so there is no import computation at all. e.g. `models.py` always emits `from __future__ import annotations`, `from dataclasses import dataclass, field`, `import enum`, `from typing import Optional, Union, List, Dict, Any, Literal`. Deterministic by construction.
**Warning signs:** the determinism test (`generate(ir) == generate(ir)`) fails intermittently.

### Pitfall 5: Hermetic server port races / hangs / leaks
**What goes wrong:** The stdlib `http.server` test server binds a fixed port (collision) or blocks the test thread.
**Why it happens:** `HTTPServer(("localhost", 8080), ...)` hardcodes a port; `serve_forever()` blocks.
**How to avoid:** Bind to port **0** (`HTTPServer(("127.0.0.1", 0), handler)`) → OS assigns a free ephemeral port; read it back via `server.server_address[1]`. Run the server in a `threading.Thread(target=server.serve_forever, daemon=True)`; call `server.shutdown()` + `server.server_close()` in a `finally`. The Rust test drives this by writing a small Python *driver script* (not part of the SDK bundle — mirror how `sdk_compile.rs` writes `smoke_test.go` separately) that imports the generated SDK, spawns the server, injects an opener, exercises 2xx+4xx, and exits non-zero on assertion failure. The Rust side just runs `python3 <driver>.py` and checks the exit code (twin of `run_go(&["test", "./..."])`).
**Warning signs:** test hangs (blocking `serve_forever`), or `OSError: Address already in use`.

### Pitfall 6: `urllib` raises on 4xx instead of returning — the typed-error path
**What goes wrong:** A naive `opener.open(req)` for a 404 raises `urllib.error.HTTPError`, not a normal response, so the SDK must catch it to build `ApiError`.
**Why it happens:** `urllib` treats >=400 as an exception (unlike Go's `Do`, which returns a response with a status).
**How to avoid:** Wrap `opener.open` in `try/except urllib.error.HTTPError as e` and read `e.code` + `e.read()` to populate `ApiError` (shown in Pattern 3). This is the natural Python idiom and the 4xx assertion in the round-trip gate depends on it. The Go SDK compares `resp.StatusCode != success_status`; the Python SDK's success path also needs to verify the 2xx status matches the operation's declared success status (`success_of` twin) when a non-error status that isn't the expected one appears.

## Code Examples

### Hermetic stdlib fake backend + injected opener (the test driver script, written by the Rust harness)
```python
# Source: Python stdlib docs — http.server.HTTPServer / urllib.request.OpenerDirector
# (analogous to tests/sdk_compile.rs's httptest smoke; this is written to a temp .py and run via python3)
import json, threading, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
import <pkg>  # the generated SDK package (import gate also exercised here)

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        # assert method/path/body, then reply 201 with a CreatedMessage-shaped body
        self.send_response(201); self.send_header("Content-Type","application/json"); self.end_headers()
        self.wfile.write(json.dumps({"message":"ok","id":1}).encode())
    def do_GET(self):
        # 404 path → typed ApiError assertion
        self.send_response(404); self.send_header("Content-Type","application/json"); self.end_headers()
        self.wfile.write(json.dumps({"message":"not found","slug":"x"}).encode())
    def log_message(self, *a): pass  # silence

srv = HTTPServer(("127.0.0.1", 0), Handler)         # port 0 → ephemeral (Pitfall 5)
port = srv.server_address[1]
t = threading.Thread(target=srv.serve_forever, daemon=True); t.start()
try:
    opener = urllib.request.build_opener()           # injectable transport
    c = <pkg>.Client(f"http://127.0.0.1:{port}", opener=opener)
    out = c.create_book(<pkg>.Book(...))             # 2xx → @dataclass round-trip
    assert out.id == 1 and out.message == "ok"
    try:
        c.get_book(999)                              # 4xx → typed ApiError
        raise SystemExit("expected ApiError")
    except <pkg>.ApiError as e:
        assert e.status_code == 404 and e.is_not_found()
finally:
    srv.shutdown(); srv.server_close()
```

### Rust harness: run python3, skip if absent, map non-zero to a typed error (twin of `run_go`)
```rust
// Source: crates/gnr8-core/tests/sdk_compile.rs (go_available / run_go / unique_temp_dir patterns)
fn python_available() -> bool {
    std::process::Command::new("python3").arg("--version")
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().is_ok()
}
// py_compile gate (a): python3 -m py_compile <file>...  → non-zero == syntax error
// import gate (b):     python3 -c "import <pkg>"  (with the temp dir on sys.path via PYTHONPATH or cwd)
// round-trip gate (c): python3 <driver>.py        → non-zero == assertion failed
// Each maps a non-zero exit to a captured-stderr failure; spawn failure → skip (mirror go_available()).
```

## State of the Art

| Old (Go SDK / Phase-3-v1) | Current (Python SDK / this phase) | Why it changes |
|---------------------------|-----------------------------------|----------------|
| `gofmt` normalization step | NO formatter; emit correct whitespace directly | No stdlib Python formatter; black/autopep8 third-party (rule 2) |
| `Type::Union` → error | `Type::Union` → `Union[...]` | Python has sum types; fixture has `BookOrError` + `rating` |
| inline `Type::Enum` → error | inline `Type::Enum` → `Literal[...]` | fixture has `BookFilters.sort` |
| `go build ./...` + `httptest` smoke | `py_compile`+import + `http.server` round-trip | Python "compile"=syntax; no mypy (stdlib-only) |
| `*T` pointer for nullable | `Optional[T]` | Python nullability is `Optional`, not pointers |
| `,omitempty` json tag for optional | dataclass field default + (required-first ordering) | Python optionality is field-default presence |

**Deprecated/outdated / explicitly avoided:**
- PEP-604 `X | None` and bare `list[int]` *evaluated* annotations: need 3.10/3.9-runtime-eval respectively — avoid on 3.9 (use `typing` + `from __future__ import annotations`).
- `dataclass(kw_only=True)`: 3.10+ only — cannot use to dodge Pitfall 1 on 3.9.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The generated SDK targets Python **3.9** (must import on sandbox 3.9.25) | Stack / Open Q1 | If the real deployment floor is higher, 3.9-safe spellings are merely conservative (no harm). If lower than 3.9 mattered, `dataclasses`/`Literal` (3.8+) still fine. Low risk. |
| A2 | Named enums → `class(str, Enum)`, inline enums → `Literal`; named unions + inline unions → `Union[...]` | Patterns 1-2 | If the reviewer prefers all-`Literal` or a different enum mixin, output shape changes but acceptance gates still pass. Reconciled with the fixture's own comments (named vs inline) — low risk. |
| A3 | "type-checks" (PYSDK-02) is satisfied by `py_compile` + import + round-trip, since mypy is unavailable/third-party | Open Q2 | CONTEXT line 57-60 explicitly accepts this bounding. Low risk — it is the decided definition. |
| A4 | Reusing the exact `gosdk::bundle` `// ====` marker framing for Python is safe | Pattern 4 | The marker never appears in emitted Python (same as Go); `parse` strips it before writing files. Low risk. |
| A5 | `@dataclass` field-order TypeError is avoided by emitting required fields before optional ones | Pitfall 1 | If missed, the IMPORT gate catches it deterministically — fail-loud, not silent. Low risk because the test surfaces it. |
| A6 | The generated `Client` uses `urllib.request.OpenerDirector` as the injection seam | CONTEXT (locked) + Pattern 3 | Locked by CONTEXT line 20/41. None. |
| A7 | A `WellKnown::DateTime` field maps to `str` (RFC-3339 wire string), not Python `datetime` | mapping table | If `datetime` objects are wanted, json marshalling needs a custom encoder (more complexity). `str` keeps the SDK dependency-free-simple and json-clean. Medium-low — flag for planner to confirm. |

## Open Questions

1. **Python version floor / annotation spelling.**
   - What we know: sandbox `python3` is 3.9.25 (verified). PEP-604 `X | None` is 3.10+; `dataclass(kw_only=True)` is 3.10+; `Literal`/`dataclasses` are 3.7-3.8+.
   - What's unclear: whether the SDK should *declare* a higher floor for end users.
   - Recommendation: **Target 3.9.** Emit `from __future__ import annotations` at the top of every module (makes all annotations lazy strings — sidesteps every 3.9 generic-subscription and forward-ref concern) AND use `typing.Optional/Union/List/Dict/Any/Literal`. The hermetic test on 3.9.25 is the enforcement.

2. **What "type-checks" means with no mypy (PYSDK-02).**
   - What we know: mypy is third-party (rule 2) and not installed; CONTEXT line 57-60 bounds "type-checks" to compiles + imports + round-trips.
   - What's unclear: whether a deeper stdlib-only static check adds value.
   - Recommendation: gates (a) `python3 -m py_compile <each file>` (syntax/indent — the strongest stdlib syntax proof), (b) `python3 -c "import <pkg>"` (executes class bodies → catches dataclass ordering, bad defaults, forward-ref/NameError, bad `Optional` usage), (c) the round-trip driver (exercises the typed surface end-to-end). Optionally add `compileall` over the dir as a belt-and-braces of (a). Do NOT attempt to hand-roll a type checker via `ast` — low value, high effort, not requested. Document in VALIDATION.md that "type-checks ≡ compiles + imports + round-trips" under the stdlib-only constraint.

3. **Snapshot test (`snapshot_pysdk.rs`) — worth it?**
   - What we know: `snapshot_sdk.rs` exists for Go and is tiny (uses `insta::assert_snapshot!`). `insta` is a dev-dependency already used by the crate. The load-bearing acceptance is the hermetic run + determinism (CONTEXT line 33-34 says snapshot is OPTIONAL).
   - What's unclear: marginal value vs. the determinism + hermetic tests.
   - Recommendation: **Add it only if it mirrors `snapshot_sdk.rs` cleanly** (same `build_graph(fixture) → pysdk::generate → assert_snapshot!` shape). It gives a reviewable artifact of the whole SDK and catches accidental output drift the determinism test (which only compares a run to itself) cannot. Low cost if it's a 3-line mirror. Gate it behind `python_available()` (it runs `build_graph` → pyextract). Decision deferred to planner — acceptable either way.

4. **Inline `Type::Object` reachability.**
   - What we know: the fixture has no inline-object field (all objects are named `$ref`s); the Go target errors on inline objects.
   - What's unclear: whether pyextract can ever emit an inline `Type::Object` field.
   - Recommendation: handle the arm explicitly (no `_ =>`) — emit a typed `SdkGen` error matching the Go target, OR map to `Dict[str, Any]`. Recommend the **typed error** (parity with Go, fail-loud) unless the planner finds a reachable case. Either way the match stays exhaustive (rule 3).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain (host) | building/running gnr8-core tests | ✓ (assumed — builds the crate) | per repo toolchain | — |
| `python3` | pyextract (Phase 2, reused) + hermetic test + generated SDK runtime | ✓ | 3.9.25 | test skips if absent (mirror Go) |
| Python stdlib: `urllib.request`, `http.server`, `dataclasses`, `enum`, `json`, `typing`, `py_compile`, `compileall`, `threading` | generated SDK + hermetic test | ✓ | bundled with 3.9.25 | — (all verified importable) |
| `go` / `gofmt` | NOT needed by this phase | ✗ (not on PATH) | — | irrelevant — Go SDK tests will skip; Python tests run |
| mypy / black / pytest / fastapi / uvicorn / requests / httpx | NONE — explicitly forbidden | ✗ | — | by design (rule 2); "type-check" = py_compile+import+round-trip |

**Missing dependencies with no fallback:** None blocking. (`go` absent is fine — this phase is Python-only and `python3` is present.)
**Missing dependencies with fallback:** `python3` — if ever absent, the hermetic test early-returns (mirrors `go_available()` skip), exactly as the Go tests already skip in this `go`-less sandbox.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` (integration tests in `crates/gnr8-core/tests/`); `insta` (dev-dep, already present) for snapshots; Python `py_compile`/`http.server` driven *from* the Rust test as subprocesses |
| Config file | none — standard `cargo test`; no Python test framework (pytest is third-party, forbidden) |
| Quick run command | `cargo test -p gnr8-core pysdk` (unit tests in `pysdk/{emit,bundle,mod}.rs`) |
| Full suite command | `cargo test -p gnr8-core` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PYSDK-01 | Dependency-free SDK: stdlib `urllib`, `@dataclass`, typed `ApiError`, injectable `OpenerDirector` | unit + integration | `cargo test -p gnr8-core --test sdk_compile_py` (asserts no `import requests/httpx`; asserts `OpenerDirector`/`@dataclass`/`ApiError` present) + emit unit tests | ❌ Wave 0 (`tests/sdk_compile_py.rs`, `pysdk/emit.rs` unit tests) |
| PYSDK-02 | Generated SDK imports + "type-checks" (py_compile+import) + round-trips vs FastAPI fixture hermetically, no 3rd-party HTTP | integration | `cargo test -p gnr8-core --test sdk_compile_py` — gates (a) py_compile, (b) import, (c) http.server round-trip 2xx+4xx; skip if `python3` absent | ❌ Wave 0 |
| PYSDK-03 | `PySdk` Target built-in; byte-identical deterministic output | unit + integration | `cargo test -p gnr8-core pysdk::` (determinism: `generate(ir)==generate(ir)`) + `--test sdk_pipeline` Python analog (PySdk in a Pipeline) | ❌ Wave 0 (determinism test in `pysdk/mod.rs`; optional `snapshot_pysdk.rs`) |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core pysdk` (fast: unit emit/bundle/determinism tests, no subprocess)
- **Per wave merge:** `cargo test -p gnr8-core` (includes the hermetic `sdk_compile_py` integration test — runs `python3`)
- **Phase gate:** full `cargo test -p gnr8-core` green (with `python3` present so the hermetic gates actually execute, not skip) before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/gnr8-core/tests/sdk_compile_py.rs` — covers PYSDK-01, PYSDK-02 (py_compile + import + round-trip gates; `python_available()` skip; `unique_temp_dir`; the Python driver-script writer)
- [ ] Unit tests in `crates/gnr8-core/src/pysdk/emit.rs` — type-mapping per `Type` variant incl. Union/inline-Enum (the cases Go rejects), enum class shape, dataclass field-order (required-first)
- [ ] Unit tests in `crates/gnr8-core/src/pysdk/mod.rs` — four-file markers present + determinism (`generate==generate`), mirror `gosdk/mod.rs` tests but WITHOUT a toolchain skip (pure string emission needs no `python3`)
- [ ] Unit tests in `crates/gnr8-core/src/pysdk/bundle.rs` — round-trip framing (copy `gosdk/bundle.rs` tests)
- [ ] `PySdk`-unconfigured-errors test in `sdk/builtins.rs` tests (mirror `targets_error_when_unconfigured`)
- [ ] OPTIONAL `crates/gnr8-core/tests/snapshot_pysdk.rs` — only if it mirrors `snapshot_sdk.rs` cleanly (Open Q3)
- [ ] No framework install needed (cargo + python3 already present)

## Security Domain

> `security_enforcement: true`, ASVS level 1. This phase generates code and spawns subprocesses; the relevant surface is subprocess/temp-dir hygiene and not emitting injectable code — NOT auth/session (the SDK *carries* an API key header but defines no auth logic).

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | SDK forwards an `X-API-Key` header it is given; defines no auth |
| V3 Session Management | no | stateless SDK |
| V4 Access Control | no | n/a |
| V5 Input Validation / Output Encoding | yes | (1) **Path-param injection** in generated URLs → escape every interpolated path value with `urllib.parse.quote` (twin of Go `url.PathEscape`, WR-04). (2) **Bundle frame-name traversal** → reuse the `gosdk` safe-name guard (reject `/`, `\`, `..`) in both `pysdk::write_to_dir` and the `PySdk` target (verified pattern at `builtins.rs:469`). (3) Strict JSON decode of responses. |
| V6 Cryptography | no | no crypto emitted |
| V12 Files & Resources | yes | **Temp-dir hygiene** in the hermetic test: unique dir from `temp_dir()` + PID + nanos, no user-supplied path component (T-03-03, mirror `unique_temp_dir`); best-effort cleanup. |
| V13/V14 (subprocess / config) | yes | **Subprocess safety**: spawn `python3` with **discrete args**, NO shell string, NO untrusted interpolation into argv (mirror `gofmt.rs` T-03-02-SC and `run_go` T-03-03-01). The driver-script content is program-generated; write it to a file and pass the path, never `-c "<interpolated user data>"`. |

### Known Threat Patterns for {Rust host emitting Python + spawning python3}
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via crafted bundle file name | Tampering | Reuse `gosdk` name guard (reject empty/`/`/`\`/`..`) in `write_to_dir` + `PySdk` target |
| URL/path injection via path-param value in generated SDK | Tampering | Emit `urllib.parse.quote(value)` around every interpolated path segment |
| Command injection when spawning `python3` | Tampering/EoP | Discrete `Command::args`, no shell, program-controlled argv (twin of `gofmt.rs`/`run_go`) |
| Temp-dir predictability / collision in test | Tampering/DoS | PID+nanos unique dir under `temp_dir()`; bind test server to ephemeral port 0 |
| Network reach during "hermetic" test | (loop-safety/hermeticity) | stdlib `http.server` on localhost only; NO pip install, NO real FastAPI run, NO module proxy (the Python analog of Go's `GOPROXY=off`) |
| Re-ingesting generated `*.py` as source | (loop safety) | `PySdk::output_anchors()` returns the output dir (twin of `GoSdk`); the pipeline excludes it from analysis |
| Production `unwrap`/`expect`/`panic` | (robustness) | Fold every `fmt::Error`/IO error to `CoreError::SdkGen`/`Io`; scoped `#[allow(clippy::unwrap_used,...)]` in `#[cfg(test)]` only (mirror twin) |

## Sources

### Primary (HIGH confidence)
- `crates/gnr8-core/src/gosdk/{mod,emit,bundle,gofmt}.rs` — the structural twin, read in full (generate/write_to_dir, `go_type` exhaustive match incl. the Union/inline-Enum/inline-Object ERROR arms the Python target must instead handle, marker framing, gofmt-as-subprocess pattern + why pysdk omits it)
- `crates/gnr8-core/src/sdk/builtins.rs` — `GoSdk` Target (lines 403-490) to clone; `OpenApi31` target; `sdk_package` name derivation; `FastApi`/`Flask` Source (already wrap `build_graph`); the name-safety guard; the `targets_error_when_unconfigured` test
- `crates/gnr8-core/src/sdk/mod.rs` — `trait Target`, `Artifacts` (sorted write), `Cx`, `Pipeline`, prelude re-export site
- `crates/gnr8-core/tests/sdk_compile.rs` — the hermetic generate→write→build→smoke pattern to mirror (`go_available` skip, `unique_temp_dir`, `run_go` typed-error mapping, separate smoke-test file written by harness, `GOPROXY=off` hermeticity)
- `crates/gnr8-core/src/graph/mod.rs` + `crates/gnr8-core/src/analyze/facts.rs` — the `Type`/`Prim`/`WellKnown`/`Field` vocabulary (closed enums; optional/nullable axes; sorted collections / determinism)
- `crates/gnr8-core/src/analyze/mod.rs` — `build_graph` already detects `Lang::Python` and runs `run_pyextract` (no new extraction work)
- `crates/gnr8-core/tests/snapshots/snapshot_fastapi_graph__fastapi_graph.snap` — **verified the fixture produces a named `Type::Union` (`BookOrError`), an inline union field (`Book.rating`), an inline enum field (`BookFilters.sort: Literal`), and a named enum (`BookFormat`)** — the cases driving the core finding
- `fixtures/fastapi-bookstore/app/{main.py,models.py}` — the IR source (routes + the optional×nullable matrix + enum/union/array cases)
- `crates/gnr8-core/src/error.rs` — `CoreError::SdkGen` (reuse; no edit needed) + `PythonToolchainMissing`
- `CLAUDE.md` — rules 1-4 (no tool coupling, no OSS deps, no fallback, config-as-code)
- Tool checks: `python3 --version` = 3.9.25; stdlib import probe (all present); `go version` absent

### Secondary (MEDIUM confidence)
- `crates/gnr8-core/src/lower/{mod,model,yaml}.rs` (grep) — confirms unions lower to `oneOf` and are legitimate named components (parity context: OpenAPI handles unions; the Go SDK is the outlier that rejects them)
- `crates/gnr8-core/tests/sdk_pipeline.rs`, `snapshot_sdk.rs`, `determinism.rs` — determinism/snapshot/skip analogs

### Tertiary (LOW confidence)
- None. Every claim is grounded in the codebase or a direct tool check; no WebSearch was needed (this is a closed, in-repo translation task).

## Project Constraints (from CLAUDE.md)

- **Rule 1 (no tool coupling):** `pysdk` derives every fact from the neutral IR (which derives from the source's own types). No annotation/sidecar-format reading. (Inherent — the SDK consumes `ApiGraph` only.)
- **Rule 2 (no OSS deps):** Zero new Rust crates in gnr8-core. Generated Python SDK uses stdlib only (`urllib`/`json`/`dataclasses`/`enum`/`typing`). Hermetic test uses Python stdlib only (`http.server`, `py_compile`) — NO fastapi/uvicorn/requests/httpx/mypy/black/pytest. NO `tempfile` crate (use `std::env::temp_dir`). Prefer hand-rolled in-repo code (the emitters are `format!`, no template engine).
- **Rule 3 (one source of truth / no fallback):** Exhaustive `match Type` with NO `_ =>` arm (a new variant must fail to compile until handled). Package name + base path from ONE source (the `PySdk` target config / `ir.base_path`) — never re-derived, no try-A-then-B.
- **Rule 4 (config-as-code):** Enablement is the `PySdk` Target built-in (a Rust value composed in `.gnr8/`), never a data file.
- **Determinism (PYSDK-03):** Identical input ⇒ byte-identical output. Consume sorted Vecs; `BTreeSet` for any set; fixed import headers; no `HashMap` iteration.
- **No production `unwrap`/`expect`/`panic` (RUST-04):** Typed `CoreError`; fold `fmt::Error`→`SdkGen` via a `sink()` helper; scope `#[allow(clippy::unwrap_used,...)]` to `#[cfg(test)]` only.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — entirely in-repo/stdlib, all read or probed directly; nothing to fetch.
- Architecture (twin mapping): HIGH — `gosdk/` read in full; the delta (no gofmt; Union/inline-Enum handling; Optional vs pointer) is precisely identified against the committed FastAPI graph snapshot.
- Pitfalls: HIGH — dataclass field ordering (Pitfall 1), 3.9 annotation spelling (Open Q1), urllib-raises-on-4xx (Pitfall 6), and port-0 hermeticity (Pitfall 5) are concrete and verified against Python 3.9.25 semantics + the existing Go test.
- Type mapping incl. sum types: HIGH — driven by the actual fixture graph snapshot, not assumption.

**Research date:** 2026-06-25
**Valid until:** ~2026-07-25 (stable — no fast-moving external deps; only invalidated by IR/lowering or fixture changes, both frozen for this milestone)
