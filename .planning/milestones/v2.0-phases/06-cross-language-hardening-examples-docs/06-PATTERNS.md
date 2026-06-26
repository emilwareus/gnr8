# Phase 6: Cross-Language Hardening + Examples + Docs - Pattern Map

**Mapped:** 2026-06-26
**Files analyzed:** 13 new/modified targets
**Analogs found:** 12 / 13 (1 with no in-repo analog — the Makefile cross-language gate)

> This is an INTEGRATION/HARDENING phase. Almost every new file COPIES a verbatim in-repo analog.
> The two source-code edits (doctor probe, watch trigger) are surgical generalizations keyed off a
> single existing decision (`analyze::detect_language`). Per CLAUDE.md: NO new OSS crate in
> `gnr8-core`; the `typescript` carve-out is RECORDED, not introduced. Map analogs only — do not
> propose new deps.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `examples/fastapi-bookstore/.gnr8/src/main.rs` (NEW) | config (code-as-config Pipeline) | transform / batch | `examples/bookstore/.gnr8/src/main.rs` | exact (only Source+SDK target differ) |
| `examples/nestjs-bookstore/.gnr8/src/main.rs` (NEW) | config | transform / batch | `examples/bookstore/.gnr8/src/main.rs` | exact |
| `examples/fastapi-bookstore/.gnr8/Cargo.toml` (NEW) | config | — | `examples/bookstore/.gnr8/Cargo.toml` | exact (name + path depth) |
| `examples/nestjs-bookstore/.gnr8/Cargo.toml` (NEW) | config | — | `examples/bookstore/.gnr8/Cargo.toml` | exact |
| `examples/{fastapi,nestjs}-bookstore/.gnr8/.gitignore` (NEW) | config | — | `examples/bookstore/.gnr8/.gitignore` | exact (verbatim) |
| `examples/{fastapi,nestjs}-bookstore/README.md` (NEW) | docs | — | `examples/bookstore/README.md` | exact (mirror structure) |
| `examples/fastapi-bookstore/app/*` (NEW, COPIED) | source app (static) | — | `fixtures/fastapi-bookstore/app/*` | exact (copy) |
| `examples/nestjs-bookstore/src/*` (NEW, COPIED) | source app (static) | — | `fixtures/nestjs-bookstore/src/*` | exact (copy) |
| `examples/{fastapi,nestjs}-bookstore/generated/*` (NEW, COMMITTED) | generated artifact | — | `examples/bookstore/generated/*` | role-match (REAL `gnr8 generate` bytes; never hand-write) |
| `crates/gnr8-core/src/analyze/mod.rs` (MOD) | analyzer / classifier | request-response | self (`detect_language`/`Lang` already exist `pub(crate)`) | self (visibility-only change) |
| `crates/gnr8-core/src/lib.rs` (MOD) | re-export surface | — | self (`pub mod analyze;` line 8) | self |
| `crates/gnr8/src/main.rs` (MOD `run_doctor`/`probe_go`) | CLI command (impure shell) | request-response / probe | self (`run_doctor` lines 209-263) | self (generalize Go→source lang) |
| `crates/gnr8/src/doctor.rs` (MOD `LifecycleHealth`) | report type (pure decision) | transform | self (`LifecycleHealth` lines 40-48; pinned test line 526) | self (rename `go_toolchain`→source field) |
| `crates/gnr8/src/watch.rs` (MOD `is_trigger_path`) | CLI watcher (pure decision) | event-driven | self (`is_trigger_path` line 95-106, ext=="go" line 105) | self (generalize trigger ext) |
| `docs/USAGE.md` (MOD) | docs | — | self (Go/Gin-only reference) | self (ADD per-language envelope) |
| `CLAUDE.md` (MOD) | governance doc | — | self (`## 2.` line 23; `## Known debt` line 66) | self (add bounded carve-out subsection) |
| `Makefile` (MOD, add `examples-check`) | build gate | batch | self (`check:` line 82; `gates:` line 57; `fixture-build:` line 64) | role-match (no exact regen-diff analog exists) |

## Pattern Assignments

### `examples/fastapi-bookstore/.gnr8/src/main.rs` (config, transform) and the NestJS twin

**Analog:** `examples/bookstore/.gnr8/src/main.rs` (verbatim v1 pattern, lines 23-36).

The ONLY differences vs. the Go bookstore are (a) the `Source` built-in, (b) the SDK `Target`, and
(c) NO `ApplySecurity` (the fixtures have no auth scheme — verify against the committed
`fixtures/*` snapshots before adding one). The doc-comment header (lines 1-21) explains "this file
IS the config"; mirror that header style adapted per language.

**Core pattern to copy** (`examples/bookstore/.gnr8/src/main.rs:23-36`):
```rust
use gnr8_core::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8_core::runner::run(
        Pipeline::new()
            .source(GoGin::new().inputs(["."]))
            .transform(SetBasePath::new("/books"))
            .transform(SetTitle::new("Bookstore API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(GoSdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

**FastAPI adaptation** — swap `GoGin`→`FastApi`, `inputs(["."])`→`inputs(["app"])` (scope to the
single-language subdir, Pitfall 2), `GoSdk`→`PySdk`:
```rust
use gnr8_core::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8_core::runner::run(
        Pipeline::new()
            .source(FastApi::new().inputs(["app"]))            // resolves vs project_root (example dir)
            .transform(SetBasePath::new("/books"))             // confirm string vs fixture snapshot
            .transform(SetTitle::new("Bookstore API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

**NestJS adaptation** — `NestJs::new().inputs(["src"])` + `TsSdk`:
```rust
            .source(NestJs::new().inputs(["src"]))
            ...
            .target(TsSdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
```

**Verified built-in signatures** (`crates/gnr8-core/src/sdk/builtins.rs`):
- `FastApi::new()` (l.100) → `.inputs<I,S>(...)` (l.106); `NestJs::new()` (l.218) → `.inputs` (l.224).
- `PySdk::new()` (l.568) → `.module(impl Into<String>)` (l.579) → `.to(dir)` (l.586).
- `TsSdk::new()` (l.663) → `.module` (l.674) → `.to` (l.681).
- `OpenApi31::new()` (l.415) → `.to(path)` (l.423).

**Prelude import** (`crates/gnr8-core/src/sdk/mod.rs:337-342`) — `FastApi`, `NestJs`, `PySdk`,
`TsSdk`, `OpenApi31`, `SetBasePath`, `SetTitle`, `Header`, `Pipeline` are ALL re-exported by
`gnr8_core::sdk::prelude::*`, so the single glob import suffices (same as the Go example).

---

### `examples/{fastapi,nestjs}-bookstore/.gnr8/Cargo.toml` (config)

**Analog:** `examples/bookstore/.gnr8/Cargo.toml` (verbatim — lines 1-10):
```toml
[package]
name = "bookstore-gnr8-gen"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
gnr8-core = { path = "../../../crates/gnr8-core" }

[workspace]
```
**Adapt:** rename `name` → `fastapi-bookstore-gnr8-gen` / `nestjs-bookstore-gnr8-gen`. The
`../../../crates/gnr8-core` path depth is IDENTICAL (`examples/<name>/.gnr8/` → repo root is three
`..`). The empty `[workspace]` table is REQUIRED (detaches the example crate from the workspace) —
copy it. Commit `Cargo.lock` (mirror bookstore — `examples/bookstore/.gnr8/Cargo.lock` exists).

### `examples/{fastapi,nestjs}-bookstore/.gnr8/.gitignore` (config)

**Analog:** `examples/bookstore/.gnr8/.gitignore` (verbatim, copy as-is):
```
# gnr8 lifecycle state — regenerated, do not commit.
/cache/
# Rust build output of the .gnr8 generation crate — not committed.
/target/
```

### `examples/{fastapi,nestjs}-bookstore/README.md` (docs)

**Analog:** `examples/bookstore/README.md` (mirror the sectioning: title → tree → "The input" →
"The config is code" → "The command" → "The output" → "What this showcases"). Adapt the language
(Python/`@app` decorators; TS/`@Controller` decorators), drop the security section (no `ApplySecurity`
in these examples unless the fixture warrants it), and keep the `gnr8 generate` / committed-output
narrative. NestJS README must NOTE that NO `node_modules` is needed (static extraction).

### `examples/fastapi-bookstore/app/*` + `examples/nestjs-bookstore/src/*` (source app, COPIED)

**Analog (= source of truth to copy from):**
- `fixtures/fastapi-bookstore/app/{__init__.py, main.py, models.py}` → copy into `examples/fastapi-bookstore/app/`.
- `fixtures/nestjs-bookstore/src/{books.controller.ts, books.dto.ts}` → copy into `examples/nestjs-bookstore/src/`.

Do NOT point `.gnr8/` at `fixtures/` (RESEARCH A2: keep examples self-contained, mirror
`examples/bookstore` where source lives beside `.gnr8/`). Keep `fixtures/*` as the snapshot-test
source of truth; the example is a copy. Do NOT copy `package.json`/`requirements.txt` unless the
README references them (the examples are never installed/run).

### `examples/{fastapi,nestjs}-bookstore/generated/*` (generated artifact, COMMITTED)

**Analog (layout):** `examples/bookstore/generated/` →
`openapi.yaml` + `sdk/{client,errors,models,operations}.go`. The Python/TS SDK file names differ
(PySdk → `*.py`; TsSdk → `*.ts`) — let `gnr8 generate` decide them; commit the REAL bytes.
**CRITICAL (Pitfall 4):** never hand-write these — run `gnr8 generate`, then sanity-check the
OpenAPI structurally matches the committed `fixtures/*` snapshots (same routes/schemas/refs).

---

### `crates/gnr8-core/src/analyze/mod.rs` + `lib.rs` (analyzer, visibility change)

**Analog:** self — the classifier already exists; this is a `pub(crate)` → `pub` exposure only
(Pitfall 1). Keep it the SAME single decision (rule 3) — do NOT duplicate the scan.

**Current visibility** (`crates/gnr8-core/src/analyze/mod.rs`):
```rust
pub(crate) enum Lang {            // line 20
    Go,
    Python,
    TypeScript,
}
pub(crate) fn detect_language(target_dir: &str) -> Result<Lang, crate::CoreError> {   // line 50
```
`build_graph` (l.149) is ALREADY `pub`. `lib.rs:8` is `pub mod analyze;` (the module is public; only
the two items are crate-private).

**Recommended (RESEARCH Pitfall 1 + Alternatives):** add a minimal NEW `pub` function rather than
widening `Lang`'s internals — e.g.
`pub fn source_toolchain(dir: &str) -> Result<Toolchain, CoreError>` returning a small `pub enum`
the CLI maps to BOTH the doctor probe binary AND the watch trigger extension(s). Delegate to the
existing `detect_language` (one source of truth). Map the existing typed errors as-is:
`CoreError::{Go,Python,TypeScript}ToolchainMissing` (`crates/gnr8-core/src/error.rs:32/45/59`,
re-exported as `gnr8_core::CoreError` via `lib.rs:6`).

**Anti-pattern (do NOT):** re-derive the toolchain from the `Source` type name, or copy the
marker-scan into the CLI — both are a second source of truth (rule 3 violation).

---

### `crates/gnr8/src/main.rs` — `run_doctor` / `probe_go` (CLI shell, generalize)

**Analog:** self (lines 209-263). Today hardcoded Go.

**Current Go-only probe** (`crates/gnr8/src/main.rs:209-214`):
```rust
fn probe_go() -> bool {
    std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok()
}
```
**Current collection** (`main.rs:226-249`):
```rust
let initialized = gnr8_core::workspace::manifest_path(&root).is_file();
let go_present = probe_go();
// ... child::run_child(&root, "__emit").ok() ...
let report = doctor::DoctorReport::assemble(
    initialized, go_present, pipeline_ran, diagnostics, drift.as_ref());
```

**Generalize (single deterministic decision, rule 3 — NO try-go-then-python fallback):** detect the
source language once via the new `pub` `gnr8_core::analyze` API over the source dir (RESEARCH Open Q2:
detect over project root EXCLUDING `.gnr8/`, since the example source under `app/`/`src/` is
single-language; validate it does not trip the ambiguity guard), then probe THAT toolchain:
- `Go` → `go version`
- `Python` → `python3 --version`
- `TypeScript` → `node --version` (the vendored `tsc` ships with `tsextract`; node is the gate)

Preserve the discrete-args / no-`sh -c` shape of `probe_go` (Security: arg-injection mitigation).

### `crates/gnr8/src/doctor.rs` — `LifecycleHealth` (pure report type)

**Analog:** self. **Pinned contract — Pitfall 3.**

**Current field** (`crates/gnr8/src/doctor.rs:40-48`):
```rust
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, serde::Serialize)]
pub(crate) struct LifecycleHealth {
    pub(crate) initialized: bool,
    pub(crate) go_toolchain: bool,   // <- rename to source_toolchain (+ add `language`?)
    pub(crate) pipeline_runs: bool,
}
```
**The `--json` field set is a PUBLISHED contract pinned by a test** (`doctor.rs:526`):
```rust
let lexpected: HashSet<&str> = ["initialized", "go_toolchain", "pipeline_runs"]
    .into_iter().collect();
assert_eq!(lkeys, lexpected, "lifecycle --json field set drifted");
```
Renaming `go_toolchain` → `source_toolchain` (RESEARCH A6, the more-honest multi-language choice)
REQUIRES updating this assertion AND the human renderer (`doctor.rs:270-277`, the "Go toolchain:"
label) AND `assemble`'s param/doc (`go_present` l.157-167) AND the `actionable_problem_count` /
`has_actionable_problem` references (l.231, l.249) AND USAGE.md's doctor row — all in the same task.
Consider adding a `language` field so the report states WHICH toolchain was probed.

### `crates/gnr8/src/watch.rs` — `is_trigger_path` (pure event-driven decision)

**Analog:** self. Today the source trigger is hardcoded to `ext == "go"`.

**Current trigger** (`crates/gnr8/src/watch.rs:95-106`):
```rust
fn is_trigger_path(path: &Path, output_set: &HashSet<PathBuf>, gnr8_src: &Path) -> bool {
    // A pipeline-source edit (`.gnr8/src/**.rs`) always triggers — recompile + re-run.
    if path.starts_with(gnr8_src) && path.extension().is_some_and(|ext| ext == "rs") {
        return true;
    }
    if is_under_any_output(path, output_set) {
        return false;
    }
    // Only Go source edits outside `.gnr8/` drive an API-change regeneration.
    path.extension().is_some_and(|ext| ext == "go")   // <- generalize to the source language's ext
}
```
**Generalize:** the last line's single `"go"` becomes the detected source language's extension
(`.go` | `.py` | `.ts`). The `.gnr8/src/**.rs` pipeline-edit trigger (l.97) and the
output-set loop-safety (l.101, `build_output_set` l.185, `canonicalize_or_keep` l.159) are ALREADY
language-agnostic — NO change. Thread the extension in (the signature already takes `gnr8_src`; add
the source-language ext the same way, derived from the one `detect_language` decision). Extend the
pure unit tests (`watch.rs` test module l.356+) with `*.py`/`*.ts` trigger cases mirroring
`go_source_triggers` (l.408) and `non_go_ignored` (l.439).

---

### `docs/USAGE.md` (docs, EXTEND — Pitfall 7)

**Analog:** self (the Go/Gin-only v1.0 reference). UPDATE in place, do NOT duplicate.

Touch points (line numbers from the current file):
- Source built-ins table (l.83) lists only `GoGin` → ADD `FastApi`/`Flask`/`NestJs`.
- Target built-ins table (l.85) lists `OpenApi31`/`GoSdk` → ADD `PySdk`/`TsSdk`.
- `watch` note (l.54): "re-runs on a `*.go` source edit" → "a source-language edit".
- doctor row (l.48) + the Go-toolchain wording → "source toolchain".
- "## Recognized Go/Gin patterns" (l.172) + "## Type mapping (Go → ...)" (l.188) are Go-specific —
  ADD a per-language envelope SECTION beside them (use the RESEARCH "honest envelope" table below).
- Point to the two new examples alongside `examples/taskflow` (l.162) and `examples/bookstore`.

**Honest per-language envelope to add** (verbatim from RESEARCH §Code Examples, derived from the
verified Phase 2/4 SUMMARYs): a table `Frontend | Lang | Status | Recognized | Limits/diagnostics`
covering Gin (Go, full), FastAPI (Python, full), Flask (Python, typed-envelope second-class),
NestJS (TypeScript, class-DTO scope). Plus the line: "Generated SDKs are dependency-free in every
language: GoSdk (net/http), PySdk (urllib + @dataclass), TsSdk (built-in fetch + typed interfaces)."

### `CLAUDE.md` (governance, ADD bounded carve-out — XLANG-05)

**Analog:** self. Anchor points: `## 2. No third-party / OSS dependencies` (line 23) and
`## Known debt` (line 66). Add a clearly-scoped subsection UNDER rule 2 (e.g.
`### Documented exception (the ONE sanctioned carve-out)`) recording the `typescript` carve-out —
do NOT loosen rule 2 generally. Use the bounded wording from RESEARCH §Code Examples (the
bright-line: ONLY OSS dep permitted in any sidecar; `gnr8-core` + generated SDKs stay
dependency-free; NEVER licenses reading `@nestjs/swagger`/`zod`/`class-validator`; FUT-04 may
retire it). Audit the wording against the PROJECT.md decision so the two agree.

**Cross-check (already logged in PROJECT.md — keep consistent):** `.planning/PROJECT.md:119` —
"TypeScript sidecar uses the `typescript` Compiler API ... Documented carve-out to rule 2.
⚠️ Revisit (the single OSS-in-toolchain exception)". CLAUDE.md is the missing record.

### `Makefile` (build gate, ADD `examples-check` — NO exact analog)

**Closest analog:** the standalone-toolchain gate pattern `fixture-build:` (l.64) /
`goextract-build:` (l.70), which `cd` into a dir and run a toolchain command, and are listed as
prerequisites of `check:` (l.82). NO existing target regenerates+diffs the examples (Pitfall 6).

**Current gate wiring:**
```makefile
.PHONY: fmt fmt-check clippy test gates fixture-build goextract-build red check all   # l.21
gates:                                                                                # l.57
	cargo test -p gnr8-core --lib
	cargo test -p gnr8
	cargo test -p gnr8-core --test snapshot_graph ... --test lifecycle
	cargo test -p gnr8-core --test snapshot_nestjs_graph --test snapshot_nestjs_openapi
fixture-build:                                                                        # l.64
	cd fixtures/goalservice && go build ./... && go vet ./...
check: fmt-check clippy test fixture-build goextract-build                            # l.82
```
**Add (mirror `fixture-build`'s cd-and-run shape; RESEARCH §Code Examples):**
```makefile
examples-check:
	cd examples/bookstore         && gnr8 generate && gnr8 check   # Go
	cd examples/fastapi-bookstore && gnr8 generate && gnr8 check   # Python
	cd examples/nestjs-bookstore  && gnr8 generate && gnr8 check   # TypeScript
```
Add `examples-check` to `.PHONY` (l.21) and as a prerequisite of `check:` (l.82). DECISIONS for the
planner: (a) build the `gnr8` binary once (`cargo build --release -p gnr8`) and put it on PATH, vs
`cargo run -p gnr8 --` (RESEARCH Open Q5 — recommend the built binary, matches the v1 `make` ethos);
(b) ensure `go` is on PATH (MEMORY: not on default PATH in this sandbox). `gnr8 check` already exits
1 on drift (`main.rs:148 run_check`) — it IS the regen-and-diff; no bespoke compare script (rule 2 +
Don't-Hand-Roll).

## Shared Patterns

### Single-decision source-language detection (rule 3)
**Source:** `crates/gnr8-core/src/analyze/mod.rs:50` (`detect_language`) + `:20` (`Lang`).
**Apply to:** the doctor probe (`main.rs run_doctor`) AND the watch trigger (`watch.rs is_trigger_path`).
**Rule:** detect ONCE via the new `pub` core API; pass the result to both the probe binary name and
the trigger extension. NEVER two independent detections, NEVER a try-go-then-python fallback, NEVER
re-derive from the `Source` type name.
```rust
pub(crate) fn detect_language(target_dir: &str) -> Result<Lang, crate::CoreError> { /* one scan, count markers */ }
```

### Typed toolchain-missing errors (the "which toolchain" source of truth)
**Source:** `crates/gnr8-core/src/error.rs:32/45/59` →
`CoreError::{Go,Python,TypeScript}ToolchainMissing` (re-exported `gnr8_core::CoreError`, `lib.rs:6`).
**Apply to:** the doctor probe maps the detected `Lang` onto the matching variant; do NOT invent a
new error type.

### Discrete-args subprocess (no shell — security)
**Source:** `crates/gnr8/src/main.rs:209-214` (`probe_go`) — `Command::new(bin).arg(...).output()`.
**Apply to:** the generalized per-language probe. Pass `go`/`python3`/`node` + the version flag as
DISCRETE args, never `sh -c` (arg-injection mitigation, preserved across goextract/pyextract/tsextract).

### `--json` field-set stability tests (published contract)
**Source:** `crates/gnr8/src/doctor.rs:498-530` (`doctor_json_field_set`) and
`crates/gnr8/src/watch.rs:478-502` (`latency_report_json_field_set`).
**Apply to:** any doctor field rename MUST update `doctor_json_field_set` in the same task; new
watch trigger cases follow the `is_trigger_path` pure-test style (no watcher, no I/O).

### `.gnr8/` example crate layout (config is code, rule 4)
**Source:** `examples/bookstore/` — `.gnr8/{Cargo.toml, Cargo.lock, .gitignore, src/main.rs}` +
`generated/` + source beside it; NO data file.
**Apply to:** both new examples verbatim (only the Source/Target/lang differ).

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `Makefile` `examples-check` target | build gate | batch | No existing target regenerates+diffs the examples. Closest is `fixture-build:`/`goextract-build:` (cd-and-run-toolchain shape) — copy that shape, but the cross-language regen-diff is new this phase. `gnr8 check` (existing) supplies the actual diff logic, so no new comparison code is written. |

## Metadata

**Analog search scope:** `examples/`, `fixtures/`, `crates/gnr8/src/{main,doctor,watch}.rs`,
`crates/gnr8-core/src/{analyze/mod,lib,error,sdk/builtins,sdk/mod}.rs`, `docs/USAGE.md`, `Makefile`,
`CLAUDE.md`, `.planning/PROJECT.md`.
**Files scanned:** ~16.
**Pattern extraction date:** 2026-06-26
**Constraint reminder:** NO new OSS crate in `gnr8-core` (serde/serde_json/blake3/thiserror are v1
known-debt, NOT retired this phase — Pitfall 5). Sidecars stay stdlib-only. The `typescript`
carve-out is RECORDED (not introduced) and is the single documented rule-2 exception.
