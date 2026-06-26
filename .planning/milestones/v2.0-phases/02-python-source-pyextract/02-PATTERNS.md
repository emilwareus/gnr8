# Phase 2: Python Source — `pyextract` - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 14 (3 new Python sidecar concerns + 8-module Python package, 3 Rust edits, 1 Rust error edit, 1 builtins edit, 4 test flips)
**Analogs found:** 14 / 14 (every target has a direct in-repo analog — this phase is a Python twin of an existing Go path)

## Orientation

This phase is a *mirror* phase: nearly every new file has an exact existing analog, and the
single load-bearing rule is **the committed snapshots ARE the spec**. The map below pairs each
new/modified file with the closest analog and the concrete excerpt to copy. The Python sidecar
mirrors `goextract/` (stdlib `go/types` → stdlib `ast`); the Rust seam mirrors the goextract
driver (`run_goextract`/`goextract_dir`); the `Source` built-ins mirror `GoGin`; the tests flip
from the red-by-design `.expect()`-panic shape to the green goalservice `build_graph` shape.

**Three CLAUDE.md guardrails the planner must hold while copying these analogs:**
- The Go analogs import `golang.org/x/tools/go/packages` (`load.go`) — that is **rule-2 debt**.
  The Python twin must use **stdlib `ast` only**; copy the *module layout and discipline*, never
  the dependency.
- Copy the *deterministic-marshal discipline* (sort every slice before emit) from `facts.go`,
  not Go-specific types.
- Copy the *single-detection* discipline into `build_graph` dispatch — one deterministic path,
  no try-Go-then-try-Python fallback (rule 3).

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `pyextract/__main__.py` (NEW) | sidecar entrypoint | request-response (argv→stdout JSON) | `goextract/main.go` | exact (role+flow) |
| `pyextract/load.py` (NEW) | sidecar loader | file-I/O / transform | `goextract/internal/load/load.go` | role-match (stdlib-only twin) |
| `pyextract/symtab.py` (NEW) | sidecar resolver | transform | `goextract/internal/handlers` index + `go/types` resolution | role-match (owned twin) |
| `pyextract/routes.py` (NEW) | sidecar recognizer | transform | `goextract/internal/routes/routes.go` | role-match |
| `pyextract/types.py` (NEW) | sidecar type mapper | transform | `goextract/internal/types/extract.go` | role-match |
| `pyextract/schemas.py` (NEW) | sidecar schema builder | transform | `goextract/internal/types/extract.go` | role-match |
| `pyextract/diagnostics.py` (NEW) | sidecar diag accumulator | event-driven (accumulate) | `goextract/internal/diag/diag.go` | exact |
| `pyextract/facts.py` (NEW) | sidecar marshal | transform / serialize | `goextract/internal/facts/facts.go` | exact (discipline) |
| `crates/gnr8-core/src/analyze/helper.rs` (MODIFY) | subprocess driver | request-response | existing `run_goextract`/`goextract_dir` in same file | exact (sibling fn) |
| `crates/gnr8-core/src/analyze/mod.rs` (MODIFY) | host seam / dispatch | request-response | existing `build_graph` in same file | exact (extend) |
| `crates/gnr8-core/src/diagnostics/mod.rs` (MODIFY, optional) | host seam / dispatch | request-response | existing `collect` in same file | exact (extend) |
| `crates/gnr8-core/src/error.rs` (MODIFY) | error type | — | `CoreError::GoToolchainMissing` variant | exact (clone variant) |
| `crates/gnr8-core/src/sdk/builtins.rs` (MODIFY) | config built-in / Source | request-response | `GoGin` `Source` impl | exact (clone struct+impl) |
| `crates/gnr8-core/tests/snapshot_{fastapi,flask}_{graph,openapi}.rs` (MODIFY ×4) | test | snapshot | `snapshot_graph.rs` / `snapshot_openapi.rs` (green goalservice) | exact (flip shape) |

## Pattern Assignments

### Python sidecar — `pyextract/` (mirrors `goextract/`)

The whole sidecar's contract surface is `goextract/main.go::run` + `goextract/internal/facts/facts.go`.
Copy the *structure and emit-discipline*; reimplement the body in stdlib `ast`.

#### `pyextract/__main__.py` (entrypoint, argv→stdout)

**Analog:** `goextract/main.go` (`main` + `run`, lines 28-82)

**Entrypoint pattern** (`main.go:28-39`) — argv guard, single target arg, errors→stderr + nonzero exit:
```go
func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: goextract <target-dir>")
		os.Exit(1)
	}
	targetDir := os.Args[1]
	if err := run(targetDir, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "goextract:", err)
		os.Exit(1)
	}
}
```
Python twin: `sys.argv[1]` → `target_dir`; on any error print to `sys.stderr` and `sys.exit(1)`;
on success `json.dumps(...)` the facts doc to `sys.stdout`. The Rust driver maps nonzero-exit to
`HelperExit` and bad JSON to `FactsParse`, so the contract is: **stdout = facts JSON only, all
diagnostics-about-the-tool go to stderr**.

**Orchestration pattern** (`main.go:44-82`) — load → diag accumulator → schemas → module → routes
→ assemble `GoFacts` → marshal. The Python `run()` mirrors this exact pipeline:
```go
res, err := load.Load(targetDir)        // -> load.py
diags := diag.New()                     // -> diagnostics.py
schemas := types.Extract(res, diags)    // -> schemas.py / types.py
module := moduleOf(res)                  // -> pathlib.Path(canonical_target).name  (Pattern 3)
recognized := routes.Recognize(res)     // -> routes.py
routeFacts := buildRoutes(...)          // -> routes.py + handler signature read
doc := facts.GoFacts{Module, Routes, Schemas, Diagnostics: diags.Items()}
return facts.Marshal(doc, w)            // -> facts.py json.dumps
```

**module derivation** — note `goextract` uses `moduleOf(res)` (the go-module path). The Python
target has none, so per RESEARCH Pattern 3 use `module = pathlib.Path(canonical_target_dir).name`
(`"fastapi-bookstore"` / `"flask-bookstore"` — confirmed in the snapshot head: `module: fastapi-bookstore`).

#### `pyextract/load.py` (loader, file-I/O)

**Analog:** `goextract/internal/load/load.go` (`Load(targetDir)` → `*Result`, line 56)

> **DEBT WARNING:** `load.go` is the file that imports `golang.org/x/tools/go/packages` (rule-2
> debt). Do NOT mirror that import. Mirror only the *shape*: a `Load(target_dir)` that returns a
> structured `Result` (the parsed modules) + per-file load errors as diagnostics, not panics.

Python twin: walk `*.py` under `target_dir` (sorted), `ast.parse(src, filename=path)` each into a
`(dotted_module, ast.Module)` record. `dotted_module = relpath(file, target).replace('/', '.')`
minus `.py` (RESEARCH symtab sketch). A parse error becomes a diagnostic (mirrors `main.go:51-54`
turning `res.Errors` into `diags.Warn(...)`), never an abort.

#### `pyextract/symtab.py` (owned resolver — no Go analog file; closest is `go/types` usage)

**Analog:** the *resolution discipline* in `goextract/internal/routes/routes.go:1-22` and
`handlers/handlers.go` — recognition is **semantic, not textual** (resolve identity, do not
string-match the import alias). There is no single Go file to copy because Go gets this for free
from `go/types`; in Python it must be **owned** (rule 2 — hand-rolled).

**Algorithm to implement** (RESEARCH "Owned cross-module symbol table" sketch): index per module
`classes`/`aliases`/`imports`; `resolve(name, in_module)` → qualified id `f"{module}.{name}"`, else
follow `from x import Y` statically (no exec), else diagnostic + omit (rule 3). Iterate modules in
sorted filename order. Schema id = `<dotted-module>.<ClassName>` (RESEARCH Pattern 4; snapshot ids
like `app.models.Author`).

#### `pyextract/routes.py` (recognizer)

**Analog:** `goextract/internal/routes/routes.go` (`Recognize(res) []Route`, line 66; `Route` type line 55)

Copy the **group-relative path discipline** (`routes.go:9-22`): the mount/group prefix is recorded
separately and **NOT folded into the path** — this is rule 1 and is the snapshot truth
(`APIRouter(prefix=)`/`Blueprint(url_prefix=)` stay out of the path; `base_path: /`). Copy the
`:param` → `{param}` normalization idea; the Python twin does FastAPI `"/{book_id}"` verbatim and
Flask `"/<int:order_id>"` → `"/{order_id}"` (strip converter, brace name) per RESEARCH Pattern 4.
AST recognition shapes are in RESEARCH "Code Examples" (decorator `Call(func=Attribute(...))`).

#### `pyextract/types.py` + `pyextract/schemas.py` (type mapper + schema builder)

**Analog:** `goextract/internal/types/extract.go` (`Extract(res, diags) []SchemaFact`, header lines 1-9)

Copy the **scope + diagnostic discipline** (`extract.go:1-9`): only named types declared in the
target are schemas; unresolvable → diagnostic + omit (never guess — rule 3). The neutral `Type`
target vocabulary is fixed by `facts.rs`/`facts.go` (see Shared Pattern A). The Python annotation→Type
mapping table is in RESEARCH "Mapping a Python annotation AST → neutral Type" and the four-axis
optional/nullable rules in Pitfall 3 — those are the byte-exact field rules.

#### `pyextract/diagnostics.py` (accumulator)

**Analog:** `goextract/internal/diag/diag.go` (`Accumulator`, `New()` line 22, `Warn(message, file, line)` line 102, `Items()` line 113)

```go
type Accumulator struct { items []facts.DiagnosticFact }
func New() *Accumulator { ... }
func (a *Accumulator) Warn(message, file string, line uint32) { ... }
func (a *Accumulator) Items() []facts.DiagnosticFact { ... }
```
Direct twin: a class accumulating `{"severity":"WARN","message":..,"file":..,"line":..}` dicts.
A `DiagnosticFact` carries a **single `line` (int), NOT a span** (facts.go:226-232). Flask diagnostic
strings + lines are byte-fixed by the Flask graph snapshot (RESEARCH Pitfall 6 — exact text at
lines 42/69/78); the snapshot is authoritative.

#### `pyextract/facts.py` (marshal — the contract boundary)

**Analog:** `goextract/internal/facts/facts.go` (the whole file; `Marshal`/`sortDoc` lines 250-337)

This is the **most load-bearing analog** — the emitted JSON must deserialize into the Rust DTO under
`deny_unknown_fields`. Copy:

1. **Every json tag** from `facts.go:23-239` exactly (`module, routes, schemas, diagnostics`;
   route keys `method, path, handler, operation_id, params, request_body, responses, span`; field keys
   `json_name, required, optional, nullable, schema, description, example`; `{"ref_id":..}`;
   span `{"file","start_line","end_line"}`).
2. **The tagged `Type` encoding** (`facts.go:94-219`): `{"type":"<kind>","of":<payload>}`;
   `primitive`→`{"prim":..,(bits,signed)}`; `int`→`{"prim":"int","bits":64,"signed":true}`;
   `float`→`{"prim":"float","bits":64}`; `any`→`{"type":"any","of":{}}` (empty object, never null);
   `named`→bare id; `enum`→string array; `union`→`Type` array.
3. **The deterministic sort discipline** (`facts.go:241-337` `sortDoc`/`sortType`/`sortRoute`):
   schemas by id; object fields by `json_name`; enum members lexically; diagnostics by `(file,line,message)`;
   routes by `(path,method)`; params by `(name,location)`; responses by status. Use
   `json.dumps(doc, sort_keys=True, separators=(",", ":"))` PLUS explicit list sorting (sort_keys
   alone does not order lists). Union members are NOT sorted (source order — Pitfall 5).

The Rust mirror of every one of these tags is `crates/gnr8-core/src/analyze/facts.rs` (`GoFacts`
struct lines 30-39, `RouteFact` 44-62, `ParamFact` 69-80) — read it as the canonical key list.

---

### Rust seam — `crates/gnr8-core/src/analyze/helper.rs` (MODIFY: add `pyextract_dir` + `run_pyextract`)

**Analog:** the *existing sibling functions in the same file* — `goextract_dir` (line 27) and
`run_goextract`/`run_goextract_with` (lines 66-91).

**Dir resolver pattern** (`helper.rs:27-29`):
```rust
pub(crate) fn goextract_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../goextract"))
}
```
Twin: `pyextract_dir()` → `concat!(env!("CARGO_MANIFEST_DIR"), "/../..")` (the **repo root** that
*holds* `pyextract/`, because the invocation is `python3 -m pyextract`). This carries the v1
compile-time-path debt forward without worsening it (CONTEXT decision; RESEARCH A6).

**Driver pattern** (`helper.rs:72-91`) — copy verbatim, swapping binary/args and the toolchain-missing variant:
```rust
let output = Command::new(go_bin)
    .args(["run", ".", target_dir])     // discrete args, no shell (T-02-01)
    .current_dir(goextract_dir())
    .output()
    .map_err(|source| CoreError::GoToolchainMissing { source })?;
if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    return Err(CoreError::HelperExit { code: output.status.code(), stderr });
}
let parsed: facts::GoFacts = serde_json::from_slice(&output.stdout)
    .map_err(|source| CoreError::FactsParse { source })?;
Ok(parsed)
```
Twin `run_pyextract`: `Command::new("python3").args(["-m","pyextract", target_dir]).current_dir(pyextract_dir())`;
spawn failure → **`CoreError::PythonToolchainMissing`** (new variant); reuse `HelperExit` + `FactsParse`
unchanged. Keep the `run_pyextract_with(py_bin, target_dir)` inner-fn split (line 72) so the
toolchain-missing unit test can force a bad binary without mutating `PATH` (lines 118-130). `resolve_target`
(lines 45-57) is language-agnostic — **reuse it unchanged** for both.

**Unit-test analog** (`helper.rs:115-131`): clone `returns_go_toolchain_missing_when_binary_absent`
as a `PythonToolchainMissing` test (force `"gnr8-nonexistent-python-binary-xyz"`).

---

### Rust seam — `crates/gnr8-core/src/analyze/mod.rs` (MODIFY `build_graph`: add language dispatch)

**Analog:** the *existing* `build_graph` in the same file (lines 27-33) — the load-bearing seam.

**Current (Go-only) excerpt** (`mod.rs:27-33`):
```rust
pub fn build_graph(fixture_dir: &str) -> Result<crate::graph::ApiGraph, crate::CoreError> {
    let target = helper::resolve_target(fixture_dir);
    let facts = helper::run_goextract(&target)?;
    Ok(crate::graph::ApiGraph::from_facts(facts, &target))
}
```
Edit: keep `resolve_target` and `from_facts` exactly; insert a **single deterministic** language
detector between them — e.g. `*.py` present → `run_pyextract`, `go.mod`/`*.go` present →
`run_goextract`, ambiguous/empty → a typed `Config` error (NOT a try-A-then-B fallback — rule 3,
RESEARCH Pitfall 1). `from_facts` is **reused unchanged** (the v2.0 bet): it already sorts/relativizes
and copies `facts.module` verbatim, so Python facts flow through the same code.

**Test analog** (`mod.rs:47-65`): `build_graph_surfaces_typed_error_for_bad_target` already
asserts a typed error for a bad path — extend the matched-variants set if dispatch can surface
`Config` for an empty target.

---

### Rust seam — `crates/gnr8-core/src/diagnostics/mod.rs` (MODIFY `collect`, OPTIONAL this phase)

**Analog:** the *existing* `collect` in the same file (lines 29-35) — identical Go-only shape to
`build_graph`. The four Python snapshots do NOT require a diagnostics-TEXT snapshot (Flask diagnostics
ride inside the *graph* snapshot), so dispatch here is optional. If added, mirror the same single-detection
dispatch as `build_graph`; `render`/`relativize` (lines 38-70) are language-agnostic — reuse unchanged.

```rust
pub fn collect(fixture_dir: &str) -> Result<String, crate::CoreError> {
    let target = helper::resolve_target(fixture_dir);
    let facts = helper::run_goextract(&target)?;          // <- the only Go-specific line
    Ok(render(facts.diagnostics, &target))
}
```

---

### Rust error — `crates/gnr8-core/src/error.rs` (MODIFY: add `PythonToolchainMissing`)

**Analog:** `CoreError::GoToolchainMissing` (lines 27-36) — clone it verbatim, swap "Go"→"Python".

```rust
#[error("Go toolchain not available (is `go` installed and on PATH?): {source}")]
GoToolchainMissing {
    #[source]
    source: std::io::Error,
},
```
Twin: `PythonToolchainMissing { #[source] source: std::io::Error }`, message
`"Python toolchain not available (is `python3` installed and on PATH?): {source}"`. **Reuse**
`HelperExit` (lines 42-48) and `FactsParse` (lines 54-59) as-is — they are already language-neutral
(do not rename the goextract mention in `FactsParse`'s message scope-wise; it is cosmetic, but the
planner may generalize the message if a snapshot does not depend on it). **Test analog:**
clone `go_toolchain_missing_renders_with_source` (lines 187-195).

---

### Rust config — `crates/gnr8-core/src/sdk/builtins.rs` (MODIFY: add `FastApi` + `Flask` Source)

**Analog:** the `GoGin` `Source` struct + impl (lines 28-82) — clone twice.

**Struct + builder pattern** (`builtins.rs:28-50`):
```rust
#[derive(Debug, Default, Clone)]
pub struct GoGin { inputs: Vec<String> }
impl GoGin {
    pub fn new() -> Self { Self::default() }
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where I: IntoIterator<Item = S>, S: Into<String> {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}
```

**`Source::load` pattern** (`builtins.rs:52-82`) — single-input guard (reject 0/many with typed
`Config`), resolve against `cx.project_root`, call `build_graph`:
```rust
impl Source for GoGin {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => return Err(CoreError::Config { message: "GoGin source has no inputs ...".into() }),
            many => return Err(CoreError::Config { message: format!("GoGin source lists {} inputs ...", many.len()) }),
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph(&resolved.to_string_lossy())
    }
}
```
Twins `FastApi` and `Flask`: identical struct/builder/`load`, swapping only the error-message proper
noun. Both call the SAME `build_graph` (dispatch happens *inside* `build_graph` by target detection,
not by which `Source` was used). **Test analog:** clone the unconfigured-error tests
(builtins.rs:500-518 style) — zero-input and many-input `Config` errors per source.

---

### Tests — flip 4 red-by-design Python tests green

**Red (current) shape** — `snapshot_fastapi_graph.rs:27-35` (and the 3 siblings):
```rust
#[test]
#[ignore = "red-by-design: pyextract lands in Phase 2; ..."]
fn graph_matches_expected_for_fastapi() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("pyextract lands in Phase 2 — intentionally red until then");
    insta::assert_yaml_snapshot!("fastapi_graph", graph);
}
```

**Green (target) analog** — `snapshot_graph.rs:20-27` (goalservice, already green):
```rust
#[test]
fn graph_matches_expected_for_goalservice() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires the Go toolchain)");
    insta::assert_yaml_snapshot!("goalservice_graph", graph);
}
```
Flip mechanics: **remove the `#[ignore]` attribute** and update the `.expect()` message; the
`build_graph(FIXTURE_DIR)` + `assert_*_snapshot!` body is already correct (the snapshot is the
hand-authored byte-target; ZERO snapshot edits). OpenAPI tests follow `snapshot_openapi.rs:33-40`
(build graph → `lower::to_openapi(&graph, title, base, &fixture_security())` → `assert_snapshot!`);
the `fixture_security()` helper is already present in each FastAPI/Flask openapi test
(`snapshot_fastapi_openapi.rs:30-37`). After green, delete the stale `*.snap.new` files.

**Determinism extension analog** — `determinism.rs:33-50` (`build_graph_is_byte_identical_across_two_runs`,
with graceful skip via `let Ok(first) = ... else { return }`). Add FastAPI/Flask twins so the
byte-identical guarantee covers Python facts too.

---

## Shared Patterns

### Pattern A — The neutral facts contract (apply to ALL `pyextract/` modules)
**Source of truth (Rust DTO):** `crates/gnr8-core/src/analyze/facts.rs` (`GoFacts` 30-39, `RouteFact`
44-62, `ParamFact` 69-80, plus the `Type`/`Prim`/`FieldFact`/`SourceSpan`/`TypeRef` tags).
**Source of truth (Go emitter twin):** `goextract/internal/facts/facts.go` (json tags 23-239).
**Apply to:** every `pyextract` module that builds a fact dict — the keys, tag strings, and
`{"type":..,"of":..}` encoding are byte-fixed. Drift fails `deny_unknown_fields` → `FactsParse`.

### Pattern B — Determinism: sort every slice before emit (apply to `pyextract/facts.py`)
**Source:** `goextract/internal/facts/facts.go:241-337` (`sortDoc`/`sortType`/`sortRoute`).
**Apply to:** the final marshal. Sort schemas/fields/enums/diagnostics/routes/params/responses by the
exact keys listed there; keep union members in source order. The host re-sorts too (`from_facts`), but
the sidecar must be internally deterministic (`determinism.rs` guard enforces byte-identical output).

### Pattern C — Absolute span paths; host relativizes (apply to `pyextract/load.py`, `routes.py`, `schemas.py`, `diagnostics.py`)
**Source:** `helper.rs::resolve_target` (45-57, canonical abspath) + `diagnostics/mod.rs::relativize`
(64-70, prefix-strip on `/` boundary). **Apply to:** emit `os.path.join(canonical_target_dir, relpath)`
for every `span.file`/diagnostic `file` so the host strips the module-root prefix to `app/main.py`
(snapshot truth: `provenance.file: app/main.py`). Mirrors goextract's canonical-abspath behavior.

### Pattern D — Typed errors, never panic (apply to ALL Rust edits)
**Source:** `helper.rs:66-91` + `error.rs` variants. **Apply to:** every Rust seam — `?`-propagate
typed `CoreError`, no `unwrap`/`expect`/`panic` in production (RUST-04). Tests scope
`#![allow(clippy::unwrap_used, clippy::expect_used)]` per-target (e.g. helper.rs:97).

### Pattern E — Single deterministic source/path (apply to `build_graph` dispatch + every fact)
**Source:** rule 3 (CLAUDE.md) + goextract's "no annotation source, no fallback" discipline
(`main.go:62-64`, `routes.go:9-22`). **Apply to:** language dispatch (one detector, not try-then-fall),
and every Python fact (unresolved → diagnostic + omit, never a guessed default).

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| (none) | — | — | Every target file has a direct in-repo analog. `pyextract/symtab.py` is the *only* concern without a one-to-one Go *file* — Go gets cross-module resolution free from `go/types`; the Python twin must own it (rule 2). Its discipline analog is the semantic-resolution note in `routes.go:1-22` and the RESEARCH symtab algorithm sketch; treat it as the one genuinely new design surface (CONTEXT places its data structures at Claude's discretion). |

## Open Questions Carried From RESEARCH (planner must resolve against the authoritative snapshot)

1. **Flask `status_code` 201 on POST `/orders/`** — no `status_code=` in Flask code and rule 1
   forbids reading the docstring. Pick ONE documented code-derived rule (RESEARCH A4 / Pitfall 7 /
   OQ1) — e.g. method-convention POST→201, else 200 — and document it. The HTTP method IS in code.
2. **Flask diagnostic lines (42/69/78)** — anchor each to the precise AST node; reconcile line 78
   against the as-built `routes.py` (RESEARCH Pitfall 6 / A5 / OQ2). Snapshot is authoritative.
3. **`fmt: Optional[BookFormat]` → `named` ref not inlined** — named class/enum params/fields become
   a `named` ref + separate schema; inline `Literal[...]` becomes an inline `enum` body (RESEARCH OQ3).

## Metadata

**Analog search scope:** `goextract/` (+ `internal/{load,routes,types,handlers,diag,facts}`),
`crates/gnr8-core/src/{analyze,sdk,diagnostics,error.rs}`, `crates/gnr8-core/tests/`,
`fixtures/{fastapi,flask}-bookstore/`.
**Files scanned:** ~18 (3 Rust seam files, error.rs, builtins.rs, facts.rs, 6 goextract files, 7 test files, 2 fixture trees, 4 snapshots).
**Pattern extraction date:** 2026-06-25
