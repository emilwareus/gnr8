# Phase 5: TypeScript Target — `TsSdk` - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 6 (4 new, 2 modified) + 1 new test file
**Analogs found:** 7 / 7 (every artifact has an exact `pysdk` twin)

> This phase is a **pure rename-and-clone** of the `pysdk` module. Every new file has an exact 1:1
> analog. The ONLY genuinely new authored logic is (a) the `ts_type` mapping table, (b) the four TS
> string templates in the emitters, and (c) the `tsc --noEmit` invocation in the test. Everything else
> (bundle framing, package derivation, path tokens, success/body resolution, path-safety, the Target
> wrapper, prelude/lib wiring) is a structural copy.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/gnr8-core/src/tssdk/mod.rs` (NEW) | service (codegen orchestration) | transform | `src/pysdk/mod.rs` | exact |
| `crates/gnr8-core/src/tssdk/emit.rs` (NEW) | service (IR→string emitters) | transform | `src/pysdk/emit.rs` | exact |
| `crates/gnr8-core/src/tssdk/bundle.rs` (NEW) | utility (multi-file framing) | transform | `src/pysdk/bundle.rs` | exact (verbatim clone) |
| `crates/gnr8-core/src/sdk/builtins.rs` (MODIFY) | provider (`Target` impl) | request-response (`generate`) | `PySdk` in same file | exact |
| `crates/gnr8-core/src/sdk/mod.rs` (MODIFY) | config (prelude re-export) | — | `PySdk` line in `prelude` | exact |
| `crates/gnr8-core/src/lib.rs` (MODIFY) | config (module registration) | — | `pub mod pysdk;` line 15 | exact |
| `crates/gnr8-core/tests/tssdk_compile.rs` (NEW) | test (hermetic typecheck) | request-response (subprocess) | `tests/pysdk_compile.rs` | exact (swap `py_compile`+`http.server` → `tsc`) |

---

## Pattern Assignments

### `crates/gnr8-core/src/tssdk/mod.rs` (service, transform)

**Analog:** `crates/gnr8-core/src/pysdk/mod.rs`

**Module-private submodule declaration + imports** (pysdk/mod.rs:14-18):
```rust
mod bundle;
mod emit;

use crate::graph::{ApiGraph, Operation};
use bundle::{SdkBundle, SdkFile};
```

**`generate` signature + body** — copy verbatim, change the four pushed file names from `.py` to `.ts`
and call `emit_index` instead of `emit_init` (pysdk/mod.rs:38-73). The fixed alpha push order becomes
`client.ts`, `errors.ts`, `index.ts`, `models.ts` (re-sort alphabetically — `i` < `m`, so `index.ts`
precedes `models.ts`; keep client/errors first as the analog does):
```rust
pub fn generate(graph: &ApiGraph, package: &str, base_path: &str) -> Result<String, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();

    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let mut client = emit::emit_client(package);
    client.push_str(&emit::emit_operations(graph, package, base_path, &ops)?);
    files.push(SdkFile { name: "client.ts".to_string(), contents: client });

    files.push(SdkFile { name: "errors.ts".to_string(), contents: emit::emit_errors(package) });
    files.push(SdkFile { name: "index.ts".to_string(), contents: emit::emit_index(graph, package) });
    files.push(SdkFile { name: "models.ts".to_string(), contents: emit::emit_models(graph, package)? });

    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}
```
> NOTE on push order: pysdk pushes `__init__.py` FIRST. For TS the alpha order is `client.ts`,
> `errors.ts`, `index.ts`, `models.ts`. Pick a fixed deterministic order (the module docs say "fixed
> sorted order"); planner's call which exact order — just keep it FIXED and assert it in the four-marker
> test, exactly like pysdk/mod.rs:186-197.

**`split_bundle` + `write_to_dir` (path-safety guard)** — copy verbatim (pysdk/mod.rs:80-111). The
unsafe-name guard is load-bearing security (T-03-01-01) and ports unchanged:
```rust
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> { bundle::parse(bundle) }

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

**Tests** — clone pysdk/mod.rs:113-262. The `#![allow(clippy::unwrap_used, ...)]` scoping comment ports
verbatim (these tests need NO toolchain — pure string emission). The four-marker test asserts
`// ==== gnr8:file client.ts ====` etc; the byte-identical-two-runs test ports unchanged; the
unsafe-frame-name test ports with a `.ts` name. **DROP** the pysdk-specific content assertions
(`def create_book`, `class BookFormat(str, enum.Enum)`, `@dataclass`) and replace with TS shapes
(`async createBook(`, `export type BookFormat =`, `export interface`).

---

### `crates/gnr8-core/src/tssdk/emit.rs` (service, transform)

**Analog:** `crates/gnr8-core/src/pysdk/emit.rs` (the largest/most important analog, 1732 lines)

**Imports + the `sink` fmt-error fold** (pysdk/emit.rs:26-39) — port verbatim, retitle the message:
```rust
use std::fmt::Write as _;
use crate::graph::{ApiGraph, Field, Operation, Param, Prim, Type};
use crate::CoreError;

fn sink(err: std::fmt::Error) -> CoreError {
    CoreError::SdkGen { message: format!("failed to format TypeScript source: {err}") }
}
```

**`ts_type` — the heart, exhaustive `Type` match, NO `_ =>`** (twin of pysdk/emit.rs:149-203 `py_type`).
RESEARCH gives the verified skeleton — port it with the full IR Type-variant→TS mapping below:
```rust
pub(crate) fn ts_type(schema: &Type, nullable: bool, graph: &ApiGraph) -> Result<String, CoreError> {
    let base = match schema {
        Type::Primitive(p) => ts_primitive(p).to_string(),      // string/number/boolean/string(bytes)
        Type::WellKnown(_) => "string".to_string(),             // date-time/uuid/email/uri → string (A7)
        Type::Array(items) => format!("{}[]", ts_type(items, false, graph)?),
        Type::Map { .. } => "Record<string, unknown>".to_string(), // (or Record<string, ts_type(value)>)
        Type::Any {} => "unknown".to_string(),                  // CONTEXT prefers unknown over any
        Type::Named(ref_id) => {                                 // dangling → SdkGen error (verbatim)
            let target = graph.schemas.iter().find(|s| &s.id == ref_id)
                .ok_or_else(|| CoreError::SdkGen {
                    message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
                })?;
            target.name.clone()
        }
        Type::Enum(members) =>                                   // inline → string-literal union
            members.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(" | "),
        Type::Union(variants) => {                              // native A | B (Go rejected this)
            let mut parts = Vec::with_capacity(variants.len());
            for v in variants { parts.push(ts_type(v, false, graph)?); }
            parts.join(" | ")
        }
        Type::Object(_) => return Err(CoreError::SdkGen {     // parity with pysdk: explicit error arm
            message: "inline object type is unsupported by the TypeScript SDK target \
                      (expected a named $ref)".to_string(),
        }),
    };
    Ok(if nullable { format!("{base} | null") } else { base })  // nullable axis: append | null
}
```

**Full IR `Type`-variant → TS mapping** (cross-checked one-to-one against `py_type`):

| IR `Type` variant | pysdk emits | tssdk emits | Notes |
|-------------------|-------------|-------------|-------|
| `Primitive(String)` | `str` | `string` | |
| `Primitive(Bool)` | `bool` | `boolean` | |
| `Primitive(Int{..})` | `int` | `number` | one numeric type; width irrelevant |
| `Primitive(Float{..})` | `float` | `number` | |
| `Primitive(Bytes)` | `bytes` | `string` | base64 on the wire |
| `WellKnown(_)` | `str` | `string` | date-time as RFC-3339 string (no `Date` import) |
| `Array(inner)` | `List[..]` | `${ts_type(inner)}[]` | |
| `Map{k,v}` | `Dict[str, Any]` | `Record<string, unknown>` | stricter `Record<string, ts_type(v)>` also OK (Open Q1) |
| `Named(ref_id)` | schema `name` | schema `name` | dangling → `SdkGen` error |
| `Enum(members)` INLINE | `Literal["a","b"]` | `"a" \| "b"` | members in graph order |
| `Union(variants)` | `Union[A,B]` | `A \| B` | recurse, join ` \| ` |
| `Object(fields)` INLINE | typed `SdkGen` ERROR | typed `SdkGen` ERROR | keep explicit arm (rule 3) |
| `Any {}` | `Any` | `unknown` | |

**`ts_primitive` helper** (twin of pysdk/emit.rs:207-215 `py_primitive`):
```rust
fn ts_primitive(prim: &Prim) -> &'static str {
    match prim {
        Prim::String => "string",
        Prim::Bool => "boolean",
        Prim::Int { .. } => "number",
        Prim::Float { .. } => "number",
        Prim::Bytes => "string",
    }
}
```

**The optional/nullable two-axis field declaration** (the cleaner-in-TS version of pysdk's
`emit_dataclass`, pysdk/emit.rs:384-462). `nullable` lives in `ts_type` (`| null`); `optional` lives at
the field site as `?:`. **DO NOT port** pysdk's required-first partition (TS `?:` is order-free):

| field flags | TS field declaration |
|-------------|----------------------|
| required, non-nullable | `name: T` |
| required, nullable | `name: T \| null` |
| optional, non-nullable | `name?: T` |
| optional, nullable | `name?: T \| null` |

**`emit_models` — per-schema body dispatch** (twin of pysdk/emit.rs:229-284). Port the schema-name
collision check verbatim (WR-05 — two ids sharing a name is a typed `SdkGen` error). The per-body match:
- `Type::Enum(members)` body → `export type X = "a" | "b";`
- `Type::Object(fields)` body → `export interface X { ... }` (one `?:`/`: T`/`| null` line per field —
  NO `from_dict`, interfaces are zero-runtime; `await res.json() as X` suffices, Assumption A1)
- any other named body (`Union`/`Array`/scalar alias) → `export type X = <ts_type(body)>;`
  **This is where TS beats pysdk:** pysdk needed a PEP-484 string forward-ref hack (emit.rs:264-280)
  because eager module-level aliases `NameError` on forward names. **TS type aliases are order-free — DO
  NOT port the forward-ref hack.**

**`emit_errors`** — replace the Python `ApiError(Exception)` body (pysdk/emit.rs:468-498) with the
verified TS `ApiError extends Error`:
```typescript
export class ApiError extends Error {
  constructor(public readonly status: number, public readonly body: unknown) {
    super(`HTTP ${status}`);
    this.name = "ApiError";
  }
  isNotFound(): boolean { return this.status === 404; }
}
```

**`emit_client`** — replace the urllib `Client` body (pysdk/emit.rs:508-571) with the verified fetch
`Client` carrying the injectable-transport seam (RESEARCH Pattern 3, empirically typechecked):
```typescript
export interface ClientOptions { baseUrl: string; fetch?: typeof fetch; }
export class Client {
  private baseUrl: string;
  private fetchFn: typeof fetch;
  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.fetchFn = opts.fetch ?? fetch;   // `typeof fetch` needs --lib dom
  }
}
```
client.ts header: `import { ApiError } from "./errors";` + a models import (e.g. `import * as models`
or named) — the analog of pysdk's `from .errors import ApiError` / `from .models import *`.

**Pure helpers — PORT VERBATIM (trivially portable, single-source-of-truth, rule 3):**
- `join_path(base_path, path)` — pysdk/emit.rs:575-583. Same `ir.base_path` source as OpenAPI lowering.
- `success_of(op, graph)` — pysdk/emit.rs:594-620. Lowest-2xx status + body model; dangling → `SdkGen`.
- `body_model_of(op, graph)` — pysdk/emit.rs:628-643. Request-body model; dangling → `SdkGen`.
- `path_tokens(path)` — pysdk/emit.rs:647-660. `{token}` extraction in first-seen order.
- `split_words(name)` — pysdk/emit.rs:59-81. The shared tokenizer behind the casing helper.

**Casing helper — `snake` → `camel`** (twin of pysdk/emit.rs:84-90). TS method names are camelCase
(`createBook`), not snake (`create_book`):
```rust
pub(crate) fn camel(name: &str) -> String {
    let words = split_words(name);              // reuse the verbatim tokenizer
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i == 0 { out.push_str(&w.to_ascii_lowercase()); }
        else { /* capitalize first char, lowercase rest */ }
    }
    out
}
```
> pysdk's `safe_ident`/`PY_KEYWORDS`/`screaming_snake`/`enum_member_ident` exist to make Python member
> identifiers safe. TS enums are string-literal unions (no member identifiers at all), so the
> SCREAMING_SNAKE + keyword-suffix machinery mostly disappears. A TS-reserved-word guard for METHOD
> names may still be wanted (planner's call) — but enum-member sanitization does NOT port.

**`emit_operations` / `emit_operation`** (twin of pysdk/emit.rs:677-901). Port the structure: filter
path/query params, assert path-token-vs-param **set equality** (pysdk/emit.rs:781-794 — keep this typed
`SdkGen` guard, WR-03 analog), resolve `body_model_of`/`success_of`. The emitted method body uses the
verified TS shape:
```typescript
async createBook(body: BookDto): Promise<CreatedMessage> {
  const path = `/books`;                                    // join_path(base_path, op.path)
  const res = await this.fetchFn(`${this.baseUrl}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status !== 201) {                                 // success_of(op) real status
    throw new ApiError(res.status, await res.json().catch(() => null));
  }
  return (await res.json()) as CreatedMessage;             // bodyless op → Promise<void>/unknown
}
```
- **Path params:** `encodeURIComponent(String(x))` (V5 path-injection mitigation; the
  `urllib.parse.quote(safe='')` analog at pysdk/emit.rs:852). **DO NOT port** the f-string-backslash
  workaround — TS template literals have no such restriction.
- **Query params:** build a `URLSearchParams`; required always appended; optional guarded by
  `if (x !== undefined)` (the WR-01 analog at pysdk/emit.rs:861-876). Append `?${qs}` when non-empty.
- **Bodyless / no-2xx-body ops:** `Promise<void>` or `Promise<unknown>` (pysdk returns `Any`).

**`emit_index`** (twin of pysdk/emit.rs:906-930 `emit_init`). Re-export `Client`, `ApiError`, and every
named schema in graph order (deterministic). Either `export * from "./client";` etc. or — to mirror
pysdk's `__all__` graph-sorted named surface (Assumption A3, preferred) — named re-exports.

**Tests** — clone pysdk/emit.rs:932-end. Port the type_mapping tests (assert `string`/`number`/`boolean`,
`"a" | "b"`, `A | B`, `string[]`, `Record<string, unknown>`, `unknown`, the `| null` nullable axis,
dangling-ref error, inline-object error). Port the models/client/errors/operations tests adapted to TS
shapes. **DROP** these pysdk tests (no TS analog, RESEARCH Pitfall 6):
`dataclass_emits_required_fields_before_optional_fields`, the `from __future__` header tests, the
forward-ref alias tests, the f-string/`safe=''` tests.

---

### `crates/gnr8-core/src/tssdk/bundle.rs` (utility, transform)

**Analog:** `crates/gnr8-core/src/pysdk/bundle.rs` — **VERBATIM CLONE.**

The frame marker is already a `//` comment, which is valid TypeScript. Copy the entire file unchanged:
`SdkFile`/`SdkBundle` structs, `MARKER_PREFIX = "// ==== gnr8:file "`, `MARKER_SUFFIX = " ===="`,
`marker_for`, the `Display` impl, `parse`, `parse_marker`, and all tests. The only adjustments are
cosmetic doc-comment wording ("Python file" → "TypeScript file") and the test fixture file
names/contents (`.py` → `.ts`). The framing logic is byte-identical-proven and ports with zero risk.

Key invariant to preserve (pysdk/bundle.rs:54-66):
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

### `crates/gnr8-core/src/sdk/builtins.rs` (provider, request-response) — MODIFY

**Analog:** `PySdk` struct + impl in the SAME file (builtins.rs:559-643).

Add a `TsSdk` struct + `impl Target` cloned from `PySdk`. Reuse `sdk_package` and `ir.base_path`
**verbatim** — no second derivation (rule 3). The only change is `crate::pysdk::` → `crate::tssdk::`
and the error-message labels (`PySdk` → `TsSdk`):
```rust
#[derive(Debug, Clone)]
pub struct TsSdk { module: String, dir: String }

impl TsSdk {
    #[must_use] pub fn new() -> Self { Self { module: String::new(), dir: String::new() } }
    #[must_use] pub fn module(mut self, module: impl Into<String>) -> Self { self.module = module.into(); self }
    #[must_use] pub fn to(mut self, dir: impl Into<String>) -> Self { self.dir = dir.into(); self }
}
impl Default for TsSdk { fn default() -> Self { Self::new() } }

impl Target for TsSdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config { message: "TsSdk target has no module — call .module(\"example.com/acme/sdk\")".to_string() });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config { message: "TsSdk target has no output dir — call .to(\"sdk\")".to_string() });
        }
        let package = sdk_package(&self.module)?;                 // SAME single source of truth
        let bundle = crate::tssdk::generate(ir, &package, &ir.base_path)?;  // ir.base_path verbatim
        let dir = self.dir.trim_end_matches('/');
        for (name, contents) in crate::tssdk::split_bundle(&bundle) {
            if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(CoreError::SdkGen { message: format!("refusing to emit SDK file with unsafe name {name:?}") });
            }
            out.write(format!("{dir}/{name}"), contents);
        }
        Ok(())
    }
    fn output_anchors(&self) -> Vec<String> {                     // loop-safety anchor (T-03-02-02)
        if self.dir.is_empty() { Vec::new() } else { vec![self.dir.trim_end_matches('/').to_string()] }
    }
}
```

**Target tests** — clone the `PySdk` tests (builtins.rs:801-846 +
`python_sources_error_when_unconfigured` at :848): `tssdk_target_writes_under_the_output_dir_and_is_deterministic`
(writes-under-dir + output_anchors + two-run byte-identical) and an unconfigured-error test. Add `TsSdk`
to the test-module `use` import list (builtins.rs:733-736).

---

### `crates/gnr8-core/src/sdk/mod.rs` (config) — MODIFY

**Analog:** the `PySdk` entry in the `prelude` re-export (sdk/mod.rs:336-343).

Add `TsSdk` to the alphabetized `builtins::{...}` re-export list:
```rust
pub mod prelude {
    pub use super::builtins::{
        ApplySecurity, FastApi, Flask, GoGin, GoSdk, Header, NestJs, OpenApi31, PySdk, /* + */ TsSdk,
        RenameOperation, RenameType, SetBasePath, SetTitle,
    };
    // ...
}
```

---

### `crates/gnr8-core/src/lib.rs` (config) — MODIFY

**Analog:** `pub mod pysdk;` (lib.rs:15).

Add `pub mod tssdk;` next to it (alphabetical: after `pub mod sdk;`? — pysdk sits before runner/sdk, so
place `pub mod tssdk;` after `pub mod sdk;` line 17 to keep alpha order):
```rust
pub mod pysdk;     // existing, line 15
// ...
pub mod sdk;       // existing, line 17
pub mod tssdk;     // NEW
pub mod workspace; // existing, line 18
```

---

### `crates/gnr8-core/tests/tssdk_compile.rs` (test, request-response) — NEW

**Analog:** `crates/gnr8-core/tests/pysdk_compile.rs` (the full hermetic generate→write→typecheck pattern).

Clone the harness scaffolding **verbatim** and swap the Python toolchain calls for `tsc`:

**Port unchanged (the security/hermeticity discipline):**
- `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` (pysdk_compile.rs:29).
- `unique_temp_dir(label)` (pysdk_compile.rs:59-69) — PID + nanosecond unique stdlib temp dir, no
  `tempfile` crate (T-03-03-SC). Relabel `gnr8-pysdk-compile-` → `gnr8-tssdk-compile-`.
- `materialize_sdk()` (pysdk_compile.rs:102-115) — `build_graph(FIXTURE_DIR)` →
  `tssdk::generate(&graph, PACKAGE, &graph.base_path)` → `tssdk::write_to_dir` into a unique temp dir.
- The supply-chain banned-import grep (pysdk_compile.rs:143-151) — grep every written `.ts` for
  `axios`, `node-fetch`, `@types`, `from "http"` and assert ABSENT (the TSSDK-02 supply-chain gate).
- The discrete-args `Command` + `current_dir` discipline (pysdk_compile.rs:75-95) — NEVER a shell string
  (V13). Map a spawn failure to `CoreError::TypeScriptToolchainMissing { source }` (error.rs:59 —
  already exists; the TS analog of pysdk's `PythonToolchainMissing` at error.rs:45). Map a non-zero exit
  to the captured-stderr `CoreError::GoBuild { code, stderr }` carrier (pysdk_compile.rs:89-92 reuses it
  as the generic exit-code+stderr carrier — reuse it the same way, no new error variant).
- The toolchain-skip guard `python_available()` (pysdk_compile.rs:47-54) → a `node_available()` /
  `tsc_available()` early-return skip (RESEARCH Pitfall 5). Skip gracefully if node/tsc absent.
- The invalid-input → captured-error-not-panic test (pysdk_compile.rs:191-221) → write a broken `.ts`,
  run `tsc`, assert a captured `CoreError::GoBuild` with non-zero code + non-empty stderr.

**Swap in (the one genuinely new mechanic — the verified invocation):**
```rust
// Replace `python3 -m py_compile` + `python3 -c import` + the http.server driver with ONE tsc run.
// VERIFIED exit 0 on a fetch-based SDK using ONLY the vendored compiler; `,dom` is load-bearing
// (omit it → "error TS2304: Cannot find name 'fetch'", exit 2).
const TSC: &str = concat!(env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc");
// Command::new("node").args([
//   TSC, "--noEmit", "--strict",
//   "--target", "es2022", "--module", "esnext", "--moduleResolution", "bundler",
//   "--lib", "es2022,dom",
//   <each generated .ts path>,
// ]).current_dir(<temp dir>)   // no node_modules nearby → no @types leak (Pitfall 3)
```

**Constants** (mirror pysdk_compile.rs:34-44):
- `FIXTURE_DIR` → `concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/nestjs-bookstore")`
  (completes the NestJS→TS path; node is present — RESEARCH A2). Planner may substitute the
  `fastapi-bookstore` fixture; both yield equivalent-rich language-neutral IR.
- `PACKAGE` → e.g. `"bookstore"`.
- `SDK_FILES` → `["client.ts", "errors.ts", "index.ts", "models.ts"]` (whatever fixed order `mod.rs`
  emits).

**DO NOT port** the `ROUND_TRIP_DRIVER` http.server smoke test (pysdk_compile.rs:238-336). TSSDK-02
asks ONLY for `tsc --noEmit` (typecheck), not a runtime round-trip (RESEARCH Open Q3). The injectable
`fetch` seam is still emitted (for swappability), but no round-trip driver is required this phase.

---

## Shared Patterns

### Determinism by construction (PYSDK-03 → TSSDK-03)
**Source:** `crates/gnr8-core/src/pysdk/emit.rs` module docs (lines 21-24) + `bundle.rs` `Display`.
**Apply to:** every `tssdk` emitter + `bundle.rs` + the `TsSdk` Target.
Iterate `graph.schemas` / `graph.operations` in the IR's already-sorted order; emit a FIXED preamble; no
`HashMap`. **TS is even simpler than pysdk:** there is NO computed import header to compute at all
(interfaces and `fetch` need no in-file imports). Assert two-run byte-identical at `generate`,
`emit_models`, `bundle`, and Target levels (clone pysdk's determinism tests).

### Typed errors, no production panic (RUST-04)
**Source:** `crates/gnr8-core/src/pysdk/emit.rs:35` (`sink`) + `crate::CoreError::SdkGen` everywhere.
**Apply to:** all `tssdk` production code.
Fold every `fmt::Write` error via `sink`; every un-representable IR fact (dangling `$ref`, inline
object, path-token/param mismatch, name collision) returns `CoreError::SdkGen`. No
`unwrap`/`expect`/`panic` outside `#[cfg(test)]`. The `#![allow(clippy::unwrap_used,...)]` is scoped to
test modules only (every analog file does this).

### Single source of truth (rules 3 & 4)
**Source:** `sdk_package` (builtins.rs:708-725) + `ir.base_path` (builtins.rs:616) + `join_path`
(pysdk/emit.rs:575).
**Apply to:** the `TsSdk` Target + `tssdk::emit`.
Package name = `sdk_package(self.module)` (reused verbatim — NO TS-specific sanitizer). Base path =
`ir.base_path` (the same value `SetBasePath` sets and OpenAPI lowering reads — never re-derived).

### Path-safety on write (T-03-01-01 / T-03-02-01)
**Source:** `pysdk/mod.rs:100` (`write_to_dir` guard) + `builtins.rs:622` (Target write guard).
**Apply to:** `tssdk::mod::write_to_dir` + `TsSdk::generate`.
Reject any frame name containing `/`, `\`, `..`, or empty before joining onto `dir`. Ported verbatim.

### Subprocess hygiene (V12/V13)
**Source:** `tests/pysdk_compile.rs:75-95` (`run_python`).
**Apply to:** the `tssdk_compile.rs` `tsc` invocation.
Discrete `Command::new("node").args([...])` + `current_dir` — NEVER `sh -c`. Unique temp dir with no
user-supplied component. Banned-import grep over every emitted `.ts`.

---

## Pitfalls That DON'T Port (RESEARCH Pitfall 6 — do not cargo-cult)

The pysdk emitter carries four Python-specific workarounds with **NO TS analog**. Porting them is a
defect:

| pysdk workaround | Where (pysdk/emit.rs) | Why it doesn't port to TS |
|------------------|------------------------|----------------------------|
| Dataclass required-first field ordering (`partition`) | :405 `emit_dataclass` | TS `?:` is order-free; emit fields in graph order |
| `from __future__ import annotations` / lazy-annotation header | :47 `MODELS_HEADER` | TS has no import header; interfaces/fetch need no imports |
| PEP-484 string forward-ref aliases | :264-280 `emit_models` alias arm | TS type aliases are order-independent — emit `export type X = A \| B;` directly |
| f-string backslash ban (`safe=''` single-quote trick) | :849-852 | TS template literals + `encodeURIComponent` have no such restriction |

Also non-porting: `from_dict` / `decode_expr` recursion (pysdk/emit.rs:352-374, 436-460) — TS
interfaces are zero-runtime; `await res.json() as Model` needs no decoder (Assumption A1). And the
`PY_KEYWORDS`/`safe_ident`/`screaming_snake`/`enum_member_ident` machinery (pysdk/emit.rs:106-335) is
largely moot: string-literal-union enums have no member identifiers to sanitize.

---

## No Analog Found

None. Every artifact in this phase has an exact `pysdk` twin. The single genuinely-new mechanic — the
`tsc --noEmit --strict --lib es2022,dom` typecheck — has its structural analog in
`pysdk_compile.rs`'s `python3 -m py_compile` gate (same skip-if-absent / discrete-args / unique-temp-dir
/ banned-import-grep harness); only the subprocess command changes.

---

## Metadata

**Analog search scope:** `crates/gnr8-core/src/pysdk/{mod,emit,bundle}.rs`,
`crates/gnr8-core/src/sdk/{mod.rs,builtins.rs}`, `crates/gnr8-core/src/lib.rs`,
`crates/gnr8-core/src/error.rs`, `crates/gnr8-core/tests/pysdk_compile.rs`.
**Files scanned:** 7 (all exact-match analogs read in full or targeted).
**Pattern extraction date:** 2026-06-25
