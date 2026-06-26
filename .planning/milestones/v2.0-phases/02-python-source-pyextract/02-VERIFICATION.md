---
phase: 02-python-source-pyextract
verified: 2026-06-25T18:05:57Z
status: passed
score: 5/5 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
---

# Phase 2: Python Source — `pyextract` Verification Report

**Phase Goal:** A developer can turn a real FastAPI service (full) and a Flask service (typed envelope) into the neutral IR via a static `pyextract` sidecar that never imports or executes the target code, so the existing Rust lowering produces OpenAPI 3.1 for Python services unchanged.
**Verified:** 2026-06-25T18:05:57Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria + PLAN must_haves)

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 (SC1) | Extract routes, path/query params, request bodies (Pydantic/`@dataclass`), response models, status codes from FastAPI; routes + blueprint/`APIRouter` prefixes + opt-in typed DTOs from Flask | ✓ VERIFIED | `pyextract/routes.py` (27.8KB): `APIRouter`/`Blueprint` prefix recorded SEPARATELY (lines 60-75, 368-383), `response_model=`/`status_code=` (lines 274-298), `<int:order_id>`→`/{order_id}` int64 converter, method-derived status (POST→201). Live run: FastAPI fixture → 4 routes/8 schemas; Flask → 4 routes/4 schemas. All 4 snapshot tests GREEN in `make check`. |
| 2 (SC2) | Sidecar resolves types statically via stdlib `ast` + owned cross-module symbol table; never imports/executes target | ✓ VERIFIED | Grep gate clean: no `exec(`/`eval(`/`compile(`/`importlib`/`__import__`/`runpy` in `pyextract/*.py`. Only stdlib imports: `ast`,`json`,`os`,`sys`,`traceback`. No `fastapi`/`flask`/`pydantic`/`marshmallow`. `load.py:98` parses file TEXT via `ast.parse`. `symtab.py` (6.7KB) is an owned cross-module table. The one `ast.parse(node.value, mode="eval")` (types.py:209) is a STATIC string parse of a forward-ref, not an `eval()` call. |
| 3 (SC3) | Unresolvable/untyped surfaces produce diagnostics, never guessed facts — no fallback | ✓ VERIFIED | Flask fixture emits 3 WARN diagnostics with exact file:line provenance (lines 42/69/78): untyped query, untyped response, untyped request body — each OMITS the fact. routes.py untyped branch has explicit "no fallback" comments. CR-01/02/03 + WR-01..03 rule-3 fixes harden guessed-fact paths into diagnose+omit (per 02-REVIEW-FIX.md), with regression tests. detect_language mixed Go/Python tree → typed Config error (not silent pick). |
| 4 (SC4) | Developer enables Python extraction from `.gnr8/` via `FastApi`/`Flask` Source built-ins; FastAPI red snapshot turns green through reused Rust pipeline | ✓ VERIFIED | `crates/gnr8-core/src/sdk/builtins.rs`: `pub struct FastApi` + `pub struct Flask`, both `impl Source` with `load()` calling `crate::analyze::build_graph` — the SAME path the Go source uses. FastAPI + Flask snapshots (graph+openapi) GREEN with NO `#[ignore]`, driven through real `build_graph → run_pyextract → to_openapi`. |
| 5 (PLAN) | Single deterministic build_graph language dispatch (no fallback); byte-identical output | ✓ VERIFIED | `analyze/mod.rs::detect_language` is one match over (has_go, has_python) with all 4 arms typed: (true,true)→Config error, (true,false)→Go, (false,true)→Python, (false,false)→Config error. `build_graph` dispatches Python→`run_pyextract`, Go→`run_goextract`. Determinism tests `fastapi_build_graph_is_byte_identical`, `flask_*`, `*_to_openapi_*` all PASS. |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `crates/gnr8-core/src/error.rs` | `CoreError::PythonToolchainMissing` | ✓ VERIFIED | Variant at line 45, constructed at line 212; no production unwrap/expect/panic. |
| `crates/gnr8-core/src/analyze/helper.rs` | `pyextract_dir()` + `run_pyextract`/`run_pyextract_with` | ✓ VERIFIED | Lines 36, 115, 121; subprocess driver via `python3 -m pyextract`. |
| `crates/gnr8-core/src/analyze/mod.rs` | language-aware `build_graph` dispatch | ✓ VERIFIED | `detect_language` (line 48) single-path; `build_graph` (125) routes Python→`run_pyextract` (130). |
| `crates/gnr8-core/src/sdk/builtins.rs` | `FastApi` + `Flask` Source built-ins | ✓ VERIFIED | `pub struct FastApi` + `pub struct Flask`; both `load()` call `build_graph`. |
| `pyextract/__main__.py` | argv→facts JSON to stdout | ✓ VERIFIED | Uses `ast`; WR-07 adds `traceback` on internal errors (stdlib). |
| `pyextract/symtab.py` | owned cross-module symbol table | ✓ VERIFIED | 6.7KB; static import resolution. |
| `pyextract/types.py` | annotation AST → neutral Type dict | ✓ VERIFIED | 13.5KB; four-axis Type mapping; CR-01/02, WR-01/02/03 fixes. |
| `pyextract/facts.py` | deterministic sorted json marshal | ✓ VERIFIED | Present (3.3KB); byte-stable marshal (confirmed by determinism tests). |
| `pyextract/routes.py` | FastAPI + Flask route recognition | ✓ VERIFIED | 27.8KB; routes/params/body/response/prefix/converter/diagnostics. |
| `crates/gnr8-core/tests/snapshot_fastapi_graph.rs` | GREEN graph snapshot (no `#[ignore]`) | ✓ VERIFIED | `fn graph_matches_expected_for_fastapi` calls real `build_graph`; passes in gate. |
| `crates/gnr8-core/tests/snapshot_flask_graph.rs` | GREEN graph snapshot (no `#[ignore]`) | ✓ VERIFIED | `fn graph_matches_expected_for_flask` calls real `build_graph`; passes in gate. |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| `analyze/mod.rs::build_graph` | `helper::run_pyextract` | language detection branch | ✓ WIRED | `Lang::Python => helper::run_pyextract(&target)?` (mod.rs:130). |
| `builtins.rs::FastApi::load` | `analyze::build_graph` | resolve against project_root | ✓ WIRED | `crate::analyze::build_graph(&resolved...)` (builtins.rs:142). |
| `builtins.rs::Flask::load` | `analyze::build_graph` | same path as Go source | ✓ WIRED | Same `build_graph` call; language by target detection. |
| `pyextract/__main__.py` | `pyextract/load.py` | `load(dir)` walks `*.py` + `ast.parse` | ✓ WIRED | load.py:98 `ast.parse(source...)`, static only. |
| `pyextract/types.py` | `pyextract/symtab.py` | `resolve(name,in_module)` | ✓ WIRED | named/foreign classification via owned table. |
| `snapshot_fastapi_graph.rs` | `build_graph (→run_pyextract)` | dispatch over fastapi-bookstore | ✓ WIRED | Test calls `build_graph(FIXTURE_DIR)`; GREEN. |
| `snapshot_flask_graph.rs` | `build_graph (→run_pyextract)` | dispatch over flask-bookstore | ✓ WIRED | Test calls `build_graph(FIXTURE_DIR)`; GREEN. |
| `routes.py (Flask untyped)` | `diagnostics.py` | `warn(message,file,line)` | ✓ WIRED | 3 live WARN diagnostics at lines 42/69/78. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| FastAPI snapshots | graph/openapi | real `build_graph → run_pyextract` over fastapi-bookstore | Yes — 4 routes, 8 schemas, OpenAPI 3.1.0 | ✓ FLOWING |
| Flask snapshots | graph/openapi | real `build_graph → run_pyextract` over flask-bookstore | Yes — 4 routes, 4 schemas, 3 diagnostics | ✓ FLOWING |

Snapshots are NOT hardcoded: `git diff --stat HEAD` over committed `*fastapi*`/`*flask*` snapshot files is EMPTY, proving they were turned green by real extraction with zero snapshot edits.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| `make check` green gate | `make check` | exit 0 (confirmed twice) | ✓ PASS |
| 4 Python acceptance snapshots GREEN in gate | grep gate log | all 4 `... ok`, none ignored | ✓ PASS |
| Only NestJS red under `--ignored` | `cargo test -p gnr8-core --no-fail-fast -- --ignored` | exactly 2 FAILED, both NestJS (red-by-design) | ✓ PASS |
| Python unittest suite | `python3 -m unittest discover pyextract` | 87 tests OK | ✓ PASS |
| Committed snapshots untouched | `git diff --stat HEAD -- *fastapi* *flask*` | empty | ✓ PASS |
| Real FastAPI extraction | `python3 -m pyextract fixtures/fastapi-bookstore` | 4 routes / 8 schemas / 0 diags | ✓ PASS |
| Real Flask extraction | `python3 -m pyextract fixtures/flask-bookstore` | 4 routes / 4 schemas / 3 diags | ✓ PASS |
| Static-only grep gate | grep exec/eval/compile/importlib/__import__/runpy | no matches | ✓ PASS |
| Stdlib-only grep gate | import scan | only ast/json/os/sys/traceback | ✓ PASS |
| Determinism (byte-identical) | determinism.rs in gate | fastapi+flask graph & openapi all ok | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| PYSRC-01 | 02-03 | Extract FastAPI routes/params/bodies/response/status | ✓ SATISFIED | routes.py FastAPI recognition; FastAPI snapshots GREEN; 4 routes/8 schemas. |
| PYSRC-02 | 02-04 | Extract Flask routes + prefixes + opt-in typed DTOs | ✓ SATISFIED | Blueprint url_prefix separate, `<int:>` converter, method-derived status, typed DTOs; Flask snapshots GREEN. |
| PYSRC-03 | 02-02 | Static stdlib `ast` + owned symbol table; never import/execute | ✓ SATISFIED | Static-only + stdlib-only grep gates clean; symtab.py owned table. |
| PYSRC-04 | 02-04 | Untyped/unresolvable → diagnostics, no fallback | ✓ SATISFIED | 3 Flask WARN diagnostics with provenance; CR/WR rule-3 fixes + regression tests. |
| PYSRC-05 | 02-01 | `.gnr8/` `FastApi`/`Flask` Source built-ins | ✓ SATISFIED | `FastApi`/`Flask` structs `impl Source`, drive `build_graph`. |

All 5 phase requirements declared in plans and accounted for. No ORPHANED requirements (REQUIREMENTS.md maps PYSRC-01..05 to Phase 2; all appear in plan `requirements:` fields).

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| (none) | — | No TBD/FIXME/XXX in phase-modified files | — | Clean |
| (none) | — | No TODO/HACK/placeholder in phase-modified files | — | Clean |
| (none) | — | No production unwrap/expect/panic in new Rust src | — | Clean (test-mod uses excluded) |

CLAUDE.md invariants honored: rule 1 (facts from own Python `ast`/types, never another tool's annotations), rule 2 (zero OSS deps — pyextract stdlib-only, gnr8-core adds no crate), rule 3 (single deterministic dispatch, untyped→diagnostic not fallback), rule 4 (security/cross-cutting via `.gnr8/` config). The 2 NestJS reds are intentional Phase-4 contracts, not Phase-2 failures.

### Human Verification Required

None. All success criteria are programmatically verifiable through the green gate, live extraction runs, grep gates, and the byte-identical snapshot/determinism tests. No visual, real-time, or external-service behavior is involved.

### Gaps Summary

No gaps. The phase goal is fully achieved and observable in the codebase:
- A real FastAPI service (full) and a Flask service (typed envelope) are turned into the neutral IR by the static `pyextract` sidecar, which only `ast.parse`s file text and imports stdlib only (never imports/executes the target).
- The reused Rust lowering produces OpenAPI 3.1.0 for both services, with byte-identical output, through the SAME `build_graph` the Go path uses.
- Untyped/unresolvable surfaces emit diagnostics with provenance and omit facts — no fallback path.
- `make check` exits 0 with all 4 Python acceptance snapshots GREEN (zero snapshot edits, confirmed by empty git diff); only the 2 NestJS Phase-4 contracts remain red-by-design under `--ignored`.

---

_Verified: 2026-06-25T18:05:57Z_
_Verifier: Claude (gsd-verifier)_
