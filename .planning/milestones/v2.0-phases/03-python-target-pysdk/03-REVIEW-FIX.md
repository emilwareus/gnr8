---
phase: 03-python-target-pysdk
fixed_at: 2026-06-25T22:10:00Z
review_path: .planning/phases/03-python-target-pysdk/03-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 9
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-06-25T22:10:00Z
**Source review:** .planning/phases/03-python-target-pysdk/03-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope (critical + warning): 9
- Fixed: 9
- Skipped: 0
- Info findings (IN-01, IN-02): out of scope (`critical_warning`), not addressed.

All fixes were validated together as one green tree:
- `cargo test -p gnr8-core` — all suites pass (incl. the hermetic `pysdk_compile`,
  now exercising the typed-`Book` request-body path for CR-01).
- `make check` — exits 0 (fmt + clippy `-D warnings` + Rust + Go).
- Determinism gate: `pysdk::generate` is byte-identical across two runs.
- Dependency gate: the generated SDK imports only stdlib
  (`__future__`/`typing`/`enum`/`dataclasses`/`json`/`urllib.*`) plus relative
  package imports — no `requests`/`httpx`/`pydantic`/etc.
- Every individual commit was verified to compile (`cargo build --tests`).

## Commit grouping note

All source changes live in one file (`crates/gnr8-core/src/pysdk/emit.rs`) and
several findings share the same new machinery (`safe_ident` / `RESERVED_ARGS` /
`resolve_op_args` / per-dataclass `from_dict`). Findings whose fixes are
genuinely independent and compile in isolation were committed atomically (CR-01,
CR-03, WR-05). The remaining findings (CR-02, CR-04, WR-01, WR-02, WR-03, WR-04)
are mutually interdependent — splitting them further would produce non-compiling
intermediate commits — so they were committed together as one coherent unit.
Each commit leaves a compiling, green tree.

## Fixed Issues

### CR-01: Request-body serialization broken for the typed model the signature demands

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`, `crates/gnr8-core/tests/pysdk_compile.rs`
**Commit:** f58e893
**Applied fix:** The emitted client gained `import dataclasses` and `_do` now
marshals a dataclass body via `dataclasses.asdict(body)` (stdlib, recursive into
nested dataclasses) before `json.dumps`, so the advertised typed happy path works
at runtime. The hermetic round-trip driver was strengthened to construct and send
an actual `Book` `@dataclass` instance (`Book(author=Author(...), format=..., id,
title)`) instead of routing around the bug with a raw dict — so the typed
request-body path is genuinely covered going forward.

### CR-02: Reserved-word field / parameter names emit a SyntaxError

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** Added a fixed `PY_KEYWORDS` set (Python's `keyword.kwlist`,
baked into the emitter — never shelled out) and a `safe_ident` helper that
suffixes a trailing `_` for a keyword or leading-digit name. Every emitted field
attribute, method argument, path-param local, and query-param local is routed
through it. The on-the-wire key (JSON `json_name`, query `p.name`) is preserved
verbatim — the generated `from_dict` binds the original wire key onto the
(possibly renamed) attribute, and the query builder writes the original key while
reading the safe local. Regression test:
`regressions::cr02_reserved_word_field_emits_safe_identifier_keeping_wire_key`.

### CR-03: Enum members collapsing to the same SCREAMING_SNAKE identifier raise TypeError

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 5db5bcc
**Applied fix:** `emit_enum_class` now tracks emitted member names and
deterministically disambiguates collisions by appending `_2`, `_3`, … (first
occurrence keeps the base). A new `enum_member_ident` guards invalid identifiers:
an empty/punctuation-only normalization becomes a stable `MEMBER` placeholder, and
a leading-digit form is prefixed with `_`. The wire `value` string is never
altered — only the Python member *name*. Regression test:
`regressions::cr03_enum_member_collisions_disambiguate_and_unsafe_values_are_guarded`.

### CR-04: `Model(**_data)` fails on extra response keys and decodes nested models shallowly

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** Each dataclass now emits a `from_dict` classmethod that (1) is
forward-compatible — it constructs only from declared fields, so an unknown server
key is silently ignored rather than crashing the constructor — and (2) recurses
into nested types: a named object-schema field decodes via `Nested.from_dict(v)`,
a list-of-named-object decodes via a comprehension, and scalars/enums/maps/unions
pass through. The operation decode site now calls `Model.from_dict(_data)` instead
of `Model(**_data)`. Required fields read `_data["key"]` (a missing required key is
a real protocol error → `KeyError`); optional fields read defensively and keep the
`None` default when absent. Regression test:
`regressions::cr04_from_dict_is_forward_compatible_and_recurses_into_nested_models`.

**Remaining limitation (documented):** A union-typed *success* (2xx) return whose
model is a Python type-alias string (e.g. `BookOrError = "Union[Book, OutOfStock]"`)
cannot dispatch to a single `from_dict` (a union has no single constructor). The
emitted decode would call `.from_dict` on the alias string — this was already
non-functional pre-fix (the old code called `AliasString(**_data)`), so it is no
regression; it is not exercised by any current test (the bookstore `get_book`
returns `BookOrError` but the round-trip only hits its 404/error path). A future
phase should emit a discriminated decode (or `Any` passthrough) for union returns.

### WR-01: Required query parameters silently emitted as optional

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** `resolve_op_args` partitions query params required-first; a
required query param is now a positional argument (no `= None`) and is written to
`_query` unconditionally (no `is not None` guard), so a caller cannot omit it.
Optional query params keep the `= None` form and the presence guard. Ordering
(positional before defaulted) stays valid Python. Regression test:
`regressions::wr01_required_query_param_is_positional_and_always_sent`.

### WR-02: `bool`/typed fields get a `= None` default whose hint forbids `None`

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** An optional-but-not-nullable field is now emitted as
`Optional[T] = None`, widening the hint so the `= None` default is no longer a
type-lie against the value type. A nullable field is already `Optional[..]`, so it
is not double-wrapped. Existing unit tests
(`optional_fields_get_a_none_default_required_do_not`,
`inline_enum_and_union_fields_use_literal_and_union_hints`) were updated to assert
the corrected, type-honest emission (these assert emitter output, not a frozen
acceptance snapshot).

### WR-03: Path/query parameter identifier collisions undetected

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** `resolve_op_args` tracks every emitted argument identifier in a
set seeded with `self` (and `body` when a typed body is present); a collision
(two params whose safe identifier matches, or a param colliding with `self`/`body`)
now returns a typed `CoreError::SdkGen` rather than emitting a duplicate-argument
`def` → `SyntaxError`. Regression test:
`regressions::wr03_param_identifier_collision_is_a_typed_error`.

### WR-04: `_query` wire key vs. snaked local mismatch

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 65bb975
**Applied fix:** Resolved as the downstream symptom of CR-02/WR-03. The f-string
path interpolation and the query builder now use the *safe* resolved identifier
for the local read (matched to the token/param by name, since tokens and path
params are set-equal but not necessarily same-order), while the wire key keeps the
original `p.name`. With keyword-safe identifiers and collision detection in place,
the local can no longer be undefined/wrong.

### WR-05: `emit_init` re-exports duplicate schema names producing duplicate imports/`__all__`

**Files modified:** `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 06ec887
**Applied fix:** `emit_models` now scans schema *names* up front and returns a
typed `CoreError::SdkGen` on a true name collision (two distinct ids mapping to one
Python class), rather than silently emitting two `class Book` definitions and a
duplicated re-export. Single deterministic check, no fallback. Regression test:
`regressions::wr05_duplicate_schema_name_is_a_typed_error`.

## Skipped Issues

None — all in-scope (critical + warning) findings were fixed.

(Info findings IN-01 `_raise -> NoReturn` and IN-02 unused `_package` arg are
outside the `critical_warning` scope and were not addressed this iteration.)

---

_Fixed: 2026-06-25T22:10:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
