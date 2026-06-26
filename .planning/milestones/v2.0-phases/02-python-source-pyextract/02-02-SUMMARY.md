---
phase: 02-python-source-pyextract
plan: 02
subsystem: api
tags: [python, pyextract, ast, symbol-table, facts-contract, static-analysis, stdlib]

# Dependency graph
requires:
  - phase: 02-python-source-pyextract (Plan 01)
    provides: "host seam — run_pyextract() spawns `python3 -m pyextract <target>`, detect_language() dispatch, FastApi/Flask Source built-ins; deserializes into facts::GoFacts"
  - phase: 01-language-neutral-ir
    provides: "neutral facts contract (GoFacts DTO, deny_unknown_fields) the sidecar must emit"
provides:
  - "pyextract/ stdlib-`ast` sidecar package runnable as `python3 -m pyextract <dir>`"
  - "load.py — static *.py discovery + ast.parse of file TEXT (never imports/executes the target)"
  - "symtab.py — OWNED static cross-module symbol table (class/alias/import index + resolve() with a distinct UNRESOLVABLE sentinel)"
  - "types.py — Python annotation AST → byte-exact neutral Type dict (primitive/array/map/named/enum/union) + the optional/nullable field axes"
  - "schemas.py — model(BaseModel)/@dataclass→object, enum.Enum→sorted enum, Union alias→union schema builder"
  - "facts.py — deterministic sorted byte-stable json marshal matching facts.go::sortDoc"
  - "stdlib unittest harness (test_load/test_symtab/test_types/test_facts) provable WITHOUT the Rust host"
affects: [fastapi-green, flask-green, python-snapshot-flip, pyextract-routes]

# Tech tracking
tech-stack:
  added: []  # rule 2: CPython stdlib ONLY (ast/json/sys/os/enum/dataclasses/typing/unittest/subprocess); zero packages
  patterns:
    - "Static-only extraction: read file TEXT, ast.parse, NEVER import/exec/eval/compile/runpy/importlib the target (threat T-static-exec / PYSRC-03)"
    - "Owned cross-module symbol table is the one no-Go-file-analog surface; resolution is static dict lookup of indexed ASTs, never an exec"
    - "One source per fact (rule 3): unresolvable name → a distinct UNRESOLVABLE sentinel → diagnostic + omit, never a guessed {type:any} default"
    - "Determinism = json.dumps(sort_keys=True) for object KEYS + explicit per-array sorts for slice ORDER (union members exempt: source order)"

key-files:
  created:
    - pyextract/__init__.py
    - pyextract/__main__.py
    - pyextract/load.py
    - pyextract/symtab.py
    - pyextract/types.py
    - pyextract/schemas.py
    - pyextract/diagnostics.py
    - pyextract/facts.py
    - pyextract/tests/__init__.py
    - pyextract/tests/test_load.py
    - pyextract/tests/test_symtab.py
    - pyextract/tests/test_types.py
    - pyextract/tests/test_facts.py
  modified: []

key-decisions:
  - "A Literal[...] alias (SortOrder) is inlined-only and NEVER a standalone schema; only a Union alias (BookOrError, referenced by ref_id from a route) becomes a schema — the snapshot is the authority and contains no SortOrder schema"
  - "Python int → {prim:int,bits:64,signed:true}; float → {prim:float,bits:64} (snapshot-fixed widths)"
  - "Optional[T] / T | None unwrap to T and surface a `nullable` signal on the FIELD axis (map_field_annotation returns (type, nullable)); the None arm is never a union member"
  - "Span anchors to ClassDef.lineno honestly this plan; exact line reconciliation against the snapshot is deferred to Plan 03/04 (RESEARCH Q2). The end-to-end golden asserts ids + type/axis shapes, NOT exact lines."

patterns-established:
  - "Pattern (static-only): the sidecar is a parser, never an interpreter — the load-bearing security invariant, proven by the grep gate"
  - "Pattern (owned symtab): hand-rolled resolver over indexed ASTs (rule 2) replaces go/types; cyclic alias chains bail to UNRESOLVABLE deterministically"
  - "Pattern (snapshot-as-spec): correctness is the committed FastAPI graph snapshot's schema section, asserted byte-for-byte by a subprocess golden test"

requirements-completed: [PYSRC-03]

# Metrics
duration: ~22min
completed: 2026-06-25
---

# Phase 2 Plan 2: pyextract stdlib-`ast` Core Summary

**A stdlib-only `pyextract/` sidecar that statically parses a Python service tree (`ast.parse`, never importing it), resolves names through an OWNED cross-module symbol table, maps Python annotations to the byte-exact neutral Type vocabulary with correct four-axis optional/nullable fields, and marshals a sorted, byte-stable facts JSON whose schema half reproduces `snapshot_fastapi_graph` — all proven by a `python3 -m unittest` harness that needs no Rust host.**

## Performance

- **Duration:** ~22 min
- **Started:** 2026-06-25
- **Completed:** 2026-06-25
- **Tasks:** 3 (Task 2 was TDD: RED → GREEN)
- **Files created:** 13

## Accomplishments
- `pyextract/` is runnable as `python3 -m pyextract <dir>`: argv guard → canonical target → `run()` → facts JSON to stdout, all tool-diagnostics to stderr + exit 1.
- Static-only loader: walks `*.py` sorted, `ast.parse`s file TEXT only; a `SyntaxError` becomes a WARN diagnostic, never an abort. Parses 3.10+ target syntax (`X | Y`, `list[T]`) on 3.9 without executing it.
- Owned cross-module symbol table: per-module class/alias/import index; `resolve()` follows `from x import Y` statically and returns a distinct `UNRESOLVABLE` sentinel for foreign names (rule 3), with cyclic-import safety.
- Annotation → neutral Type mapper covering primitives (int64-signed/float64), `list`/`dict`, `Literal`→sorted inline enum, `Union`/`A|B`→source-order union, named class/enum→`named` ref; `Optional`/`|None`→FIELD nullable axis.
- Schema builder over `BaseModel`/`@dataclass`→object, `enum.Enum`→sorted-value enum, `Union` alias→union schema; FieldFacts carry the four optional/nullable axes (all four BookFilters combinations distinct).
- Deterministic marshal mirroring `facts.go::sortDoc` (schemas by id, fields by json_name, enums lexical, diagnostics by file/line/message; union members exempt) + `sort_keys` for object keys; output is byte-identical across runs.
- End-to-end subprocess golden over `fixtures/fastapi-bookstore` asserts `module` + the exact 8 schema ids + type/axis shapes from `snapshot_fastapi_graph`.

## Task Commits

Each task was committed atomically:

1. **Task 1: Entrypoint + loader + owned cross-module symbol table** - `63b12d2` (feat)
2. **Task 2: Annotation → neutral Type mapper + schema builder + four-axis fields (TDD)** - `c245992` (test, RED) → `779f565` (feat, GREEN)
3. **Task 3: Deterministic facts marshal + end-to-end schema golden harness** - `c4d4207` (feat)

**Plan metadata:** (this commit) (docs: complete plan)

## Files Created/Modified
- `pyextract/__init__.py` - package marker + the static-only / stdlib-only / one-source-per-fact invariants.
- `pyextract/__main__.py` - argv guard, canonicalize target, `run()` orchestration (load → diagnostics → symtab → schemas → module basename → routes(empty) → marshal), errors → stderr + exit 1.
- `pyextract/load.py` - static `*.py` discovery + `ast.parse` of file TEXT; SyntaxError → diagnostic; dotted-module id derivation.
- `pyextract/symtab.py` - owned cross-module symbol table; class/alias/import index; `resolve()` + `UNRESOLVABLE` sentinel; per-module abs_path for diagnostics.
- `pyextract/types.py` - `map_annotation` / `map_field_annotation` → neutral Type dicts; Optional/`|None` stripping into the nullable axis.
- `pyextract/schemas.py` - `build_schemas`: class (model/dataclass/enum) + Union-alias → SchemaFact; FieldFact four-axis rules.
- `pyextract/diagnostics.py` - WARN accumulator emitting `severity/message/file/line` dicts.
- `pyextract/facts.py` - `build_doc` + deterministic `marshal` (per-array sorts + `sort_keys`).
- `pyextract/tests/{test_load,test_symtab,test_types,test_facts}.py` - 43 stdlib-unittest cases incl. the subprocess golden + determinism guard.

## Decisions Made
- **Literal alias inlined, never a schema:** `SortOrder = Literal[...]` is only ever rendered inline (`BookFilters.sort` → an inline `enum` body); only `Union` aliases (`BookOrError`, referenced by `ref_id`) become standalone schemas. The committed snapshot contains no `SortOrder` schema and is authoritative.
- **int64-signed / float64** primitive widths, hardcoded from the snapshot (Python ints/floats map to 64-bit).
- **Nullable is a FIELD axis:** `map_field_annotation` returns `(type, nullable)`; the `None` arm of `Optional`/`Union[..., None]`/`A | None` never appears as a union member.
- **Span anchors to `ClassDef.lineno` this plan;** exact line reconciliation against the snapshot is the explicit job of Plan 03/04 (RESEARCH Q2). The golden test deliberately asserts ids + type/axis shapes, not exact span lines.

## Deviations from Plan

None - plan executed exactly as written. The only judgment call (excluding the `Literal` alias from standalone schemas) is a direct reconciliation against the authoritative snapshot, recorded under Decisions rather than as a deviation because the plan's interfaces explicitly name the snapshot as the spec.

## Issues Encountered
- The plan's Task-2 prose ("top-level `Literal`-or-`Union` alias into an `enum`/`union` body") would have emitted an extra `app.models.SortOrder` schema absent from `snapshot_fastapi_graph`. Resolved by treating the snapshot as authoritative: `Literal` aliases are inline-only; only `Union` aliases become schemas. The end-to-end golden now asserts the exact 8-id snapshot set.
- `diff`/`cmp` are absent on the sandbox; determinism was verified by comparing two subprocess runs in Python (byte-identical, 4894 bytes).

## CLAUDE.md Compliance
- **Rule 1 (no coupling to another tool's conventions):** every fact derives from Python's own constructs — type annotations, `BaseModel`/`@dataclass`/`enum.Enum` base/decorator NAMES recognized statically. Nothing reads pydantic runtime schema, FastAPI `/openapi.json`, or any annotation dialect.
- **Rule 2 (no third-party deps):** the sidecar imports only CPython stdlib (`ast`, `json`, `sys`, `os`, plus `subprocess` in tests). Grep gate over `pyextract/*.py` confirms zero third-party imports and the target's pydantic/fastapi/flask are never imported. The symbol table is hand-rolled (no library).
- **Rule 3 (no fallback / one path):** an unresolvable name returns the distinct `UNRESOLVABLE` sentinel → a diagnostic + the fact is omitted; there is never a guessed default. No try-A-then-B anywhere.
- **Static-only (PYSRC-03 / T-static-exec):** grep gate `exec\(|eval\(|compile\(|importlib|__import__|runpy` over `pyextract/*.py` returns NOTHING — the sidecar only `ast.parse`s file text.
- **Determinism:** byte-identical output across two runs, verified.

## Next Phase Readiness
- The shared schema/type core is landed and snapshot-accurate for the FastAPI schema half. Plan 03 (FastAPI route recognizer) and Plan 04 (Flask) build `routes.py` on top of this loader + symtab + types, populate the currently-empty `routes` array, reconcile exact span/diagnostic lines, and flip the four `#[ignore]` Python snapshot tests green.
- No blockers. The Rust workspace is fully green (Go path regression-free, 128 lib tests + integration); the four Python snapshot tests remain `#[ignore]` red-by-design as the plan requires.

## Self-Check: PASSED

- All 13 `pyextract/` files exist on disk.
- Task commits `63b12d2`, `c245992`, `779f565`, `c4d4207` all present in git history.
- `python3 -m unittest discover -s pyextract/tests` → 43 passed.
- Static-only + stdlib-only grep gates over `pyextract/*.py` both empty.
- `python3 -m pyextract fixtures/fastapi-bookstore` → module `fastapi-bookstore`, 8 snapshot schema ids, byte-identical across two runs.

---
*Phase: 02-python-source-pyextract*
*Completed: 2026-06-25*
