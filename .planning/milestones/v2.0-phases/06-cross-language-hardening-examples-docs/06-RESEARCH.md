# Phase 6: Cross-Language Hardening + Examples + Docs - Research

**Researched:** 2026-06-26
**Domain:** Integration / hardening of an existing multi-language code-gen pipeline (Rust host + Go/Python/TS sidecars); examples, docs, CLI parity, determinism, invariant recording. NO new extraction/SDK logic.
**Confidence:** HIGH (all findings grounded in the in-repo source; no external deps; this is an integration phase over code that already exists and is green).

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **gnr8-core takes ZERO OSS crates** (rule 2). The ONLY toolchain OSS is the `typescript` carve-out (TS sidecar only, behind the JSON-facts boundary). Generated SDKs stay dependency-free.
  - *(Researcher note — MUST READ: gnr8-core TODAY links `thiserror`, `serde`, `serde_json`, `blake3`. These are the v1.0 **known debt** CLAUDE.md says to retire **later**, NOT in this phase. See Open Question 1 + the Pitfall on this. "Zero OSS in gnr8-core" here means "add no NEW OSS crate this phase" and "the carve-out is the only sanctioned toolchain exception" — it does NOT mean "retire the four existing debt crates this phase." Do not let a literal reading turn this hardening phase into a serde/blake3 rewrite.)*
- **Every sidecar stdlib-only in its language:** Go (stdlib go/* — modulo the v1 known-debt golang.org/x/tools to retire later), Python (`ast`), TS (`typescript` only). Document, don't expand.
- **Deterministic, byte-identical output** across languages and runs (the committed example output is the determinism proof; CI/`make check` must regenerate byte-identically).
- **Config is code:** examples are driven by `.gnr8/` Rust Pipeline crates, NEVER data files (rule 4).
- **Honest limits (XLANG-03):** the Flask typed-envelope gaps and the NestJS class-DTO scope are stated plainly in docs/USAGE.md — no overclaiming.
- **The `typescript` carve-out must be RECORDED** in CLAUDE.md + PROJECT.md as the documented, bounded rule-2 exception (language's own reference compiler, TS sidecar toolchain only, bright-line excludes @nestjs/swagger/zod/class-validator; gnr8-core + generated SDKs stay dependency-free; FUT-04 may retire it). Explicit deliverable (XLANG-05), not a violation.

### Claude's Discretion
- Example project layout, the exact USAGE.md table shape, the toolchain-detection refactor design, and which hardening items (WR-02/WR-04) to fold in vs. backlog — all at Claude's discretion, guided by the v1 example + doctor/check/watch code and the locked invariants.
- Recommended defaults (auto-accepted): add `examples/fastapi-bookstore/` + `examples/nestjs-bookstore/` (or reuse `fixtures/` as the example source); generalize `doctor`/`check`/`watch` toolchain detection by reusing `detect_language` + the `*ToolchainMissing` variants (single deterministic decision, no fallback); per-language table in USAGE.md; a bounded carve-out note under rule 2; a `make check`-wired regen-and-diff determinism gate.

### Deferred Ideas (OUT OF SCOPE)
- FUT-01..03 (Hono/Express/Fastify/Rust sources); FUT-04 (stdlib-pure TS path retiring the carve-out).
- Retiring the v1 known-debt (goextract golang.org/x/tools; compile-time goextract path; gnr8-core serde/blake3/thiserror).
- Any Phase-5 hardening item (WR-02/WR-04) not folded in here → backlog 999.x.
- New source frontends or SDK targets; changing IR/lowering/extraction behavior; new languages.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| XLANG-01 | FastAPI end-to-end (`gnr8 generate`) → OpenAPI 3.1 + Python SDK from a `.gnr8/` lifecycle, with real committed example output. | "End-to-End Examples" section: mirror `examples/bookstore/.gnr8/src/main.rs` with `FastApi::new().inputs([...])` + `OpenApi31` + `PySdk`; source lives IN the example dir (`FastApi::load` resolves `inputs` against `cx.project_root`). |
| XLANG-02 | NestJS end-to-end → OpenAPI 3.1 + TS SDK from a `.gnr8/` lifecycle, real committed output. | Same pattern with `NestJs::new()` + `TsSdk`. `tsextract` reads types statically from the `.ts` source — no `npm install` / `node_modules` needed in the example. |
| XLANG-03 | docs/USAGE.md per-language honest envelope (FastAPI full; Flask typed-only; NestJS class DTOs) with limits. | "docs/USAGE.md Envelope" section: per-frontend table derived from the Phase 2/4 SUMMARYs (verified). |
| XLANG-04 | `doctor`/`check`/`watch` across all language sidecars (toolchain detection, drift, loop-safety). | "doctor/check/watch Parity" section: `doctor` hardcodes `go_toolchain`; `watch` hardcodes `*.go` triggers; both must become source-language-aware via a new `pub` `detect_language`/probe API. `check` already works for any output path. |
| XLANG-05 | Every sidecar stdlib-only; gnr8-core takes zero (NEW) OSS deps; deterministic; RECORD the typescript carve-out. | "Carve-Out Recording" + "Cross-Language Determinism" sections + the gnr8-core dep audit (Open Q 1). |
</phase_requirements>

## Summary

This is the **milestone capstone — an integration/hardening/docs phase, not a feature phase.** Phases 1–5 already shipped (and `make check`-green) the entire multi-language pipeline: a language-neutral IR + JSON facts contract, `pyextract` (FastAPI full + Flask typed-envelope), `PySdk`, `tsextract` (NestJS), and `TsSdk`. The `FastApi`/`Flask`/`NestJs` Sources and `OpenApi31`/`PySdk`/`TsSdk` Targets all exist as composable `.gnr8/` built-ins. Phase 6 PROVES the whole thing end-to-end from real `.gnr8/` lifecycles with committed output, documents the honest envelope, achieves CLI parity across sidecars, guarantees cross-language determinism, and records the `typescript` carve-out.

Four concrete code/artifact deliverables: (1) two new `examples/` projects (FastAPI + NestJS), each a hand-authored `.gnr8/` Pipeline crate plus committed `generated/` output, mirroring the v1 `examples/bookstore/` Go pattern; (2) a CLI-parity refactor of `doctor` and `watch` so they follow the SOURCE language's toolchain/file-extensions instead of hardcoding Go — `check` already generalizes; (3) a per-language honest-envelope section in `docs/USAGE.md`; (4) a bounded carve-out note added to `CLAUDE.md` (PROJECT.md already logs it). Plus a `make check`-wired cross-language regen-and-diff that makes the committed example output the determinism proof.

**Primary recommendation:** Hand-author both example `.gnr8/` crates (init scaffolds Go-only — language-aware init is OUT of scope). Generalize `doctor`/`watch` by promoting `analyze::detect_language` (or a thin new `gnr8_core` API exposing the detected `Lang` + its toolchain identity) to `pub`, then dispatch the probe and the watch trigger-extensions on it — keeping it the single deterministic decision (rule 3). Add the carve-out as a bounded note under CLAUDE.md rule 2; do NOT loosen rule 2 generally and do NOT attempt to retire the four existing gnr8-core debt crates (that is FUT/cleanup, explicitly deferred).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Example `.gnr8/` Pipeline crates (FastAPI/NestJS) | `.gnr8/` child crate (code-as-config) | gnr8-core built-ins | Config is code (rule 4); the child composes existing `Source`/`Target` built-ins and is `cargo run` by the host. |
| Committed example `generated/` output | gnr8 host (trusted writer) | gnr8-core lower/sdk targets | The host owns writes (ownership manifest, no-op skip); the targets produce the bytes. |
| Source-language detection for CLI parity | gnr8-core `analyze::detect_language` | gnr8 CLI (`doctor`/`watch`) | Single deterministic classifier (rule 3) already in core; the CLI must consume it instead of assuming Go. |
| Toolchain probe (`go`/`python3`/`node`) | gnr8 CLI (`doctor`) | gnr8-core `*ToolchainMissing` typed errors | The probe is an impure shell concern (binary boundary, D-09); the typed error is the single source of "which toolchain a target needs." |
| Watch trigger-extension set (`.go`/`.py`/`.ts`) | gnr8 CLI (`watch`) | gnr8-core `detect_language` | Watch is a binary-boundary I/O shell; the language drives which source extensions trigger regen. |
| Drift detection (`check`) | gnr8-core `lifecycle::plan_only` | — | Already language-agnostic — operates on arbitrary declared output paths (verified). |
| Honest envelope docs | `docs/USAGE.md` (artifact) | Phase 2/4 SUMMARYs (source of truth) | Documentation tier; derived from the locked extraction behavior. |
| Carve-out invariant record | `CLAUDE.md` + `PROJECT.md` (artifacts) | — | Governance/docs tier; a bounded, audited prose edit. |
| Cross-language determinism gate | `Makefile` (`make check`) | gnr8 `check` / committed examples | The committed output + a regen-diff is the proof; the gate enforces it. |

## Standard Stack

This phase adds NO new libraries. The "stack" is the already-shipped, in-repo machinery the examples compose and the CLI generalizes. There is **nothing to `npm install` / `pip install` / `cargo add`** — that is the whole point (rules 2 + 5).

### Core (existing, reused unchanged)
| Component | Location | Purpose | Why Standard |
|-----------|----------|---------|--------------|
| `FastApi` / `Flask` / `NestJs` Sources | `crates/gnr8-core/src/sdk/builtins.rs` | source dir → `ApiGraph` via `build_graph` | The composable Source built-ins the example `.gnr8/` crates use. `.load()` resolves `inputs` against `cx.project_root` (verified). |
| `OpenApi31` / `PySdk` / `TsSdk` Targets | `crates/gnr8-core/src/sdk/builtins.rs` | frozen IR → artifact bytes | The Targets the examples compose (OpenAPI + the per-language SDK). |
| `Pipeline` + `runner::run` | `crates/gnr8-core/src/sdk/`, `src/runner/` | the `.gnr8/` child entry; `__emit`/`__inspect` | The exact pattern `examples/bookstore/.gnr8/src/main.rs` follows (verified). |
| `analyze::detect_language` + `Lang` | `crates/gnr8-core/src/analyze/mod.rs` | single deterministic source-language classifier | The one source of "which sidecar/toolchain" (rule 3). Currently `pub(crate)` — must be exposed for CLI parity (see Pitfall 1). |
| `CoreError::{Go,Python,TypeScript}ToolchainMissing` | `crates/gnr8-core/src/lib.rs` (typed errors) | "which toolchain a target needs" | The typed variants `doctor` should map onto per language. |
| `lifecycle::{regenerate, plan_only}` + `manifest` (blake3) | `crates/gnr8-core/src/lifecycle/`, `src/manifest/` | ownership, no-op, drift, loop-safety | Already language-agnostic (operate on declared output paths). |

### Supporting (the example sources)
| Asset | Location | Purpose | When to Use |
|-------|----------|---------|-------------|
| `fixtures/fastapi-bookstore/` (`app/main.py`, `app/models.py`) | repo | a real static FastAPI service | Copy into `examples/fastapi-bookstore/` as the example's source app. |
| `fixtures/nestjs-bookstore/` (`src/books.controller.ts`, `src/books.dto.ts`) | repo | a real static NestJS service | Copy into `examples/nestjs-bookstore/` as the example's source app. |
| `examples/bookstore/` (Go) + `examples/taskflow/` (Go) | repo | the v1 committed-output example pattern | Mirror their `.gnr8/` + `generated/` layout exactly. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Copy fixtures into `examples/` | Point the example `.gnr8/` at the existing `fixtures/` dir via a relative `inputs(["../../fixtures/fastapi-bookstore/app"])` | RECOMMEND COPY. `FastApi::load` resolves `inputs` against `cx.project_root` (the example dir), so a self-contained `examples/fastapi-bookstore/app/` is cleaner, mirrors `examples/bookstore` (source lives beside `.gnr8/`), and keeps the example runnable/portable. Pointing at `fixtures/` couples two trees and breaks the "an example is a self-contained project" story. (Flag for planner: copying duplicates source — acceptable for an example; keep the fixtures as the snapshot-test source of truth.) |
| Promote `detect_language` to `pub` | Add a small purpose-built `gnr8_core::analyze::source_toolchain(dir) -> Result<Toolchain, CoreError>` | Either works. A thin new API that returns an enum like `{ Go, Python, TypeScript }` (or a `&'static str` binary name) is cleaner than leaking the whole `Lang`/`detect_language` internals, and keeps the "single decision" contract explicit. RECOMMEND a minimal new `pub` function over widening `Lang`'s visibility. |
| Hand-author example `.gnr8/` crates | Make `gnr8 init` language-aware | Hand-author. Language-aware init is explicitly OUT of scope (CONTEXT open question resolved below); `MAIN_RS_BODY` is a hardcoded Go scaffold and changing it is a separate feature. |

**Installation:** None. (No package is added in this phase. `cargo`, `go`, `python3`, `node`+vendored `typescript` are already required toolchains.)

## Package Legitimacy Audit

**Not applicable — this phase installs zero external packages.** Rule 2 forbids it, and the phase adds no new dependency to any crate, the Python sidecar, or the Node sidecar. The only pre-existing third-party toolchain (`typescript`, vendored in `tsextract/node_modules`, pinned EXACT 5.9.3) was added and audited in Phase 4 and is the documented carve-out this phase RECORDS — it is not installed or changed here.

slopcheck was therefore not run (no install set). If the planner adds any package (it should not), gate it behind a `checkpoint:human-verify` task — but the correct disposition for any "add a package" task in this phase is **reject as a rule-2 violation.**

## Architecture Patterns

### System Architecture Diagram

```
                         gnr8 CLI (host = orchestrator + trusted writer)
                                          │
        ┌───────────────┬────────────────┼────────────────┬──────────────────┐
   gnr8 generate    gnr8 check        gnr8 watch       gnr8 doctor        gnr8 init
        │               │                 │                 │            (Go scaffold;
        │ __emit        │ __emit          │ __emit          │ __emit      NOT lang-aware —
        ▼               ▼ (dry run)       ▼ (on trigger)    ▼ (probe)     out of scope)
   ┌───────────────────────────────────────────────────────────────────┐
   │  child::run_child  →  cargo run --manifest-path <example>/.gnr8/    │
   │                       Cargo.toml -- __emit   (cwd = example root)   │
   └───────────────────────────────────────────────────────────────────┘
                                          │ ArtifactBundle (JSON on stdout)
                                          ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  .gnr8/src/main.rs  (CODE-AS-CONFIG, hand-authored per example)     │
   │  Pipeline::new()                                                    │
   │    .source( FastApi | NestJs )  ── inputs resolve vs project_root ──┤
   │    .transform( SetBasePath / SetTitle / ApplySecurity … )           │
   │    .target( OpenApi31 ).target( PySdk | TsSdk )                     │
   │    .post( Header::generated() )                                     │
   └───────────────────────────────────────────────────────────────────┘
        │ source.load() → analyze::build_graph(dir)
        ▼
   ┌──────────────────────────┐   detect_language(dir)  ── ONE decision (rule 3)
   │  analyze::build_graph     │──────────┬──────────────┬──────────────┐
   └──────────────────────────┘          ▼              ▼              ▼
                              run_goextract     run_pyextract    run_tsextract
                              (go run .)        (python3 -m      (node index.js)
                                                 pyextract)
                                    │                │                │
                                    ▼                ▼                ▼
                              SAME neutral JSON facts contract (facts.rs, deny_unknown_fields)
                                    │
                                    ▼
                          ApiGraph IR  →  lower::to_openapi  +  {go,py,ts}sdk::generate
                                    │
                                    ▼
                  host writes generated/ (ownership manifest, no-op skip, edit protection)
                                    │
              committed examples/*/generated/  ←── make check regen-and-diff == determinism proof
```

A reader traces the FastAPI example: `gnr8 generate` in `examples/fastapi-bookstore/` → host `cargo run`s that example's `.gnr8/` crate → the crate's `FastApi` Source calls `build_graph("app")` → `detect_language` returns Python → `run_pyextract` → neutral facts → IR → `OpenApi31` + `PySdk` targets → host writes `generated/openapi.yaml` + `generated/sdk/*.py`.

### Recommended Project Structure (the two new examples — mirror `examples/bookstore/`)
```
examples/
├── fastapi-bookstore/
│   ├── README.md            # what it showcases (mirror examples/bookstore/README.md)
│   ├── app/                 # COPIED from fixtures/fastapi-bookstore/app/ (the source app)
│   │   ├── main.py
│   │   ├── models.py
│   │   └── __init__.py
│   ├── .gnr8/
│   │   ├── Cargo.toml       # name "fastapi-bookstore-gnr8-gen"; gnr8-core path dep; empty [workspace]
│   │   ├── Cargo.lock       # committed (mirror bookstore)
│   │   ├── .gitignore       # /target/ /cache/
│   │   └── src/main.rs      # THE CONFIG: Pipeline.source(FastApi).target(OpenApi31).target(PySdk)
│   └── generated/           # COMMITTED output of `gnr8 generate`
│       ├── openapi.yaml
│       └── sdk/*.py
└── nestjs-bookstore/
    ├── README.md
    ├── src/                 # COPIED from fixtures/nestjs-bookstore/src/ (no node_modules needed)
    │   ├── books.controller.ts
    │   └── books.dto.ts
    ├── .gnr8/ … (same shape)  # src/main.rs: Pipeline.source(NestJs).target(OpenApi31).target(TsSdk)
    └── generated/
        ├── openapi.yaml
        └── sdk/*.ts
```

### Pattern 1: The example `.gnr8/src/main.rs` (mirror the v1 bookstore lifecycle)
**What:** A tiny binary crate composing a `Pipeline` and handing it to `runner::run`. The ONLY differences from the Go bookstore are the Source and the SDK Target.
**When to use:** Both new examples.
**Example (FastAPI):**
```rust
// Source: examples/bookstore/.gnr8/src/main.rs (verified v1 pattern), adapted to FastApi/PySdk.
use gnr8_core::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8_core::runner::run(
        Pipeline::new()
            .source(FastApi::new().inputs(["app"]))          // resolves vs project_root (example dir)
            .transform(SetBasePath::new("/books"))           // APIRouter(prefix="/books") is a base path (rule 1)
            .transform(SetTitle::new("Bookstore API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```
**Example (NestJS):**
```rust
use gnr8_core::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8_core::runner::run(
        Pipeline::new()
            .source(NestJs::new().inputs(["src"]))           // @Controller('books') is a base path (rule 1)
            .transform(SetBasePath::new("/books"))
            .transform(SetTitle::new("Bookstore API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(TsSdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```
*Note: confirm the exact base-path string + module string against the committed fixture snapshots so the example OpenAPI structurally matches the proven extraction. `inputs(["app"])`/`(["src"])` scopes detection to the single-language subdir, avoiding the `.gnr8/target` Rust-artifact contamination (see Pitfall 2).*

### Pattern 2: Source-language-aware CLI parity (the doctor/watch generalization)
**What:** Replace the two hardcoded-Go assumptions (`probe_go()` / `go_toolchain` bool in doctor; the `*.go` trigger in watch) with a single `detect_language`-driven decision.
**When to use:** XLANG-04.
**Approach (doctor):**
```rust
// Today (crates/gnr8/src/main.rs): hardcoded Go.
fn probe_go() -> bool { Command::new("go").arg("version").output().is_ok() }
let go_present = probe_go();
// ... LifecycleHealth { go_toolchain: bool }

// Generalize: derive the source language from the example's Source input dir, then probe THAT toolchain.
// Single deterministic decision (rule 3) — reuse detect_language; no try-go-then-python fallback.
//   Go         -> probe "go version"
//   Python     -> probe "python3 --version"
//   TypeScript -> probe "node --version"  (the vendored tsc is bundled with tsextract; node is the gate)
// Rename the LifecycleHealth field go_toolchain -> source_toolchain (or keep go_toolchain + add a
// `language` field). NOTE: the doctor --json field-set test pins {initialized, go_toolchain,
// pipeline_runs}; renaming REQUIRES updating doctor::tests::doctor_json_field_set (verified at
// crates/gnr8/src/doctor.rs:526). Decide field naming deliberately — it is a published --json contract.
```
**Approach (watch):** `is_trigger_path` hardcodes `ext == "go"` (crates/gnr8/src/watch.rs:105). Generalize the trigger extension(s) to the detected source language: `.go` | `.py` | `.ts`. The `.gnr8/src/**.rs` pipeline-edit trigger and the output-filter loop-safety are already language-agnostic and need NO change.

### Anti-Patterns to Avoid
- **A per-command language switch / dual detection.** Detect once (one `detect_language` call), pass the result down. Two independent detections in doctor and watch is two sources of truth (rule 3 risk).
- **A try-go-then-python-then-node probe chain.** That is exactly the forbidden fallback (rule 3). Probe the ONE toolchain the detected language needs.
- **Making `gnr8 init` language-aware.** Out of scope. Examples are hand-authored.
- **"Fixing" the gnr8-core serde/blake3 debt to satisfy "zero OSS."** Out of scope (deferred). This phase adds no NEW dep; it does not retire the existing debt.
- **Re-deriving the toolchain identity from the `Source` type name** (e.g. "it's `FastApi` so it's Python"). The single source is `detect_language` over the input dir (rule 3/4) — the Source built-in already delegates to it.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Which toolchain does this source need? | A new mapping in doctor/watch | `analyze::detect_language` (promote to `pub`) + the `*ToolchainMissing` variants | Single source of truth (rule 3); already battle-tested with 3-arm dispatch + ambiguity errors. |
| Drift / staleness across SDK output paths | New per-language check logic | `lifecycle::plan_only` (already path-generic) | `check` already partitions Write/UserEdited/Unchanged for ANY declared output path — verified language-agnostic. |
| Loop-safe watch over py/ts outputs | New ignore logic | `watch::build_output_set` + manifest-recorded paths | The loop-safety filter keys on the ownership manifest's recorded output paths — language-agnostic already; only the *trigger extension* is Go-specific. |
| Committed-output determinism proof | A bespoke comparison script | `gnr8 check` (exit 1 on drift) wired into `make check` | `check` IS the regen-and-diff: run it against the committed examples; non-zero = drift. |
| Carve-out wording | A new invariants doc | A bounded note under existing CLAUDE.md rule 2 | PROJECT.md already logs it; CLAUDE.md just needs the bounded exception recorded in place. |

**Key insight:** Almost everything for XLANG-04/05 already exists and is language-agnostic. The real work is TWO surgical generalizations (the doctor probe + the watch trigger extension), both keyed off the single existing `detect_language` decision — plus authoring artifacts (examples, docs, the carve-out note) and wiring a determinism gate. This is integration, not construction.

## Runtime State Inventory

> This phase RENAMES no stored string and MIGRATES no datastore. It ADDS example projects + committed output, edits CLI source, and edits two prose docs. The closest "runtime state" concern is committed generated output that must stay byte-identical to a fresh regen.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — no database, datastore, collection, or user_id is touched. Verified: no DB layer in the repo. | None |
| Live service config | None — gnr8 has no external live service (no n8n/Datadog/etc.). The example FastAPI/NestJS apps are STATIC source, never run. | None |
| OS-registered state | None — no Task Scheduler / launchd / systemd / pm2 registration. | None |
| Secrets/env vars | The only env vars are `GNR8_CARGO`/`CARGO` (child cargo override) — unchanged this phase. No secret keys. | None |
| Build artifacts / committed generated output | **The committed `examples/*/generated/` output IS state that must match a fresh `gnr8 generate`.** Each example's `.gnr8/target/` + `.gnr8/cache/` are git-ignored build/lifecycle artifacts. `examples/*/.gnr8/Cargo.lock` is committed (mirror bookstore). | The phase deliberately CREATES this committed state and gates it with the regen-and-diff. The NestJS example needs NO `node_modules` (static extraction). The FastAPI example needs NO installed deps (static `ast`). |

**The key question — what runtime systems hold an old string after the repo is updated?** None: there is no rename/migration in this phase. The single artifact-consistency obligation is "committed example output == fresh regen," enforced by `make check`.

## Common Pitfalls

### Pitfall 1: `detect_language` / `Lang` are `pub(crate)` — unreachable from the `gnr8` CLI crate
**What goes wrong:** `doctor`/`watch` live in `crates/gnr8/` (the binary), but `detect_language` + `Lang` are `pub(crate)` inside `crates/gnr8-core/src/analyze/mod.rs` (verified). The CLI cannot call them today.
**Why it happens:** They were only ever consumed inside core (`build_graph`).
**How to avoid:** Add a minimal `pub` API to `gnr8_core` — recommended: a new `pub fn analyze::source_toolchain(dir: &str) -> Result<Toolchain, CoreError>` (or `pub fn detect_language` + a `pub enum Lang`). Keep it the SAME single decision (rule 3) — do not duplicate the classifier. The new `pub` enum should expose enough to drive BOTH the doctor probe binary name and the watch trigger extension(s).
**Warning signs:** A compile error `detect_language is private`; a temptation to copy the marker-scan into the CLI (that would be a second source of truth — rule 3 violation).

### Pitfall 2: Language detection over the WHOLE example dir mis-classifies as ambiguous
**What goes wrong:** `detect_language` scans recursively and errors on >1 language (the WR-05 ambiguity guard). An example dir contains the `.gnr8/` Rust crate (`*.rs`, but Rust is not a marker) AND, after a build, `.gnr8/target/` debris. More importantly, if doctor/watch ever scan the example ROOT rather than the Source's `inputs` subdir, a mixed tree could surface.
**Why it happens:** The example root is not single-language if you point detection at it broadly.
**How to avoid:** Always detect over the Source's INPUT dir (`app/` or `src/`), not the example root — exactly as `build_graph` does (it receives the resolved `inputs` path). The example layout (source under `app/`/`src/`) keeps each input subdir single-language. Rust `.rs` is not a detect marker, so the `.gnr8/` crate itself is invisible to detection — good — but `.gnr8/target/` debug builds can drop `*.go`-free debris; scoping to `app/`/`src/` sidesteps it entirely.
**Warning signs:** `gnr8 doctor` in an example reports a `Config: ambiguous source language` error.

### Pitfall 3: The `doctor --json` field set is a pinned contract
**What goes wrong:** Renaming `LifecycleHealth.go_toolchain` breaks `doctor::tests::doctor_json_field_set` (it asserts the exact set `{initialized, go_toolchain, pipeline_runs}` — verified crates/gnr8/src/doctor.rs:526) AND any external CI parsing `doctor --json`.
**Why it happens:** The doctor JSON shape is intentionally a stable published contract (documented in the doctor module header).
**How to avoid:** Decide deliberately: either (a) rename `go_toolchain` → `source_toolchain` + add a `language` field and UPDATE the test + USAGE.md, or (b) keep `go_toolchain` semantics generalized to "the source toolchain" with a doc note. Option (a) is more honest for a multi-language tool; do it and update the test in the same task.
**Warning signs:** `doctor_json_field_set` fails; `make check` red on the doctor test.

### Pitfall 4: The committed example output drifts from the snapshot-proven extraction
**What goes wrong:** The example's `.gnr8/` config (base path, title, module, output paths) produces an OpenAPI/SDK that differs from what the Phase 2/4 fixture snapshots proved, so the example silently misrepresents the tool.
**Why it happens:** The example is a fresh composition; its transforms (base path, title) are config choices not pinned by the fixture snapshots.
**How to avoid:** Generate the example output with `gnr8 generate`, then sanity-check the OpenAPI structurally matches the committed `fixtures/*` snapshots (same routes/schemas/refs). Commit the REAL generated bytes (never hand-write them). Wire `gnr8 check` over the examples into `make check` so any future drift fails the gate.
**Warning signs:** The example OpenAPI has routes/schemas the fixture snapshot doesn't, or vice versa.

### Pitfall 5: Reading "gnr8-core takes zero OSS deps" literally and rewriting serde/blake3
**What goes wrong:** Someone treats XLANG-05 as "make `cargo tree -p gnr8-core` empty" and tries to hand-roll serde/blake3/thiserror — a multi-week rewrite that blows the phase and risks all green tests.
**Why it happens:** The requirement text says "zero OSS deps"; the literal current state has four debt crates.
**How to avoid:** Read XLANG-05 as "add no NEW OSS dep this phase; the carve-out is the only sanctioned toolchain exception; record it." The four debt crates (`serde`, `serde_json`, `blake3`, `thiserror`) are v1.0 known-debt CLAUDE.md schedules for LATER retirement, and CONTEXT explicitly defers "retiring the v1 known-debt." See Open Question 1. The phase's determinism/zero-new-dep claim is verified by a `cargo tree` diff showing no NEW crate added.
**Warning signs:** A task proposes editing `crates/gnr8-core/Cargo.toml` `[dependencies]`.

### Pitfall 6: `make check` does not currently gate the examples or run a cross-language regen-diff
**What goes wrong:** Today `make check` = fmt + clippy + test + go fixture builds (verified Makefile:82). It does NOT regenerate the examples or diff them; `make gates` runs the `determinism` test (fixture-level) but not the example output. A committed example could rot.
**Why it happens:** The examples were not part of the v1 gate beyond their inclusion in repo.
**How to avoid:** Add a `make examples-check` (or fold into `check`) that, for each example, runs `gnr8 generate` then `gnr8 check` (exit 1 on drift) — across Go/Python/TS. This is the cross-language determinism proof (XLANG-05). Requires `go`+`python3`+`node`+`cargo` present (the sandbox has all — see Environment Availability).
**Warning signs:** No new target/step references `examples/fastapi-bookstore` / `examples/nestjs-bookstore`.

### Pitfall 7: USAGE.md references a non-existent `examples/taskflow` Source pattern, and is Go-only throughout
**What goes wrong:** `docs/USAGE.md` says "The full runnable example: `examples/taskflow/`" (verified line 162) — taskflow DOES exist (Go), but the doc's Source built-ins table lists only `GoGin` (line 83) and the whole envelope/type-mapping is Go/Gin-only. Extending it must ADD the Python/TS frontends without breaking the accurate Go content.
**Why it happens:** USAGE.md is the v1.0 Go-only reference.
**How to avoid:** ADD a per-language envelope section + update the Source/Target built-ins tables to list `FastApi`/`Flask`/`NestJs` and `PySdk`/`TsSdk`; update the `watch` note ("re-runs on a `*.go` edit" → "a source-language edit") and the doctor row (Go toolchain → source toolchain). Update, don't duplicate (CONTEXT). Point to the two new examples.
**Warning signs:** USAGE.md still says "Go only; Gin only" as the global envelope after the phase.

## Code Examples

### The honest per-language envelope (derived from the Phase 2/4 SUMMARYs — VERIFIED behavior)
```markdown
<!-- docs/USAGE.md — proposed per-language envelope table (XLANG-03) -->
## Supported source frontends (the honest envelope)

| Frontend | Lang | Status | Recognized | Limits / diagnostics |
|---|---|---|---|---|
| Gin | Go | full | route groups, path/query params, ShouldBindJSON body, c.JSON responses, const enums, nested structs | ONE route group only; float64→float32 narrowing (diag); map[string]any free-form (diag); untyped c.Query → string (diag); Gin-only. |
| FastAPI | Python | full | @app/@router verbs, APIRouter/literal prefixes, path params (template∩args), typed query params (defaults→required/optional), Pydantic/@dataclass bodies, response_model=, status_code=, Literal/Enum, Union aliases | static `ast` only (never imports/executes); unresolvable/foreign type → diagnostic + omit (no guess). |
| Flask | Python | typed-envelope (honest second-class) | @app.route/methods=, Blueprint(url_prefix=), <int:id> converter path params, OPT-IN typed DTOs/returns; method-derived status (typed POST→201 else 200) | untyped request.json / unannotated request.args / missing return annotation → diagnostic, NEVER inferred. State plainly: untyped surfaces are NOT recovered. |
| NestJS | TypeScript | class-DTO scope | @nestjs/common verb + @Param/@Query/@Body decorators, @Controller prefix (provenance, never folded), DTO CLASSES, enums + string-literal-union, method-derived status (@HttpCode override) | DTO classes only (bare `interface`s are erased — not extracted); never reads @nestjs/swagger / zod / class-validator (rule 1); unresolvable → diagnostic + omit. |

Generated SDKs are dependency-free in every language: GoSdk (net/http), PySdk (urllib + @dataclass),
TsSdk (built-in fetch + typed interfaces).
```

### The bounded CLAUDE.md carve-out note (XLANG-05 — RECORD, do not loosen rule 2)
```markdown
<!-- Add under CLAUDE.md "## 2. No third-party / OSS dependencies", as a clearly-scoped subsection. -->
### Documented exception (the ONE sanctioned carve-out)

Exactly one third-party toolchain is permitted, narrowly scoped: the **`typescript`** npm package,
used SOLELY by the `tsextract` Node sidecar to read TypeScript types via the language's own reference
Compiler API. Rationale: TS has no stdlib type-checker (unlike Go's `go/types` or Python's `ast`);
`typescript` is the language implementation itself, zero-dependency (Microsoft), and runs as an
isolated sidecar behind the JSON-facts boundary — the same isolation as the Go/Python sidecars.

Bright line (this carve-out does NOT loosen rule 2 anywhere else):
- It is the ONLY OSS dependency permitted in any sidecar; every other sidecar stays stdlib-only.
- `gnr8-core` and every generated SDK remain dependency-free.
- It NEVER licenses reading `@nestjs/swagger`, `zod`, `class-validator`, or any third-party
  schema/annotation tool (rule 1 still forbids those absolutely).
- A future stdlib-pure TS path (FUT-04) may retire it.
```
*(PROJECT.md already logs this as a Key Decision — verified; CLAUDE.md is the missing record this requirement adds. Keep the wording bounded and audit it against the PROJECT.md decision so the two agree.)*

### The cross-language determinism gate (Makefile — XLANG-05)
```makefile
# Proposed: regenerate each example and assert byte-identical (the committed output IS the proof).
# Requires go + python3 + node + cargo (all present in the dev/CI sandbox).
examples-check:
	cd examples/bookstore         && gnr8 generate && gnr8 check   # Go
	cd examples/fastapi-bookstore && gnr8 generate && gnr8 check   # Python
	cd examples/nestjs-bookstore  && gnr8 generate && gnr8 check   # TypeScript
# then add examples-check as a prerequisite of `check` (or run via the release gnr8 binary).
```
*Note: `gnr8 check` already exits 1 on stale/drifted output (verified main.rs run_check). The examples need the built `gnr8` binary on PATH (or `cargo run -p gnr8 --`). Confirm the planner decides binary-on-PATH vs cargo-run invocation for CI portability.*

## State of the Art

| Old Approach (v1.0 / pre-Phase-6) | Current Approach (this phase) | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Examples are Go-only (`examples/bookstore`, `examples/taskflow`) | + FastAPI + NestJS examples with committed output | Phase 6 | Proves the IR narrow-waist end-to-end across 3 languages. |
| `doctor` probes `go version`; `LifecycleHealth.go_toolchain` | source-language-aware probe via `detect_language` | Phase 6 | doctor works in a python/ts project (XLANG-04). |
| `watch` triggers on `*.go` only | triggers on the source language's extension (`.go`/`.py`/`.ts`) | Phase 6 | watch works in a python/ts project (XLANG-04). |
| `docs/USAGE.md` is a Go/Gin-only reference | + per-language honest envelope | Phase 6 | Users see what each frontend actually covers (XLANG-03). |
| Carve-out logged only in PROJECT.md | also recorded (bounded) in CLAUDE.md | Phase 6 | The single rule-2 exception is in the invariants doc itself (XLANG-05). |
| `make check` does not regen/diff examples | a cross-language regen-and-diff gate | Phase 6 | Committed output is enforced as the determinism proof (XLANG-05). |

**Deprecated/outdated:**
- `check`/`watch`/`doctor` Go-toolchain assumptions — superseded by source-language dispatch this phase (the only code behavior that changes; extraction/SDK logic is untouched).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | "Zero OSS in gnr8-core" (XLANG-05) means "no NEW dep + carve-out recorded," NOT "retire the existing serde/blake3/thiserror debt." | User Constraints note, Pitfall 5, Open Q 1 | If wrong, the phase scope balloons into a serde rewrite. Backed by CONTEXT deferring "retiring the v1 known-debt" + CLAUDE.md scheduling it later — but the literal requirement text is "zero." NEEDS planner/user confirmation. |
| A2 | Examples should COPY the fixture source into `examples/*/` (self-contained), not point `.gnr8/` at `fixtures/`. | Standard Stack / Alternatives | If wrong, duplicate-source maintenance instead of a single tree. Low risk (matches v1 `examples/bookstore` pattern). |
| A3 | `gnr8 init` stays Go-only (examples hand-authored); language-aware init is out of scope. | Anti-Patterns, Open Q 3 | If wrong (init must be language-aware), a separate feature is needed — but CONTEXT scopes init out. Low risk. |
| A4 | WR-02/WR-04 are BACKLOG (999.x), not folded in — folding them would change the green TsSdk acceptance snapshots. | Open Q 4 | If folded carelessly, the committed TsSdk snapshots change and the NestJS example output shifts. The Phase-5 REVIEW-FIX explicitly skipped them for this reason (verified). Recommend backlog. |
| A5 | The NestJS example needs NO `node_modules` (static extraction reads `.ts` source types only). | Runtime State Inventory, Structure | If wrong, the example needs vendored deps (it does not — tsextract reads source types, treats `@nestjs/common` imports as unresolved decorators by design). Low risk (verified by the Phase-4 static-extraction design). |
| A6 | The doctor `--json` field rename (go_toolchain → source_toolchain) is acceptable as a contract change with a test update. | Pitfall 3 | If an external consumer depends on `go_toolchain`, this is breaking. Mitigated: pre-1.0 tool; document in USAGE.md. NEEDS a deliberate naming decision. |

## Open Questions

1. **Does "gnr8-core takes zero OSS deps" require retiring the existing `serde`/`serde_json`/`blake3`/`thiserror` debt this phase?**
   - What we know: `crates/gnr8-core/Cargo.toml` currently links those four (verified). CLAUDE.md's known-debt section lists exactly these as "debt to be removed, not precedent to copy," scheduled for LATER. CONTEXT's Deferred Ideas explicitly defers "retiring the v1 known-debt."
   - What's unclear: whether XLANG-05's verification accepts "no NEW dep + carve-out recorded + `cargo tree` shows no addition" as satisfying "zero OSS deps," or demands an empty dep tree.
   - Recommendation: Treat as A1 — no new dep, record the carve-out, verify via `cargo tree` diff. Surface to the user at planning if they want the literal-zero reading (that becomes its own milestone). **This is the single most important scope question.**

2. **doctor/watch: how does the CLI learn the source dir to detect the language over?**
   - What we know: doctor/watch run the `.gnr8/` child to get the ArtifactBundle, but the bundle carries artifacts + diagnostics, NOT the Source's input dir. `detect_language` needs a directory.
   - What's unclear: the cleanest way for the host to know "the source dir" without re-parsing the user's `.gnr8/` config (which is opaque Rust code, by design — rule 4).
   - Recommendation: Options — (a) detect over the project root EXCLUDING `.gnr8/` (works because the example source under `app/`/`src/` is single-language and the example root has no other language); (b) have the child/runner report the detected language or source dir in a NEW bundle field (a small `runner` addition — but that touches the wire schema/version). Prefer (a) for this phase (no schema change); validate it does not trip the ambiguity guard (Pitfall 2). FLAG for the planner — this is the trickiest design point.

3. **Does `gnr8 init` need language-awareness?** — RESOLVED: No. `MAIN_RS_BODY` is a hardcoded Go scaffold (verified workspace/mod.rs:105). CONTEXT scopes init out. Examples are hand-authored.

4. **Fold WR-02/WR-04 in, or backlog?** — Recommend BACKLOG (999.x). Both alter emitted TsSdk client behavior and would change the green acceptance snapshots + the NestJS example output (the Phase-5 REVIEW-FIX skipped them precisely for this reason — verified). Folding them is a SDK behavior/contract decision, not hardening. If the user wants them, do it in a dedicated task with snapshot regeneration, separate from the example/determinism work.

5. **CI invocation of the example regen-diff: built `gnr8` binary on PATH, or `cargo run -p gnr8 --`?** — The examples reference a `gnr8` binary in their READMEs. For `make examples-check`, decide whether to `cargo build --release -p gnr8` first and use the binary, or invoke via `cargo run`. Recommend building the binary once (matches the v1 `make` ethos) — minor planner decision.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo / Rust | build gnr8 + run `.gnr8/` child crates | ✓ (assumed — v1 + Phases 1-5 built here) | workspace edition 2021 | — (blocking) |
| go | the FastAPI/NestJS examples don't need it, but the Go example + `make check` fixture-build do | ✓ | at `/home/vercel-sandbox/.local/go-install/go/bin` (not on default PATH — see MEMORY) | — |
| python3 | the FastAPI example (`run_pyextract`) + pysdk_compile gate | ✓ | 3.9.25 (per CONTEXT) | — (blocking for the FastAPI example regen) |
| node | the NestJS example (`run_tsextract`) + tssdk_compile gate | ✓ | v24 (per CONTEXT) | — (blocking for the NestJS example regen) |
| typescript (vendored) | tsextract Compiler API | ✓ | EXACT 5.9.3, committed at `tsextract/node_modules/typescript` (gitignore negation) | — (the carve-out; do not change) |

**Missing dependencies with no fallback:** None — the sandbox has go, python3, node, and the vendored tsc (verified via CONTEXT + the green Phase 1-5 gates). NOTE: `go` is NOT on the default PATH in this sandbox (MEMORY: sandbox-toolchains) — ensure PATH is set before any `make check` run.

**Missing dependencies with fallback:** None.

## Validation Architecture

> nyquist_validation: config not found at `.planning/config.json` (absent ⇒ treated as enabled). Section included.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` / `cargo test` (gnr8-core + gnr8); Python `unittest` (pyextract); the `make gates` aggregate; the `make check` local gate. No external test runner (rule 2). |
| Config file | `Makefile` (the gate definitions); no separate test config. |
| Quick run command | `cargo test -p gnr8` (CLI parity tests: doctor/watch/check) |
| Full suite command | `make check` (fmt + clippy -D warnings + test + go fixture builds) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| XLANG-01 | FastAPI example regens byte-identically (OpenAPI + PySdk) | integration / determinism | `cd examples/fastapi-bookstore && gnr8 generate && gnr8 check` | ❌ Wave 0 (example + gate target) |
| XLANG-02 | NestJS example regens byte-identically (OpenAPI + TsSdk) | integration / determinism | `cd examples/nestjs-bookstore && gnr8 generate && gnr8 check` | ❌ Wave 0 |
| XLANG-03 | USAGE.md documents the per-language envelope | doc review (manual) + a grep gate that the frontends are named | `grep -E 'FastAPI|Flask|NestJS' docs/USAGE.md` | ❌ Wave 0 (manual-justified: prose accuracy is human-verified against the Phase 2/4 envelope) |
| XLANG-04 | doctor probes the source toolchain; watch triggers on the source extension | unit (pure decision) | `cargo test -p gnr8` (extend `doctor::tests` + `watch::tests` for py/ts) | ⚠️ exists, needs new cases |
| XLANG-04 | check detects drift on py/ts SDK output paths | unit / integration | reuse `lifecycle` tests (already path-generic) + the example gate | ✅ (lifecycle suite) |
| XLANG-05 | no NEW OSS dep added to gnr8-core | gate | `cargo tree -p gnr8-core` diff vs baseline (no new crate) | ❌ Wave 0 (a documented check, or a CI assertion) |
| XLANG-05 | cross-language byte-identical determinism | integration | `make examples-check` (all 3 examples regen + check) | ❌ Wave 0 |
| XLANG-05 | carve-out recorded in CLAUDE.md | doc review + grep | `grep -i 'carve-out\|documented exception' CLAUDE.md` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8` (fast — the doctor/watch parity unit tests) + `cargo fmt --check` + `cargo clippy -D warnings`.
- **Per wave merge:** `make gates` (the full blocking contract set) + the new `examples-check` for any touched example.
- **Phase gate:** `make check` green AND `make examples-check` green (all three languages regen byte-identically) before `/gsd:verify-work`.

### Wave 0 Gaps
- [ ] `examples/fastapi-bookstore/` (source app + `.gnr8/` crate + committed `generated/`) — covers XLANG-01
- [ ] `examples/nestjs-bookstore/` (source + `.gnr8/` crate + committed `generated/`) — covers XLANG-02
- [ ] `Makefile` `examples-check` target (regen + `gnr8 check` across Go/Py/TS) — covers XLANG-05 determinism
- [ ] New `doctor::tests` case: a Python/TS source reports the right toolchain probe (not `go_toolchain`) — covers XLANG-04
- [ ] New `watch::tests` case: a `*.py` / `*.ts` source edit triggers; the right language's extension is honored — covers XLANG-04
- [ ] A `pub` `gnr8_core::analyze` API (source-language/toolchain) so the CLI can detect — prerequisite for XLANG-04
- [ ] Update `doctor::tests::doctor_json_field_set` if the lifecycle field is renamed — contract test
- [ ] (Optional) a `cargo tree -p gnr8-core` no-new-dep assertion — covers XLANG-05 zero-new-OSS

## Security Domain

> security_enforcement: config absent ⇒ treated as enabled. This phase adds no network surface, no auth, no input parsing of untrusted data beyond what already exists.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | gnr8 is a local CLI; no auth surface. |
| V3 Session Management | no | No sessions. |
| V4 Access Control | no | Local filesystem tool. |
| V5 Input Validation | yes (existing) | Subprocess args (`target_dir`) are passed as DISCRETE `Command` args, never `sh -c` (verified helper.rs — threats T-02-01/T-04-01). The example dirs are local trusted source. No new input surface this phase. |
| V6 Cryptography | no (existing) | blake3 is a content fingerprint for the ownership manifest, not a security primitive; unchanged this phase. |

### Known Threat Patterns for this phase's surface
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Command/arg injection via a malicious source path in a probe | Tampering | Discrete `Command` args, no shell (the existing pattern across goextract/pyextract/tsextract — preserve it in the new doctor probe). |
| Executing target source at analysis time (Python/TS) | Elevation | Already mitigated: pyextract is static `ast` (grep-gated against exec/eval/import); tsextract reads types via the Compiler API without running the app. The STATIC examples are never executed by gnr8. Preserve — do not add any "run the example app" step. |
| A `postinstall`/build-script supply-chain vector | Tampering | N/A — no package install this phase; the vendored `typescript` is committed and pinned (Phase 4). |

## Sources

### Primary (HIGH confidence)
- `examples/bookstore/.gnr8/src/main.rs`, `examples/bookstore/README.md`, `examples/taskflow/` — the v1 committed-output example pattern to mirror.
- `crates/gnr8/src/main.rs` (`run_init`/`run_generate`/`run_check`/`run_doctor`/`run_watch`, `probe_go`) — the CLI lifecycle + the hardcoded-Go probe.
- `crates/gnr8/src/doctor.rs` — `LifecycleHealth.go_toolchain`, `DoctorReport`, the pinned `--json` field-set test.
- `crates/gnr8/src/watch.rs` — `is_trigger_path` (`ext == "go"`), the language-agnostic output-set loop-safety.
- `crates/gnr8/src/child.rs` — the host↔child `cargo run --manifest-path` boundary.
- `crates/gnr8-core/src/analyze/mod.rs` — `detect_language` (`pub(crate)`), `Lang`, `build_graph` 3-arm dispatch.
- `crates/gnr8-core/src/analyze/helper.rs` — `run_{go,py,ts}extract`, the `*ToolchainMissing` typed errors.
- `crates/gnr8-core/src/sdk/builtins.rs` — `FastApi`/`Flask`/`NestJs` Sources, `OpenApi31`/`PySdk`/`TsSdk` Targets, `FastApi::load` resolving `inputs` vs `cx.project_root`.
- `crates/gnr8-core/src/workspace/mod.rs` — `MAIN_RS_BODY` (hardcoded Go scaffold ⇒ init is not language-aware).
- `crates/gnr8-core/Cargo.toml` — the four existing OSS debt deps (serde/serde_json/blake3/thiserror).
- `docs/USAGE.md` — the Go-only reference to extend.
- `Makefile` — `check`/`gates`/`red` targets; no example regen-diff today.
- `fixtures/fastapi-bookstore/`, `fixtures/nestjs-bookstore/` — the static example sources.
- `.planning/phases/02-*/02-03-SUMMARY.md`, `02-04-SUMMARY.md`, `04-*/04-03-SUMMARY.md` — the verified per-language envelope behavior.
- `.planning/phases/05-*/05-REVIEW-FIX.md`, `05-VERIFICATION.md` — the WR-02/WR-04 deferred findings + why they were skipped.
- `.planning/PROJECT.md` (Key Decisions) — the carve-out already logged; `CLAUDE.md` — the carve-out NOT yet recorded.
- `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `06-CONTEXT.md` — the locked requirements/decisions.

### Secondary (MEDIUM confidence)
- `docs/milestone-v2-multi-language.md` — the design brief (note: its phase numbering is pre-`--reset-phase-numbers`; it calls this phase "11"; the actual phase is 6).

### Tertiary (LOW confidence)
- None — all findings are grounded in the in-repo source; no external/web sources were needed (rule 2 means there is no external stack to research).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — everything is in-repo, already shipped and green; nothing to install.
- Architecture (examples + CLI parity): HIGH — the example pattern is a verbatim mirror of `examples/bookstore`; the doctor/watch generalization points are pinpointed in source (the exact hardcoded-Go lines).
- Pitfalls: HIGH — each is verified against a specific source location (pub(crate) visibility, the pinned json test, the Makefile gaps, the dep audit).
- The one open scope risk (Open Q 1 / A1 — literal "zero OSS"): MEDIUM — recommendation is well-grounded but the literal requirement text could be read more strictly; flagged for user confirmation.

**Research date:** 2026-06-26
**Valid until:** 2026-07-26 (stable — integration phase over an internal, dependency-frozen codebase; the only volatility is the user's reading of XLANG-05's "zero OSS" scope).
