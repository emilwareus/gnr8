---
phase: 02-python-source-pyextract
plan: 03
subsystem: api
tags: [python, pyextract, fastapi, routes, ast, snapshot, lowering, static-analysis]

# Dependency graph
requires:
  - phase: 02-python-source-pyextract (Plan 01)
    provides: "host seam — run_pyextract() spawns python3 -m pyextract, detect_language() Python dispatch, build_graph -> from_facts -> to_openapi pipeline"
  - phase: 02-python-source-pyextract (Plan 02)
    provides: "pyextract CORE — load.py/symtab.py/types.py/schemas.py/facts.py; the 8 FastAPI schema ids byte-match the snapshot"
provides:
  - "pyextract/routes.py — FastAPI @app/@router route recognition (method, group-relative path, params, body, response_model/status_code)"
  - "APIRouter(prefix=) recorded separately, NEVER folded into the code-derived path (rule 1)"
  - "FastAPI graph + openapi snapshots GREEN through real extraction (zero snapshot edits) and in the make-check gate"
  - "reconciled fastapi-bookstore fixture whose AST anchors land on the snapshot's asserted lines"
  - "lower_named_schema handles a Union-bodied NAMED schema -> oneOf component (Python sum types as refs)"
  - "FastAPI determinism twins (build_graph + to_openapi byte-identical across runs)"
affects: [fastapi-green, python-snapshot-flip, flask-routes, lowering-unions]

# Tech tracking
tech-stack:
  added: []  # rule 2: CPython stdlib ONLY (ast); zero packages. Rust: std + existing serde DTOs.
  patterns:
    - "FastAPI route recognition is STATIC + by NAME (rule 1): @<router>.<verb>(...) decorator on a def/async def whose <router> is a module-level FastAPI()/APIRouter() binding; nothing reads /openapi.json"
    - "Group-relative path discipline (goextract analog): APIRouter(prefix=) is recorded separately, never folded — snapshot base_path stays '/'"
    - "Span anchors: route span = handler def line (FunctionDef.lineno, NOT decorator); param span = the param's own signature line; schema span = ClassDef line; alias = Assign line — one consistent rule"
    - "Snapshot-as-spec: the committed .snap is the byte-exact target; the FIXTURE source lines are reconciled to it (rule 1 — pure source positions, never read from a docstring), never the reverse"
    - "A Union alias is a legitimate NAMED OpenAPI component -> oneOf (reused lowering); Object/Enum/Union are the valid component bodies"

key-files:
  created:
    - pyextract/routes.py
    - pyextract/tests/test_routes.py
    - pyextract/tests/test_fastapi_golden.py
  modified:
    - pyextract/__main__.py
    - pyextract/tests/test_facts.py
    - fixtures/fastapi-bookstore/app/main.py
    - fixtures/fastapi-bookstore/app/models.py
    - crates/gnr8-core/src/lower/mod.rs
    - crates/gnr8-core/tests/snapshot_fastapi_graph.rs
    - crates/gnr8-core/tests/snapshot_fastapi_openapi.rs
    - crates/gnr8-core/tests/determinism.rs
    - crates/gnr8-core/src/analyze/mod.rs   # cargo fmt only (pre-existing fmt debt)
    - crates/gnr8-core/src/sdk/builtins.rs  # cargo fmt only (pre-existing fmt debt)
    - crates/gnr8-core/src/sdk/mod.rs       # cargo fmt only (pre-existing fmt debt)

key-decisions:
  - "response_model= resolves through a schema-ref resolver that accepts a class OR a Union alias (BookOrError is a Union alias that schemas.py emits as a standalone schema, so it is a valid response ref); a Literal alias is inline-only and never a ref (rule 3)"
  - "A request_body parameter is the FIRST param whose annotation resolves to a model/@dataclass class and is not a path param; path/query params come from the typed signature (path if name in {template}, else query; query required = no default)"
  - "lower_named_schema gained a Type::Union arm delegating to lower_schema_type (oneOf component). Go never emits a union-bodied named schema, so the arm is inert there; the FastAPI BookOrError response needs it"
  - "Fixture line reconciliation was achieved ENTIRELY by adjusting blank lines / docstring length in the fixture source (rule 1); ZERO snapshot lines were corrected — the preferred outcome"

patterns-established:
  - "Pattern (route recognition): decorator Attribute(value=Name(router), attr=verb) + typed signature is the single source of every route fact; the prefix is provenance-only"
  - "Pattern (line reconciliation): honest AST-anchor lines + a Python golden test (test_fastapi_golden.py) that asserts produced line == snapshot line, guarding equality independently of the Rust harness"
  - "Pattern (union component): a sum-type alias is a first-class named component; the same oneOf lowering serves inline and named positions"

requirements-completed: [PYSRC-01]

# Metrics
duration: ~40min
completed: 2026-06-25
---

# Phase 2 Plan 3: FastAPI Route Recognition + Snapshot Flip Summary

**`pyextract/routes.py` recognizes the full FastAPI envelope — `@app`/`@router` decorator routes, group-relative paths with the `APIRouter(prefix=)` recorded separately (never folded, rule 1), path/query params from the typed signature, Pydantic/`@dataclass` request bodies, and `response_model=`/`status_code=` responses (including a `Union`-alias response) — and the two committed FastAPI snapshots (`snapshot_fastapi_graph`, `snapshot_fastapi_openapi`) flip from `#[ignore]` red to GREEN through REAL extraction with ZERO snapshot edits, after reconciling the fixture's source lines to the snapshot's asserted anchors and teaching the reused lowering to emit a union-bodied named component.**

## Performance

- **Duration:** ~40 min
- **Started / Completed:** 2026-06-25
- **Tasks:** 3 (all `type=auto`)
- **Files created:** 3; **modified:** 11 (3 of which are `cargo fmt`-only on pre-existing debt)

## Accomplishments
- `pyextract/routes.py`: static FastAPI route recognition. Module-level `FastAPI()`/`APIRouter(prefix=)` bindings are indexed; every `def`/`async def` carrying an `@<router>.<verb>(...)` decorator becomes a RouteFact with EXACTLY the host DTO key set.
- Path/query split from the typed signature: a name in the `{template}` is a required `path` param (int->int64); otherwise a `query` param whose required-ness comes from the presence of a default. `Optional[BookFormat]` resolves to a `named` ref (OQ3).
- `request_body` = the first model/`@dataclass`-typed parameter; `response_model=` (class OR Union alias) drives the response body ref; `status_code=` drives status (default 200). `create_book` emits `201`.
- The four routes (`list_books`/`create_book`/`get_book`/`update_book`) reproduce the snapshot's methods/paths/params/bodies/responses byte-for-byte, including the `BookFormat` named-ref query param and the `BookOrError` union response.
- Fixture line reconciliation (`main.py` + `models.py`): every route `def` line, param signature line, schema `ClassDef` line, and the `BookOrError` `Assign` line now lands on the snapshot's asserted line — by adjusting blank lines / docstring length only (rule 1), guarded by `test_fastapi_golden.py`.
- Lowering: `lower_named_schema` gained a `Type::Union` arm (delegating to `lower_schema_type`) so the `BookOrError` named component lowers to `oneOf` — the OpenAPI snapshot needs it; Go never exercises it.
- Both FastAPI snapshots are now in the GREEN gate (`#[ignore]` removed); FastAPI `build_graph` + `to_openapi` determinism twins added; stale `*fastapi*.snap.new` removed; `make check` green.

## Task Commits

1. **Task 1: FastAPI route/param/body/response recognition** — `d81e10b` (feat)
2. **Task 2: Fixture span-line reconciliation + union-bodied named component lowering** — `2733ff3` (fix)
3. **Task 3: Flip FastAPI snapshots GREEN, determinism twins, gate** — `e0a8e34` (test)

**Plan metadata:** (this commit) (docs: complete plan)

## Decisions Made
- **`response_model=` accepts a class OR a `Union` alias.** `BookOrError = Union[Book, OutOfStock]` is emitted by `schemas.py` as a standalone schema, so it is a valid response ref. A `Literal` alias stays inline-only and never a ref (rule 3 — one source).
- **`request_body` = first model/`@dataclass` non-path param.** Path/query params come from the typed signature; the prefix is provenance-only, never folded.
- **Anchor rule pinned explicitly:** route span = handler `def` line (`FunctionDef.lineno`, which points to `def`, NOT the decorator), param span = the param's own line, schema span = `ClassDef` line, alias = `Assign` line — one consistent rule across all nodes.
- **All reconciliation was fixture-side.** ZERO snapshot lines were corrected (the plan's preferred outcome); the committed `.snap` files are byte-unchanged.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical functionality] Union-bodied named schema could not lower to a component**
- **Found during:** Task 2 (running the OpenAPI snapshot against the committed `.snap`).
- **Issue:** `lower_named_schema` rejected `Type::Union` with `"non-object/non-enum body that cannot be a component"`, but the snapshot requires `BookOrError` as a `oneOf` component. The Go fixture has no sum types, so this path was never exercised before Python.
- **Fix:** Added a `Type::Union` arm to `lower_named_schema` delegating to the existing `lower_schema_type` (which already emits `oneOf`), plus a `named_schema_with_union_body_lowers_to_a_one_of_component` unit test. Other named-body errors (array/scalar/ref) still error.
- **Files modified:** `crates/gnr8-core/src/lower/mod.rs`
- **Commit:** `2733ff3`

**2. [Rule 3 - Blocking] Stale `test_facts.test_routes_empty_until_plan_03` assertion**
- **Found during:** Task 1. The Plan-02 end-to-end test asserted `routes == []`; this plan populates routes, making the assertion stale by design.
- **Fix:** Renamed to `test_routes_recognized_for_fastapi`, asserting the four route ids.
- **Files modified:** `pyextract/tests/test_facts.py`
- **Commit:** `d81e10b`

**3. [Rule 3 - Blocking] Pre-existing `cargo fmt` debt blocked `make check`**
- **Found during:** Task 3. `make check` runs `fmt-check` first; it failed on pre-existing un-formatted code in `analyze/mod.rs`, `sdk/builtins.rs`, `sdk/mod.rs` (NOT introduced by this plan) plus my new `determinism.rs` const.
- **Fix:** Ran `cargo fmt` workspace-wide so the gate is green. The out-of-scope files are formatting-only changes (no logic touched).
- **Files modified:** `crates/gnr8-core/src/analyze/mod.rs`, `crates/gnr8-core/src/sdk/builtins.rs`, `crates/gnr8-core/src/sdk/mod.rs`
- **Commit:** `e0a8e34`

**Snapshot-line corrections:** none — all reconciliation was fixture-side (the preferred outcome; Task 2 acceptance).

## Issues Encountered
- `FunctionDef.lineno` for a decorated handler points to the `def` line (not the decorator), which matches the snapshot's route anchor exactly — no special-casing needed.
- Reconciling `models.py` lines required progressive, non-monotonic blank-line deltas; converged with a one-off in-repo Python helper that inserts/removes blank lines per anchor (the helper is not committed — only its output, the reconciled fixture).

## CLAUDE.md Compliance
- **Rule 1 (no coupling to another tool's conventions):** routes derive from FastAPI's own decorator/signature AST; nothing reads `/openapi.json`, pydantic runtime schema, or any annotation dialect. The `APIRouter(prefix=)` is recorded separately, never folded. Fixture lines were reconciled as pure source positions, never read from a docstring.
- **Rule 2 (no third-party deps):** `pyextract/routes.py` imports only `ast` + in-repo modules; the Rust lowering change uses `std` + existing typed DTOs. Zero packages added.
- **Rule 3 (no fallback / one path):** a route with no constant path -> diagnostic + omit; an unresolved response/body name -> the field is omitted, never guessed. A `Literal` alias is never a standalone ref.
- **Static-only (PYSRC-03 / T-static-exec):** grep gate over `pyextract/*.py` for `exec/eval/compile/importlib/__import__/runpy` returns NOTHING; recognition is pure `ast` inspection.
- **Determinism:** two `python3 -m pyextract` runs are byte-identical; the Rust FastAPI determinism twins (graph + openapi) pass.

## Next Phase Readiness
- FastAPI is fully green (both snapshots in the gate; determinism guarded). Plan 04 (Flask) builds the Flask recognizer on the same `routes.py` seam (it currently emits nothing for a non-FastAPI tree — a deterministic empty, no fallback) and flips the two `#[ignore]` Flask snapshots green.
- No blockers. The union-component lowering is now available for any sum-typed named schema a future language emits.

## Self-Check: PASSED

- `pyextract/routes.py`, `pyextract/tests/test_routes.py`, `pyextract/tests/test_fastapi_golden.py` all exist on disk.
- Task commits `d81e10b`, `2733ff3`, `e0a8e34` all present in git history.
- `python3 -m unittest discover -s pyextract/tests` -> 55 passed; static + stdlib grep gates empty.
- `cargo test -p gnr8-core --test snapshot_fastapi_graph --test snapshot_fastapi_openapi` GREEN with NO `--ignored`; committed `.snap` files byte-unchanged (last touched by `37148c5`, a Phase-1 commit).
- `cargo test -p gnr8-core --test determinism` GREEN incl. the two FastAPI twins; Flask + NestJS snapshots remain `ignored` red-by-design; `make check` green.

---
*Phase: 02-python-source-pyextract*
*Completed: 2026-06-25*
