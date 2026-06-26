---
phase: 02-python-source-pyextract
plan: 04
subsystem: api
tags: [python, pyextract, flask, routes, ast, snapshot, diagnostics, static-analysis, typed-envelope]

# Dependency graph
requires:
  - phase: 02-python-source-pyextract (Plan 01)
    provides: "host seam — run_pyextract() spawns python3 -m pyextract; detect_language() Python dispatch; build_graph -> from_facts -> to_openapi pipeline"
  - phase: 02-python-source-pyextract (Plan 02)
    provides: "pyextract CORE — load/symtab/types/schemas/facts; the four Flask dto schema ids byte-match the snapshot"
  - phase: 02-python-source-pyextract (Plan 03)
    provides: "pyextract/routes.py FastAPI recognizer (extended here); fixture line-reconciliation pattern; union-bodied named component lowering"
provides:
  - "pyextract/routes.py — recognize_flask: @bp.route/@app.route + methods= (one route per method), Blueprint(url_prefix=) recorded separately (never folded, rule 1), <int:order_id> converter -> /{order_id} int64 path param"
  - "Method-derived status (Q1): typed POST->201, typed non-POST->200 — a CODE fact, never a docstring"
  - "Untyped-surface diagnostics (rule 3): untyped request.json body, unannotated request.args.get query, missing return annotation -> diagnostic + OMITTED fact (no fallback)"
  - "Flask graph + openapi snapshots GREEN through real extraction (zero snapshot edits) and in the make-check gate"
  - "Reconciled flask-bookstore fixture: every route/param/schema span + the three diagnostics anchor to the snapshot's exact lines (42/69/78)"
  - "Flask determinism twins (build_graph + to_openapi byte-identical across runs)"
affects: [flask-green, python-snapshot-flip, phase-02-complete]

# Tech tracking
tech-stack:
  added: []  # rule 2: CPython stdlib ONLY (ast); zero packages. Rust: std + existing serde DTOs + insta.
  patterns:
    - "Flask route recognition is STATIC + by NAME (rule 1): @<bp>.route(...) on a def whose <bp> is a module-level Flask()/Blueprint() binding; nothing reads marshmallow / a runtime openapi.json"
    - "FastAPI and Flask are PARALLEL deterministic recognizers keyed off their own ctor NAMES; a tree of one shape yields an empty list from the other — detection by source shape, NOT a try-A-fall-back-to-B path (rule 3)"
    - "Method-derived status: a typed handler's status comes from the HTTP method (POST->201, else 200); the docstring is NEVER read (Q1 resolution, rule 1)"
    - "Per-method split: methods=[...] emits one RouteFact per entry; body/query facts are derived once (on the first method) so a diagnostic anchors to its node exactly once (a _NullDiags sink swallows the per-method re-walk)"
    - "Untyped surface -> exact-string diagnostic + omit (rule 3); the message names the operation by HTTP method + code-derived path (both code facts), never a docstring"
    - "Snapshot-as-spec: the committed .snap is the byte-exact target; the FIXTURE source lines are reconciled to it with non-fact blank lines / comments only (rule 1)"

key-files:
  created:
    - pyextract/tests/test_flask_routes.py
    - pyextract/tests/test_flask_golden.py
  modified:
    - pyextract/routes.py
    - pyextract/__main__.py
    - fixtures/flask-bookstore/app/routes.py
    - fixtures/flask-bookstore/app/dto.py
    - crates/gnr8-core/tests/snapshot_flask_graph.rs
    - crates/gnr8-core/tests/snapshot_flask_openapi.rs
    - crates/gnr8-core/tests/determinism.rs
    - .gitignore   # ignore generated __pycache__/*.pyc

key-decisions:
  - "Q1 (Flask 201 with no status_code=) resolved as a CODE fact: status = 201 if method==POST else 200 for a typed handler; an untyped handler emits responses:[] + a diagnostic. The docstring is never read (rule 1)."
  - "Q2 (diagnostic lines 42/69/78) resolved by honest fixture reconciliation: each diagnostic anchors to its precise AST node (the request.args.get('q') read, the untyped def, the request.json read); the fixture grew with non-fact blank lines/comments so those nodes land on the snapshot's asserted lines (rule 1). ZERO snapshot lines corrected."
  - "The body/query walk inspects each handler-body statement once, top to bottom (single deterministic pass): an annotated local reading request.json/T(**request.json) -> request_body $ref; an annotated local reading request.args.get(...) -> a typed query param; a PLAIN assign reading either untyped surface -> diagnostic + omit (rule 3)."
  - "Path params come from the route's <int:...> converters (sorted by name), not the signature — Flask path params are not function-arg-typed in the fixture; the int converter drives int64."

patterns-established:
  - "Pattern (parallel recognizers): recognize_fastapi + recognize_flask run side by side; each fires only on its own router-construct names; the union of their outputs is the route list (no fallback, no precedence)."
  - "Pattern (per-method diagnostic dedup): derive untyped diagnostics on methods[0] only; reuse the same body/query facts for the remaining methods via a no-op _NullDiags sink so a warning is recorded exactly once."
  - "Pattern (axes split across DTOs): the four optional x nullable combinations live across OrderInput (3) + OrderConfirmation.message (the 'nullable only' axis) — the golden test asserts the union of both, matching the snapshot."

requirements-completed: [PYSRC-02, PYSRC-04]

# Metrics
duration: ~35min
completed: 2026-06-25
---

# Phase 2 Plan 4: Flask Typed-Envelope Recognition + Snapshot Flip Summary

**`pyextract/routes.py` now recognizes the HONEST Flask typed envelope — `@bp.route`/`@app.route` with `methods=` (one route per method), `Blueprint(url_prefix=)` recorded separately (never folded, rule 1), the `<int:order_id>` converter lowered to a `/{order_id}` int64 path param, method-derived status (typed POST→201, typed non-POST→200, a CODE fact never read from a docstring), and typed DTO request bodies / response refs — while every UNTYPED surface (raw `request.json`, unannotated `request.args.get(...)`, missing return annotation) emits an exact-string DIAGNOSTIC and OMITS the fact (rule 3, no fallback). Both committed Flask snapshots (`snapshot_flask_graph`, `snapshot_flask_openapi`) flip from `#[ignore]` red to GREEN through REAL extraction with ZERO snapshot edits, after reconciling the fixture so all route/param/schema spans and the three diagnostics anchor to the snapshot's exact lines (42/69/78).**

## Performance

- **Duration:** ~35 min
- **Started / Completed:** 2026-06-25
- **Tasks:** 3 (all `type=auto`)
- **Files created:** 2; **modified:** 8

## Accomplishments
- `recognize_flask(modules, table, diags)`: indexes module-level `Flask()`/`Blueprint()` bindings (with their separately-recorded `url_prefix`), then walks every `def` carrying a `@<bp>.route(...)` decorator and assembles ONE RouteFact per `methods=[...]` entry.
- Path handling: `<int:order_id>` → `/{order_id}` (converter stripped, name braced, `int` → int64 path param); a bare `/` stays `/`; the blueprint prefix is provenance-only, never folded (rule 1) — snapshot `base_path` stays `/`.
- Status (Q1): typed handler → `201 if method==POST else 200`; untyped handler (no resolvable return annotation) → `responses: []` + an untyped-response diagnostic. The docstring is never read.
- Body/query: an annotated local reading `request.json` / `T(**request.json)` → `request_body` $ref; an annotated `request.args.get(...)` local → a typed query param; the unannotated `request.json` / `request.args.get(...)` reads → diagnostics + omitted facts.
- The four routes (`list_orders`/`create_order`/`create_order_raw`/`get_order`) reproduce the snapshot's methods/paths/params/bodies/responses byte-for-byte, including the empty `responses` on the untyped raw route and the three diagnostics with byte-exact message strings.
- Fixture reconciliation (`routes.py` + `dto.py`): every route `def` line, the typed-query AnnAssign line, the converter path-param line, every schema `ClassDef` line, AND the three diagnostic lines (42/69/78) now land on the snapshot's asserted lines — by inserting non-fact blank lines / comments only (rule 1), guarded by `test_flask_golden.py`.
- Both Flask snapshots are now in the GREEN gate (`#[ignore]` removed); Flask `build_graph` + `to_openapi` determinism twins added; stale `*flask*.snap.new` removed; `make check` fully green.

## Task Commits

1. **Task 1: Flask recognition (prefix, converter, method-status, typed DTOs, untyped diagnostics)** — `2cd1b9c` (feat)
2. **Task 2: Reconcile Flask fixture span + diagnostic lines (42/69/78)** — `a297aa2` (fix)
3. **Task 3: Flip Flask graph + openapi snapshots GREEN, determinism twins, gate** — `383eaa2` (test)

**Plan metadata:** (this commit) (docs: complete plan)

## Decisions Made
- **Q1 — status is method-derived.** A typed Flask handler's status comes from the HTTP method (`POST`→201, else 200); an untyped handler emits `responses: []` + a diagnostic. The docstring is never consulted (rule 1).
- **Q2 — diagnostic lines via honest reconciliation.** Each diagnostic anchors to its precise AST node; the fixture was grown (non-fact blank lines / comments only, rule 1) so those nodes land on 42/69/78. ZERO snapshot lines were corrected.
- **Parallel recognizers, no fallback.** `recognize_fastapi` and `recognize_flask` each fire only on their own ctor names; the route list is the union of both. A tree of one shape yields an empty list from the other — detection by source shape, never a try-then-fall-back path (rule 3).
- **Path params from converters.** Flask path params come from the `<int:...>` converters (sorted by name), not the function signature; `int` drives int64.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `__pycache__` left untracked by the test runs**
- **Found during:** Task 1. Running `python3 -m pyextract` and the unittest suite generates `pyextract/__pycache__` + `pyextract/tests/__pycache__`, which `git status` flagged as untracked generated output (no gitignore rule existed).
- **Fix:** Added `__pycache__/` + `*.pyc` to `.gitignore` so generated bytecode is never committed.
- **Files modified:** `.gitignore`
- **Commit:** `2cd1b9c`

**2. [Rule 1 - Bug] Golden-test axis assertion copied the FastAPI shape (all four axes on one DTO)**
- **Found during:** Task 2. `test_flask_golden.py` was first written like its FastAPI twin, asserting all four optional×nullable axes on `OrderInput`. But the Flask snapshot splits the axes: the "nullable only" (F,T) axis lives on `OrderConfirmation.message` (required, no default, `Optional[str]`), not on `OrderInput`. The committed snapshot is the spec.
- **Fix:** Assert the union of axes across `OrderInput` + `OrderConfirmation`, and pin `message` as the (F,T) axis specifically — matching the snapshot reality.
- **Files modified:** `pyextract/tests/test_flask_golden.py`
- **Commit:** `a297aa2`

**3. [Rule 3 - Blocking] clippy `doc_markdown` denied a new determinism.rs doc comment**
- **Found during:** Task 3. `make check` runs clippy with `-D warnings`; `determinism.rs` (unlike the snapshot test targets) has no `doc_markdown` allow, so an un-backticked "FastAPI" in my new Flask-fixture doc comment failed the lint.
- **Fix:** Backticked `` `FastAPI` `` in the comment (the minimal, consistent fix).
- **Files modified:** `crates/gnr8-core/tests/determinism.rs`
- **Commit:** `383eaa2`

**Snapshot-line corrections:** none — all reconciliation was fixture-side (the preferred outcome; Task 2 acceptance).

**Intentionally-still-red Flask assertions:** none — both Flask snapshots are FULLY green within the honest typed envelope, exactly as RESEARCH predicted.

## Issues Encountered
- Fixture line reconciliation required iterative blank-line/comment deltas across both files (header size, inter-class gaps); converged by measuring produced-vs-target lines and adjusting non-fact prose only. No fact is encoded in any docstring or comment (rule 1).

## CLAUDE.md Compliance
- **Rule 1 (no coupling to another tool's conventions):** Flask routes derive from Flask's own decorator/signature/body AST; nothing reads marshmallow, a runtime `openapi.json`, or any annotation dialect. `Blueprint(url_prefix=)` is recorded separately, never folded. Status is method-derived (a code fact), never read from a docstring. Fixture lines are pure source positions, never read from a docstring.
- **Rule 2 (no third-party deps):** `pyextract/routes.py` imports only `ast` + in-repo modules; the Rust changes use `std` + existing typed DTOs + `insta` (existing dev dep). Zero packages added.
- **Rule 3 (no fallback / one path):** every untyped surface (raw `request.json`, unannotated `request.args.get`, missing return annotation) is a diagnostic + omitted fact — never a guessed default, never a try-typed-then-fallback. FastAPI/Flask are parallel deterministic recognizers (detection by shape), not a fallback chain.
- **Static-only (PYSRC-03 / T-static-exec):** grep gate over `pyextract/*.py` for `exec/eval/compile/importlib/__import__/runpy` returns NOTHING; recognition is pure `ast` inspection.
- **Determinism:** two `python3 -m pyextract` runs are byte-identical; the Rust Flask determinism twins (graph + openapi) pass.

## Next Phase Readiness
- Phase 02 (Python Source — pyextract) is COMPLETE: both FastAPI snapshots (Plan 03) and both Flask snapshots (Plan 04) are green in the make-check gate; the four `#[ignore]` Python acceptance snapshots are all flipped with ZERO snapshot edits. Only the two NestJS snapshots remain red-by-design (they flip in Phase 4 / tsextract).
- No blockers. The phase is ready for verification.

## Self-Check: PASSED

- `pyextract/tests/test_flask_routes.py`, `pyextract/tests/test_flask_golden.py` exist on disk; `recognize_flask` + `url_prefix` present in `pyextract/routes.py`.
- Task commits `2cd1b9c`, `a297aa2`, `383eaa2` all present in git history.
- `python3 -m unittest discover -s pyextract/tests` → 69 passed; static + stdlib grep gates empty.
- `cargo test -p gnr8-core --test snapshot_flask_graph --test snapshot_flask_openapi` GREEN with NO `--ignored`; committed `.snap` files byte-unchanged (`git diff --stat` empty); `grep -c '#[ignore'` is 0 for both Flask test targets.
- `cargo test -p gnr8-core --test determinism` GREEN incl. the two Flask twins; stale `*flask*.snap.new` removed; NestJS snapshots remain `ignored` red-by-design; `make check` fully green.

---
*Phase: 02-python-source-pyextract*
*Completed: 2026-06-25*
