# Phase 2: Python Source — `pyextract` - Research

**Researched:** 2026-06-25
**Domain:** Static Python AST extraction (stdlib `ast`) → language-neutral JSON facts contract; Rust subprocess seam
**Confidence:** HIGH (everything is codebase-grounded: the facts contract, the goextract analog, and the byte-exact acceptance snapshots are all committed and read in full)

## Summary

This phase builds `pyextract`, a stdlib-`ast`-only Python sidecar that statically reads a FastAPI service (full envelope) and a Flask service (typed envelope) and emits the **same** neutral JSON facts document the Go sidecar (`goextract/`) emits. The Rust host deserializes that JSON with `deny_unknown_fields` and runs it through the *unchanged* `ApiGraph::from_facts` → lowering → OpenAPI pipeline. The acceptance target is byte-exact and already committed: four red-by-design snapshots (`snapshot_fastapi_graph`, `snapshot_fastapi_openapi`, `snapshot_flask_graph`, `snapshot_flask_openapi`) plus the determinism guard flip green the moment the extractor produces the right facts, with **zero snapshot edits**.

The work splits cleanly into three concerns: (1) a Python sidecar that recognizes FastAPI/Flask routing constructs and resolves types via an **owned cross-module symbol table** built from parsed ASTs (never importing/executing target code); (2) the Rust seam — a `run_pyextract` driver analogous to `run_goextract`, plus **language dispatch inside `build_graph`** because the test harness calls the single `build_graph(FIXTURE_DIR)` entry point for Python fixtures too; and (3) `FastApi`/`Flask` `Source` built-ins in `builtins.rs` cloned from `GoGin`.

The single most important constraint is that **the committed snapshots ARE the specification**. Every field name, every sort order, every diagnostic string, every `bits: 64` integer width, and the exact `module: fastapi-bookstore` value are already fixed. The extractor is correct iff it reproduces them; this RESEARCH maps each snapshot fact back to the precise Python source construct that must produce it.

**Primary recommendation:** Build `pyextract/` as a pure-stdlib package mirroring `goextract/internal/` (load → symbol-table → routes → types → diagnostics → facts-marshal), emit absolute span paths + `module = basename(target_dir)`, and dispatch language in `build_graph` by detecting Python vs Go in the target dir. Drive correctness exclusively against the four committed snapshots.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Parse target Python, recognize routes/types | `pyextract/` (Python sidecar) | — | Static `ast` analysis is the sidecar's whole job; it owns all Python-specific knowledge |
| Cross-module name/type resolution | `pyextract/` symbol table | — | Resolution must never import/execute; only the sidecar holds the parsed ASTs |
| Neutral facts JSON emission (sorted, byte-stable) | `pyextract/` facts module | — | Contract boundary; the sidecar marshals, the host deserializes strictly |
| Subprocess invocation + typed errors | `analyze::helper` (Rust) | — | Mirrors `run_goextract`; the host never parses Python |
| Language dispatch (Go vs Python target) | `analyze::build_graph` (Rust) | `analyze::helper` | Single `build_graph` entry is shared by the harness across languages |
| Facts → `ApiGraph` lowering, sorting, relativization | `graph::ApiGraph::from_facts` (Rust) | — | **Reused unchanged** — the v2.0 bet; Python facts flow through the same code |
| `.gnr8/` enablement (which dirs, which framework) | `sdk::builtins` `FastApi`/`Flask` Source | — | Config-is-code (rule 4); thin wrapper over `build_graph` |
| OpenAPI 3.1 generation | `lower::to_openapi` (Rust) | — | Reused unchanged; language-agnostic |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Python stdlib `ast` | bundled with CPython 3.9.25 [VERIFIED: `python3 --version` on sandbox = 3.9.25] | Parse target `.py` files into an AST without executing them | The ONLY permitted Python dependency (CLAUDE.md rule 2 / PYSRC-03); the language's own reference parser, the Python analog of `go/types` |
| Python stdlib `json` | bundled | Marshal the neutral facts document to stdout | stdlib-only; `sort_keys=True` + explicit list sorting gives byte-stable output |
| Python stdlib `sys`, `pathlib`, `os` | bundled | Argv, file discovery, path handling | stdlib-only |
| Rust `std::process::Command` | std | Spawn `python3` as a subprocess with discrete args | Already the goextract pattern; no shell, no interpolation (T-02-01) |
| serde / serde_json | existing workspace dep (debt) | Deserialize facts JSON in the host | Already the goextract deserialize path; this phase adds **zero** new Rust deps |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ast.parse(src, filename, type_comments=False)` | stdlib | Entry point per module | Parse each discovered `.py`; pass `filename` so `node.lineno` provenance is correct |
| `ast.get_source_segment` | stdlib (3.8+) | Optional, for diagnostic text | Only if needed; line numbers come from `node.lineno` directly |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| stdlib `ast` | `importlib` + runtime introspection / FastAPI `/openapi.json` | **FORBIDDEN** — importing = executing (security boundary, PYSRC-03); `/openapi.json` is another tool's output (rule 1). Not an option. |
| Owned symbol table | `typing.get_type_hints` (runtime) | **FORBIDDEN** — requires importing the module. Static-only. |
| `python3 -m pyextract` | A frozen/zipapp bundle | Plain package dir mirrors `goextract/` and needs no build step; prefer it. |

**Installation:** No installation. `pyextract/` is committed source run via the already-present `python3`. No `pip install`, no `requirements`. (The fixtures' `requirements.txt` files are NOT installed — the sidecar never imports fastapi/flask/pydantic; it parses their *source usage*.)

**Version verification:** `python3 --version` → `Python 3.9.25` at `/usr/bin/python3` [VERIFIED: Bash on sandbox]. Note: `ast` in 3.9 **parses** PEP 604 `X | Y` and `list[T]` subscript syntax found in the *target* fixtures — it does not need to *evaluate* them, so 3.9 is sufficient even though the fixtures use 3.10-style spellings (`__future__ annotations` is present in every fixture file, which is why the source is valid to 3.9's parser as well).

## Package Legitimacy Audit

> Not applicable in the usual sense: this phase installs **zero** external packages in either ecosystem (CLAUDE.md rule 2; PYSRC-03; XLANG-05). The Python sidecar uses only the CPython standard library; `gnr8-core` adds no new Rust crate. There is no registry surface to slopcheck.

| Package | Registry | Disposition |
|---------|----------|-------------|
| (none) | — | No third-party packages introduced this phase |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*Verification: the only dependency is the CPython stdlib (`ast`, `json`, `sys`, `pathlib`, `os`), confirmed present on the sandbox via `python3 --version`. Any task that proposes a `pip install` or a new Rust crate is a rule-2 defect and must be rejected.*

## Architecture Patterns

### System Architecture Diagram

```
.gnr8/ Pipeline  (user code, rule 4)
   │  FastApi::new().inputs(["."])   /   Flask::new().inputs(["."])
   ▼
Source::load(cx)                       [crates/gnr8-core/src/sdk/builtins.rs]
   │  resolves input against cx.project_root
   ▼
analyze::build_graph(target_dir)       [crates/gnr8-core/src/analyze/mod.rs]
   │  ── LANGUAGE DISPATCH (NEW) ──┐
   │   detect Go vs Python target  │
   ▼                               ▼
run_goextract(dir)        run_pyextract(dir)        [analyze/helper.rs]
 (existing)                 │  Command::new("python3")
                           │    .args(["-m","pyextract", target_dir])
                           │    .current_dir(repo_root_holding_pyextract)
                           │    capture stdout → bytes
                           ▼
              ┌────────────  pyextract  (Python, stdlib ast only) ───────────┐
              │  __main__:  argv → target_dir                                │
              │     ▼                                                        │
              │  load:     walk *.py under target, ast.parse each            │
              │     ▼                                                        │
              │  symtab:   index ClassDef/Assign(alias)/Enum by qualified    │
              │            name; resolve `from x import Y` statically        │
              │     ▼                                                        │
              │  routes:   recognize @app.<method> / APIRouter(prefix=) /    │
              │            @bp.route(methods=) / Blueprint(url_prefix=);      │
              │            read handler signature for params/body/response    │
              │     ▼                                                        │
              │  types:    map Python types → neutral Type enum via symtab    │
              │     ▼                                                        │
              │  diag:     untyped request.json / untyped query / dynamic     │
              │            prefix / foreign type → DiagnosticFact (no guess)   │
              │     ▼                                                        │
              │  facts:    sort every slice; json.dumps → stdout             │
              └──────────────────────────────────────────────────────────────┘
                           │  JSON facts document (same shape as goextract)
                           ▼
serde_json::from_slice::<facts::GoFacts>  (deny_unknown_fields)   [analyze/facts.rs]
   │  (Rust DTO is generic — its doc says "every language sidecar emits this")
   ▼
ApiGraph::from_facts(facts, module_root)   ── REUSED UNCHANGED ──  [graph/mod.rs]
   │  sorts ops/schemas/params/fields/enums; relativizes spans vs module_root
   ▼
lower::to_openapi(ir, …)                    ── REUSED UNCHANGED ──  [lower/]
   ▼
OpenAPI 3.1 document  +  ApiGraph  →  the four committed snapshots
```

### Recommended Project Structure
```
pyextract/                       # new top-level dir (mirrors goextract/)
├── __main__.py                  # argv → target_dir → run() → json to stdout; nonzero exit on error
├── load.py                      # discover *.py under target_dir, ast.parse each into a Module record
├── symtab.py                    # owned cross-module symbol table (class/enum/alias index + import resolution)
├── routes.py                    # FastAPI + Flask route recognition (decorators, prefixes, methods)
├── types.py                     # Python annotation AST → neutral Type dict (Prim/WellKnown/Array/Map/Named/Object/Enum/Union/Any)
├── schemas.py                   # ClassDef (BaseModel/@dataclass) + Enum/Literal → SchemaFact dict
├── diagnostics.py               # diagnostic accumulator (severity/message/file/line)
└── facts.py                     # the neutral facts dict builders + deterministic json.dumps marshal
```
(Module names are Claude's discretion per CONTEXT; this mirrors `goextract/internal/{load,routes,handlers,types,diag,facts}`.)

### Pattern 1: Stdlib-only sidecar → neutral JSON → strict deserialize (the v2.0 narrow waist)
**What:** The sidecar emits exactly the JSON the Rust `facts::GoFacts` DTO accepts. The DTO is already language-neutral — its module doc says *"every language sidecar emits this one shared facts contract"* and no field name is Go-specific.
**When to use:** Always — this is the whole contract.
**Example (the neutral facts shape pyextract must emit, from the committed Rust DTO):**
```jsonc
// Source: crates/gnr8-core/src/analyze/facts.rs (GoFacts / RouteFact / SchemaFact / Type)
{
  "module": "fastapi-bookstore",            // = basename(target_dir); copied verbatim into graph.module
  "routes": [ {
    "method": "GET", "path": "/", "handler": "list_books", "operation_id": "list_books",
    "params": [ { "name": "genre", "location": "query", "required": true,
                  "schema": {"type":"primitive","of":{"prim":"string"}},
                  "span": {"file":"<ABS>/app/main.py","start_line":39,"end_line":39} } ],
    "request_body": null,                    // or {"ref_id":"app.models.Book"}
    "responses": [ {"status":200,"body":{"ref_id":"app.models.ListBooksResponse"}} ],
    "span": {"file":"<ABS>/app/main.py","start_line":38,"end_line":38}
  } ],
  "schemas": [ {
    "id": "app.models.Author", "name": "Author",
    "body": {"type":"object","of":[ {"json_name":"bio","required":true,"optional":false,
              "nullable":true,"schema":{"type":"primitive","of":{"prim":"string"}},
              "description":null,"example":null} ]},
    "span": {"file":"<ABS>/app/models.py","start_line":53,"end_line":53}
  } ],
  "diagnostics": [ {"severity":"WARN","message":"…","file":"<ABS>/app/routes.py","line":42} ]
}
```
**Key shape rules (from facts.rs, all `deny_unknown_fields`):**
- `Type` is adjacently tagged: `{"type":"<variant>","of":<payload>}`. `Any` is `{"type":"any","of":{}}` (empty object, NOT a bare string).
- `Prim` is internally tagged on `prim`: `{"prim":"string"}`, `{"prim":"bool"}`, `{"prim":"int","bits":64,"signed":true}`, `{"prim":"float","bits":64}`, `{"prim":"bytes"}`.
- `WellKnown` is a plain snake_case string payload: `{"type":"well_known","of":"uuid"}`.
- `Named` payload is the bare id string: `{"type":"named","of":"app.models.Author"}`.
- `Enum` payload is a string array: `{"type":"enum","of":["asc","desc"]}`.
- `Union` payload is an array of `Type`: `{"type":"union","of":[ … ]}`.
- `FieldFact` keys EXACTLY: `json_name, required, optional, nullable, schema, description, example` — `description`/`example` are always `null` here (those were annotation-only facts, now gone).
- `request_body`/`response.body` are `{"ref_id": "<id>"}` (a `TypeRef`), or `null`.
- A diagnostic has EXACTLY `severity, message, file, line` (line is a single `u32`, NOT a span).

### Pattern 2: Emit ABSOLUTE span/diagnostic paths; let the Rust host relativize
**What:** Spans in the snapshot read `app/main.py`, but the Rust `from_facts`/`relativize` STRIPS the `module_root` prefix on a path-separator boundary. The Go sidecar emits canonical absolute paths; pyextract must do the same.
**Why:** `relativize(file, root)` only strips when `file` starts with `root + "/"`. If pyextract emitted already-relative `app/main.py`, the strip is a no-op and it still matches — BUT the helper resolves `target_dir` to a CANONICAL absolute path (`resolve_target`), and the determinism + portability guarantee depends on the sidecar seeing and emitting paths under that same canonical root. **Recommendation:** emit `os.path.join(canonical_target_dir, relpath)` (absolute), exactly like goextract, so `relativize` produces `app/main.py`.
**Example:**
```python
# Source: mirrors goextract canonical-abspath behavior; Rust strips via graph::relativize
span = {"file": str(abs_file), "start_line": node.lineno, "end_line": node.lineno}
```

### Pattern 3: `module = basename(target_dir)` (NOT a package path)
**What:** The snapshots show `module: fastapi-bookstore` and `module: flask-bookstore` — the directory basename, because `from_facts` copies `facts.module` verbatim and the Python target has no go-module path.
**Example:**
```python
module = pathlib.Path(canonical_target_dir).name   # "fastapi-bookstore"
```

### Pattern 4: Schema id = `<dotted-module>.<ClassName>`; path = group-relative
**What:** Snapshot ids are `app.models.Book`, `app.dto.OrderInput` — the import-dotted module path (relative to target root, `/`→`.`, drop `.py`) joined with the class name. Route paths are group-relative (`/`, `/{book_id}`); the `/books` (`APIRouter(prefix=)`) and `/orders` (`Blueprint(url_prefix=)`) prefixes are **NOT folded into the path** (rule 1 — they are a lowering-time base path; the snapshots show `base_path: /` because no transform sets it, and operation paths are prefix-free).
**Path param syntax conversion:**
- FastAPI `"/{book_id}"` → neutral `"/{book_id}"` (already brace form; keep verbatim).
- Flask `"/<int:order_id>"` → neutral `"/{order_id}"` (strip the `<int:...>` converter, brace the name; the `int` converter also drives `book_id`/`order_id` param schema = `int64`).

### Anti-Patterns to Avoid
- **Folding the router prefix into the path** — `/books/` must stay `/` (rule 1; the prefix is a base path, not a code-derived path).
- **Reading pydantic/marshmallow/FastAPI runtime artifacts** — derive every fact from the Python type annotations themselves (rule 1). The fixtures import `pydantic.BaseModel` only as a *base class to recognize statically*, never to introspect at runtime.
- **A fallback when a type can't be resolved** — emit a diagnostic and OMIT the fact (rule 3). Never "guess string".
- **Ranging an unordered dict into output** — sort every collection before marshal (the goextract Pitfall-1 lesson; determinism guard enforces it).
- **`bits: 32` for Python `int`/`float`** — the snapshots use `int` → `{"prim":"int","bits":64,"signed":true}` and `float` → `{"prim":"float","bits":64}`. Python ints/floats map to 64-bit signed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parsing Python | A hand-rolled tokenizer/parser | stdlib `ast.parse` | `ast` IS the language's own parser; rule 2 permits stdlib, and re-parsing Python by hand is absurd risk |
| Facts → graph lowering | A second Python→graph path in Rust | `ApiGraph::from_facts` (reused) | The neutral contract is language-agnostic; forking lowering breaks the v2.0 bet and IR-03 |
| Sorting/relativizing in the host | Re-sorting Python facts differently | `from_facts` already sorts ops/schemas/params/fields/enums + relativizes spans | The Rust side normalizes; the sidecar only needs its own internal determinism |
| Subprocess error taxonomy | New ad-hoc error strings | A `PythonToolchainMissing` variant mirroring `GoToolchainMissing` + reuse `HelperExit`/`FactsParse` | Typed errors, no panics (RUST-04); consistent with the Go driver |
| JSON serialization in the sidecar | A custom JSON writer | stdlib `json.dumps(…, sort_keys=True, separators=(",", ":"))` | stdlib-only; deterministic |

**Key insight:** The entire Rust pipeline downstream of the facts JSON is *already written and tested for Go*. This phase's only Rust work is (a) a ~30-line subprocess driver, (b) language dispatch in `build_graph`, and (c) two thin `Source` built-ins. All real new logic lives in the Python sidecar, and its correctness target is fully specified by the committed snapshots.

## Runtime State Inventory

> Greenfield-ish: this phase ADDS a sidecar and a seam; it renames/migrates nothing. Most categories are N/A, but two host-side seams currently hardcode Go and MUST be made language-aware — documented here so the planner treats them as edits, not new files.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — no datastore, no persisted keys. Verified: facts flow stdout→stdin only. | none |
| Live service config | None — no external service holds state. Verified by repo grep. | none |
| OS-registered state | None — no scheduled tasks/daemons. | none |
| Secrets/env vars | None. `CI=true` / `INSTA_UPDATE=no` already govern snapshot acceptance; no new env. | none |
| Build artifacts | None for Python (no compile step). The `.snap.new` files present (`*_graph.snap.new`, `*_openapi.snap.new`) are stale insta artifacts from the red runs — they should be cleaned/ignored when the tests turn green. | clean `*.snap.new` after green |
| **Host-side hardcoded-Go seams (code edit)** | `analyze::build_graph` calls `run_goextract` unconditionally; `diagnostics::collect` does too; `helper.rs` only has `goextract_dir`/`run_goextract`. The harness calls the SAME `build_graph(FIXTURE_DIR)` for Python fixtures. | Add language dispatch in `build_graph`; add `pyextract_dir`/`run_pyextract`; (optionally make `collect` dispatch too, though no Python diagnostics-text snapshot is required this phase). |

**The key question — after every file is updated, what still points only at Go?** `build_graph` and `collect`. They are the load-bearing edits; the snapshots cannot turn green until `build_graph` routes Python targets to `run_pyextract`.

## Common Pitfalls

### Pitfall 1: `build_graph` has no language dispatch — the harness will still call goextract for Python
**What goes wrong:** The red tests call `gnr8_core::analyze::build_graph(FASTAPI_FIXTURE)`. Today that runs `go run . <python-dir>`, which produces no routes/garbage. The snapshot never matches.
**Why it happens:** Phase-1 `build_graph` was Go-only by design; the seam was always going to need a fork point.
**How to avoid:** Add a deterministic language detector (rule 3: ONE detection path, no fallback chain) — e.g. presence of `*.py` under the target → Python, presence of `go.mod`/`*.go` → Go; an ambiguous/empty target is a typed error, not a guess. Route to `run_pyextract` vs `run_goextract` accordingly.
**Warning signs:** FastAPI graph snapshot diff shows an empty/Go-shaped graph.

### Pitfall 2: `int`/`float` width and signedness
**What goes wrong:** Emitting `{"prim":"int"}` without `bits`/`signed`, or using 32-bit.
**Why it happens:** Python ints are unbounded; there's no obvious width.
**How to avoid:** The snapshots fix it: `int` → `{"prim":"int","bits":64,"signed":true}`, `float` → `{"prim":"float","bits":64}`, `str` → `{"prim":"string"}`, `bool` → `{"prim":"bool"}`. Hardcode these mappings.
**Warning signs:** Param/field schema diffs on every integer.

### Pitfall 3: optional vs nullable are INDEPENDENT axes (the four-way matrix)
**What goes wrong:** Conflating "has a default" with "type admits None".
**Why it happens:** `Optional[T]` and `= default` look related but encode different axes.
**How to avoid:** From the fixtures + snapshots, the exact rules are:
- `optional = True` iff the field has a **default value** (`= x`, `= field(default=…)`, `= field(default_factory=…)`).
- `nullable = True` iff the **type admits None** (`Optional[T]` or `T | None`).
- `required = not optional` in the snapshots (every field with no default is `required:true`; every field with a default is `required:false`). Note `nullable` does NOT affect `required` — e.g. `published: Optional[int]` (no default) is `required:true, optional:false, nullable:true`.
Worked rows (BookFilters): `genre` (req,!opt,!null), `in_stock: bool = True` (!req,opt,!null), `published: Optional[int]` (req,!opt,null), `sort: Optional[SortOrder] = "asc"` (!req,opt,null).
**Warning signs:** Field axis diffs; the fixtures were authored to exercise all four — any single wrong rule shows.

### Pitfall 4: enum members must be SORTED; both `enum.Enum` and `Literal[...]` → `Type::Enum`
**What goes wrong:** Emitting declaration order. `BookFormat` is declared `PAPERBACK, HARDCOVER` but the snapshot is `["hardcover","paperback"]`. `Availability` declared `OUT_OF_STOCK, IN_STOCK` → `["in_stock","out_of_stock"]`.
**Why it happens:** Source declares out of lexical order *on purpose* to force sorting.
**How to avoid:** Sort enum members lexically. Emit `enum.Enum` **values** (`"paperback"`, not `PAPERBACK`) and `Literal[...]` literal strings identically — both become `{"type":"enum","of":[…sorted…]}`. `Currency = Literal["usd","eur"]` inline on `Price.currency` → `["eur","usd"]`.
**Warning signs:** Enum order diffs.

### Pitfall 5: `Union[int,float]` order is preserved; union-of-objects → array of `named`
**What goes wrong:** Sorting union members (they are NOT sorted) or mis-rendering an object union.
**How to avoid:** `Union`/`|` members keep **source order** (snapshot `rating` → `[int64, float64]`; `BookOrError = Union[Book, OutOfStock]` → `[{named:Book},{named:OutOfStock}]`). `Optional[Union[int,float]]` = nullable+optional union of `[int,float]` (the `None` arm becomes the `nullable` axis, it is NOT a union member). A top-level `BookOrError = Union[Book, OutOfStock]` alias becomes a **schema** whose `body` is the union (id `app.models.BookOrError`), referenced by `ref_id`.
**Warning signs:** Union member order or count diffs.

### Pitfall 6: Flask diagnostics — exact strings, exact lines, line ≠ span
**What goes wrong:** Diagnostic text or line numbers don't match; diagnostics carry a span instead of a single line.
**How to avoid:** The Flask graph snapshot fixes three WARN diagnostics verbatim (sorted by file,line,message by the host):
- line 42: `untyped query param 'q' on GET /: read via request.args.get with no annotation; param type/required-ness under-specified, type inferred as string only`
- line 69: `untyped response on POST /raw: handler has no return annotation; response shape under-specified, no schema inferred`
- line 78: `untyped request body on POST /raw: read via request.json with no typed DTO; body shape under-specified, no schema inferred`
A `DiagnosticFact` has `severity,message,file,line` (single `u32` line), NOT a span. Note line 78 is in `dto.py`? No — it is `app/routes.py:78`? Check: the snapshot says `file: app/routes.py, line: 78` for the body diagnostic, but `routes.py` ends at line 77. **Open question flagged below** — the planner must reconcile the exact line the body-diagnostic anchors to against the as-built `routes.py` (the snapshot is the authority; pyextract must emit whatever line makes it match, likely the `request.json` read).
**Warning signs:** diagnostics array diff in the Flask graph snapshot.

### Pitfall 7: Flask `status_code` — POST `/orders/` is 201 with no FastAPI `status_code=`
**What goes wrong:** Flask has no `status_code=` decorator arg, yet the snapshot shows `create_order` → `status:201`.
**Why it happens:** The fixture docstring says "Response: 201"; the snapshot encodes 201 for the typed POST and 200 for typed GETs.
**How to avoid:** This is a **real open question** — there is no obvious *code* source for 201 on the Flask POST (rule 1 forbids reading the docstring). The planner MUST resolve where 201 comes from: either a convention (POST→201, GET→200) derived from the HTTP method, or the snapshot needs a documented derivation. FastAPI is unambiguous (`status_code=201` literal on `create_book`; default 200 elsewhere). Flagged in Open Questions.

### Pitfall 8: FastAPI query-param defaults and `Optional` → required-ness
**What goes wrong:** Wrong `required` for query params.
**How to avoid:** From `list_books(genre: str, sort: str = "asc", cursor: Optional[str] = None)` → `genre` required:true (no default), `sort` required:false (default), `cursor` required:false (default). Params are sorted by name by the host (snapshot order: cursor, genre, sort). `fmt: Optional[BookFormat] = None` → query param, required:false, schema `{"type":"named","of":"app.models.BookFormat"}` (a `named` ref to the enum schema, NOT inlined). Path params (`book_id: int`) → location path, required:true, `int64`.

## Code Examples

### Recognizing a FastAPI route decorator (AST shape)
```python
# Source: pattern derived from fixtures/fastapi-bookstore/app/main.py + ast docs
# @router.get("/", response_model=ListBooksResponse)  is:
#   FunctionDef(decorator_list=[ Call(
#       func=Attribute(value=Name(id="router"), attr="get"),
#       args=[Constant(value="/")],
#       keywords=[keyword(arg="response_model", value=Name(id="ListBooksResponse")),
#                 keyword(arg="status_code", value=Constant(value=201))] ) ])
# method = attr.upper() if attr in {get,post,put,delete,patch,...}
# decorator target Name "router" must be an APIRouter(prefix="/books") instance →
#   group-relative path = the Constant arg; prefix recorded separately (NOT folded)
```

### Recognizing a Flask route + blueprint prefix
```python
# Source: pattern from fixtures/flask-bookstore/app/routes.py
# bp = Blueprint("orders", __name__, url_prefix="/orders")  → Assign; record prefix
# @bp.route("/", methods=["GET"])  → Call(func=Attribute(value=Name("bp"), attr="route"),
#     args=[Constant("/")], keywords=[keyword("methods", List([Constant("GET")]))])
# one route PER method in the methods list. path "<int:order_id>" → strip converter,
# brace name → "/{order_id}", and book the param schema as int64.
```

### Owned cross-module symbol table (algorithm sketch)
```python
# Source: design for pyextract/symtab.py — static, no import/exec (PYSRC-03)
# 1. load: for each *.py under target_dir, ast.parse → (dotted_module, ast.Module).
#    dotted_module = relpath(file, target).replace('/', '.').removesuffix('.py')
# 2. index per module:
#      classes: {ClassName: ClassDef}          # incl. bases (BaseModel / dataclass / str,Enum)
#      aliases: {Name: annotation-AST}         # e.g. SortOrder = Literal[...]; BookOrError = Union[...]
#      imports: {local_name: (source_dotted_module, original_name)}  # from app.models import Book
# 3. resolve(name, in_module):
#      if name in module.classes/aliases → qualified id = f"{module}.{name}"
#      elif name in module.imports → recurse into the imported module (STATIC lookup, no exec)
#      else → diagnostic("foreign/unresolvable type") and omit the fact (rule 3)
# Deterministic: iterate modules in sorted filename order; the host re-sorts the final slices.
```

### Mapping a Python annotation AST → neutral Type (the core of types.py)
```python
# Source: design mapping, cross-checked against both committed graph snapshots
# str/bool/int/float            → {"type":"primitive","of":{"prim":...,(bits/signed)}}
# Optional[T] / T | None        → unwrap to T; set nullable=True on the FIELD axis
# Union[A,B] (no None)          → {"type":"union","of":[map(A),map(B)]}  (source order kept)
# list[T] / List[T]             → {"type":"array","of":map(T)}
# dict[K,V]                     → {"type":"map","of":{"key":map(K),"value":map(V)}}
# Literal["a","b"]              → {"type":"enum","of": sorted([...])}
# a class that subclasses Enum  → schema body {"type":"enum","of":sorted(values)}; field refs it by named
# a BaseModel/@dataclass class  → emit a SchemaFact (object body); field refs it via {"type":"named","of":id}
# unresolvable name             → diagnostic + omit (NEVER {"type":"any"} as a silent default)
# (Type::Any is reserved for an *explicit* free-form, e.g. a bare `dict`/untyped — used sparingly)
```

### Rust seam: language dispatch + pyextract driver (sketch)
```rust
// Source: design mirroring analyze/helper.rs run_goextract + analyze/mod.rs build_graph
// helper.rs:
pub(crate) fn pyextract_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))  // repo root holding pyextract/
}
pub(crate) fn run_pyextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let output = Command::new("python3")
        .args(["-m", "pyextract", target_dir])     // discrete args; no shell (T-02-01)
        .current_dir(pyextract_dir())
        .output()
        .map_err(|source| CoreError::PythonToolchainMissing { source })?; // NEW variant
    if !output.status.success() { /* HelperExit */ }
    serde_json::from_slice(&output.stdout).map_err(|source| CoreError::FactsParse { source })
}
// mod.rs build_graph: detect language (ONE path), call run_pyextract or run_goextract.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `build_graph` is Go-only (`run_goextract`) | `build_graph` dispatches Go vs Python | This phase | The single seam serves both languages; harness unchanged |
| Facts contract named `GoFacts` | Same struct, doc already says "every language sidecar emits this" | Phase 1 | No rename needed; `GoFacts` is the neutral DTO (cosmetic name only) |
| Python services unsupported | FastAPI (full) + Flask (typed envelope) extracted statically | This phase | PYSRC-01..05 satisfied |

**Deprecated/outdated:**
- Any notion of reading FastAPI `/openapi.json` or pydantic runtime schema — permanently out (rule 1, security). Confirmed in REQUIREMENTS "Out of Scope".

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Language dispatch keys on `*.py` vs `go.mod`/`*.go` presence in the target | Pitfall 1 | A wrong detector mis-routes a target; but it is host-side and easily tested — low risk, must be a single deterministic path (rule 3) |
| A2 | Spans must be emitted as canonical ABSOLUTE paths (host relativizes) | Pattern 2 | If emitted relative, `relativize` is a harmless no-op IF the relpaths already match `app/...`; still recommend absolute to match goextract exactly. Verify against the determinism guard. |
| A3 | `module = basename(target_dir)` | Pattern 3 | Snapshot shows `fastapi-bookstore`; high confidence, directly observed |
| A4 | Flask POST `/orders/` → 201 derives from HTTP method convention (POST→201) | Pitfall 7 | **HIGH risk** — rule 1 forbids docstring reading; if there is no code source, the snapshot's 201 needs a documented derivation rule. Must be resolved at plan time. |
| A5 | The Flask body-diagnostic `line: 78` anchors to the `request.json` read inside `create_order_raw` | Pitfall 6 | Medium — `routes.py` as read ends at 77; the snapshot line must be reproduced exactly. Reconcile against the as-built file. |
| A6 | `pyextract_dir()` resolves to the repo root (carrying v1 compile-time-path debt) | Code Examples | Low — explicitly allowed by CONTEXT ("carry the v1 compile-time-path tech debt forward, do not worsen it") |
| A7 | `python3` on PATH is the invocation (not a pinned interpreter path) | Code Examples | Low — mirrors `go` on PATH; `PythonToolchainMissing` covers absence |

## Open Questions (RESOLVED)

> All three resolved at plan time (Phase 2 plans 02-02/02-03/02-04). Resolutions recorded inline below.

**RESOLVED (Q1 — Flask 201): method-derived status (code fact).** Typed POST → 201, typed non-POST → 200,
derived from the HTTP method (which IS in the code); never reads the docstring (rule 1/3). Implemented in
02-04 Task 1 (threat T-status-source).
**RESOLVED (Q2 — diagnostic + span lines): per-node AST anchors + fixture reconciliation.** Each diagnostic/
span anchors to its precise AST node; 02-03 Task 2 and 02-04 Task 2 reconcile the FIXTURE source (insert
blank lines / non-fact comments only — rule 1) so each honest anchor lands on the snapshot's line, with a
golden test asserting produced line == snapshot line. Snapshot authoritative for shape; correcting a
snapshot line is the documented fallback only if a line is genuinely unreachable.
**RESOLVED (Q3 — named vs inline enum): both implemented.** Named class/`enum.Enum` → `named` ref + a
separate schema; inline `Literal[...]` → inline `enum` body. Implemented in 02-02 Task 2 + 02-03 interfaces.

1. **Flask `status_code` source (201 on POST `/orders/`)**
   - What we know: the Flask graph snapshot encodes `create_order` → 201, typed GETs → 200; FastAPI uses an explicit `status_code=` literal.
   - What's unclear: there is no `status_code=` in Flask code and rule 1 forbids reading the docstring. Where does 201 come from in *code*?
   - Recommendation: adopt a documented method-convention rule (POST→201, others→200) as a code-derived fact (the HTTP method IS in the code), OR confirm with the snapshot author. Plan must pick ONE deterministic rule and document it (rule 3).

2. **Exact line numbers for the three Flask diagnostics (42, 69, 78)**
   - What we know: the host sorts diagnostics by (file,line,message); the snapshot fixes the three lines.
   - What's unclear: line 78 vs the as-read `routes.py` ending at 77; line 69 is the `def create_order_raw` line, line 42 is the `q = request.args.get("q")` line.
   - Recommendation: anchor each diagnostic to the precise AST node (`q` assignment line, untyped `def` line, `request.json` read line) and verify the emitted lines reproduce the snapshot byte-for-byte. The snapshot is authoritative.

3. **`fmt: Optional[BookFormat]` as `named` not inlined enum**
   - What we know: snapshot `get_book` param `fmt` → `{"type":"named","of":"app.models.BookFormat"}` (a ref), and `BookFormat` appears as a separate enum schema.
   - What's unclear: nothing material — but confirms that *named* enum/class params/fields always become a `named` ref + a separate schema, while *inline* `Literal[...]` becomes an inline `enum` body (e.g. `BookFilters.sort`). Plan must implement both: named class/enum → ref+schema; inline `Literal` → inline enum.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `python3` (CPython, stdlib `ast`) | pyextract sidecar | ✓ | 3.9.25 at /usr/bin/python3 | none needed |
| `go` toolchain | existing goextract tests + determinism | ✓ (assumed, Phase-1 tests rely on it) | — | Go tests skip gracefully if absent |
| Rust + cargo + insta | host build/test | ✓ (Phase-1 green) | — | — |
| `pip` / fastapi / flask / pydantic | NOT required (never installed/imported) | n/a | — | n/a — static parsing only |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none. The sandbox has everything; no installs this phase.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` + `insta` snapshot tests (host); Python sidecar testable via direct `python3 -m pyextract <fixture>` + golden JSON |
| Config file | `crates/gnr8-core/Cargo.toml`; insta snapshots under `crates/gnr8-core/tests/snapshots/` |
| Quick run command | `cargo test -p gnr8-core --test snapshot_fastapi_graph -- --ignored` (and the three siblings) |
| Full suite command | `make check` (clippy `-D warnings` + all tests; `CI=true` keeps `INSTA_UPDATE=no`) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PYSRC-01 | FastAPI routes/params/bodies/responses/status → graph | snapshot | `cargo test -p gnr8-core --test snapshot_fastapi_graph` (remove `#[ignore]`) | ✅ committed snapshot |
| PYSRC-01 | FastAPI → OpenAPI 3.1 | snapshot | `cargo test -p gnr8-core --test snapshot_fastapi_openapi` | ✅ committed snapshot |
| PYSRC-02 | Flask routes + prefix + typed DTOs → graph | snapshot | `cargo test -p gnr8-core --test snapshot_flask_graph` | ✅ committed snapshot |
| PYSRC-02 | Flask → OpenAPI 3.1 | snapshot | `cargo test -p gnr8-core --test snapshot_flask_openapi` | ✅ committed snapshot |
| PYSRC-03 | Static `ast`, owned symtab, no import/exec | unit (Python) | `python3 -m pyextract fixtures/fastapi-bookstore` (asserts JSON; no network/import) + Rust seam test | ❌ Wave 0 (sidecar tests) |
| PYSRC-04 | Untyped/foreign → diagnostics, no fallback | snapshot (in Flask graph) + unit | covered by `snapshot_flask_graph` diagnostics array | ✅ committed snapshot |
| PYSRC-05 | `.gnr8/` FastApi/Flask Source built-ins | unit | `cargo test -p gnr8-core builtins` (new tests mirroring GoGin) | ❌ Wave 0 (builtins tests) |
| (determinism) | byte-identical across two runs | integration | `cargo test -p gnr8-core --test determinism` (extend to FastAPI/Flask fixtures) | ✅ test exists, Go-only today |

### Sampling Rate
- **Per task commit:** the one affected snapshot test (e.g. `--test snapshot_fastapi_graph`), plus `cargo clippy -D warnings`.
- **Per wave merge:** all four Python snapshot tests + determinism + the Go suite (no regression).
- **Phase gate:** `make check` fully green with the four `#[ignore]` attributes removed; `*.snap.new` files gone.

### Wave 0 Gaps
- [ ] `pyextract/` package with a unit/golden harness (`python3 -m pyextract <fixture>` → compare to a committed golden JSON, so the sidecar is testable without the Rust host).
- [ ] Remove `#[ignore]` from the four `snapshot_{fastapi,flask}_{graph,openapi}.rs` tests as they turn green (and from the determinism additions).
- [ ] New Rust unit tests for `run_pyextract` (toolchain-missing → `PythonToolchainMissing`, mirroring the goextract test) and for `FastApi`/`Flask` Source built-ins (mirroring `GoGin` zero/many-input errors).
- [ ] `CoreError::PythonToolchainMissing` variant + its `Display` (mirror `GoToolchainMissing`).

## Security Domain

> `security_enforcement` not found as `false` in config — treated as enabled. This phase's security surface is narrow but real: it spawns a subprocess and parses untrusted source.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth in the extractor |
| V3 Session Management | no | Stateless CLI/subprocess |
| V4 Access Control | no | — |
| V5 Input Validation | yes | Host deserializes sidecar JSON under `deny_unknown_fields` (rejects malformed/forward-incompatible output, threat T-01-05); the sidecar treats target source as untrusted *data* (parses, never executes) |
| V6 Cryptography | no | None |
| V12 Files/Resources | yes | Sidecar only reads `.py` under the target dir; never writes; never follows the source into execution |

### Known Threat Patterns for {stdlib-ast sidecar + subprocess seam}
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Arbitrary code execution via importing target | Elevation of Privilege | **Static `ast` only — NEVER import/exec the target** (PYSRC-03); this is the load-bearing security invariant |
| Shell/argument injection in subprocess spawn | Tampering | `Command::new("python3").args([…])` — discrete args, no `sh -c`, no interpolation (mirrors goextract T-02-01) |
| Malformed sidecar output trusted blindly | Tampering | `serde_json` + `deny_unknown_fields`; any drift hard-fails deserialize → typed `FactsParse` error, never a partial graph |
| Path traversal writing outside target | Tampering | Sidecar is read-only; the host's artifact-write guards are unchanged and out of this phase's scope |
| Toolchain absence → panic/DoS | Denial of Service | `PythonToolchainMissing` typed error; no `unwrap`/`expect`/`panic` in production (RUST-04) |

## Project Constraints (from CLAUDE.md)

These OVERRIDE convenience. The planner must verify every task against them:
- **Rule 1 — no coupling to another tool's conventions:** derive facts ONLY from Python's own constructs (type hints, `BaseModel`/`@dataclass` field types, `enum.Enum`/`Literal`). FORBIDDEN to read marshrmallow/`@nestjs/swagger`-style annotations or consume FastAPI's runtime `/openapi.json`. The fixtures import `pydantic`/`fastapi`/`flask` only as *names to recognize statically*.
- **Rule 2 — no third-party deps:** the Python sidecar uses CPython stdlib ONLY (`ast`, `json`, `sys`, `pathlib`, `os`). `gnr8-core` adds ZERO new crates. STRONGLY PREFER hand-rolled, in-repo code (e.g. the symbol table is owned, not a library). There is no approval path for an OSS dep.
- **Rule 3 — no fallback / no dual control flow:** exactly ONE deterministic source per fact. Unresolvable/untyped/foreign → a diagnostic and the fact is OMITTED, never guessed, never "try A then B". Language dispatch must be a single deterministic detection, not a try-Go-then-try-Python chain.
- **Rule 4 — config is code:** Python extraction is enabled via `.gnr8/` `FastApi`/`Flask` `Source` built-ins (Rust method calls), never a data file.
- **Determinism:** identical input ⇒ byte-identical output. Sort every slice in the sidecar; the host re-sorts and relativizes. No production `unwrap`/`expect`/`panic` (RUST-04) — typed `CoreError` everywhere.
- **Known debt (do not worsen):** the compile-time-baked sidecar path (`goextract_dir`) — `pyextract_dir` may carry the same debt forward but must not deepen it.

## Sources

### Primary (HIGH confidence)
- `crates/gnr8-core/src/analyze/facts.rs` — the neutral facts DTO (every field name, tag, enum representation) — the exact JSON contract pyextract must emit.
- `crates/gnr8-core/tests/snapshots/snapshot_fastapi_graph__fastapi_graph.snap` and `…_flask_graph.snap` — the byte-exact acceptance targets (every fact, sort order, diagnostic string, line number, integer width).
- `crates/gnr8-core/src/analyze/helper.rs` (`run_goextract`, `goextract_dir`, `resolve_target`) — the subprocess driver to mirror.
- `crates/gnr8-core/src/analyze/mod.rs` (`build_graph`) — the seam that needs language dispatch.
- `crates/gnr8-core/src/graph/mod.rs` (`from_facts`, `relativize`, sorting) — proves the host re-sorts/relativizes and copies `facts.module` verbatim.
- `crates/gnr8-core/src/sdk/builtins.rs` (`GoGin` Source) — the built-in to clone as `FastApi`/`Flask`.
- `goextract/main.go` + `goextract/internal/facts/facts.go` — the sidecar structure + deterministic-marshal discipline to mirror.
- `fixtures/fastapi-bookstore/app/{main,models}.py`, `fixtures/flask-bookstore/app/{routes,dto}.py` — the source whose constructs map to the snapshot facts.
- `crates/gnr8-core/tests/snapshot_{fastapi,flask}_graph.rs` + `determinism.rs` + `snapshot_diagnostics.rs` — the harness (calls the single `build_graph(FIXTURE_DIR)` entry; `#[ignore]` red-by-design).
- `python3 --version` on sandbox → 3.9.25 [VERIFIED via Bash].
- `.planning/phases/02-python-source-pyextract/02-CONTEXT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `CLAUDE.md`.

### Secondary (MEDIUM confidence)
- CPython `ast` module behavior (decorator/Call/keyword/Attribute node shapes; `lineno` provenance; parses PEP 604/585 syntax under 3.9) — standard library knowledge cross-checked against the fixtures' actual syntax.

### Tertiary (LOW confidence)
- None. Every claim is grounded in committed repo files; the two genuine unknowns (Flask 201 derivation; exact diagnostic line 78) are raised as Open Questions, not asserted.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — the only dependency is stdlib `ast`, verified present; zero new packages by mandate.
- Architecture/seam: HIGH — the goextract analog, the facts DTO, and `from_facts` are all read in full; the dispatch gap is concretely identified.
- Snapshot-as-spec mapping: HIGH — both graph snapshots read end-to-end and reconciled to source constructs.
- Two open questions (Flask 201; diagnostic line 78): MEDIUM — flagged for plan-time resolution against the authoritative snapshot.

**Research date:** 2026-06-25
**Valid until:** 2026-07-25 (stable — the contract is frozen by Phase 1 and the snapshots are committed; only the two open questions can move).
