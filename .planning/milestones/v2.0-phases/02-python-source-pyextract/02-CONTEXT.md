# Phase 2: Python Source — `pyextract` - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous) — decisions grounded in locked PROJECT/REQUIREMENTS decisions; recommended defaults auto-accepted

<domain>
## Phase Boundary

Build a static `pyextract` sidecar that turns a real **FastAPI** service (full) and a **Flask** service
(typed envelope) into the neutral JSON facts contract from Phase 1 — so the existing Rust lowering →
OpenAPI 3.1 pipeline produces OpenAPI for Python services unchanged. The Phase-1 `fixtures/fastapi-bookstore/`
and `fixtures/flask-bookstore/` red-by-design snapshots are the acceptance contract this phase turns GREEN
(at least the FastAPI graph+OpenAPI snapshots; Flask to the honest typed-envelope limit).

**In scope:**
- A stdlib-`ast` `pyextract` sidecar (new top-level `pyextract/` dir, analogous to `goextract/`) that
  emits the SAME neutral facts JSON on stdout — NEVER imports or executes the target code.
- An owned cross-module symbol table that statically resolves names/types (Pydantic `BaseModel`,
  `@dataclass`, enums, `Optional`/`Union`/`Literal`, `list[T]`, nested models) WITHOUT import/exec.
- FastAPI recognition: routes (`@app.<method>` / `APIRouter` + `prefix=`), path/query params (signature +
  `Path()`/`Query()`), request bodies (Pydantic/`@dataclass` params), `response_model=`, `status_code=`.
- Flask recognition: routes (`@app.route` / `Blueprint` + `url_prefix=`, `methods=[...]`), opt-in typed
  DTOs/returns. Honest limits — untyped `request.json`/dynamic prefixes → diagnostics, not guesses.
- Diagnostics for every unresolvable/untyped/foreign surface — there is NO fallback path (rule 3).
- `.gnr8/` `FastApi` / `Flask` `Source` built-ins (in `crates/gnr8-core/src/sdk/builtins.rs`, analogous
  to `GoGin`) wiring the sidecar into the `Pipeline`; a Rust subprocess driver analogous to
  `analyze::helper::run_goextract` (runs `python3` against the target, deserializes neutral facts).

**Out of scope:** the Python SDK target (`PySdk` — Phase 3); any TS work; changing the IR/lowering
(Phase 1 froze the neutral contract — this phase produces facts INTO it, never forks it).
</domain>

<decisions>
## Implementation Decisions

### Locked (from PROJECT.md / REQUIREMENTS / STATE — non-negotiable)
- **stdlib `ast` only**, the Python analog of `go/types`. The sidecar is stdlib-only IN PYTHON (rule 2 /
  XLANG-05) — no pip installs, no `pydantic`/`fastapi`/`flask` imported to introspect.
- **Static-only — never import or execute the target** (importing = executing = a security boundary).
  Resolve types via an OWNED cross-module symbol table built from parsed ASTs.
- **Unresolved → diagnostic, never a fallback** (rule 3): untyped `request.json`, dynamic prefixes,
  foreign/unresolvable types each emit a diagnostic; the fact is omitted, never guessed.
- **Rule 1:** derive every fact from the source's OWN Python constructs (type hints, Pydantic/dataclass
  field types, enum classes). NEVER read marshmallow / `@nestjs/swagger`-style annotations, and NEVER
  consume FastAPI's runtime `/openapi.json` (that requires running the app).
- **One neutral facts contract:** emit the SAME JSON the Rust host deserializes strictly
  (`deny_unknown_fields`); no Python terms leak into the IR. Reuse Phase-1's neutral `Type` vocabulary.
- **Config is code:** Python extraction is enabled via `.gnr8/` `FastApi`/`Flask` `Source` built-ins, not
  a data file (rule 4).

### Recommended defaults (auto-accepted; Claude's discretion at plan/exec time, guided by RESEARCH)
- **Sidecar packaging:** a `pyextract/` top-level dir (mirrors `goextract/`), a pure-stdlib Python package
  runnable as `python3 -m pyextract <target_dir>` (or `python3 pyextract/__main__.py <target_dir>`),
  emitting neutral facts JSON to stdout. No third-party Python deps; works on the sandbox's Python 3.9.
  NOTE: target `list[T]`/`X | Y` PEP-604 syntax may appear in FIXTURE source even if the sidecar itself
  runs on 3.9 — the sidecar parses target syntax via `ast`, it does not execute it, so newer target
  syntax is fine to recognize.
- **Subprocess driver:** a Rust module analogous to `analyze::helper::run_goextract` — typed errors
  (`PythonToolchainMissing` analog), never a panic; resolves the `pyextract/` dir relative to the crate
  like `goextract_dir()` does (carry the v1 compile-time-path tech debt forward, do not worsen it).
- **FastAPI "full" envelope:** decorator-based routes on `FastAPI()`/`APIRouter()` instances, nested
  `APIRouter(prefix=...)` includes, params from the typed function signature + `Query()`/`Path()`
  defaults, request body from Pydantic-`BaseModel`/`@dataclass` parameters, `response_model=` and
  `status_code=`. Document anything not covered as a diagnostic, not a silent gap.
- **Flask "typed envelope":** `@app.route`/`Blueprint.route` with `methods=`, `Blueprint(url_prefix=)`
  static prefixes, opt-in typed DTOs (function annotations / dataclass returns). Untyped bodies, dynamic
  prefixes, `**kwargs` view args → diagnostics. Be honest about the narrower surface (XLANG-03).
- **Symbol table:** owned, built by walking each module's AST once, indexing class/enum/alias defs by
  qualified name, resolving cross-module `from x import Y` statically (no execution). Deterministic,
  sorted output (byte-identical across runs).

### Claude's Discretion
Exact module layout of `pyextract/`, the symbol-table data structures, the diagnostic code taxonomy, and
how FastApi/Flask Source built-ins expose inputs — all at Claude's discretion, guided by the goextract
analog, the Phase-1 facts contract, and the fixtures' acceptance snapshots. Discuss was auto-mode.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets / Analogs
- `goextract/` — the Go sidecar to mirror: a self-contained module emitting neutral facts JSON to stdout,
  invoked as a subprocess. `pyextract/` is its Python twin (stdlib `ast` instead of `go/types`).
- `crates/gnr8-core/src/analyze/helper.rs` (`run_goextract`, `goextract_dir`) — the subprocess driver
  pattern: typed errors, relative dir resolution, JSON capture → strict deserialize.
- `crates/gnr8-core/src/analyze/mod.rs` (`build_graph`) — facts → `ApiGraph`; reuse unchanged (the neutral
  contract is language-agnostic; Python facts flow through the SAME `build_graph`).
- `crates/gnr8-core/src/analyze/facts.rs` — the neutral facts DTO (Phase 1). `pyextract` JSON must match it.
- `crates/gnr8-core/src/sdk/builtins.rs` (`GoGin` `Source`) — the `Source` built-in to clone as
  `FastApi` / `Flask`; wraps the analyze/subprocess driver and feeds the Pipeline.
- `fixtures/fastapi-bookstore/`, `fixtures/flask-bookstore/` — the acceptance fixtures; their Phase-1
  red graph+OpenAPI snapshots become the green target. The hand-authored snapshots already encode the
  exact neutral shape the lowering produces (corrected in Phase-1 code review), so they are an achievable
  target — turning them green proves the extractor produces the right facts.

### Established Patterns
- Source sidecar → neutral JSON facts → `deny_unknown_fields` deserialize → `build_graph` → reused
  lowering/OpenAPI. The whole Rust pipeline downstream of facts is reused, never forked (the v2.0 bet).
- Snapshot-driven acceptance + determinism guard (`tests/determinism.rs`); `make check` green gate.

### Integration Points
- New `pyextract/` dir + new Rust subprocess driver module + `FastApi`/`Flask` Source in builtins.rs.
- The 6 Phase-1 red fixture snapshots: FastAPI ones (graph+OpenAPI) MUST flip to green via real extraction
  this phase; Flask ones flip to the honest typed-envelope limit (document any intentionally-still-red gap).

</code_context>

<specifics>
## Specific Ideas

- `pyextract/` mirrors `goextract/` exactly in role: stdlib-only sidecar, subprocess, neutral facts JSON.
- The Phase-1 fixtures + their (lowering-accurate) snapshots are the spec — turn FastAPI green; Flask to
  its documented typed limit. Determinism (byte-identical) holds for Python facts too.
- Toolchain: sandbox has `python3` 3.9.25 at /usr/bin/python3 (stdlib `ast` present). No installs needed.

</specifics>

<deferred>
## Deferred Ideas

- Python SDK target (`PySdk`) — Phase 3.
- Hono/typed-Express/Fastify Python-or-TS frontends — out of v2.0 (FUT-01..03).
- Importing user code / consuming runtime `/openapi.json` — permanently out of scope (security + rule 1).

</deferred>
