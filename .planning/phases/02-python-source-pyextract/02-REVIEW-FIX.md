---
phase: 02-python-source-pyextract
fixed_at: 2026-06-25T00:00:00Z
review_path: .planning/phases/02-python-source-pyextract/02-REVIEW.md
iteration: 1
findings_in_scope: 10
fixed: 10
skipped: 0
status: all_fixed
---

# Phase 2: Code Review Fix Report

**Fixed at:** 2026-06-25
**Source review:** .planning/phases/02-python-source-pyextract/02-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 10 (3 critical/blocker + 7 warning; Info out of scope for `critical_warning`)
- Fixed: 10
- Skipped: 0

**Acceptance verified after all fixes:**
- `make check` exits 0 (GREEN).
- The 4 Python acceptance snapshots (FastAPI graph+openapi, Flask graph+openapi) stay GREEN with ZERO edits to any committed `.snap`/`expected` file — confirmed via `git diff --name-only` over the whole session (no snapshot/expected file touched).
- The 2 NestJS snapshots stay red-by-design (FAILED under `--ignored`, ignored in `make check`).
- `python3 -m unittest discover pyextract` passes (87 tests), with a new regression test for every blocker fix and every rule-3 warning fix.
- Static-only grep gate (`exec`/`eval`/`compile`/`importlib`/`__import__`/`runpy`) over `pyextract/*.py` is clean.
- Stdlib-only grep gate over `pyextract/*.py` is clean (only `ast`/`os`/`sys`/`json`/`traceback`; the one added import is `traceback`, stdlib).

All fixes honour the CLAUDE.md invariants: no third-party import added to the Python sidecar, no new Rust crate, no exec/eval/import of the target (only `ast.parse` of file text), and every "fabricated/guessed fact" is now a DIAGNOSTIC + omission (rule 3), never a different fallback.

## Fixed Issues

### CR-01: `dict`/`Dict` with wrong arity fabricated a `{string: any}` map (rule-3 fallback)

**Files modified:** `pyextract/types.py`, `pyextract/tests/test_types.py`
**Commits:** c4b626d (fix), c251ead (regression test)
**Applied fix:** In `_map_subscript`, a `dict`/`Dict`/`Mapping`/`MutableMapping` subscript without exactly two type args now emits a WARN diagnostic ("...needs exactly two type args; fact omitted (no fallback)") and returns `None`, replacing the fabricated `{"type":"map","of":{"key":string,"value":any}}` default. Regression tests `test_single_arg_dict_diagnoses_and_omits` / `test_three_arg_dict_diagnoses_and_omits` lock the diagnose+omit behavior and assert the old default is never produced.

### CR-02: string forward references (`field: "Author"`) silently dropped without symbol-table resolution

**Files modified:** `pyextract/types.py`, `pyextract/tests/test_types.py`
**Commits:** b855c22 (fix), c251ead (regression test)
**Applied fix:** In `_map`, before the catch-all "unsupported type annotation" diagnostic, a `Constant` whose value is a `str` is now re-parsed with `ast.parse(value, mode="eval")` (static parse only — no exec/eval/import) and routed back through the SAME single `_map` path, so the owned symbol table can resolve the forward ref to a named class/primitive/container. `ast.copy_location` preserves the line for any downstream diagnostic. This is not a dual path: a string annotation has exactly one deterministic meaning (its parsed expression), and a genuinely unresolvable name still falls through to the normal diagnostic. Regression tests cover resolution to a named ref, to a primitive, nested inside `list[...]`, and the still-unresolvable case.

### CR-03: typed-query `AnnAssign` with a non-`Name` target emitted a param `name: ""` (invalid OpenAPI)

**Files modified:** `pyextract/routes.py`, `pyextract/tests/test_routes_unit.py` (new file)
**Commit:** ffe18ab
**Applied fix:** In `_flask_body_and_params`, the typed-query-param branch now checks `isinstance(stmt.target, ast.Name)` BEFORE building the param; a non-Name target (e.g. `obj.attr:` / `d['k']:`) emits a WARN diagnostic ("typed query param on {method} {path} has a non-name target; param omitted (no fallback)") and `continue`s, instead of appending `"name": ""`. The param dict now reads `stmt.target.id` unconditionally (the guard guarantees it). Regression tests assert no empty-named param is ever appended and a diagnostic is recorded for Attribute and Subscript targets.

### WR-01: `_strip_optional` did not normalize an `ast.Index` for `Optional[T]` (3.8 shape -> dropped field)

**File modified:** `pyextract/types.py`
**Commit:** 578e094
**Applied fix:** The `Optional[T]` branch now routes `node.slice` through `_subscript_args` (which already unwraps the `ast.Index` wrapper) instead of returning `node.slice` directly, matching every other subscript site so a 3.8-style inner annotation is no longer dropped.

### WR-02: `_literal_members` silently dropped non-string members (wrong/empty enum)

**Files modified:** `pyextract/types.py`, `pyextract/tests/test_types.py`
**Commit:** c727e91
**Applied fix:** `_literal_members` now returns `(sorted_string_members, faithful)`; `faithful` is False if any member is non-string or the member list is empty. The `Literal` branch in `_map_subscript` emits a WARN diagnostic + returns `None` when not faithful, instead of an empty/partial string enum. Regression tests cover all-string (still works), all-int (omitted), and mixed (omitted).

### WR-03: `_map_union` could emit an empty union `{"type":"union","of":[]}`

**Files modified:** `pyextract/types.py`, `pyextract/tests/test_types.py`
**Commit:** 9a8f874
**Applied fix:** `_map_union` now emits a WARN diagnostic ("degenerate union annotation: no non-None members; fact omitted (no fallback)") and returns `None` when, after dropping None arms, no members remain (e.g. `Union[None]`). Regression test `test_union_of_only_none_diagnoses_and_omits` locks this and asserts an empty oneOf is never produced.

### WR-04: a Flask GET handler could derive a `request.json` request body (semantically wrong)

**Files modified:** `pyextract/routes.py`, `pyextract/tests/test_routes_unit.py`
**Commits:** fd93077 (fix), fba5c8a (regression test)
**Applied fix:** Added module constant `_BODYLESS_METHODS = {"GET","HEAD","DELETE"}`. In `_flask_body_and_params`, `allows_body = method not in _BODYLESS_METHODS` now gates the typed-request-body emission, so a body-less method never yields a `request_body` fact even if the handler reads `request.json`. The method is a code fact (the decorator's `methods=[...]`). Regression tests assert POST derives the body while GET/DELETE omit it. The Flask fixture lists one method per route and no GET reads `request.json` for a typed body, so the committed snapshots are unaffected.

### WR-05: `detect_language` silently routed a mixed Go/Python tree to Go

**File modified:** `crates/gnr8-core/src/analyze/mod.rs`
**Commit:** b4a5f77
**Applied fix:** The `(true, true)` arm (both Go and Python markers present) now returns a typed `CoreError::Config` naming BOTH languages and directing the user to scope the source's `inputs` to a single-language subdir, instead of silently picking Go. `(true, false) => Go` and `(false, true) => Python` are unchanged, preserving the single-language fixtures. A Rust regression test (`detect_language_rejects_a_mixed_go_python_tree`) creates a temp dir with one `.go` and one `.py` and asserts the ambiguous `Config` error; written with `matches!` (no `panic!`) to satisfy `clippy::panic`.

### WR-06: `_has_default`/param iteration ignored `posonlyargs` and dropped keyword-only params

**Files modified:** `pyextract/routes.py`, `pyextract/tests/test_routes_unit.py`
**Commit:** 426da2b
**Applied fix:** Added `_positional_args(args)` returning `posonlyargs + args` so the default END-alignment math in `_has_default` counts positional-only params. `_build_params` now iterates that combined list and additionally walks `func.args.kwonlyargs`, deriving required-ness per slot from `kw_defaults[i] is None` (kwonly defaults are not END-aligned). Keyword-only query params (common after `*` in FastAPI handlers) are no longer silently dropped. Regression tests cover a `*, genre, sort='asc'` kwonly signature and an `a, b, /, c='x'` positional-only signature. The FastAPI fixture uses only plain positional params (empty posonly/kwonly lists), so the committed snapshots are unaffected.

### WR-07: `main()`'s broad `except Exception` masked internal bugs as a stack-trace-less one-liner

**File modified:** `pyextract/__main__.py`
**Commit:** 33ca6e8
**Applied fix:** The `except Exception` handler now also writes `traceback.format_exc()` to stderr (alongside the existing `pyextract: <exc>` one-liner) so a genuine internal bug (an unhandled AST shape -> AttributeError/KeyError) is debuggable. stdout stays reserved for the facts JSON and the non-zero exit still maps to `HelperExit`. `traceback` is a stdlib module (stdlib-only gate stays clean).

## Skipped Issues

None — all 10 in-scope findings were fixed. (IN-01..IN-04 are Info-tier and out of scope for the `critical_warning` fix scope; IN-04 is a documented v1 narrowing and IN-01/IN-02 are doc/message hygiene with no behavioral impact.)

---

_Fixed: 2026-06-25_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
