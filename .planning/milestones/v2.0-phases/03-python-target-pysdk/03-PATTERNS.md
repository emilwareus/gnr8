# Phase 3: Python Target — `PySdk` - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 8 (3 new module files, 3 modified wiring files, 1+ new test files)
**Analogs found:** 8 / 8 (every file has a precise in-repo twin)

This phase is a **structural twin** exercise: `pysdk/` mirrors `gosdk/` (minus `gofmt.rs`), `PySdk`
mirrors `GoSdk`, and `tests/sdk_compile_py.rs` mirrors `tests/sdk_compile.rs`. The planner should treat
every analog below as the literal copy-from source and translate Go idioms → Python-stdlib idioms at each
seam. **The one place that is NOT a mechanical port is `pysdk/emit.rs`'s `py_type`** — the Go `go_type`
*rejects* `Union` / inline `Enum` / inline `Object`; the Python target *must emit* them (see the
load-bearing excerpt and the mapping table below).

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/gnr8-core/src/pysdk/mod.rs` (NEW) | module / generator orchestration | transform (IR→string) | `crates/gnr8-core/src/gosdk/mod.rs` | exact |
| `crates/gnr8-core/src/pysdk/emit.rs` (NEW) | emitter / utility | transform (IR→string) | `crates/gnr8-core/src/gosdk/emit.rs` | exact (DIVERGES at `py_type`) |
| `crates/gnr8-core/src/pysdk/bundle.rs` (NEW) | utility / framing | transform (string framing) | `crates/gnr8-core/src/gosdk/bundle.rs` | exact (copy verbatim) |
| `crates/gnr8-core/src/sdk/builtins.rs` (MODIFY: add `PySdk`) | config built-in / target | request-response (IR→Artifacts) | `GoSdk` @ `builtins.rs:403-490` | exact |
| `crates/gnr8-core/src/sdk/mod.rs` (MODIFY: prelude) | config / re-export | — | prelude @ `sdk/mod.rs:336-343` | exact |
| `crates/gnr8-core/src/lib.rs` (MODIFY: register module) | config / module decl | — | `pub mod gosdk;` @ `lib.rs:10` | exact |
| `crates/gnr8-core/tests/sdk_compile_py.rs` (NEW) | test (integration) | event-driven (subprocess + http round-trip) | `crates/gnr8-core/tests/sdk_compile.rs` | exact (Go→Python idioms) |
| `crates/gnr8-core/tests/snapshot_pysdk.rs` (OPTIONAL) | test (snapshot) | transform (assert artifact) | `crates/gnr8-core/tests/snapshot_sdk.rs` | exact |

**NO `pysdk/gofmt.rs`** — the `gosdk/gofmt.rs` subprocess normalizer has no Python analog (no stdlib
formatter; `black`/`autopep8` are third-party, rule 2). `pysdk::emit` must emit correct
significant-whitespace Python directly. See "Dropped Analog" below.

## Pattern Assignments

### `crates/gnr8-core/src/pysdk/mod.rs` (module, IR→string orchestration)

**Analog:** `crates/gnr8-core/src/gosdk/mod.rs`

**`generate()` signature + push-in-fixed-order + bundle pattern** (`gosdk/mod.rs:40-65`):
```rust
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();
    files.push(go_file("client.go", &emit::emit_client(package))?);
    files.push(go_file("errors.go", &emit::emit_errors(package))?);
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let raw = emit::emit_operations(graph, package, base_path, &ops)?;
    files.push(go_file("operations.go", &raw)?);
    files.push(go_file("models.go", &emit::emit_models(graph, package)?)?);
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}
```
**Twin guidance:** keep the SAME `(graph, package, base_path) -> Result<String, CoreError>` signature.
Python file split (per RESEARCH §Recommended Project Structure): push in fixed sorted order
`__init__.py`, `client.py`, `errors.py`, `models.py` (alpha-sorted so the bundle is deterministic and
matches `Artifacts` binary-search insert). **Drop the `go_file()` gofmt wrapper** — push
`SdkFile { name, contents: emit::emit_xxx(..)? }` directly (no normalization step).

**`split_bundle` re-export of the private framing** (`gosdk/mod.rs:74-76`):
```rust
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> {
    bundle::parse(bundle)
}
```

**`write_to_dir` with the name-safety guard** (`gosdk/mod.rs:100-115`):
```rust
pub fn write_to_dir(bundle: &str, dir: &std::path::Path) -> Result<(), crate::CoreError> {
    for (name, contents) in bundle::parse(bundle) {
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(crate::CoreError::SdkGen {
                message: format!("refusing to write SDK file with unsafe name {name:?}"),
            });
        }
        let path = dir.join(&name);
        std::fs::write(&path, contents).map_err(|err| crate::CoreError::SdkGen {
            message: format!("failed to write SDK file {}: {err}", path.display()),
        })?;
    }
    Ok(())
}
```
**Twin guidance:** copy `write_to_dir` verbatim (the guard is the V5/V12 path-traversal mitigation —
reuse, do not re-derive).

**Unit-test pattern** (`gosdk/mod.rs:117-301`): mirror the marker-presence test and the
byte-identical-across-two-runs determinism test, but **WITHOUT any toolchain skip** — `pysdk::generate`
is pure string emission with no subprocess (no `gofmt_available()` gate). Assert the four Python markers
(`__init__.py`, `client.py`, `errors.py`, `models.py`) and `generate(ir) == generate(ir)`.

---

### `crates/gnr8-core/src/pysdk/emit.rs` (emitter, IR→string) — THE LOAD-BEARING DIVERGENCE

**Analog:** `crates/gnr8-core/src/gosdk/emit.rs`

**The `sink()` helper — fold `fmt::Error` → `SdkGen` (copy verbatim, RUST-04)** (`gosdk/emit.rs:33-37`):
```rust
fn sink(err: std::fmt::Error) -> CoreError {
    CoreError::SdkGen {
        message: format!("failed to format Python source: {err}"),
    }
}
```

**CRITICAL — `go_type`'s exhaustive match WITH the reject arms the Python target must REPLACE**
(`gosdk/emit.rs:113-173`). This is the single most important excerpt in the phase. Note the
`Type::Object`, `Type::Enum`, and `Type::Union` arms all return `CoreError::SdkGen` — **`py_type` must
instead emit a Python type for each**:
```rust
fn go_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(prim) => go_primitive(prim).to_string(),
        Type::WellKnown(well_known) => go_well_known(well_known).to_string(),
        Type::Array(items) => {
            return Ok(format!("[]{}", go_type(items, false, graph)?));
        }
        Type::Map { .. } | Type::Any {} => "map[string]any".to_string(),
        Type::Named(ref_id) => {
            let target = graph.schemas.iter().find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            return Ok(maybe_pointer(target.name.clone(), nullable, is_value_ref(target)));
        }
        // ↓↓↓ THESE THREE ARMS ARE WHAT PYSDK CHANGES ↓↓↓
        Type::Object(_) => {           // Go: ERROR. Python: typed error (parity) OR Dict[str, Any]
            return Err(CoreError::SdkGen {
                message: "inline object type is unsupported by the Go SDK target \
                          (expected a named $ref)".to_string(),
            });
        }
        Type::Enum(_) => {             // Go: ERROR. Python: MUST emit Literal["asc","desc"]
            return Err(CoreError::SdkGen {
                message: "inline enum type is unsupported by the Go SDK target \
                          (expected a named $ref)".to_string(),
            });
        }
        Type::Union(_) => {            // Go: ERROR. Python: MUST emit Union[A, B]
            return Err(CoreError::SdkGen {
                message: "union type is unsupported by the Go SDK target (Go has no sum types)"
                    .to_string(),
            });
        }
    };
    let is_value = matches!(base.as_str(), "bool" | "int64" | "float32" | "time.Time");
    Ok(maybe_pointer(base, nullable, is_value))
}
```

**REQUIRED `py_type` mapping (every `Type` variant — keep the match exhaustive, NO `_ =>` arm, rule 3):**

| `Type` variant | Go (`go_type`) | Python (`py_type`) — REQUIRED |
|----------------|----------------|-------------------------------|
| `Primitive(String)` | `string` | `str` |
| `Primitive(Bool)` | `bool` | `bool` |
| `Primitive(Int{..})` | `int64` | `int` |
| `Primitive(Float{..})` | `float32` | `float` |
| `Primitive(Bytes)` | `[]byte` | `bytes` |
| `WellKnown(DateTime)` | `time.Time` | `str` (RFC-3339 wire string — see A7) |
| `WellKnown(Uuid/Date/Duration/Decimal/Email/Uri)` | `string` | `str` |
| `Array(T)` | `[]T` | `List[<py_type(T)>]` |
| `Map{..}` / `Any{}` | `map[string]any` | `Dict[str, Any]` / `Any` |
| `Named(ref)` | resolve → name, `*T` if nullable | resolve → `name`; wrap `Optional[name]` if nullable |
| `Object(fields)` | **ERROR** | typed `SdkGen` error (parity) unless plan finds a reachable case → then `Dict[str, Any]` |
| `Enum(members)` | **ERROR** | **`Literal["a","b"]`** (members in graph order) |
| `Union(variants)` | **ERROR** | **`Union[<py_type(v)…>]`** |

**Optional vs nullable — carry the two axes independently** (`gosdk/emit.rs` `maybe_pointer`:226-232 /
`json_tag`:236-242 / `emit_struct_field`:335-341). Go: `nullable`→`*T` pointer, `optional`→`,omitempty`
tag. Python twin: `nullable`→`Optional[...]` wrap; `optional`→dataclass field default. **PITFALL 1
(RESEARCH §Common Pitfalls):** `@dataclass` forbids a non-default field after a default field, and the
graph sorts fields alphabetically by `json_name`. The Python `emit_struct` MUST partition fields
(required-first, optional-last) before writing — a Python-presentation reorder that does not change wire
behavior (json keys are name-addressed). `kw_only=True` is 3.10+ and NOT available — partitioning is the
3.9-safe fix.

**Named-vs-inline enum split** (`gosdk/emit.rs` `emit_enum`:344-354 emits `type X string` + const block).
Python twin (RESEARCH Pattern 2): in `emit_models`, a `Schema` whose `body` is `Type::Enum` → emit
`class X(str, enum.Enum)` with `SCREAMING_SNAKE` members; in `py_type` (fields/params/returns), a
`Type::Enum` → emit `Literal[...]`. Never the reverse (PITFALL 2).

**The `exported()` / `split_words()` identifier helpers** (`gosdk/emit.rs:44-94`) are reusable
conceptually — Python class names want CamelCase (reuse `exported`), enum members want SCREAMING_SNAKE,
method names want snake_case (`create_book`, not Go's CamelCase `CreateGoal`). Plan the Python casing
helpers off these.

**Operation / client / errors emitters** (`emit_client`:361-400, `emit_errors`:406-430,
`emit_operations`:570-598, `emit_request_dispatch`:685-778, `success_of`:451-477, `error_model_of`:496-519,
`emit_url` with `url.PathEscape`:927-971). Python twins per RESEARCH Pattern 3:
- `Client.__init__(base_url, *, api_key=None, opener=None)` with `opener or urllib.request.build_opener()`.
- `_do(method, path, body)` building `urllib.request.Request`, `try/except urllib.error.HTTPError`
  (PITFALL 6 — urllib raises on 4xx; that is the typed-`ApiError` path).
- Per-op method: `urllib.parse.quote(...)` on each path segment (V5 path-injection — twin of
  Go's `url.PathEscape` @ `emit_url:962`), `urllib.parse.urlencode` for query, compare against the
  operation's real success status (`success_of` twin), raise `ApiError(status_code=…)` otherwise.
- `ApiError(Exception)` with `status_code`/`message`/`slug`/`hints` + `is_not_found()` (twin of
  `emit_errors`'s `APIError` + `IsNotFound()`).

**Import header — fixed, not computed** (DIVERGENCE from `gosdk`). Go computes imports via `BTreeSet`
(`query_imports`:887-898, `file`:1029-1052) because `go build` rejects unused imports. Python tolerates
unused imports at runtime, so emit a **fixed deterministic header per file** (RESEARCH Pitfall 4): e.g.
`models.py` always emits `from __future__ import annotations`, `from dataclasses import dataclass, field`,
`import enum`, `from typing import Optional, Union, List, Dict, Any, Literal`. Emit
`from __future__ import annotations` at the TOP of every module (PITFALL 3 — makes all annotations lazy
strings, sidesteps 3.9 generic-subscription + forward-ref ordering).

---

### `crates/gnr8-core/src/pysdk/bundle.rs` (utility, framing) — COPY VERBATIM

**Analog:** `crates/gnr8-core/src/gosdk/bundle.rs`

Copy the entire file unchanged: `SdkFile`/`SdkBundle` structs, the `// ==== gnr8:file <name> ====`
`MARKER_PREFIX`/`MARKER_SUFFIX` constants, the `Display` impl, `parse`, `parse_marker`, and the
round-trip tests. **The marker is a Go `//` comment but it does NOT need to be valid Python** —
`parse` strips marker lines before `write_to_dir` materializes files, and the marker never appears inside
emitted Python (RESEARCH Pattern 4, A4). Keeping it byte-identical to the proven twin is the recommended
path. Frame serialization (`Display`, `bundle/mod.rs:52-64`):
```rust
impl std::fmt::Display for SdkBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for file in &self.files {
            writeln!(f, "{}", marker_for(&file.name))?;
            f.write_str(&file.contents)?;
            if !file.contents.ends_with('\n') { writeln!(f)?; }
        }
        Ok(())
    }
}
```

---

### `crates/gnr8-core/src/sdk/builtins.rs` (target, IR→Artifacts) — ADD `PySdk`

**Analog:** `GoSdk` struct + builder + `impl Target` @ `builtins.rs:403-490`; name derivation
`sdk_package` @ `builtins.rs:555-572`.

**The `impl Target` body to clone** (`builtins.rs:447-490`):
```rust
impl Target for GoSdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no module — call .module(\"example.com/acme/sdk\")".to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        let package = sdk_package(&self.module)?;
        let bundle = crate::gosdk::generate(ir, &package, &ir.base_path)?;
        let dir = self.dir.trim_end_matches('/');
        for (name, contents) in crate::gosdk::split_bundle(&bundle) {
            if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(CoreError::SdkGen {
                    message: format!("refusing to emit SDK file with unsafe name {name:?}"),
                });
            }
            out.write(format!("{dir}/{name}"), contents);
        }
        Ok(())
    }
    fn output_anchors(&self) -> Vec<String> {            // loop-safety anchor — copy
        if self.dir.is_empty() { Vec::new() }
        else { vec![self.dir.trim_end_matches('/').to_string()] }
    }
}
```
**Twin guidance:** `PySdk` keeps the same `.module(..)` + `.to(..)` builder, calls
`crate::pysdk::generate(ir, &package, &ir.base_path)` and `crate::pysdk::split_bundle`, and reuses the
identical safe-name guard + `output_anchors` (loop-safety so the pipeline never re-ingests the generated
`*.py`). `ir.base_path` is the SAME single source of truth the OpenAPI lowering uses (rule 3/4 — never
re-derive). **Reuse the existing `sdk_package` helper** (last-segment, ASCII-alnum, leading-digit-trim) —
it already produces a valid Python identifier; do not write a second derivation (rule 3).

**`sdk_package` (single-source name derivation, reuse as-is)** (`builtins.rs:555-572`):
```rust
fn sdk_package(module: &str) -> Result<String, CoreError> {
    let last = module.rsplit('/').next().unwrap_or("");
    let kept: String = last.chars().filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase()).collect();
    let pkg = kept.trim_start_matches(|c: char| c.is_ascii_digit());
    if pkg.is_empty() {
        return Err(CoreError::Config { message: format!(/* … */) });
    }
    Ok(pkg.to_string())
}
```

**Unconfigured-error test to extend** (`builtins.rs:619-636`): add `PySdk::new()` and
`PySdk::new().module("x.com/sdk")` cases asserting `Err(CoreError::Config { .. })` — mirror the existing
`GoSdk` arms in `targets_error_when_unconfigured`.

---

### `crates/gnr8-core/src/sdk/mod.rs` (prelude re-export) — ADD `PySdk`

**Analog:** prelude @ `sdk/mod.rs:336-343`:
```rust
pub mod prelude {
    pub use super::builtins::{
        ApplySecurity, FastApi, Flask, GoGin, GoSdk, Header, OpenApi31, RenameOperation,
        RenameType, SetBasePath, SetTitle,
    };
    pub use super::{Artifact, Artifacts, Cx, Pipeline, PostProcess, Source, Target, Transform};
    pub use crate::graph::SecurityScheme;
}
```
**Twin guidance:** add `PySdk` to the `super::builtins::{…}` list (alpha-near `OpenApi31`). One-line change.

---

### `crates/gnr8-core/src/lib.rs` (module registration) — ADD `pub mod pysdk;`

**Analog:** `lib.rs:10` `pub mod gosdk;` (sits between `diagnostics` and `graph`). Add `pub mod pysdk;`
in alpha order (after `pub mod manifest;` / before `pub mod runner;`, or wherever alpha lands). One line.
`pub use sdk::prelude;` @ `lib.rs:22` already re-exports the prelude — no change needed there.

---

### `crates/gnr8-core/tests/sdk_compile_py.rs` (integration test) — THE HERMETIC TWIN

**Analog:** `crates/gnr8-core/tests/sdk_compile.rs` (full file, 273 lines).

**Reusable harness pieces to copy and translate:**

`unique_temp_dir` — copy verbatim (no `tempfile` crate, T-03-03) (`sdk_compile.rs:43-53`):
```rust
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-sdk-compile-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}
```

`go_available()` → `python_available()` (toolchain skip-if-absent) (`sdk_compile.rs:32-39`):
```rust
fn go_available() -> bool {
    Command::new("go").arg("version")
        .stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}
// Python twin: Command::new("python3").arg("--version") … .status().is_ok()
```

`run_go(args, dir)` → `run_python(args, dir)` (discrete args, NO shell — V13 subprocess safety;
map non-zero exit to a captured-stderr failure; spawn failure → skip) (`sdk_compile.rs:58-76`):
```rust
fn run_go(args: &[&str], dir: &Path) -> Result<String, gnr8_core::CoreError> {
    let output = Command::new("go").args(args).current_dir(dir)
        .env("GOPROXY", "off").env("GOFLAGS", "-mod=mod")   // hermetic — Pitfall 5
        .output().map_err(|source| gnr8_core::CoreError::GoToolchainMissing { source })?;
    if !output.status.success() {
        return Err(gnr8_core::CoreError::GoBuild {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
```
**Python twin error mapping:** spawn failure → `CoreError::PythonToolchainMissing { source }` (the
variant already exists @ `error.rs:45`); non-zero exit → reuse `CoreError::GoBuild`-shaped capture or a
generic stderr capture (no new error variant needed — `SdkGen`/`Io` already exist; do NOT add a variant).

`materialize_sdk()` (`sdk_compile.rs:97-107`) — twin: build the graph from the **FastAPI fixture**,
generate via `pysdk::generate`, write via `pysdk::write_to_dir`:
```rust
// Go original:
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");
let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect(/* … */);
let bundle = gnr8_core::gosdk::generate(&graph, "goalservice", "/goal").expect(/* … */);
gnr8_core::gosdk::write_to_dir(&bundle, &dir).expect(/* … */);
write_go_mod(&dir);
// Python twin:  FIXTURE_DIR = ".../fixtures/fastapi-bookstore"
//   build_graph(FIXTURE_DIR) → pysdk::generate(&graph, "<pkg>", &graph.base_path)
//   → pysdk::write_to_dir(&bundle, &dir)   (NO go.mod analog — Python needs no manifest)
```
> NOTE: `build_graph` on a Python dir already routes to `run_pyextract` (RESEARCH — `analyze/mod.rs`
> has the `Lang::Python` arm). Since `python3` IS present and `go` is NOT in this sandbox, the Python
> hermetic test actually RUNS (the Go tests skip) — this is the test that proves the phase.

**Three gates (twin of `go build` + `httptest` smoke):**
- (a) SYNTAX: `run_python(&["-m", "py_compile", <each .py>], &dir)` — non-zero == IndentationError/syntax.
- (b) IMPORT: `run_python(&["-c", "import <pkg>"], &dir)` with the temp dir on `PYTHONPATH`/cwd —
  executes class bodies, catches the dataclass field-order `TypeError` (PITFALL 1), bad `Optional`, NameError.
- (c) ROUND-TRIP: write a **separate Python driver script** to the temp dir (twin of how the Go test
  writes `smoke_test.go` separately @ `sdk_compile.rs:157-235`, NOT part of the SDK bundle), then
  `run_python(&["<driver>.py"], &dir)`. The driver spawns a stdlib `http.server` on **ephemeral port 0**
  (PITFALL 5), injects an `OpenerDirector` into the generated `Client`, asserts a 2xx `@dataclass`
  round-trip AND a 4xx → `ApiError(status_code=…, is_not_found())`, exiting non-zero on assertion failure.
  Driver content is program-generated and written to a file — **never `-c "<interpolated user data>"`**
  (V13 command-injection mitigation). The driver shape is in RESEARCH §Code Examples.

**PYSDK-01 assertion (no third-party HTTP):** also assert the generated `client.py` contains
`OpenerDirector` / `@dataclass` / `ApiError` and does NOT contain `import requests` / `import httpx`
(REQUIREMENTS Out of Scope).

---

### `crates/gnr8-core/tests/snapshot_pysdk.rs` (OPTIONAL snapshot test)

**Analog:** `crates/gnr8-core/tests/snapshot_sdk.rs` (28 lines, full):
```rust
#[test]
fn sdk_matches_expected_for_goalservice() {
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect(/* … */);
    let sdk = gnr8_core::gosdk::generate(&graph, "goalservice", "/goal").expect(/* … */);
    insta::assert_snapshot!("goalservice_sdk", sdk);
}
```
**Twin guidance:** add ONLY if it mirrors cleanly (3-line change: FastAPI fixture, `pysdk::generate`,
`assert_snapshot!("fastapi_sdk", sdk)`). `insta` is an existing dev-dependency (do not add it). Gate
behind `python_available()` since it runs `build_graph` → pyextract. Decision deferred to planner
(CONTEXT line 33-34 + RESEARCH Open Q3) — acceptable either way.

## Shared Patterns

### Name-safety guard (path-traversal mitigation, V5/V12)
**Source:** `gosdk/mod.rs:104-108` AND `builtins.rs:469-473` (the identical guard appears in both
`write_to_dir` and the `GoSdk` target).
**Apply to:** `pysdk::write_to_dir` AND the `PySdk` target's `generate` loop.
```rust
if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
    return Err(crate::CoreError::SdkGen {
        message: format!("refusing to write SDK file with unsafe name {name:?}"),
    });
}
```

### `sink()` — infallible `format!` folded to a typed error (RUST-04, no panic)
**Source:** `gosdk/emit.rs:33-37`.
**Apply to:** every `pysdk::emit` function that does `writeln!`/`write!` into a `String`. Map
`std::fmt::Error` → `CoreError::SdkGen`. No production `unwrap`/`expect`/`panic`; scope
`#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` to `#[cfg(test)]` only (every analog
module does this at its `mod tests` top).

### Determinism discipline (PYSDK-03)
**Source:** `gosdk/emit.rs:17-21` (module doc) + `Artifacts::write` binary-search sorted insert
(`sdk/mod.rs:96-108`).
**Apply to:** all of `pysdk`. Consume the graph's already-sorted `Vec`s in order; never iterate a
`HashMap`; use `BTreeSet` only if a set is genuinely needed (RESEARCH recommends a FIXED import header
instead → zero set computation). The bundle's fixed push order + `Artifacts` sorted insert give
byte-identical output for free.

### Exhaustive `match Type` with NO `_ =>` (rule 3)
**Source:** every `match schema {…}` in `gosdk/emit.rs` (`go_type`:114, `is_value_ref`:208,
`field_needs_time`:247, `emit_models`:283) spells out every variant so a new IR variant fails to compile
until handled.
**Apply to:** `py_type` and every type-walking helper in `pysdk::emit` — list all of
`Primitive/WellKnown/Array/Map/Named/Object/Enum/Union/Any`, no catch-all.

### Single source of truth: package name + base path (rules 3/4)
**Source:** `GoSdk::generate` @ `builtins.rs:462-463` (`sdk_package(&self.module)` + `&ir.base_path`).
**Apply to:** `PySdk::generate` — derive the Python package from `.module(..)` via the SAME
`sdk_package`, take the URL prefix from `ir.base_path` (the value `SetBasePath` set and the OpenAPI
lowering reads). No second derivation, no fallback.

## Dropped Analog (intentional)

| Analog | Why NOT mirrored |
|--------|------------------|
| `crates/gnr8-core/src/gosdk/gofmt.rs` (`gofmt` subprocess normalizer @ `gofmt.rs:28-30`) | No Python stdlib formatter exists; `black`/`autopep8` are third-party (rule 2). `pysdk::emit` emits already-correct significant-whitespace Python DIRECTLY. Consequently `pysdk/mod.rs` drops the `go_file()` gofmt-wrapper (`gosdk/mod.rs:79-84`) and pushes `SdkFile`s straight from the emitters. No `CoreError::GoFmt`-equivalent variant is needed. |

## No Analog Found

None. Every file in this phase has a precise in-repo twin (the entire `gosdk/` module, the `GoSdk`
target, and the `sdk_compile.rs`/`snapshot_sdk.rs` tests were read in full). The only genuinely new code
is (1) `py_type`'s emission of the `Union`/inline-`Enum` cases Go rejects, and (2) significant-whitespace
emission without a formatter — both are translations layered onto the existing twin, not new structures.

## Metadata

**Analog search scope:** `crates/gnr8-core/src/gosdk/`, `crates/gnr8-core/src/sdk/`,
`crates/gnr8-core/src/lib.rs`, `crates/gnr8-core/tests/` (sdk_compile, snapshot_sdk, sdk_pipeline),
`crates/gnr8-core/src/error.rs`.
**Files scanned (read):** `gosdk/mod.rs`, `gosdk/emit.rs` (lines 1-1282 of 1851 — `go_type` + all helpers
+ emitters captured), `gosdk/bundle.rs`, `gosdk/gofmt.rs` (head), `sdk/builtins.rs` (GoSdk, FastApi,
sdk_package, tests), `sdk/mod.rs` (Artifacts, traits, prelude), `lib.rs`, `tests/sdk_compile.rs`,
`tests/snapshot_sdk.rs`; grep over `error.rs`.
**Pattern extraction date:** 2026-06-25
