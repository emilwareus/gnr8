# Phase 4: TypeScript Source — `tsextract` - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 13 (7 new sidecar modules + package.json, 6 Rust/test/fixture edits)
**Analogs found:** 12 / 13 (1 new-but-strongly-templated: the sidecar `*.js` modules)

> **CLAUDE.md guardrails for every plan in this phase:**
> - `tsextract/`'s ONLY dependency is `typescript` (the documented rule-2 carve-out). NO second npm package, ever.
> - `gnr8-core` adds ZERO Rust crates — it reuses `serde`/`serde_json` (existing debt) only.
> - Bright line (rule 1): tsextract reads facts ONLY from the source's own TS types via the Compiler API. NEVER read `@nestjs/swagger`, `zod`, `class-validator`, or any runtime/emitted OpenAPI. `@nestjs/common` decorators are recognized for ROUTING facts only.
> - No fallback (rule 3): unresolved/untyped → diagnostic + OMIT the fact; never guess. ONE deterministic `detect_language`.
> - The two committed nestjs snapshots ARE the spec; flip them green with ZERO snapshot edits.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `tsextract/index.js` | entrypoint | request-response (argv→stdout) | `pyextract/__main__.py` | exact (twin) |
| `tsextract/load.js` | loader | file-I/O / parse | `pyextract/load.py` | role-match (TS Program vs ast.parse) |
| `tsextract/routes.js` | recognizer | transform (AST→RouteFact) | `pyextract/routes.py` | role-match (decorator AST) |
| `tsextract/types.js` | mapper | transform (TS Type→neutral Type) | `pyextract/types.py` | role-match |
| `tsextract/schemas.js` | builder | transform (class/alias→SchemaFact) | `pyextract/schemas.py` | role-match |
| `tsextract/diagnostics.js` | accumulator | event-driven (warn collector) | `pyextract/diagnostics.py` | exact (twin) |
| `tsextract/facts.js` | marshaller | transform (dict→sorted JSON) | `pyextract/facts.py` | exact (twin) |
| `tsextract/package.json` | config | n/a | `goextract/go.mod` (manifest role) | role-match |
| `crates/gnr8-core/src/analyze/mod.rs` | dispatch seam | request-response | (self — extend in place) | exact (edit) |
| `crates/gnr8-core/src/analyze/helper.rs` | subprocess driver | request-response | `run_pyextract` / `pyextract_dir` | exact (twin) |
| `crates/gnr8-core/src/diagnostics/mod.rs` | dispatch seam | request-response | (self — extend the 2nd match) | exact (edit) |
| `crates/gnr8-core/src/error.rs` | error type | n/a | `PythonToolchainMissing` | exact (twin) |
| `crates/gnr8-core/src/sdk/builtins.rs` | Source built-in | config-as-code | `FastApi` / `Flask` Source | exact (clone) |
| `crates/gnr8-core/tests/snapshot_nestjs_graph.rs` | test | request-response | `snapshot_fastapi_graph.rs` | exact (flip) |
| `crates/gnr8-core/tests/snapshot_nestjs_openapi.rs` | test | request-response | `snapshot_fastapi_openapi.rs` | exact (flip) |
| `fixtures/nestjs-bookstore/src/*.ts` | fixture | n/a (line reconcile) | Phase-2 fixture reconcile (FastAPI) | role-match |
| `.gitignore` | config | n/a | existing `__pycache__/` entry | role-match |

## The neutral facts contract (shared — every sidecar module emits into this)

**Source DTO:** `crates/gnr8-core/src/analyze/facts.rs` (the host deserialize target, `#[serde(deny_unknown_fields)]` on every struct). The `Go` in `GoFacts` is historical — it is language-neutral; pyextract already feeds it unchanged.

**Exact wire shapes the sidecar must emit** (from facts.rs + verified against the committed nestjs snapshot):
- Top-level keys EXACTLY: `module, routes, schemas, diagnostics`.
- `Type` is adjacently tagged: `{"type":"<variant>","of":<payload>}`. `Any` = `{"type":"any","of":{}}`.
- `Prim` internally tagged on `prim`: `{"prim":"string"}`, `{"prim":"bool"}`, `{"prim":"float","bits":64}` (TS `number` → THIS), `{"prim":"int","bits":64,"signed":true}`, `{"prim":"bytes"}`.
- `Named` payload = bare id string: `{"type":"named","of":"src/books.dto.AuthorDto"}`.
- `Enum` payload = string array (sorted): `{"type":"enum","of":["asc","desc"]}`.
- `Union` payload = array of `Type`, SOURCE order (NOT sorted): `{"type":"union","of":[…]}`.
- `FieldFact` keys EXACTLY: `json_name, required, optional, nullable, schema, description, example` (last two always `null`).
- `RouteFact` keys EXACTLY: `method, path, handler, operation_id, params, request_body, responses, span`.
- `ParamFact` keys EXACTLY: `name, location, required, schema, span`.
- `ResponseFact` keys EXACTLY: `status, body`. `request_body`/`body` = `{"ref_id":"<id>"}` or `null`.
- `DiagnosticFact` keys EXACTLY: `severity, message, file, line` (`line` is a single `u32`, NOT a span).
- `SourceSpan` keys EXACTLY: `file, start_line, end_line`.

> The schema-id rule (verified from the snapshot): `id = relpath(file).removesuffix(".ts") + "." + Name`, slash form kept → `src/books.dto.AuthorDto`. `module = basename(target_dir)` → `nestjs-bookstore`.

## Pattern Assignments

### `tsextract/index.js` (entrypoint, request-response)

**Analog:** `pyextract/__main__.py` (lines 24-73)

**Orchestration pattern** — the run() pipeline order + the strict stdout/stderr split + non-zero exit:
```python
def run(target_dir):
    diags = Diagnostics()
    modules = load.load(target_dir, diags)         # → TS: ts.createProgram + getTypeChecker
    symtab = SymbolTable(modules)                   # TS: the TypeChecker IS the symbol resolver
    schemas = build_schemas(modules, symtab, diags)
    module = os.path.basename(target_dir)
    routes = routes_mod.recognize_fastapi(...)      # → TS: recognizeNestController(...)
    doc = facts.build_doc(module, routes, schemas, diags.items())
    return facts.marshal(doc)

def main(argv):
    if len(argv) < 2:
        sys.stderr.write("usage: ...\n"); return 1
    target_dir = os.path.realpath(argv[1])
    try:
        sys.stdout.write(run(target_dir)); sys.stdout.write("\n")
    except Exception as exc:                          # ANY failure → stderr + exit 1
        sys.stderr.write("pyextract: {}\n".format(exc))
        return 1
    return 0
```
**Copy:** the EXACT pipeline order (load → diagnostics → schemas → module basename → routes → assemble → marshal); stdout = facts JSON and nothing else; all tool-about-itself output + traceback → stderr; non-zero exit on any failure (host maps it to `HelperExit`). Node twin: `process.argv[2]`, `fs.realpathSync`, `process.stdout.write`, `process.exitCode = 1`.

---

### `tsextract/load.js` (loader, file-I/O)

**Analog:** `pyextract/load.py` (lines 55-109)

**Discovery + parse pattern** (sorted discovery for internal determinism; per-file failure → WARN, never abort):
```python
def discover(target_dir):
    found = []
    for root, dirs, files in os.walk(target_dir):
        dirs.sort()
        for name in files:
            if name.endswith(".py"): found.append(os.path.join(root, name))
    found.sort()
    return found
```
**Copy:** recursively discover `*.ts` under target (sorted). **CRITICAL DIVERGENCE (rule 1 / static-only):** Python uses `ast.parse` (text only, no exec); TS uses `ts.createProgram(files, opts)` + `program.getTypeChecker()` — NEVER `require`/`import`/`transpile-and-run` the target (security boundary). Synthesize CompilerOptions in the sidecar (do NOT read the target's own `tsconfig.json`):
```javascript
ts.createProgram(files, {
  target: ts.ScriptTarget.ES2020,
  experimentalDecorators: true,   // REQUIRED — NestJS decorators (Pitfall 6)
  strictNullChecks: true,         // REQUIRED — | null / | undefined survive (Pitfall 1)
  skipLibCheck: true,
});
```

---

### `tsextract/routes.js` (recognizer, transform)

**Analog:** `pyextract/routes.py` (lines 1-40 + module body)

**Recognition discipline** (verbatim rules to carry over):
```python
# Recognition is STATIC and derived from the SOURCE's own constructs (rule 1).
# The APIRouter(prefix="/books") prefix is recorded SEPARATELY and NEVER folded
# into the code-derived path (rule 1): operation paths stay group-relative.
_HTTP_METHODS = frozenset({"get","post","put","delete","patch", ...})
_ROUTER_CTORS = frozenset({"FastAPI", "APIRouter"})   # recognized by NAME (rule 1)
def _span(abs_path, node):
    line = getattr(node, "lineno", 0)
    return {"file": abs_path, "start_line": line, "end_line": line}
```
**Copy:** decorator recognition by NAME; group-relative paths; `_span` shape. **TS specifics (verified, RESEARCH Pattern 2):** walk the `@Controller`-decorated class via `ts.getDecorators(node)` (guard with `ts.canHaveDecorators`); verb map `Get→GET, Post→POST, Put→PUT, Patch→PATCH, Delete→DELETE`; path `:name → {name}`; do NOT fold `@Controller('books')` prefix into op path (snapshot ops are `/`, `/{bookId}`). `@Param`→`location:"path",required:true`; `@Query`→`location:"query"` (required from `?`/default); `@Body`→`request_body` TypeRef (NOT a param). `operation_id` = `handler` = method name verbatim (`listBooks`, etc.). Status: POST→201, else→200 (code-derived, the Flask Phase-2 rule); `@HttpCode(n)` overrides if present. Always pass the SourceFile to `node.getStart(sf)`/`node.getText(sf)` (Pitfall 6).

---

### `tsextract/types.js` (mapper, transform)

**Analog:** `pyextract/types.py` (lines 1-40 — the mapping-table docstring is the template)

**The optional/nullable axis-stripping pattern** (the single most important rule):
```python
#  * Optional[T] / T | None -> unwrap to T; the None arm is the FIELD's
#    `nullable` axis, NOT a union member.
#  * Union[A,B] (no None) -> {"type":"union","of":[map(A),map(B)]}, SOURCE order.
#  * Literal["a","b"] -> {"type":"enum","of":sorted([...])} (inline).
#  * a name resolving to a model class -> {"type":"named","of":"<id>"} (a ref).
#  * an unresolvable name -> (None, diagnostic) — NEVER {"type":"any"} (rule 3).
```
**Copy:** strip the null/undefined arms FIRST to compute axes, then map the residual. **TS mapping table (VERIFIED against the snapshot, RESEARCH Pattern 3 / Pitfall 2-5):**
- `number` → `{"prim":"float","bits":64}` (NEVER int — every numeric field/param in the snapshot is float64).
- `string`→`{"prim":"string"}`; `boolean`→`{"prim":"bool"}`; `T[]`→`{"type":"array","of":map(T)}`.
- `field?:` (questionToken) → `optional=true`, strip the `| undefined` arm. `T | null` → `nullable=true`, strip the `null` arm. `field?: T | null` → both true, strip both, leaves a SINGLE type (NOT a union) — e.g. `rating?: number | null` → single float64 with `optional:true,nullable:true`.
- `A | B` (no null/undefined) → `{"type":"union","of":[…]}` SOURCE order.
- **named-vs-inline enum (Pitfall 4 / Open Q1 — pin at plan time):** check `type.aliasSymbol` on the RESIDUAL type (after stripping). Present → `{"type":"named","of":"<id>"}` + emit schema (`format: BookFormat` → named). Absent (bare literal union) → inline `{"type":"enum","of":sorted([...])}` (`sort?: SortOrder | null` → inline `enum:[asc,desc]`). Add a golden test for BOTH.
- DTO class type → `{"type":"named","of":"<id>"}` + emit class as a SchemaFact.

---

### `tsextract/schemas.js` (builder, transform)

**Analog:** `pyextract/schemas.py` (lines 1-25)

**Schema emission + axes pattern:**
```python
# one SchemaFact per model class (object body of FieldFacts) / Enum (sorted member
# values) / alias whose value is Literal[...] (inline enum) or Union (union body).
# A FieldFact has EXACTLY json_name, required, optional, nullable, schema,
# description, example — description/example always None.
#  * optional = the field has a default;  nullable = type admits None;
#  * required = not optional (nullable does NOT affect required).
```
**Copy:** one SchemaFact per class/alias; `id = relpath.removesuffix(".ts") + "." + name`; the four-axis matrix (`required = !optional`; nullable independent). **TS specifics (VERIFIED):** TRANSITIVE collection — every DTO referenced from any route param/body/response AND transitively from any field or union arm (e.g. `OutOfStockDto` appears only via `BookOrError`'s union arm — all 8 schemas must emit). Union-of-objects → `{"type":"union",...}` of `named` (source order: `BookDto` then `OutOfStockDto`); string-literal union → `enum` (sorted). `BookFormat` declared `'paperback'|'hardcover'` → snapshot `["hardcover","paperback"]` (sorted).

---

### `tsextract/diagnostics.js` (accumulator, event-driven)

**Analog:** `pyextract/diagnostics.py` (lines 13-37) — near-verbatim twin

```python
class Diagnostics:
    def __init__(self): self._items = []
    def warn(self, message, file, line):
        self._items.append({"severity":"WARN","message":message,"file":file,"line":int(line)})
    def items(self): return list(self._items)
```
**Copy:** identical shape. Note: the committed nestjs snapshot has `diagnostics: []` (the in-envelope fixture emits zero); the accumulator exists for the out-of-envelope/untyped-surface unit-test taxonomy (rule 3 — emit + OMIT, never guess).

---

### `tsextract/facts.js` (marshaller, transform)

**Analog:** `pyextract/facts.py` (lines 18-93) — the deterministic-marshal twin

**Build + sort + stringify pattern:**
```python
def build_doc(module, routes, schemas, diagnostics):
    return {"module":module, "routes":list(routes), "schemas":list(schemas), "diagnostics":list(diagnostics)}

def marshal(doc):
    _sort_doc(doc)
    return json.dumps(doc, sort_keys=True, separators=(",",":"), ensure_ascii=False)

def _sort_doc(doc):
    doc["schemas"].sort(key=lambda s: s["id"])
    doc["diagnostics"].sort(key=lambda d: (d["file"], d["line"], d["message"]))
    doc["routes"].sort(key=lambda r: (r["path"], r["method"]))
    # object fields by json_name; enum members lexically; union members NOT sorted (source order)
```
**Copy:** sort EVERY array by the exact keys above (schemas by `id`; fields by `json_name`; enum members lexical; diagnostics by `(file,line,message)`; routes by `(path,method)`; params by `(name,location)`; responses by `status`; UNION members NOT sorted). Node twin: pre-sort arrays, then `JSON.stringify` with a sorted-key replacer (objects built key-ordered or use a recursive key-sorter) — the host re-sorts too, but internal determinism is required.

---

### `tsextract/package.json` (config)

**Analog:** `goextract/go.mod` (manifest role); the contract is the carve-out.
**Copy:** name + `"dependencies": { "typescript": "5.9.3" }` ONLY (pin the exact version — a floating `^5` could change `typeToString` formatting). NO second dependency — that is a rule-2 defect. `private: true`. Plain-JS sidecar so no build step is needed at runtime (`node index.js <target_dir>`).

---

### `crates/gnr8-core/src/analyze/mod.rs` (dispatch seam — EDIT)

**Current `Lang` enum (lines 19-25):**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang { Go, Python }
```
→ add `TypeScript`.

**Current `detect_language` (lines 48-75) — 2-boolean decision to make 3-marker:**
```rust
pub(crate) fn detect_language(target_dir: &str) -> Result<Lang, crate::CoreError> {
    let mut has_go = false; let mut has_python = false;
    scan_markers(std::path::Path::new(target_dir), &mut has_go, &mut has_python);
    match (has_go, has_python) {
        (true, true) => Err(crate::CoreError::Config { message: format!("ambiguous ... BOTH Go ... and Python ...") }),
        (true, false) => Ok(Lang::Go),
        (false, true) => Ok(Lang::Python),
        (false, false) => Err(crate::CoreError::Config { message: format!("cannot determine ...") }),
    }
}
```
**Current `scan_markers` (lines 82-107):**
```rust
fn scan_markers(dir: &std::path::Path, has_go: &mut bool, has_python: &mut bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { scan_markers(&path, has_go, has_python); continue; }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if name == "go.mod" || ext.eq_ignore_ascii_case("go") { *has_go = true; }
        else if ext.eq_ignore_ascii_case("py") { *has_python = true; }
    }
}
```
**Current `build_graph` match (lines 129-132) — the 2-arm to make 3-arm:**
```rust
let facts = match detect_language(&target)? {
    Lang::Python => helper::run_pyextract(&target)?,
    Lang::Go => helper::run_goextract(&target)?,
};
```
**Edit plan:** add `ts: bool` to `scan_markers` (marker = `tsconfig.json` OR `*.ts` — ⚠️ MUST include `*.ts`; the nestjs fixture has NO tsconfig but HAS `*.ts`). Make `detect_language` count present languages: `1 → that lang`; `0 → Config "no Go/Python/TypeScript source"`; `>1 → Config "ambiguous: multiple languages"` (extend the WR-05 ambiguity arm to three — see the existing mixed-Go/Python tests at mod.rs:218-240 to add a TS row). Add `Lang::TypeScript => helper::run_tsextract(&target)?` to the `build_graph` match (exhaustiveness forces it). RESEARCH sketch at 04-RESEARCH.md lines 412-427.

---

### `crates/gnr8-core/src/analyze/helper.rs` (subprocess driver — twin)

**Analog:** `run_pyextract` / `pyextract_dir` (lines 36-38, 102-141)

**Path resolver:**
```rust
pub(crate) fn pyextract_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))   // repo root holding pyextract/
}
```
**Driver (the exact shape to mirror — split `_with(bin)` for the toolchain-missing test):**
```rust
pub(crate) fn run_pyextract(target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    run_pyextract_with("python3", target_dir)
}
fn run_pyextract_with(py_bin: &str, target_dir: &str) -> Result<facts::GoFacts, CoreError> {
    let output = Command::new(py_bin)
        .args(["-m", "pyextract", target_dir])         // DISCRETE args, no shell (T-02-01)
        .current_dir(pyextract_dir())
        .output()
        .map_err(|source| CoreError::PythonToolchainMissing { source })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CoreError::HelperExit { code: output.status.code(), stderr });
    }
    serde_json::from_slice(&output.stdout).map_err(|source| CoreError::FactsParse { source })
}
```
**Copy:** add `tsextract_dir() -> PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tsextract"))` (the dir holding `index.js` + `node_modules`, one level like `goextract_dir`, NOT the repo root). Add `run_tsextract` + `run_tsextract_with("node", ...)` invoking `Command::new("node").args(["index.js", target_dir]).current_dir(tsextract_dir())`, mapping spawn failure → the NEW `TypeScriptToolchainMissing` (or `NodeToolchainMissing`) variant, reusing `HelperExit`/`FactsParse`. Mirror the unit tests (helper.rs:210-228): `run_tsextract_with("gnr8-nonexistent-node-binary-xyz", …)` → toolchain-missing. Reuse `resolve_target` (lines 54-66) unchanged.

---

### `crates/gnr8-core/src/diagnostics/mod.rs` (dispatch seam — EDIT)

**Current `collect` match (lines 37-40) — the SECOND 2-arm match:**
```rust
let facts = match crate::analyze::detect_language(&target)? {
    Lang::Python => helper::run_pyextract(&target)?,
    Lang::Go => helper::run_goextract(&target)?,
};
```
**Edit plan:** add `Lang::TypeScript => helper::run_tsextract(&target)?` (exhaustiveness forces it). No nestjs diagnostics-text snapshot this phase (`diagnostics: []`), but the arm is mandatory for consistency + to avoid a `Config` error if exercised. `render`/`relativize` (lines 45-80) are language-agnostic — reused unchanged.

---

### `crates/gnr8-core/src/error.rs` (error type — twin)

**Analog:** `PythonToolchainMissing` (lines 38-49) + its Display test (lines 210-218)
```rust
#[error("Python toolchain not available (is `python3` installed and on PATH?): {source}")]
PythonToolchainMissing {
    #[source]
    source: std::io::Error,
},
```
**Copy:** add a `TypeScriptToolchainMissing` (or `NodeToolchainMissing`) variant with the same `#[source] std::io::Error` shape and an analogous `#[error("... is \`node\` installed and on PATH? ...")]` message. Reuse the existing `HelperExit`/`FactsParse`/`Config` variants (no new ones for exit/parse). Mirror the Display test (error.rs:210-218) asserting the message renders and contains the source. NOTE: also update the `build_graph_surfaces_typed_error_for_bad_target` test's accepted-error `matches!` set in mod.rs:160-170 to include the new variant.

---

### `crates/gnr8-core/src/sdk/builtins.rs` (Source built-in — clone)

**Analog:** `FastApi` Source (lines 84-144) / `Flask` (lines 146-200) — verbatim clone differing only in the proper noun

```rust
#[derive(Debug, Default, Clone)]
pub struct FastApi { inputs: Vec<String> }
impl FastApi {
    pub fn new() -> Self { Self::default() }
    pub fn inputs<I, S>(mut self, inputs: I) -> Self where I: IntoIterator<Item = S>, S: Into<String> {
        self.inputs = inputs.into_iter().map(Into::into).collect(); self
    }
}
impl Source for FastApi {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => return Err(CoreError::Config { message: "FastApi source has no inputs ...".into() }),
            many => return Err(CoreError::Config { message: format!("FastApi source lists {} inputs ...", many.len()) }),
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph(&resolved.to_string_lossy())   // SAME build_graph; lang detected from target
    }
}
```
**Copy:** clone as `NestJs` — single-input guard, project-root resolution, and the CRITICAL invariant: it calls the SAME `crate::analyze::build_graph` (language is detected from the TARGET, never from which Source was used — rule 3/4). Only the error proper noun changes ("NestJs source has no inputs ..."). Add the unit test mirroring `python_sources_error_when_unconfigured` (builtins.rs:790-820) for zero/many inputs. Re-export `NestJs` wherever `FastApi`/`Flask` are exported (check `crates/gnr8-core/src/sdk/mod.rs`).

---

### `crates/gnr8-core/tests/snapshot_nestjs_graph.rs` + `snapshot_nestjs_openapi.rs` (flip RED→GREEN)

**Current RED shape (graph test, lines 24-31):**
```rust
#[test]
#[ignore = "red-by-design: tsextract lands in Phase 4; ..."]
fn graph_matches_expected_for_nestjs() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("tsextract lands in Phase 4 — intentionally red until then");
    insta::assert_yaml_snapshot!("nestjs_graph", graph);
}
```
**Green analog (`snapshot_fastapi_graph.rs`, lines 24-31 — already green in Phase 2):**
```rust
#[test]
fn graph_matches_expected_for_fastapi() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("analyze::build_graph must succeed (requires the python3 toolchain)");
    insta::assert_yaml_snapshot!("fastapi_graph", graph);
}
```
**Edit plan:** remove the `#[ignore]` attribute and the "red until then" wording so the test runs against real tsextract; KEEP the assertion + the committed `.snap` UNCHANGED (zero snapshot edits — the snapshot is the spec). The openapi test (lines 35-44) flips identically; KEEP its `fixture_security()` helper + the `to_openapi(&graph, "bookstore", "/books", &fixture_security())` call (title/base_path/security are TEST-supplied code-as-config, NOT scraped — Pitfall 9). ⚠️ **DIVERGENCE from the FastAPI analog:** the FastAPI tests hard-require the toolchain (no skip). CONTEXT/RESEARCH require the nestjs tests to **skip-if-(node OR typescript)-absent** so a node-less / not-yet-installed env never hard-fails `make check` — there is NO existing skip-pattern analog in the test dir, so the planner must DESIGN one (e.g. probe `node` + `require('typescript')` and `return` early, or gate on `build_graph` returning the new toolchain-missing variant). Clean the stale `*.snap.new` files when green.

---

### `fixtures/nestjs-bookstore/src/*.ts` (fixture — line reconciliation)

**Analog:** the Phase-2 FastAPI/Flask fixture line reconciliation (resolved Q2).
**Current vs target lines (VERIFIED, RESEARCH Pitfall 8 — the snapshot is authoritative):**
- Operations (current AST anchor → snapshot line): listBooks 40→41, createBook 50→51, getBook 57→57, updateBook 66→65. Params: genre 41→42, sort 42→43, cursor 43→44, bookId(get) 59→58, fmt 60→59, bookId(put) 68→66.
- DTO schemas (class/alias line → snapshot): `BookFormat` 21→36, `AuthorDto` 29→41, `BookDto` 37→47, `BookFilters` 48→56, `OutOfStockDto` 60→70, `BookOrError` 65→73, `CreatedMessage` 67→75, `ListBooksResponse` 72→80.
**Edit plan:** pick ONE consistent anchor convention (operation = method-name line; param = param-name line; schema = class/type-alias-name line), then edit the fixture inserting BLANK LINES and NON-FACT COMMENTS ONLY — rule 1: NEVER add a swagger/zod/class-validator annotation or any API-fact-bearing construct. Add a golden test asserting produced line == snapshot line. Correcting a snapshot line is allowed ONLY if a line is genuinely unreachable (it is not — blank lines give ample freedom).

---

### `.gitignore` (config)

**Analog:** the existing `__pycache__/` / `*.pyc` entries (generated-by-sidecar artifacts).
**Edit plan:** `.gitignore` currently lists `target` + `*.snap.new` only — it does NOT ignore `node_modules`. The node_modules vendoring decision is OPEN (RESEARCH lines 299-304): EITHER (1) commit `tsextract/node_modules/typescript` (hermetic, +23M, one package, aligns with "own our pipeline") OR (2) `.gitignore tsextract/node_modules` + committed `package-lock.json` + an `npm ci` step in `make check`. Pin at plan time. EITHER WAY the nestjs tests MUST skip-if-toolchain-absent. If option 2: add `tsextract/node_modules` (mirror the `__pycache__/` entry style).

## Shared Patterns

### Sidecar → neutral JSON → strict deserialize (the v2.0 narrow waist)
**Source:** `crates/gnr8-core/src/analyze/facts.rs` (the `deny_unknown_fields` DTO).
**Apply to:** every `tsextract/*.js` module + `run_tsextract`. The entire Rust pipeline downstream of the facts JSON (`ApiGraph::from_facts` → `lower::to_openapi`) is REUSED UNCHANGED — never forked. The sidecar marshals, the host deserializes strictly + re-sorts + relativizes spans.

### Single deterministic language dispatch (rule 3)
**Source:** `analyze::detect_language` (mod.rs:48) — ONE scan, ONE decision; mirrored identically in `diagnostics::collect` (diagnostics/mod.rs:37).
**Apply to:** both dispatch-seam edits. Extend to 3 markers; mixed tree → typed `Config` error. NEVER a try-Go-then-Py-then-TS chain.

### Typed errors, no panics (RUST-04)
**Source:** `error.rs` `PythonToolchainMissing` + `HelperExit` + `FactsParse`; every driver propagates with `?`.
**Apply to:** `run_tsextract` + the new toolchain-missing variant. No `unwrap`/`expect`/`panic` in production paths; tests scope `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

### DISCRETE subprocess args (threat T-02-01)
**Source:** `helper.rs` `Command::new(bin).args([...discrete...])` — no `sh -c`, no string interpolation of `target_dir`.
**Apply to:** the `node index.js <target_dir>` invocation.

### Code-as-config security (rule 4)
**Source:** `snapshot_*_openapi.rs` `fixture_security()` + `to_openapi(&graph, title, base_path, &security)`; `ApplySecurity`/`SetTitle`/`SetBasePath` transforms (builtins.rs:206-282).
**Apply to:** the nestjs openapi test — title/base_path/security are SUPPLIED, never scraped (Pitfall 9).

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `tsextract/load.js` (TS Program setup) | loader | parse | `pyextract/load.py` gives the SHAPE (discover + parse + per-file diag), but `ts.createProgram` + `getTypeChecker` has no Python analog (Python uses `ast.parse`). The Compiler-API mechanics are new (RESEARCH Pattern 1 / Code Examples lines 365-388). |
| nestjs-test skip-if-toolchain guard | test | — | No existing test in `crates/gnr8-core/tests/` skips on a missing toolchain (FastAPI/Flask tests hard-require it). CONTEXT/RESEARCH require this NEW behavior for nestjs — design it (no analog to copy). |

## Metadata

**Analog search scope:** `pyextract/` (entrypoint + per-concern modules), `goextract/` (layout), `crates/gnr8-core/src/{analyze,diagnostics,sdk,error}.rs`, `crates/gnr8-core/tests/snapshot_{nestjs,fastapi}_{graph,openapi}.rs`, `crates/gnr8-core/tests/snapshots/snapshot_nestjs_*.snap`, `fixtures/nestjs-bookstore/`, `.gitignore`.
**Files scanned:** 18
**Pattern extraction date:** 2026-06-25
