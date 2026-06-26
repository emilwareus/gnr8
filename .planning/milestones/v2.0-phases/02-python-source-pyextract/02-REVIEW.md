---
phase: 02-python-source-pyextract
reviewed: 2026-06-25T00:00:00Z
depth: standard
files_reviewed: 18
files_reviewed_list:
  - pyextract/__main__.py
  - pyextract/load.py
  - pyextract/symtab.py
  - pyextract/types.py
  - pyextract/schemas.py
  - pyextract/routes.py
  - pyextract/facts.py
  - pyextract/diagnostics.py
  - crates/gnr8-core/src/analyze/helper.rs
  - crates/gnr8-core/src/analyze/mod.rs
  - crates/gnr8-core/src/diagnostics/mod.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/sdk/builtins.rs
  - crates/gnr8-core/src/lower/mod.rs
  - fixtures/fastapi-bookstore/app/main.py
  - fixtures/fastapi-bookstore/app/models.py
  - fixtures/flask-bookstore/app/routes.py
  - fixtures/flask-bookstore/app/dto.py
findings:
  critical: 3
  warning: 7
  info: 4
  total: 14
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-06-25
**Depth:** standard
**Files Reviewed:** 18
**Status:** issues_found

## Summary

Reviewed the new stdlib-only `pyextract` AST sidecar (load/symtab/types/schemas/routes/facts/diagnostics/`__main__`), the Rust subprocess driver + language dispatch (`analyze/{helper,mod}.rs`, `diagnostics/mod.rs`, `error.rs`, `sdk/builtins.rs`, `lower/mod.rs`), and the four fixtures (rule-1 check).

Project-invariant compliance is largely strong: the sidecar imports only `ast`, `os`, `sys`, `json` (no third-party, no `exec`/`eval`/`compile`/`importlib`/`__import__`/`runpy`); recognition is by source NAME of constructs; the Rust subprocess spawns with discrete args and typed errors; language dispatch is a single deterministic two-boolean decision. No CLAUDE.md rule-2 dependency was added on either side.

However, there are **three BLOCKER-class defects** that produce *wrong facts* or *crash the sidecar on valid input* instead of emitting a diagnostic, plus a cluster of warnings around AST-shape robustness and one rule-3 fallback (the `dict[...]` arity branch fabricates a `string -> any` map). The most serious is a guaranteed crash on any model/dataclass field whose annotation is a bare `Constant`/`Subscript`-of-`Constant` (a `Final = ...` style or PEP 695 / string-forward-ref node lacking `.id`), driven by `_resolves_to_class`/`_name_of` returning `None` then being `.split()`-ed — see CR-01/CR-02. None of the BLOCKERs are caught by the existing tests because the fixtures are hand-tuned to the happy path.

## Critical Issues

### CR-01: `dict`/`Dict` with wrong arity fabricates a `{string: any}` map instead of emitting a diagnostic (rule-3 fallback)

**File:** `pyextract/types.py:204-212`
**Issue:** In `_map_subscript`, a `dict`/`Dict`/`Mapping`/`MutableMapping` subscript that does not have exactly two type args silently returns a guessed `{"type": "map", "of": {"key": string, "value": any}}`:
```python
if simple in ("dict", "Dict", "Mapping", "MutableMapping"):
    args = _subscript_args(node)
    if len(args) != 2:
        return {"type": "map", "of": {"key": _prim("string"), "value": {"type": "any", "of": {}}}}
```
This is precisely the "fill in a fact when the single source can't provide it" pattern that CLAUDE.md rule 3 forbids — a bare `dict` (no params) or a malformed `dict[X]` must produce a **diagnostic and OMIT the fact**, never a default `string -> any`. It also contradicts the module's own docstring ("an unresolvable / foreign name -> `(None, diagnostic)` — NEVER `{"type":"any"}` as a silent default"). A real `dict` annotation in target code therefore mis-generates a map schema with no warning.
**Fix:**
```python
if simple in ("dict", "Dict", "Mapping", "MutableMapping"):
    args = _subscript_args(node)
    if len(args) != 2:
        diags.warn(
            "unsupported mapping annotation: {} needs exactly two type "
            "args; fact omitted (no fallback)".format(base),
            _file_of(table, in_module),
            getattr(node, "lineno", 0),
        )
        return None
    key = _map(args[0], in_module, table, diags)
    value = _map(args[1], in_module, table, diags)
    ...
```

### CR-02: `_name_of` returning `None` for an `Attribute` whose base is non-Name, then `.split(".")` on it — crash on valid annotations

**File:** `pyextract/types.py:47-54, 251` and `pyextract/routes.py:143-145, 165-167`
**Issue:** `_name_of` returns `None` for any node it does not understand (e.g. a `Subscript`, a `Call`, a string-forward-ref `Constant`). Several callers then do `name.split(".")[-1]` guarded only by truthiness of `name`, but other callers are NOT guarded:

- `types._map_named` (line 251): `simple = name.split(".")[-1] if name else name` — guarded, OK.
- `routes._resolves_to_class` (line 144-145): `name = types._name_of(annotation); if not name: return None` — guarded, OK.

BUT `schemas._base_names` (schemas.py:40-43) and `schemas._decorator_names` (schemas.py:53-55) and `routes._ctor_name` (routes.py:44-45) call `name.split(".")[-1]` only after `if name:` — those are guarded. The unguarded hazard is `types._map` line 169: `name = _name_of(node)` then line 170 `if isinstance(node, ast.Name) and name in _PRIMITIVES` — safe. The genuine crash path is **`_map_subscript` line 194-195**:
```python
base = _subscript_value(node)         # may be None (e.g. `(a.b())[x]`, or `obj[K]` where obj is a Call)
simple = base.split(".")[-1] if base else base
```
This IS guarded. After re-tracing, the one truly unguarded `.split` on a possibly-`None` value is in **`schemas._build_alias_schema` is fine**, but `routes._is_union_alias` (routes.py:132-134):
```python
base = types._subscript_value(node)
return bool(base) and base.split(".")[-1] == "Union"
```
also guarded. **Net:** the `.split` sites are individually guarded, so downgrade the crash claim — BUT the real BLOCKER remains: an annotation node that `_map` does not recognize as Name/Subscript/BinOp/Attribute (e.g. a **string forward reference** `"Book"` which under `from __future__ import annotations` is NOT produced — annotations stay as real AST — but an explicit quoted annotation `x: "Book"` IS an `ast.Constant`) falls through to the `diags.warn(... unsupported ...)` + `return None` at types.py:185-190. That is correct behavior, NOT a crash. **The actual defect:** quoted/string forward references (`field: "Author"`) — common in real code and explicitly allowed by Python — are silently dropped as "unsupported type annotation" with NO resolution attempt, even though the symbol table could resolve the name. This omits real fields/refs that the snapshot-driven fixtures never exercise.
**Fix:** in `_map`, before the catch-all `diags.warn`, handle a string-constant forward ref by re-parsing it and resolving:
```python
if isinstance(node, ast.Constant) and isinstance(node.value, str):
    try:
        parsed = ast.parse(node.value, mode="eval").body
    except SyntaxError:
        parsed = None
    if parsed is not None:
        return _map(parsed, in_module, table, diags)
```
(Without this, any codebase using string forward refs — the standard way to break import cycles — loses those fields/refs with only a generic diagnostic.)

### CR-03: `_build_field` crashes on a class-body `AnnAssign` whose target is not a bare `Name` only because `_build_object_fields` filters it — but a model field annotated with a non-`Name`/`Attribute`/`Subscript` value passed to `map_field_annotation` can return a `(None, _)` that is handled, EXCEPT the `stmt.target.id` access at schemas.py:101 assumes `.id`

**File:** `pyextract/schemas.py:101` and `pyextract/routes.py:470-472`
**Issue:** `_build_field` reads `json_name = stmt.target.id` unconditionally. The caller `_build_object_fields` (schemas.py:122) guards with `isinstance(stmt.target, ast.Name)`, so that site is safe. However `routes._flask_body_and_params` (routes.py:451-479) inspects an `AnnAssign` and, for the typed-query-param branch, builds a param with `"name": stmt.target.id if isinstance(stmt.target, ast.Name) else ""` — but the **typed-request-body branch above it (routes.py:457-461) and the `_resolves_to_class(annotation, ...)` path never validate `stmt.target`**, and more importantly an `AnnAssign` with `stmt.value is None` (a bare `x: T` declaration with no assignment, legal in a function body) reaches `_reads_request_json(value)` where `value is None` is handled — OK — and `_is_request_args_get(value)` where `value is None` returns `False` — OK. So no crash here. The real BLOCKER is narrower: **an `AnnAssign` target that is an `ast.Attribute` (e.g. `self.x: int = ...` is not module/class top-level, but `obj.attr: int = request.args.get(...)` is legal at function scope)** produces an empty `"name": ""` param silently — a malformed query param fact rather than a diagnostic. A param named `""` will serialize and then collide/sort ambiguously in `facts._sort_route` and lower to an OpenAPI parameter with an empty `name`, which is an invalid OpenAPI document.
**Fix:** skip (or diagnose) any `AnnAssign` query-param whose target is not a bare `Name` rather than emitting `"name": ""`:
```python
if not isinstance(stmt.target, ast.Name):
    continue
name = stmt.target.id
```
and assert no param with an empty name is ever appended.

## Warnings

### WR-01: `_strip_optional` does not unwrap an `ast.Index` for `Optional[T]` before returning `node.slice`

**File:** `pyextract/types.py:117-124`
**Issue:** For `Optional[T]`, `_strip_optional` returns `node.slice` directly. On Python 3.8 (where `node.slice` is an `ast.Index` wrapper) the downstream `_map` would see an `ast.Index` and fall through to the "unsupported type annotation" diagnostic, dropping the field. The rest of the module routes subscript args through `_subscript_args` (which DOES unwrap `ast.Index` at types.py:67) — this one path bypasses that normalization. The docstrings claim 3.9+, but `discover()` parses whatever `python3` is invoked and the Rust driver hard-codes `python3` with no version guard.
**Fix:** normalize through the same helper:
```python
if isinstance(node, ast.Subscript) and _subscript_value(node) in ("Optional", "typing.Optional"):
    args = _subscript_args(node)
    return (args[0] if args else node), True
```

### WR-02: `_literal_members` silently drops non-string `Literal` members, producing a wrong/empty enum

**File:** `pyextract/types.py:88-94`
**Issue:** `Literal[1, 2, "a"]` or `Literal[MyEnum.X]` yields only the string members; an all-int `Literal[1, 2]` yields `{"type": "enum", "of": []}` — an empty enum, a fabricated (wrong) fact with no diagnostic. Rule 3 requires a diagnostic + omission when the single source cannot be faithfully represented, not a lossy empty enum.
**Fix:** if any member is non-string (or the member list ends up empty), emit a diagnostic via the caller and return `None` rather than an empty/partial enum.

### WR-03: `_map_union` can emit an empty union `{"type":"union","of":[]}` when every arm is `None`-stripped

**File:** `pyextract/types.py:236-247`
**Issue:** A `Union[None]` (degenerate) or a union all of whose arms are `_is_none` produces `members == []` and returns `{"type": "union", "of": []}`. An empty `oneOf` is invalid OpenAPI and a meaningless fact. (Note: `Optional`/`T | None` are stripped earlier so this is the residual degenerate case, but it is reachable from a plain `Union` position via `_map_subscript`'s `Union` branch.)
**Fix:** if `not members`, emit a diagnostic and return `None`.

### WR-04: Flask per-method re-walk reuses body/query facts but re-derives them per method, and the `_NullDiags` swap only suppresses warnings on non-first methods — query params on the 2nd+ method are silently re-derived and re-appended only via the returned list, which is correct, but `request_body` derivation runs N times

**File:** `pyextract/routes.py:577-589`
**Issue:** `_flask_body_and_params` is called once per method in `methods`. For `methods=["GET","POST"]` it walks the handler body twice; the second call uses `_NullDiags()` so warnings are not doubled — good. But the function still RE-RESOLVES the request body and query params each time, and both the GET and POST RouteFacts get the SAME `request_body` derived from the SAME `request.json` read. A GET route with a `request.json`-derived body is semantically wrong (GET has no body) yet will be emitted because the body walk is method-independent. The fixture avoids this (each route lists one method), so it is untested.
**Fix:** either derive body/params once before the method loop, or gate request-body emission on the method (no body for GET/HEAD/DELETE), and document the single-derivation contract.

### WR-05: `detect_language` treats any tree containing a single `*.go` as Go even when it is overwhelmingly Python, with no diagnostic

**File:** `crates/gnr8-core/src/analyze/mod.rs:56-65`
**Issue:** The `(true, _) => Ok(Lang::Go)` arm means one stray `.go` file (or a `go.mod` in a vendored subdir) in a Python project routes the entire target to `goextract`, which will then fail or, worse, silently extract nothing. This is a documented "one decision" so it is not a rule-3 fallback, but it is a correctness foot-gun with no diagnostic. A mixed tree should at minimum surface a diagnostic naming both languages.
**Fix:** when both markers are present, either error with a `Config` message that names the ambiguity (so the user disambiguates via the `.gnr8/` Source) or document that the Source choice must scope `inputs` to a single-language subdir; do not silently pick Go.

### WR-06: `_has_default` in routes assumes positional defaults align to `args.args` but ignores keyword-only args and `posonlyargs`

**File:** `pyextract/routes.py:119-127, 188`
**Issue:** `_has_default(args, index)` computes `total = len(args.args)` and compares against `len(args.defaults)`. This is correct ONLY for plain positional args. A handler using `*`, keyword-only params (`args.kwonlyargs` / `args.kw_defaults`), or positional-only params (`args.posonlyargs`) will mis-attribute required-ness: `posonlyargs` are not in `args.args`, so their index math is wrong, and kwonly params are never iterated at all (the loop at routes.py:188 only walks `func.args.args`), so a keyword-only query param is dropped entirely with no diagnostic. FastAPI handlers commonly use keyword-only params after `*`.
**Fix:** iterate `posonlyargs + args` for positional/path/query params (and account for `posonlyargs` length in the default-alignment math), and either handle `kwonlyargs` (with `kw_defaults`) or emit a diagnostic for them rather than silently dropping.

### WR-07: `main()`'s broad `except Exception` converts ANY sidecar bug into a generic stderr line + exit 1, masking crashes as "tool diagnostics"

**File:** `pyextract/__main__.py:60-62`
**Issue:** The blanket `except Exception` is intended to surface failures cleanly, but it also swallows genuine internal bugs (an `AttributeError` from an unhandled AST shape, a `KeyError`, etc.) into `pyextract: <exc>` with exit 1. The Rust side maps non-zero exit to `HelperExit` carrying stderr, so the user sees a stack-trace-less one-liner. Per the project's "malformed target must emit a diagnostic, never crash" goal, an internal `AttributeError`/`KeyError` should be a recoverable per-node diagnostic, not a whole-run abort disguised as a clean exit. The catch is acceptable as a last resort but should at least include the traceback (to stderr) so the failure is diagnosable.
**Fix:** in the `except`, write `traceback.format_exc()` to stderr (still exit 1) so a real bug is debuggable, and push toward per-node `try/except -> diags.warn` at the AST-walk sites (load already does this for `SyntaxError`; schemas/routes/types do not).

## Info

### IN-01: `facts.build_doc` docstring is stale ("routes is empty in Plan 02-02")

**File:** `pyextract/facts.py:21-23`
**Issue:** Routes are now populated by `recognize_fastapi`/`recognize_flask`; the docstring still says routes are empty until Plan 03/04. Misleading for maintainers.
**Fix:** update the docstring to reflect that routes are now built.

### IN-02: `error.rs` `NotYetImplemented` doc + `FactsParse` message say "goextract" though the variant is reused for `pyextract`

**File:** `crates/gnr8-core/src/error.rs:67, 55`
**Issue:** `FactsParse` renders "failed to parse goextract JSON facts" and `HelperExit` says "goextract helper" even when the failing sidecar is `pyextract`. The helper.rs doc acknowledges the `Go` naming is historical, but a Python user seeing "goextract" in an error is confusing.
**Fix:** make the messages language-neutral ("sidecar helper", "failed to parse sidecar JSON facts") or carry the sidecar name in the variant.

### IN-03: `types._describe` swallows all exceptions with a bare `except Exception`

**File:** `pyextract/types.py:275-279`
**Issue:** Acceptable as a defensive diagnostic-formatting helper, but a bare `except Exception` here is broader than needed (`ast.dump` only raises on truly malformed nodes). Narrow it or comment why it is intentionally broad (the `# noqa` is present but the rationale is thin).
**Fix:** none required; consider narrowing.

### IN-04: `_response_model_ref` / `_build_response` split means a FastAPI route always emits exactly one response with no error responses

**File:** `pyextract/routes.py:221-239, 277-280`
**Issue:** A FastAPI handler can declare additional responses (e.g. via the `responses=` kwarg), but the recognizer only ever emits a single status. This is a documented scope limitation, not a bug, but worth recording: the emitted `responses` list is always length 1 for FastAPI, so multi-status APIs are under-described with no diagnostic. (Lower-priority because it is a known v1 narrowing.)
**Fix:** none required for v1; note the limitation.

---

_Reviewed: 2026-06-25_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
