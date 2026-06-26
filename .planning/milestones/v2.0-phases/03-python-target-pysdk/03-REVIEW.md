---
phase: 03-python-target-pysdk
reviewed: 2026-06-25T21:19:10Z
depth: standard
files_reviewed: 5
files_reviewed_list:
  - crates/gnr8-core/src/pysdk/mod.rs
  - crates/gnr8-core/src/pysdk/emit.rs
  - crates/gnr8-core/src/pysdk/bundle.rs
  - crates/gnr8-core/src/sdk/builtins.rs
  - crates/gnr8-core/tests/pysdk_compile.rs
findings:
  critical: 4
  warning: 5
  info: 2
  total: 11
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-06-25T21:19:10Z
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

The `pysdk` emitter is well-structured: the IR→Python mapping is genuinely derived from the
neutral graph (rule 1 clean), the `py_type` match is exhaustive with explicit typed-error arms and
no `_ =>` catch-all (rule 3 clean), the import header is a fixed string and every collection is
consumed in the graph's already-sorted order (determinism clean), no production
`unwrap`/`expect`/`panic` appears in library code, no new Rust crate is introduced, and the emitted
SDK + hermetic test use Python stdlib only (rule 2 clean). The framing/bundle round-trip and the
path-traversal guards are sound.

The defects are concentrated in **the correctness of the emitted Python on input shapes the
bookstore fixture happens not to exercise** — exactly the load-bearing risk the prompt flags. Four
of these are BLOCKERs because they make the generated SDK either fail at import time or fail on the
primary happy path, and the hermetic test cannot catch them because it (a) decodes only a flat
`CreatedMessage` and (b) deliberately bypasses the broken request-body path by sending a raw dict.

## Critical Issues

### CR-01: Request-body serialization is broken for the typed model the signature demands

**File:** `crates/gnr8-core/src/pysdk/emit.rs:362-364` (the `_do` body) + `:551-553`, `:599-609` (the call site)
**Issue:** Every body-bearing method is emitted with the signature `def create_book(self, body: Book)`
and dispatches via `self._do("POST", path, body=body)`. `_do` then does
`json.dumps(body).encode("utf-8")`. But `body` is a `@dataclass` instance, and
`json.dumps(<dataclass instance>)` raises `TypeError: Object of type Book is not JSON serializable`.
The advertised happy path — construct the typed model, pass it to the method — is broken at runtime.
The hermetic round-trip test passes only because the driver deliberately sends a raw `dict` instead
of a `Book`, and its own comment admits "a dataclass instance is not json-serializable; the body
hint is unenforced" (`tests/pysdk_compile.rs:234-235`, `:278-287`). A test that has to route
*around* the generated signature to pass is evidence the signature is wrong.
**Fix:** Serialize the dataclass before dumping, e.g. emit a body that converts it. Either marshal in
`_do`:
```python
import dataclasses
# ...
def _do(self, method, path, *, body=None):
    if body is not None and dataclasses.is_dataclass(body):
        body = dataclasses.asdict(body)
    data = json.dumps(body).encode("utf-8") if body is not None else None
```
or have the operation method pass `dataclasses.asdict(body)`. (`dataclasses.asdict` recurses into
nested dataclasses, also addressing CR-04's encode direction.) Then add a round-trip test that
constructs and sends an actual `Book` instance, not a dict.

### CR-02: Reserved-word field / parameter names emit a `SyntaxError`

**File:** `crates/gnr8-core/src/pysdk/emit.rs:279`, `:285` (dataclass fields), `:550`, `:555` (method args), `:588` (query key var)
**Issue:** Field names (`field.json_name`) and param names (`snake(&p.name)`) are written verbatim
as Python identifiers with no keyword sanitization. A field or path/query param named after a Python
keyword — `from`, `class`, `import`, `def`, `return`, `lambda`, `global`, `None`, `True`, `False`,
`async`, `await`, etc. — produces invalid Python. Verified: `class X:\n    from: str` →
`SyntaxError: invalid syntax`; `def f(self, class): pass` → `SyntaxError`. `from`/`id`/`type`/`class`
are extremely common JSON field and query-param names. No reserved-word guard exists anywhere in the
core (`grep` for `keyword`/`reserved`/`sanitize` finds none). The bookstore fixture has no such name,
so the hermetic test is silent on it.
**Fix:** Add a `safe_ident` helper that suffixes a trailing `_` when the snake/identifier form is a
Python keyword (and when it starts with a digit), and route every emitted field/param/local through
it:
```rust
fn safe_ident(s: &str) -> String {
    const KEYWORDS: &[&str] = &["False","None","True","and","as","assert","async","await",
        "break","class","continue","def","del","elif","else","except","finally","for","from",
        "global","if","import","in","is","lambda","nonlocal","not","or","pass","raise","return",
        "try","while","with","yield"];
    if KEYWORDS.contains(&s) || s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("{s}_")
    } else { s.to_string() }
}
```
Apply at `:279`, `:285` (field names), `:550`, `:555` (arg names), and the f-string/query var sites.
Note the wire key in `_query["{}"]` (`:588`) and the JSON field name must stay the *original*
name — only the Python identifier is renamed.

### CR-03: Enum members that collapse to the same `SCREAMING_SNAKE` identifier raise `TypeError` at import

**File:** `crates/gnr8-core/src/pysdk/emit.rs:240-252` (`emit_enum_class`)
**Issue:** Each enum member identifier is `screaming_snake(value)`. Two distinct wire values that
normalize to the same identifier — e.g. `"out-of-stock"` and `"out_of_stock"`, or `"a.b"` and
`"a-b"`, or `"Foo"` and `"foo"` — emit two `OUT_OF_STOCK = ...` lines in the same class body. Python
raises `TypeError: Attempted to reuse key: 'OUT_OF_STOCK'` at class-definition (import) time
(verified on 3.9). The whole package then fails to import. `screaming_snake` is also not injective
for purely-numeric or punctuation-only values (e.g. `""` → empty identifier → `SyntaxError`; `"1"`
→ `1` → invalid leading-digit identifier).
**Fix:** Detect collisions while emitting and disambiguate deterministically (e.g. append `_2`,
`_3` on repeat), and guard empty/leading-digit identifiers. The member's wire `value` string stays
unchanged; only the Python member *name* is made unique. Alternatively return a typed
`CoreError::SdkGen` rather than emit un-importable Python — but silent corruption (current
behavior) is the worst option.

### CR-04: `Model(**_data)` fails on any response field the model doesn't declare (and on nested models)

**File:** `crates/gnr8-core/src/pysdk/emit.rs:612-614`
**Issue:** Typed-return decode is `_data = json.loads(_raw)...; return Model(**_data)`. Two runtime
failure modes:
1. **Extra keys:** if the server response carries *any* key the dataclass does not declare,
   `Model(**_data)` raises `TypeError: __init__() got an unexpected keyword argument '...'`
   (verified). Forward-compatible APIs routinely add response fields; a freshly-generated SDK then
   crashes against a newer-but-compatible server. Symmetrically, a server that omits a model-required
   key raises `TypeError: missing required argument`.
2. **Nested models:** for a `Book` whose `author` field is typed `Author`, `Book(**_data)` binds
   `author` to a raw `dict`, not an `Author` instance — so `result.author` is not the advertised
   type. The decode is shallow.
The hermetic test only decodes a flat two-field `CreatedMessage` whose server reply matches exactly,
so it never exercises either path.
**Fix:** For (1), filter to known fields before construction, e.g. emit
`Model(**{k: v for k, v in _data.items() if k in Model.__dataclass_fields__})`, or generate a
`from_dict` classmethod that tolerates extra keys / fills missing optionals. For (2), recursively
construct nested dataclasses in `from_dict`. At minimum, document and test the extra-key case; the
current shape is a latent crash on a normal API-evolution scenario.

## Warnings

### WR-01: Required query parameters are silently emitted as optional

**File:** `crates/gnr8-core/src/pysdk/emit.rs:554-556`, `:583-596`
**Issue:** Every query param is emitted as `{name}=None` with an `if {name} is not None:` guard,
regardless of `Param::required`. The `required` field is never read in operation emission. A required
query param can therefore be omitted by the caller with no client-side error and no value sent — the
server gets a malformed request the SDK should have prevented. (`Param::required` is read nowhere in
`emit_operations`.)
**Fix:** For a required query param, emit it as a positional/no-default arg and skip the `is not None`
guard (always include it in `_query`), or raise a `TypeError`/`ValueError` when it is `None`. Mirror
whatever the Go twin does for required query params.

### WR-02: `bool`/typed fields get a `= None` default whose type hint forbids `None`

**File:** `crates/gnr8-core/src/pysdk/emit.rs:281-286`
**Issue:** An optional-but-not-nullable field emits e.g. `in_stock: bool = None` (asserted in the
test at `:938-941`). The annotation says `bool` but the default value is `None`, which the type
contradicts. With `from __future__ import annotations` this imports without error, but it is a
type-lie: static checkers (mypy/pyright) on the *generated* SDK flag it, and a caller relying on the
hint gets surprising `None` values. The "optional presence" axis (key may be absent) is being modeled
by a `None` sentinel that the value-type does not admit.
**Fix:** When a field is `optional` but not `nullable`, either widen the hint to `Optional[T]` for
the defaulted form, or use a dedicated sentinel / `field(default=...)` that the hint permits. Keep
the `optional` vs `nullable` axes distinct in the *hint*, not just the default.

### WR-03: Path/query parameter identifier collisions are undetected

**File:** `crates/gnr8-core/src/pysdk/emit.rs:547-556`
**Issue:** Arg names are built by `snake(p.name)` independently for path and query params, plus the
fixed `self`/`body`. Two distinct source params whose snake forms collide (e.g. `bookId` and
`book_id`, or a param literally named `self`/`body`), or a query param named `body` alongside a typed
body, produce a duplicate-argument `def` → `SyntaxError: duplicate argument`. The token/param
set-equality check at `:527-538` only compares path tokens to path-param names; it does not catch
path↔query or param↔`self`/`body` collisions.
**Fix:** Track the emitted argument names in a set as they are appended; on a collision return a typed
`CoreError::SdkGen` (or disambiguate deterministically). Include `self` and `body` as reserved.

### WR-04: `_query` builds wire keys from the raw `p.name`, but reads from the snaked local — a mismatch is possible

**File:** `crates/gnr8-core/src/pysdk/emit.rs:585-588`
**Issue:** The local variable is `snake(&p.name)` (`:585`) but the wire key is the raw `p.name`
(`:588`: `_query["{}"] = {arg}`). This is *correct* for the common case (wire name preserved, local
is the Python identifier). But if two params share a `snake` form (see WR-03) or a param name is a
keyword (see CR-02), the local `{arg}` is wrong/undefined while the wire key looks fine — producing a
`NameError` or wrong binding at runtime rather than a clean error. This is a downstream symptom of
CR-02/WR-03; flagged separately because the value/key split makes the failure mode non-obvious.
**Fix:** Resolved once CR-02 (keyword-safe identifiers) and WR-03 (collision detection) land; ensure
the *local* uses the safe identifier while the *wire key* keeps `p.name`.

### WR-05: `emit_init` re-exports `s.name` for every schema, but duplicate names produce duplicate imports/`__all__`

**File:** `crates/gnr8-core/src/pysdk/emit.rs:631-646`
**Issue:** `__init__.py` imports and lists `s.name` for every schema, iterating `graph.schemas`
(sorted by `id`). Two schemas with distinct ids but the same `name` (e.g. `pkg_a.Book` and
`pkg_b.Book`) emit `from .models import (Book, Book,)` and a duplicated `__all__` entry, and
`models.py` would emit two `class Book` definitions (the second silently shadowing the first). The
graph does not guarantee name-uniqueness across ids. Not exercised by the single-package bookstore
fixture.
**Fix:** Either rely on a documented upstream invariant that schema names are globally unique (and
assert it), or de-duplicate / disambiguate names at emit time with a typed error on a true collision.

## Info

### IN-01: Dead fall-through after `self._raise(...)` in every operation

**File:** `crates/gnr8-core/src/pysdk/emit.rs:610-616`
**Issue:** `_raise` always raises `ApiError`, so the lines after `self._raise(_status, _raw)` (the
JSON decode / return) are unreachable when `_status != success_status`. It is correct at runtime, but
`_raise` is annotated `-> None`, so a static checker may warn "missing return" on the typed-return
branch and a reader cannot see that `_raise` is `NoReturn`.
**Fix:** Annotate the emitted `_raise` as `-> NoReturn` (from `typing`) so the control-flow is
explicit and type-checkers on the generated SDK are satisfied.

### IN-02: `_package` argument threaded through every emitter but unused

**File:** `crates/gnr8-core/src/pysdk/emit.rs:198`, `:294`, `:334`, `:499`, `:624`
**Issue:** `emit_models`, `emit_errors`, `emit_client`, `emit_operations`, and `emit_init` all take a
`_package` arg that is unused (Python files carry no package clause). The docs justify this as Go-twin
symmetry, which is a reasonable call, but it is dead surface that invites a future caller to assume it
does something.
**Fix:** Acceptable as-is for twin symmetry; if the symmetry is not load-bearing, drop the parameter
to remove the dead surface. No behavior change either way.

---

_Reviewed: 2026-06-25T21:19:10Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
