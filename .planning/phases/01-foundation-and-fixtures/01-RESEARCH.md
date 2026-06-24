# Phase 1: Foundation And Fixtures - Research

**Researched:** 2026-06-24
**Domain:** Rust CLI/workspace scaffolding, `clap` derive CLI, `thiserror`/`anyhow` error strategy, `insta` red-by-design snapshot harness, Go/Gin fixture authoring, CI quality gates
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Scope locked to **Go source → OpenAPI → Go SDK**. No other source languages or SDK targets this milestone.
- **D-02:** First supported router family is **Gin** (`gin-gonic/gin`): `router.Group(prefix)` + `group.METHOD(path, handler)`, `c.ShouldBindJSON(&t)`, `c.Param`/`c.Query`. Single PoC router.
- **D-03:** Extraction model is **router-agnostic**: graph stores *HTTP route facts* (method, path template, params, request type, response type+status), not Gin internals. Do NOT bake Gin-only assumptions into graph types. (Supersedes earlier `go-chi-basic` placeholder.)
- **D-04:** OpenAPI target version is **3.1.0**. Lowering must emit a diagnostic when a graph fact cannot be represented cleanly (OAPI-03 groundwork). 3.0 compatibility is future, not PoC scope.
- **D-05:** Go SDK shape: single SDK package exposing a **`Client`** built with functional options (base URL + custom `*http.Client`), **tag-grouped typed operation methods**, generated **request/response model structs**, JSON encode/decode, and a **typed API error**. Idiomatic Go (`context.Context` first arg) — NOT the openapi-generator builder pattern.
- **D-06:** `.gnr8/` layout (documented now, implemented Phase 4): checked-in **code-as-config** customization dir + git-ignored **cache/output lifecycle** dir. Detailed customization surface deferred to Phase 4; Phase 1 only records the split.
- **D-07:** Cargo **workspace** with library crate `gnr8-core` (extraction, graph, lowering, SDK gen, diagnostics as modules) + thin binary crate `gnr8` (CLI only).
- **D-08:** Edition **2021**, latest stable toolchain. Pin shared lints via `[workspace.lints]`.
- **D-09:** Errors: **`thiserror`** typed errors in library code; **`anyhow`** only at binary boundary (CLI `main`). No production `unwrap`/`expect` in library paths. Test helpers may use `anyhow`.
- **D-10:** **`clap`** (derive API) for argument parsing.
- **D-11:** Command surface present (skeletal): `init`, `generate`, `watch`, `check`, `inspect`, `doctor`. `inspect` has subcommands `routes | schemas | graph`.
- **D-12:** Output: human-readable tables by default, machine output behind a global `--json` flag; `-v/--verbose` for detail. Skeletal commands parse args and exit with a clear "not yet implemented in phase N" message rather than panicking.
- **D-13:** Snapshot testing via the **`insta`** crate. Snapshots kept small and reviewable.
- **D-14:** Go fixture is a **real Gin service module** under `fixtures/` mirroring TARGET-API.md §6: CRUD resource (POST/GET-list/PUT/DELETE) + list-with-query-filters resource. Covers path params, request/response bodies, JSON tags, optional (pointer/omitempty) fields, nested structs, enum newtypes, well-known types (`uuid.UUID`, `time.Time`), error responses, auth-middleware group, and **at least one unsupported pattern** (e.g. `map[string]any`) for diagnostics.
- **D-15:** Expected `graph`, `openapi`, `sdk`, `diagnostics` snapshots defined as the acceptance target. Because the analyzer does not exist yet, these tests must **fail clearly** (FIX-04) — not be silently skipped. Red-by-design until Phases 2–3 land.
- **D-16:** Local + CI gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test`. Wrapped in a **`Makefile`** and enforced by a **GitHub Actions** CI workflow.

### Claude's Discretion
- Exact crate module layout inside `gnr8-core`, snapshot file naming, Makefile target names, CI matrix details, and precise wording of skeletal-command messages are left to planning/execution.

### Deferred Ideas (OUT OF SCOPE)
- Detailed `.gnr8/` customization surface and code-as-config language — Phase 4.
- Additional router families (chi, echo, net/http) — post-PoC (v2); only the generic extraction seam is reserved now.
- OpenAPI 3.0 downstream-generator compatibility mode — future, behind diagnostics.
- TypeScript/Python SDK targets — v2 (out of scope this milestone).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| POC-01 | PoC scope locked to Go source, OpenAPI output, Go SDK output | §7 PoC Contract Doc — record scope lock in `docs/poc-contract.md` |
| POC-02 | First router set, OpenAPI version, Go SDK shape, `.gnr8/` layout documented before implementation expands | §7 — contract doc enumerates Gin / OpenAPI 3.1.0 / Client+functional-options SDK / `.gnr8/` split |
| POC-03 | Explicit non-goals prevent dynamic plugins, macro APIs, graph DBs, full framework coverage, multi-language | §7 — non-goals table copied from REQUIREMENTS.md "Out of Scope" into contract doc |
| RUST-01 | Minimal Rust workspace: thin CLI binary + library modules | Standard Stack + Architecture Patterns — `gnr8-core` lib + `gnr8` bin, module skeleton |
| RUST-02 | CLI exposes init/generate/watch/check/inspect/doctor, even if skeletal | clap derive skeleton (§2 of focus) — `Commands` enum, nested `InspectAction`, global `--json` |
| RUST-03 | Passes `cargo fmt`, `cargo test`, clippy `-D warnings` | Quality Gates — Makefile + GitHub Actions; verified toolchain present |
| RUST-04 | Library uses typed errors, avoids prod `unwrap`/`expect` | Error Strategy — `thiserror` enum in core, `anyhow` in `main`, clippy lints to enforce |
| FIX-01 | Realistic Go service fixtures for selected router patterns | Go Gin Fixture Module — `fixtures/goalservice/` Gin module |
| FIX-02 | Fixtures cover path params, bodies, JSON tags, optional fields, package boundaries, ≥1 unsupported pattern | Fixture DTO design — full coverage table incl. `map[string]any` |
| FIX-03 | Snapshot tests cover graph, OpenAPI, Go SDK, diagnostics | insta Harness — four snapshot families, one assertion per concern |
| FIX-04 | Fixture tests fail clearly before behavior is implemented | insta Red-by-Design — CI `INSTA_UPDATE=no` fails on missing snapshot; `unimplemented!()` panics = test failure |
</phase_requirements>

## Summary

This is a greenfield Rust + Go scaffolding phase. The toolchain is fully present and current on this machine (cargo/rustc **1.96.0**, clippy **0.1.96**, rustfmt **1.9.0**, Go **1.26.2**), so there is no environment risk. The job is to lay down a two-crate Cargo workspace (`gnr8-core` lib + `gnr8` bin), a `clap` derive CLI exposing six commands (one with nested subcommands) plus a global `--json` flag, a `thiserror`/`anyhow` error split, an `insta` snapshot harness that is **red by design**, a realistic Gin fixture module under `fixtures/`, and `fmt`/`clippy`/`test` quality gates in a Makefile and GitHub Actions workflow. No analysis, lowering, or SDK-generation logic is built here — only the contract and the harness that fails loudly until Phases 2–3 fill it in.

All recommended Rust crates were verified against the crates.io sparse index (authoritative) and are independently named in the project's own STACK.md and vendored `rust-best-practices` skill: **clap 4.6.1**, **thiserror 2.0.18** (note: v2, not the v1 most training data assumes), **anyhow 1.0.102**, **insta 1.48.0**, **serde 1.0.228**, **serde_json 1.0.150**. Gin **v1.12.0** and **google/uuid v1.6.0** were verified via the official Go module proxy. Two findings deserve planner attention: (1) `serde_yaml` is **deprecated** (last release 2024-03-25) — but `insta`'s `yaml` feature no longer depends on it (insta 1.48 vendors its own YAML emitter), so the OpenAPI-expected snapshots can use YAML safely without adding a deprecated dependency; (2) the red-by-design requirement (FIX-04) is satisfied automatically because **insta in CI defaults to `INSTA_UPDATE=no`**, which makes a missing snapshot a hard test failure rather than a silent skip.

**Primary recommendation:** Scaffold a 2-crate workspace with `[workspace.lints]` + `[workspace.dependencies]` inheritance, a clap derive CLI whose unimplemented arms return a typed `NotYetImplemented` error surfaced as a clean non-panicking message, a `thiserror` `CoreError` enum in `gnr8-core` with `anyhow` confined to `gnr8/src/main.rs`, and `insta` integration tests that assert on the output of not-yet-written `gnr8-core` functions (which return `Err(CoreError::NotYetImplemented)` / `unimplemented!()`), making the suite red-by-design and turning green only as Phases 2–3 implement those functions.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| CLI argument parsing, command dispatch, `--json`/`-v` flags | `gnr8` binary | — | Binary owns the user surface; keep it thin (D-07). |
| Process error→exit-code mapping, human/JSON rendering of top-level errors | `gnr8` binary | `gnr8-core` (typed errors) | `anyhow` boundary lives only in `main` (D-09); core returns typed errors. |
| Extraction / graph / lowering / SDK-gen / diagnostics logic | `gnr8-core` library (future modules) | — | Testable library modules consumed by binary + snapshot tests (D-07). Phase 1 only stubs the module seams. |
| API graph data model (route facts, schemas, provenance) | `gnr8-core` (graph module) | — | Router-agnostic graph types (D-03); HTTP facts not Gin internals. |
| Go source fixture (the *input* to be analyzed) | Go module under `fixtures/` | — | External Go module; compiled by `go build`, not by cargo. Phase 2's analyzer reads it. |
| Expected-output contracts (graph/openapi/sdk/diagnostics snapshots) | `insta` snapshots in `gnr8-core/tests/` (or `fixtures/.../expected/`) | — | Snapshots are the acceptance target Phases 2–3 must satisfy (D-15). |
| Quality enforcement (fmt/clippy/test, Go compile) | Makefile + GitHub Actions | — | Local + CI parity (D-16). |

## Standard Stack

All Rust versions verified 2026-06-24 against the crates.io sparse index (`https://index.crates.io/...`) — the authoritative registry. Gin/uuid verified against the official Go module proxy (`https://proxy.golang.org/...`).

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` (with `derive`) | **4.6.1** (MSRV 1.85) | CLI parsing, subcommands, global flags | De-facto Rust CLI crate; derive API is the idiomatic, declarative approach (D-10). `[CITED: docs.rs/clap/4.6.1]` |
| `thiserror` | **2.0.18** (MSRV 1.68) | Typed library error enums | Project guardrail crate for library errors (D-09). ⚠ **v2.x**, not v1 — see Pitfalls. `[VERIFIED: crates.io sparse index]` |
| `anyhow` | **1.0.102** | Ergonomic errors at the binary boundary only | Project guardrail crate for the `main` boundary (D-09). `[VERIFIED: crates.io sparse index]` |
| `insta` (feature `yaml`) | **1.48.0** (MSRV 1.66) | Snapshot testing of graph/openapi/sdk/diagnostics output | Project guardrail crate for snapshots (D-13). 1.48 no longer depends on deprecated `serde_yaml`. `[VERIFIED: crates.io sparse index]` |
| `serde` (feature `derive`) | **1.0.228** | Serialize graph/report types for snapshotting + future `--json` output | Needed so `insta::assert_yaml_snapshot!` / `assert_json_snapshot!` can render typed structs. `[VERIFIED: crates.io sparse index]` |
| `serde_json` | **1.0.150** | JSON rendering for the global `--json` flag (D-12) and JSON snapshots | Standard JSON lib. `[VERIFIED: crates.io sparse index]` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `cargo-insta` (CLI) | matches `insta` 1.48 | `cargo insta review` / `cargo insta test` developer UX | Dev convenience only — install via `cargo install cargo-insta`; NOT a Cargo dependency. Optional; not required for CI (CI uses plain `cargo test`). `[CITED: insta.rs/docs/quickstart]` |
| Go `gin-gonic/gin` | **v1.12.0** (requires Go ≥1.25) | The fixture web framework | Fixture module only. Local Go is 1.26.2 → satisfies. `[VERIFIED: proxy.golang.org]` |
| Go `google/uuid` | **v1.6.0** | `uuid.UUID` well-known type in fixture DTOs | Fixture module only. `[VERIFIED: proxy.golang.org]` |

**Deliberately NOT added in Phase 1** (avoid premature dependencies; pull in when the owning phase needs them):
- `notify` (file watching) → Phase 4 only. Current latest is a release candidate (`9.0.0-rc.4`) — defer the version decision. `[VERIFIED: crates.io sparse index]`
- Any Go-AST/type-checking integration → Phase 2.
- An OpenAPI/YAML emitter crate → Phase 3 (and may be hand-rolled per the "owned pipeline" constraint).
- `comfy-table`/`tabled` for human tables → optional; Phase 1 skeletal output can use plain `println!`. Defer unless the planner wants polished tables now.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `clap` derive | `clap` builder API | Builder is more verbose for a static command tree; derive is locked by D-10. No reason to deviate. |
| `insta` `yaml` snapshots | `insta` `json` snapshots, or `assert_snapshot!` (plain text) | YAML reads best in PR diffs (per skill ch.5 + insta docs) and is safe now that insta dropped `serde_yaml`. Use YAML for structured graph/openapi/sdk; plain text (`assert_snapshot!`) for `diagnostics.txt`. |
| `thiserror` 2.x | `thiserror` 1.x | 1.x is legacy; 2.x is current and what new training-stale code often gets wrong. Use 2.x. |
| Adding `serde_yaml` directly | (nothing — don't) | `serde_yaml` is **deprecated**. Do not add it as a direct dependency. `insta`'s vendored YAML covers snapshot needs. If YAML *output* is ever needed (Phase 3 OpenAPI), evaluate `serde_norway`/hand-rolled then — not now. |

**Installation (run from repo root after `cargo new`/manual scaffold):**
```bash
# Rust deps are declared in Cargo.toml (see Architecture Patterns), not via `cargo add` necessarily,
# but cargo add is the idiomatic way and pins current versions:
cargo add clap --features derive
cargo add thiserror
cargo add serde --features derive
cargo add serde_json
cargo add --dev insta --features yaml          # dev-dependency in gnr8-core
cargo add anyhow                               # in the `gnr8` binary crate only
cargo install cargo-insta                      # optional dev CLI, not a project dep

# Go fixture module (run inside fixtures/goalservice/):
go mod init github.com/<org>/gnr8-fixtures/goalservice
go get github.com/gin-gonic/gin@v1.12.0
go get github.com/google/uuid@v1.6.0
```

**Version verification performed (2026-06-24):**
```
clap        = 4.6.1            (sparse index, pub 2026-04-15, MSRV 1.85)
thiserror   = 2.0.18           (sparse index, pub 2026-01-18, MSRV 1.68)
anyhow      = 1.0.102          (sparse index, pub 2026-02-20)
insta       = 1.48.0           (sparse index, pub 2026-06-11, MSRV 1.66; no serde_yaml dep)
serde       = 1.0.228          (sparse index)
serde_json  = 1.0.150          (sparse index)
serde_yaml  = 0.9.34+deprecated (DEPRECATED — do not use directly)
gin-gonic/gin = v1.12.0        (Go proxy, pub 2026-02-28, requires Go 1.25)
google/uuid   = v1.6.0         (Go proxy)
```

## Package Legitimacy Audit

> slopcheck could not be installed in this environment (`pip install slopcheck` failed, network/policy). Per the graceful-degradation rule, packages are tagged according to provenance. **Mitigating factor:** every recommended Rust crate is independently named in the project's own `.planning/research/STACK.md` and the vendored `thoughts/skills/rust-best-practices/SKILL.md`, and all were confirmed on the authoritative crates.io sparse index (not a third-party mirror). These are among the most-downloaded crates in the Rust ecosystem with long histories and well-known source repos. Gin/uuid verified on the official Go proxy. None are new, obscure, or typosquat-adjacent.

| Package | Registry | Age | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-------------|-----------|-------------|
| `clap` | crates.io | 8+ yrs (v4.6.1) | github.com/clap-rs/clap | unavailable | Approved (registry-verified + named in STACK.md/skill) |
| `thiserror` | crates.io | 6+ yrs (v2.0.18) | github.com/dtolnay/thiserror | unavailable | Approved (registry-verified + named in skill ch.4) |
| `anyhow` | crates.io | 6+ yrs (v1.0.102) | github.com/dtolnay/anyhow | unavailable | Approved (registry-verified + named in skill ch.4) |
| `insta` | crates.io | 6+ yrs (v1.48.0) | github.com/mitsuhiko/insta | unavailable | Approved (registry-verified + named in skill ch.5) |
| `serde` | crates.io | 9+ yrs (v1.0.228) | github.com/serde-rs/serde | unavailable | Approved (registry-verified, ecosystem-standard) |
| `serde_json` | crates.io | 9+ yrs (v1.0.150) | github.com/serde-rs/json | unavailable | Approved (registry-verified, ecosystem-standard) |
| `gin-gonic/gin` | Go proxy | 8+ yrs (v1.12.0) | github.com/gin-gonic/gin | n/a (Go) | Approved (Go proxy verified + D-02 locked) |
| `google/uuid` | Go proxy | (v1.6.0) | github.com/google/uuid | n/a (Go) | Approved (Go proxy verified) |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
**Packages explicitly rejected:** `serde_yaml` (deprecated upstream, not slop — do not add as a direct dep).

*slopcheck was unavailable at research time. Because all packages are ecosystem-standard, registry-verified on authoritative sources, AND independently named in the project's own STACK.md/skill docs, the risk is low. The planner MAY still insert a single `checkpoint:human-verify` confirming the dependency list before the first `cargo build` if it wants belt-and-suspenders; a per-package checkpoint is unnecessary given the provenance.*

## Architecture Patterns

### System Architecture Diagram

This phase builds the **skeleton** of the runtime pipeline (logic arrives in Phases 2–5). Data flow that Phase 1 wires up:

```text
                 ┌───────────────────────── gnr8 (binary crate) ─────────────────────────┐
   user CLI ───► │  clap parse (Cli + global --json/-v)                                   │
   args          │     │                                                                  │
                 │     ▼                                                                   │
                 │  match Commands { Init | Generate | Watch | Check | Inspect{..} |      │
                 │                   Doctor }                                              │
                 │     │  (Phase 1: every arm calls a gnr8_core stub)                      │
                 │     ▼                                                                   │
                 │  gnr8_core::<op>(...) ──► Result<T, gnr8_core::CoreError>               │
                 │     │                                                                   │
                 │     ├─ Ok(report)  ──► render: --json ? serde_json::to_string : table   │
                 │     └─ Err(e)      ──► map to anyhow at main boundary ──► stderr + exit │
                 └─────────────────────────────────┬─────────────────────────────────────┘
                                                   │ depends on (lib)
                 ┌─────────────────────── gnr8-core (library crate) ──────────────────────┐
                 │  CoreError (thiserror enum, incl. NotYetImplemented{phase})            │
                 │  module seams (stubs in Phase 1, filled later):                        │
                 │    analyze::  (Phase 2)   graph::  (Phase 2)                            │
                 │    lower::    (Phase 3)   sdk::    (Phase 3)                            │
                 │    diagnostics:: (Phase 2+)                                             │
                 │  each stub fn returns Err(CoreError::NotYetImplemented{..})             │
                 └────────────────────────────────┬──────────────────────────────────────┘
                                                  │ consumed by
                 ┌──────────────── tests (insta snapshots, red-by-design) ────────────────┐
                 │  assert that <op>(fixture) == expected graph/openapi/sdk/diagnostics    │
                 │  FAIL today (stub errors / missing .snap) ──► turn green in Phase 2–3   │
                 └────────────────────────────────────────────────────────────────────────┘

   fixtures/goalservice/ (separate Go module)  ──► is the INPUT Phase 2's analyzer will read.
       Phase 1 only authors it + its expected/ snapshots; cargo does not compile it.
```

### Recommended Project Structure
```text
gnr8/                                  # repo root (Cargo workspace root)
├── Cargo.toml                         # [workspace] + [workspace.lints] + [workspace.dependencies]
├── Cargo.lock                         # committed (binary project) — needed for --locked clippy
├── Makefile                           # fmt/clippy/test/fixture targets
├── rust-toolchain.toml                # optional: pin channel = "stable" for CI reproducibility
├── .github/workflows/ci.yml           # fmt + clippy + test + go-build
├── docs/
│   └── poc-contract.md                # POC-01/02/03 contract + non-goals (see §PoC Contract)
├── crates/
│   ├── gnr8-core/                     # library crate
│   │   ├── Cargo.toml                 # lints.workspace = true; thiserror, serde; dev: insta
│   │   ├── src/
│   │   │   ├── lib.rs                 # pub mod error; pub use error::CoreError; module seams
│   │   │   ├── error.rs               # CoreError thiserror enum
│   │   │   ├── analyze/mod.rs         # Phase 2 seam — stub fns returning NotYetImplemented
│   │   │   ├── graph/mod.rs           # Phase 2 seam — router-agnostic route-fact types (skeletal)
│   │   │   ├── lower/mod.rs           # Phase 3 seam (OpenAPI lowering)
│   │   │   ├── sdk/mod.rs             # Phase 3 seam (Go SDK gen)
│   │   │   └── diagnostics/mod.rs     # Phase 2+ seam
│   │   └── tests/
│   │       ├── snapshot_graph.rs      # FIX-03: graph snapshot (red-by-design)
│   │       ├── snapshot_openapi.rs    # FIX-03: openapi snapshot (red-by-design)
│   │       ├── snapshot_sdk.rs        # FIX-03: sdk snapshot (red-by-design)
│   │       ├── snapshot_diagnostics.rs# FIX-03: diagnostics snapshot (red-by-design)
│   │       └── snapshots/             # committed .snap files appear here once green
│   └── gnr8/                          # binary crate
│       ├── Cargo.toml                 # lints.workspace = true; clap, serde_json, anyhow; dep gnr8-core
│       └── src/
│           ├── main.rs               # anyhow boundary; parse + dispatch + render + exit codes
│           └── cli.rs                # clap derive structs (Cli, Commands, InspectAction)
└── fixtures/
    └── goalservice/                   # SEPARATE Go module (not in cargo workspace)
        ├── go.mod
        ├── internal/goal/ports/http.go      # routes
        ├── internal/goal/ports/handlers.go  # handlers (mixed inference + annotations)
        ├── internal/common/dto/goal.go      # CreateGoalInput, UpdateGoalInput, GoalResponse, ListGoalsOutput
        ├── internal/common/dto/common.go    # HttpError, CommandMessage, CommandMessageWithUUID, TargetDirection
        └── expected/
            ├── openapi.yaml           # snapshot scaffold: expected OpenAPI 3.1.0
            ├── sdk/                    # snapshot scaffold: expected Go SDK files
            └── diagnostics.txt        # snapshot scaffold: expected warnings
```

> **Layout note (Claude's discretion per D — confirm with planner):** `crates/gnr8-core` + `crates/gnr8` (subdir) vs. flat `gnr8-core/` + `gnr8/` at root are both fine. The `crates/` prefix is the common convention for multi-crate Rust workspaces and keeps the root clean. Either satisfies RUST-01.

### Pattern 1: Workspace `Cargo.toml` with shared lints + deps
**What:** Root manifest declares members, shared lints (D-08), and shared dependency versions. Members inherit with `.workspace = true`.
**When to use:** Always for a multi-crate workspace — single source of truth for versions and lint policy.
**Example:**
```toml
# ./Cargo.toml  (workspace root)
# Source: doc.rust-lang.org/cargo/reference/workspaces.html (CITED)
[workspace]
members = ["crates/gnr8-core", "crates/gnr8"]
resolver = "2"

[workspace.package]
edition = "2021"          # D-08
rust-version = "1.85"     # MSRV floor: clap 4.6 requires 1.85; bump if a dep needs more
license = "MIT"           # match repo LICENSE

# Shared lint policy (skill ch.2 + D-08). priority: higher number wins on conflict.
[workspace.lints.rust]
unsafe_code = "forbid"
unreachable_pub = "warn"
# missing_docs = "warn"   # consider once public API stabilizes (skill suggests #![deny(missing_docs)])

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }     # deny the whole `all` group...
pedantic = { level = "warn", priority = -1 }
# RUST-04 enforcement: ban panicking unwraps/expects in production code.
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "warn"             # allow todo!() during scaffolding but surface it
# tests legitimately use unwrap/expect — re-allow at module scope in test code (see Pattern 4).

[workspace.dependencies]
clap = { version = "4.6", features = ["derive"] }
thiserror = "2.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
insta = { version = "1.48", features = ["yaml"] }
gnr8-core = { path = "crates/gnr8-core" }
```

```toml
# ./crates/gnr8-core/Cargo.toml
[package]
name = "gnr8-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lints]
workspace = true          # inherits [workspace.lints] — Source: cargo workspaces docs (CITED)

[dependencies]
thiserror = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
insta = { workspace = true }
```

```toml
# ./crates/gnr8/Cargo.toml
[package]
name = "gnr8"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "gnr8"
path = "src/main.rs"

[lints]
workspace = true

[dependencies]
gnr8-core = { workspace = true }
clap = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }     # anyhow ONLY here, never in gnr8-core (D-09)
```

### Pattern 2: clap derive CLI with six commands, nested `inspect`, global `--json`
**What:** A `Cli` struct with a global `--json`/`-v` flag and a `Commands` enum; `Inspect` carries a nested `InspectAction` subcommand enum.
**When to use:** The whole RUST-02 surface.
**Example:**
```rust
// crates/gnr8/src/cli.rs
// Source: docs.rs/clap/4.6.1/_derive (CITED) — #[arg(global=true)], #[command(subcommand)]
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "gnr8", version, about = "Code-first OpenAPI + Go SDK generator")]
pub struct Cli {
    /// Emit machine-readable JSON instead of human tables.
    #[arg(long, global = true)]
    pub json: bool,

    /// Increase output detail (-v, -vv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scaffold a project-local .gnr8/ workspace.
    Init,
    /// Generate OpenAPI + Go SDK from Go source.
    Generate,
    /// Watch source and regenerate on change.
    Watch,
    /// Verify generated outputs are up to date.
    Check,
    /// Explain inferred API facts and diagnostics.
    Inspect {
        #[command(subcommand)]
        action: InspectAction,
    },
    /// Summarize unsupported patterns and lifecycle issues.
    Doctor,
}

#[derive(Debug, Subcommand)]
pub enum InspectAction {
    /// Show discovered routes.
    Routes,
    /// Show discovered schemas.
    Schemas,
    /// Show the raw API graph.
    Graph,
}
```

```rust
// crates/gnr8/src/main.rs  — the ONLY anyhow boundary (D-09)
use anyhow::Result;
use clap::Parser;

mod cli;
use cli::{Cli, Commands, InspectAction};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let outcome = dispatch(&cli);           // returns Result<Report, gnr8_core::CoreError>
    match outcome {
        Ok(report) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                report.render_table();       // plain println! is fine for Phase 1
            }
            Ok(())
        }
        // NotYetImplemented is an expected, non-panicking exit — clear message, non-zero code.
        Err(e @ gnr8_core::CoreError::NotYetImplemented { .. }) => {
            eprintln!("gnr8: {e}");          // e.g. "command 'generate' is not yet implemented (arrives in phase 2)"
            std::process::exit(2);
        }
        Err(e) => Err(e.into()),             // real errors bubble through anyhow → stderr + exit 1
    }
}

fn dispatch(cli: &Cli) -> Result<Report, gnr8_core::CoreError> {
    match &cli.command {
        Commands::Init      => gnr8_core::not_yet("init", 4),
        Commands::Generate  => gnr8_core::not_yet("generate", 3),
        Commands::Watch     => gnr8_core::not_yet("watch", 4),
        Commands::Check     => gnr8_core::not_yet("check", 4),
        Commands::Doctor    => gnr8_core::not_yet("doctor", 5),
        Commands::Inspect { action } => match action {
            InspectAction::Routes  => gnr8_core::not_yet("inspect routes", 2),
            InspectAction::Schemas => gnr8_core::not_yet("inspect schemas", 2),
            InspectAction::Graph   => gnr8_core::not_yet("inspect graph", 2),
        },
    }
}
```

> **Skeletal-message design (satisfies D-12 "exit cleanly, don't panic"):** route the "not yet implemented" path through a typed `CoreError::NotYetImplemented { command, phase }` rather than `unimplemented!()`/`panic!` in the binary. The CLI then prints a clean message and exits with a deliberate non-zero code (suggest exit code 2 = "recognized but unavailable"). This keeps `--help`, arg validation, and `--version` fully working today while honoring RUST-04 (no panics).

### Pattern 3: `thiserror` core error + `not_yet` helper (RUST-04)
**What:** A single crate-level error enum in `gnr8-core`, with a `NotYetImplemented` variant used by every Phase-1 stub.
**Example:**
```rust
// crates/gnr8-core/src/error.rs
// Source: skill ch.4 (thiserror for crate errors) + docs.rs/thiserror/2.0
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("'{command}' is not yet implemented (arrives in phase {phase})")]
    NotYetImplemented { command: String, phase: u8 },

    // Real variants added as Phases 2–3 land, e.g.:
    // #[error("failed to load Go packages at {path}: {source}")]
    // PackageLoad { path: String, #[source] source: std::io::Error },
}

// crates/gnr8-core/src/lib.rs
pub mod error;
pub use error::CoreError;

/// Stub used by Phase-1 CLI arms. Returns a typed, non-panicking error.
pub fn not_yet<T>(command: &str, phase: u8) -> Result<T, CoreError> {
    Err(CoreError::NotYetImplemented { command: command.to_string(), phase })
}
```
**Why this shape:** no `unwrap`/`expect`/`panic` anywhere in library paths; the binary's `anyhow` only appears in `main.rs`; clippy's `unwrap_used`/`expect_used`/`panic` denials (Pattern 1) enforce RUST-04 mechanically.

### Pattern 4: insta snapshot test that is RED BY DESIGN (FIX-03/FIX-04)
**What:** An integration test that calls a not-yet-implemented `gnr8-core` function and asserts a snapshot. It fails today (the function errors / the `.snap` doesn't exist) and turns green only when Phases 2–3 implement the function and the snapshot is reviewed/accepted.
**Why it fails clearly (not silently skipped):** GitHub Actions sets the `CI` env var, so insta runs in `INSTA_UPDATE=no` mode — a **missing or mismatched snapshot is a hard test failure**, never an auto-create. `[CITED: insta.rs/docs/advanced]`
**Example:**
```rust
// crates/gnr8-core/tests/snapshot_graph.rs
// Source: skill ch.5 (insta snapshots) + insta.rs/docs/advanced (CI = INSTA_UPDATE=no)
#![allow(clippy::unwrap_used, clippy::expect_used)] // tests may unwrap (skill ch.4 + ch.5)

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn graph_matches_expected_for_goalservice() {
    // Phase 1: build_graph(...) returns Err(NotYetImplemented) → .unwrap() panics → test FAILS (red).
    // Phase 2: build_graph(...) returns the real graph → snapshot compared → turns green on review.
    let graph = gnr8_core::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 2 must implement build_graph; red-by-design until then");
    insta::assert_yaml_snapshot!("goalservice_graph", graph);
}
```
Repeat for `snapshot_openapi.rs` (`lower::to_openapi`), `snapshot_sdk.rs` (`sdk::generate`), `snapshot_diagnostics.rs` (`diagnostics::collect`, plain-text `assert_snapshot!`).

> **Two complementary red-by-design mechanisms — use the first as primary:**
> 1. **Stub returns typed error → test `.expect()` panics → test fails.** Robust, explicit, and the failure message names the responsible phase. This is the recommended primary mechanism and does NOT require a pre-authored `.snap` file. ✅
> 2. **Pre-author `expected/openapi.yaml` etc. + missing inline `.snap`.** insta's CI `no` mode also fails on a missing snapshot, but a stub that errors before reaching `assert_*` is simpler and self-documenting. Keep the `fixtures/.../expected/` scaffolds as the human-readable acceptance target (documented contract), and let mechanism (1) drive the actual test redness.
>
> **Anti-flake guard:** Do NOT use `#[ignore]` on these tests — an ignored test is silently skipped and violates FIX-04 ("fail clearly, not skipped"). Redness must come from a real failing assertion. A CI step `grep -rL '#\[ignore' ...` or a review check should ensure none of the four snapshot tests is ignored.

### Anti-Patterns to Avoid
- **Putting `anyhow` in `gnr8-core`:** violates D-09. `anyhow` lives only in `crates/gnr8/`. Enforce by NOT adding `anyhow` to the core crate's `Cargo.toml`.
- **`unimplemented!()`/`panic!`/`todo!()` in the *binary* dispatch path:** panics print an ugly backtrace and a non-clean exit; D-12 wants a clear message. Use the typed `NotYetImplemented` error path instead. (`todo!()`/stubs returning typed errors inside `gnr8-core` functions are acceptable since the test harness expects them to error.)
- **`#[ignore]` on the red-by-design snapshot tests:** silently skipped = FIX-04 violation. They must FAIL.
- **Baking Gin specifics into graph types** (e.g., a `gin_context` field): violates D-03 router-agnosticism. Graph stores method/path-template/params/request-type/response+status only.
- **Adding `serde_yaml` as a direct dependency:** it's deprecated. Rely on insta's vendored YAML for snapshots.
- **Committing without `Cargo.lock`:** this is a binary project; commit `Cargo.lock` so `clippy --locked` works in CI.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CLI arg parsing, help, subcommands, global flags | Manual `std::env::args` parsing / match trees | `clap` derive 4.6 | Help text, `--version`, validation, nested subcommands, global flags all free; D-10. |
| Library error type + `Display`/`From`/`std::error::Error` | Hand-written `enum` with manual `impl Display`/`From` | `thiserror` 2.0 derive | Boilerplate-free, `?`-friendly, `#[from]`/`#[source]` chaining; skill ch.4. |
| Binary error ergonomics + context | Manual `Box<dyn Error>` plumbing | `anyhow` 1.0 (in `main` only) | Context-rich top-level errors without typed-error overhead; skill ch.4. |
| Golden-output comparison for generated artifacts | Manual `assert_eq!` against giant string constants | `insta` 1.48 snapshots | Reviewable diffs, `.snap` files in git, CI red-on-mismatch; skill ch.5; D-13. |
| Struct serialization for snapshots / `--json` | Manual JSON/YAML string building | `serde` + `serde_json` (+ insta yaml) | Type-driven, stable, no escaping bugs. |
| Go web routing / binding in the fixture | A toy `net/http` server | real `gin-gonic/gin` v1.12 | Fixture must mirror the *real* target (D-14, TARGET-API.md §2). |

**Key insight:** This phase is almost entirely "wire up well-known crates correctly and lay seams," not "solve a hard problem." The only genuinely custom artifact is the **red-by-design test harness** — and even that is just standard insta + a typed-error stub, not new machinery. Resist adding any dependency whose owning phase hasn't arrived (notify, AST tooling, OpenAPI emitter).

## Common Pitfalls

### Pitfall 1: Assuming `thiserror` 1.x API
**What goes wrong:** Training data overwhelmingly references `thiserror = "1.0"`. Current is **2.0.18**. v2 changed some edge behaviors (e.g., stricter handling of `#[from]`/`#[source]` and display-attribute parsing).
**Why it happens:** v2 shipped after many models' cutoffs.
**How to avoid:** Pin `thiserror = "2.0"` in `[workspace.dependencies]`. The basic `#[derive(Error)]` + `#[error("...")]` + `#[from]` patterns used in Pattern 3 are unchanged between v1 and v2, so the examples here are v2-safe.
**Warning signs:** Compile errors referencing removed/renamed attributes — re-check against docs.rs/thiserror/2.0.

### Pitfall 2: clippy `unwrap_used`/`expect_used` firing in test code
**What goes wrong:** Denying `unwrap_used`/`expect_used` workspace-wide (correct for RUST-04) also flags tests, where `unwrap`/`expect` are idiomatic (skill ch.4 explicitly permits them in tests).
**Why it happens:** `--all-targets` lints tests too.
**How to avoid:** Add `#![allow(clippy::unwrap_used, clippy::expect_used)]` at the top of each `tests/*.rs` file (as in Pattern 4), or scope it to `#[cfg(test)] mod` blocks. Do NOT weaken the workspace policy.
**Warning signs:** `cargo clippy --all-targets` fails only on test files.

### Pitfall 3: insta auto-creating snapshots locally hides the redness
**What goes wrong:** Running `cargo insta test --accept` or `INSTA_UPDATE=always` locally would auto-write the missing `.snap`, accidentally turning a red-by-design test green with empty/wrong content.
**Why it happens:** Developer convenience commands write snapshots.
**How to avoid:** The primary mechanism (stub returns `Err`, test `.expect()` panics *before* `assert_*`) cannot be silenced by snapshot acceptance — the panic happens first. Reserve `cargo insta accept` for Phases 2–3 once the function is implemented. CI uses plain `cargo test` (insta `no` mode), which never auto-creates.
**Warning signs:** A snapshot test passes in Phase 1 — it should not.

### Pitfall 4: Missing `Cargo.lock` breaks `clippy --locked` in CI
**What goes wrong:** D-16's clippy invocation uses `--locked`, which fails if `Cargo.lock` is absent or stale.
**Why it happens:** Lockfiles are sometimes gitignored out of library habit; gnr8 is a binary project.
**How to avoid:** Commit `Cargo.lock`. Ensure `.gitignore` does NOT ignore it (the existing repo `.gitignore` should be checked — see Environment/Open Questions).
**Warning signs:** CI clippy step: "the lock file ... needs to be updated but --locked was passed."

### Pitfall 5: Go fixture compilation not gated in CI
**What goes wrong:** The Go fixture can rot (won't compile) without anyone noticing, because cargo doesn't build it.
**Why it happens:** It's a separate Go module outside the cargo workspace.
**How to avoid:** Add a dedicated CI job (and Makefile target) that runs `go build ./...` (and `go vet ./...`) inside `fixtures/goalservice/` with `setup-go`. See Quality Gates.
**Warning signs:** Phase 2 starts and the "realistic fixture" doesn't compile.

### Pitfall 6: `inspect` with no subcommand
**What goes wrong:** `gnr8 inspect` (no `routes|schemas|graph`) — by default clap requires the nested subcommand and errors. That's usually desired, but confirm the UX.
**How to avoid:** Default behavior (error with usage) is acceptable for RUST-02. If a default like "show help" is wanted, add `#[command(subcommand_required = true, arg_required_else_help = true)]` on the `Inspect` variant. Planner's call; document the chosen behavior.

## Code Examples

### Makefile (D-16 + Go fixture gate)
```makefile
# Source: skill ch.2 (clippy invocation) + D-16
.PHONY: fmt fmt-check clippy test check fixture-build all

fmt:
	cargo fmt

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets --all-features --locked -- -D warnings

test:
	cargo test --all-features

fixture-build:
	cd fixtures/goalservice && go build ./... && go vet ./...

# Full local gate, mirrors CI.
check: fmt-check clippy test fixture-build

all: check
```

### GitHub Actions CI (RUST-03 + fixture compile)
```yaml
# .github/workflows/ci.yml
# Source: skill ch.2 invocation + D-16; GitHub Actions sets CI=true → insta runs in `no` mode.
name: ci
on:
  push: { branches: [main] }
  pull_request:
jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2          # cache target/ + registry
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features --locked -- -D warnings
      - run: cargo test --all-features         # insta red-by-design tests run here and FAIL (expected in Phase 1)
  go-fixture:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with: { go-version: '1.26' }
      - run: cd fixtures/goalservice && go build ./... && go vet ./...
```

> **⚠ Phase-1 CI reality (flag for planner):** the four red-by-design snapshot tests (D-15/FIX-04) make `cargo test` **fail on purpose** until Phase 2/3. A green `main` CI and red-by-design tests are in tension. Options the planner must pick from (see Open Questions Q1): (a) gate the four red tests behind a `#[cfg_attr(not(feature = "expected-green"), ignore)]`-style mechanism — **rejected**, that's the forbidden `#[ignore]` skip; (b) run the red tests in a **separate, allowed-to-fail CI job** (`continue-on-error: true`) so they're visible-red but don't block; (c) accept a red `main` for Phases 1–2 and rely on local `cargo test` to show the contract; (d) split: the fmt/clippy/compile gates are the *blocking* CI (RUST-03), and the red snapshot suite runs as a **non-blocking "contract" job** that flips to blocking once Phase 3 completes. **Recommendation: option (d)** — it satisfies RUST-03 (gates pass) and FIX-04 (tests visibly fail, not skipped) simultaneously.

### `.gitignore` additions (Rust)
```gitignore
# Rust
/target
# Do NOT ignore Cargo.lock — binary project needs it committed for clippy --locked.
# insta leftovers:
*.snap.new
*.pending-snap
```

## Go Gin Fixture Module — Concrete Shape (FIX-01/FIX-02, D-14)

Faithful reduction of TARGET-API.md §6. One Gin module, two resources, exercising every extraction concern. Authoring guidance for the planner (the fixture is real, compilable Go):

**Routes (`internal/goal/ports/http.go`):** a `Router.Group("/goal")` with `api.Use(h.AuthMiddleware)` (security on the whole group), registering:
- `POST /goal/` → `createGoal` (body `CreateGoalInput`, `201 CommandMessageWithUUID`, `400 HttpError`)
- `GET /goal/list` → `listGoals` (response `ListGoalsOutput`; query params `cursor`, `page_size`, `aggregation` w/ `Enums(...)` annotation)
- `PUT /goal/:uuid` → `updateGoal` (path param `uuid`, full swaggo annotation block, body `UpdateGoalInput`, `200/400/404`)
- `DELETE /goal/:uuid` → `deleteGoal` (path param `uuid`, `200 CommandMessage`)

**DTOs to include (drives FIX-02 coverage):**

| DTO feature | Where in fixture | Maps to |
|-------------|------------------|---------|
| `json:"name"` tag (name ≠ Go field) | every struct field | schema property naming |
| `binding:"required"` | `CreateGoalInput.Name`, `.AnalyticsQuery` | OpenAPI `required` list |
| optional pointer + `,omitempty` | `*float64 TargetValue`, `*TargetDirection TargetDirection` | optional fields |
| nested struct | `GoalAnalyticsQuery` field on `CreateGoalInput` | `$ref` |
| embedded/composed struct | `CommandMessageWithUUID` embeds `CommandMessage` | flattened/promoted fields |
| array of well-known type | `[]uuid.UUID WorkflowChainIDs` | `array` items `uuid` |
| enum newtype | `type TargetDirection string` + `const (gte, lte)` | string enum inference |
| well-known types | `uuid.UUID`, `time.Time` (add a `CreatedAt time.Time` on `GoalResponse`) | type-mapping table (TARGET-API §4) |
| `example:`/`description:` tags | `HttpError`, `CreateGoalInput` | schema metadata (polish) |
| error response struct | `HttpError` returned on 400/404 | error responses |
| **unsupported pattern (≥1, FIX-02)** | a `Metadata map[string]any` field on a DTO **and/or** a handler that builds a response dynamically | diagnostics: free-form map → `additionalProperties: true` + warning (TARGET-API §5.1) |
| precision-loss trigger | the `*float64 TargetValue` | `float64→float32` narrowing diagnostic (TARGET-API §5.2) |
| query-param-without-struct | `c.Query("cursor")` with no typed struct | "param type unknown" diagnostic (TARGET-API §5.4) |
| annotation escape hatch | swaggo comment block on `updateGoal` / query params on `listGoals` | comment-as-escape-hatch path |

**`expected/` scaffolding (the documented acceptance target, D-15):**
- `expected/openapi.yaml` — the OpenAPI 3.1.0 document gnr8 must eventually produce for this service. In Phase 1 it is authored by hand as the *target*, kept small. (Whether the insta test reads this file or uses an inline snapshot is the planner's call; recommended: keep it as the human-readable contract and let insta's own `.snap` be the test oracle once green — see Pattern 4 note.)
- `expected/sdk/` — sketches of the expected Go SDK files: a `client.go` (Client + functional options), `goals.go` (tag-grouped typed ops with `ctx context.Context` first arg), `models.go` (request/response structs), `errors.go` (typed API error). Per D-05. These are the SDK-shape contract; the compiling SDK is Phase 3.
- `expected/diagnostics.txt` — expected warnings: `float64→float32 narrowing`, `untyped query param 'cursor'`, `free-form map field 'Metadata'`. Plain text for `assert_snapshot!`.

> Note: the `expected/` directory documents intent; the *enforcing* tests live in `crates/gnr8-core/tests/` and are red-by-design (Pattern 4). Keep both in sync — the `expected/` files are the reviewable spec, the `.snap` files become the machine oracle in Phases 2–3.

## PoC Contract Documentation (POC-01/02/03)

**Where to record it:** a single committed `docs/poc-contract.md` (committed because `commit_docs: true`). This is the "documented before implementation expands" artifact POC-02 demands.

**It must contain:**
1. **Scope lock (POC-01):** Go source → OpenAPI → Go SDK. Nothing else this milestone. (verbatim D-01)
2. **Locked surface (POC-02):**
   - Router family: **Gin** only; recognized patterns = `Router.Group` + `group.METHOD(path, handler)`, `ShouldBindJSON`, `c.Param`/`c.Query` (D-02).
   - OpenAPI target: **3.1.0** (D-04).
   - Go SDK shape: **Client + functional options + tag-grouped typed ops + model structs + typed API error + `context.Context`-first** (D-05).
   - `.gnr8/` layout: checked-in **code-as-config** dir + git-ignored **cache/output lifecycle** dir; detailed surface deferred to Phase 4 (D-06).
3. **Non-goals (POC-03):** copy the REQUIREMENTS.md "Out of Scope" table — dynamic plugins, macro-heavy config API, graph database, full framework coverage, multi-language source/SDK, OpenAPI 3.2 full coverage, arbitrary handler-body interpretation, wrapping existing generators. State that any of these entering the PoC requires an explicit roadmap update.

**Relationship to extraction generality (D-03):** the contract doc should explicitly state that although Gin is the only *recognized* router, the internal graph stores router-agnostic HTTP route facts so chi/echo/net-http can be added later without reshaping — this is the design seam, not a feature.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `thiserror = "1.0"` | `thiserror = "2.0"` | v2 GA (2024) | Use `2.0`; basic derive patterns unchanged. |
| `insta` depends on `serde_yaml` for YAML | `insta` vendors its own YAML emitter | insta ~1.40+ (current 1.48) | YAML snapshots no longer pull the deprecated `serde_yaml`. |
| `serde_yaml` for YAML output | `serde_yaml` **deprecated** (0.9.34+deprecated, last release 2024-03-25) | 2024 | Do not add as a direct dep; if YAML *output* needed later, evaluate `serde_norway`/hand-roll. |
| `clap` v3 / builder-first | `clap` v4 derive | v4 (2022); current 4.6.1 | Derive is idiomatic; MSRV now 1.85. |
| Lints declared per-crate | `[workspace.lints]` + `[lints] workspace = true` | Cargo 1.74+ | Single source of lint policy (D-08). |

**Deprecated/outdated:**
- `serde_yaml`: deprecated upstream. Avoid as a direct dependency.
- `clap` builder-only patterns for static command trees: superseded by derive for this use case.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `crates/` subdirectory layout (vs flat root crates) is preferred | Project Structure | Cosmetic; either satisfies RUST-01. Planner/Claude discretion per D. |
| A2 | Exit code `2` for `NotYetImplemented` skeletal commands | clap Pattern 2 | Low; any documented non-zero code works. Confirm desired convention. |
| A3 | MSRV floor of `1.85` (driven by clap 4.6) | Workspace Cargo.toml | Low; local toolchain is 1.96. If a future dep needs higher, bump. clap MSRV verified = 1.85. |
| A4 | Skeletal-message wording "not yet implemented (arrives in phase N)" | clap Pattern 2 | None; D explicitly leaves wording to execution. |
| A5 | CI should treat the red-by-design suite as a non-blocking job (option d) | Quality Gates / Open Q1 | Medium — this is a real workflow decision; planner must choose. Wrong choice either blocks `main` forever (Phase 1–2) or hides the contract. |
| A6 | OpenAPI snapshots use insta `yaml`; diagnostics use plain-text `assert_snapshot!` | insta Harness | Low; snapshot format is reviewable-diff preference, not correctness. |
| A7 | Existing repo `.gitignore` does not (or must not) ignore `Cargo.lock` | Pitfall 4 | Medium if it does — would silently break `clippy --locked` in CI. Verify (Open Q2). |

**If this table is empty:** N/A — assumptions exist and are flagged above for the planner/discuss-phase.

## Open Questions

1. **How should CI reconcile RUST-03 (gates must pass) with FIX-04 (snapshot tests must visibly fail)?**
   - What we know: GitHub Actions sets `CI=true` → insta `no` mode → missing/mismatched snapshot = hard failure; `#[ignore]` is forbidden (silent skip violates FIX-04).
   - What's unclear: whether `main` CI is allowed to be red during Phases 1–2.
   - Recommendation: **Option (d)** — blocking job runs `fmt`/`clippy`/`go build`/`go vet` + any genuinely-green unit tests; a separate **non-blocking "contract" job** runs the four red-by-design snapshot tests (visible red, doesn't block merges) and is promoted to blocking when Phase 3 completes. Document this in `docs/poc-contract.md`.

2. **Does the existing `.gitignore` ignore `Cargo.lock`?** (Needs a one-line check during planning/execution.)
   - What we know: gnr8 is a binary project; `clippy --locked` (D-16) needs a committed lockfile.
   - Recommendation: ensure `Cargo.lock` is committed and not ignored; add `/target`, `*.snap.new`, `*.pending-snap` to `.gitignore`.

3. **Flat-root vs `crates/` workspace layout** (A1) — purely cosmetic; planner to pick. No requirement impact.

4. **Should `inspect` with no subcommand show help or error?** (Pitfall 6) — default clap behavior (error+usage) is acceptable; if "show help" is desired, add `arg_required_else_help = true`. Planner to decide and document.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo / rustc | All Rust scaffolding (RUST-01..04) | ✓ | 1.96.0 (2026-05-25) | — |
| clippy | RUST-03 gate | ✓ | 0.1.96 | `rustup component add clippy` |
| rustfmt | RUST-03 gate | ✓ | 1.9.0-stable | `rustup component add rustfmt` |
| go | Fixture build + CI go-fixture job (FIX-01) | ✓ | 1.26.2 darwin/arm64 (≥ gin's 1.25 min) | — |
| gofmt / go vet | Fixture quality | ✓ | bundled with Go | — |
| gh | CI/PR ops (not needed to author CI) | ✓ | 2.92.0 | — |
| cargo-insta (CLI) | Dev convenience (`insta review`) | ✗ | — | `cargo install cargo-insta`; **not required** — CI uses plain `cargo test`. |
| slopcheck | Package legitimacy audit | ✗ | — | Provenance via crates.io sparse index + STACK.md/skill (see Audit). |

**Missing dependencies with no fallback:** none. The full Rust + Go toolchain is present and current.
**Missing dependencies with fallback:** `cargo-insta` (optional dev CLI — install on demand; CI does not need it); `slopcheck` (mitigated by authoritative-registry + project-doc provenance).

## Validation Architecture

> `workflow.nyquist_validation: true` → section included.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`#[test]`) + `insta` 1.48 (snapshots) |
| Config file | none (cargo convention); `insta` settings via env (`CI` → `INSTA_UPDATE=no`) |
| Quick run command | `cargo test -p gnr8-core --lib` (fast unit tests: error/CLI-stub behavior) |
| Full suite command | `cargo test --all-features` (includes the four red-by-design snapshot integration tests) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| RUST-02 | CLI parses all six commands + `inspect` subcommands; `--help`/`--version` work | unit | `cargo test -p gnr8 cli_` (use `Cli::try_parse_from`) | ❌ Wave 0 |
| RUST-02 | Skeletal command exits cleanly (non-panicking) with "not yet implemented" | unit | `cargo test -p gnr8-core not_yet_returns_typed_error` | ❌ Wave 0 |
| RUST-04 | `CoreError::NotYetImplemented` Display message is correct | unit | `cargo test -p gnr8-core core_error_` | ❌ Wave 0 |
| RUST-03 | fmt/clippy/test gates pass (the non-red-by-design parts) | gate | `make check` (minus red suite) / CI blocking job | ❌ Wave 0 |
| FIX-01/02 | Go fixture compiles and vets | gate | `cd fixtures/goalservice && go build ./... && go vet ./...` | ❌ Wave 0 |
| FIX-03 | Graph snapshot defined | snapshot (red) | `cargo test -p gnr8-core --test snapshot_graph` | ❌ Wave 0 |
| FIX-03 | OpenAPI snapshot defined | snapshot (red) | `cargo test -p gnr8-core --test snapshot_openapi` | ❌ Wave 0 |
| FIX-03 | SDK snapshot defined | snapshot (red) | `cargo test -p gnr8-core --test snapshot_sdk` | ❌ Wave 0 |
| FIX-03 | Diagnostics snapshot defined | snapshot (red) | `cargo test -p gnr8-core --test snapshot_diagnostics` | ❌ Wave 0 |
| FIX-04 | The four snapshot tests FAIL clearly today (not skipped) | meta | `cargo test --test snapshot_graph 2>&1 | grep -q FAILED` (and assert none are `#[ignore]`) | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo fmt --all -- --check && cargo clippy --all-targets --all-features --locked -- -D warnings && cargo test -p gnr8-core --lib`
- **Per wave merge:** `make check` (fmt + clippy + full test + go fixture build)
- **Phase gate:** fmt/clippy/go-build green; the four red-by-design snapshot tests confirmed visibly-red (expected) and NOT `#[ignore]`d, before `/gsd:verify-work`.

### Wave 0 Gaps
- [ ] `crates/gnr8-core/src/error.rs` + `lib.rs` — `CoreError` + `not_yet` (RUST-04)
- [ ] `crates/gnr8-core/tests/snapshot_graph.rs` — red-by-design (FIX-03/04)
- [ ] `crates/gnr8-core/tests/snapshot_openapi.rs` — red-by-design (FIX-03/04)
- [ ] `crates/gnr8-core/tests/snapshot_sdk.rs` — red-by-design (FIX-03/04)
- [ ] `crates/gnr8-core/tests/snapshot_diagnostics.rs` — red-by-design (FIX-03/04)
- [ ] `crates/gnr8/src/cli.rs` + `main.rs` — clap derive surface + dispatch (RUST-02)
- [ ] CLI parse tests via `Cli::try_parse_from(["gnr8","inspect","routes"])` (RUST-02)
- [ ] `fixtures/goalservice/**` Go module (FIX-01/02) + `expected/` scaffolds (D-15)
- [ ] Framework install: none needed — Rust test harness + insta cover all phase requirements; only add `insta` dev-dependency.

## Security Domain

> `security_enforcement: true`, ASVS level 1. This is a developer CLI scaffolding + fixture phase with **no network listener, no auth surface, no persisted secrets, no user-data processing at runtime** in the gnr8 tool itself. ASVS applicability is therefore minimal; recorded for completeness.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | gnr8 CLI has no auth; the *fixture's* `AuthMiddleware` is mock Go code that never runs in production. |
| V3 Session Management | no | No sessions. |
| V4 Access Control | no | No multi-user surface; CLI runs with the invoking user's permissions. |
| V5 Input Validation | partial (low) | CLI inputs are parsed/validated by clap; future file-path inputs (Phase 2) must be validated then. Phase 1: clap arg validation suffices. |
| V6 Cryptography | no | No crypto in Phase 1. |
| V12 Files & Resources | partial (low) | Phase 1 reads `fixtures/` paths in tests only via `CARGO_MANIFEST_DIR`-relative constants (no user-controlled path traversal). Future `gnr8 generate` reading arbitrary source paths is a Phase 2/4 concern. |
| V14 Configuration | partial (low) | Supply-chain: pin dependency versions (done), commit `Cargo.lock`, prefer `[workspace.dependencies]` single-source. `unsafe_code = "forbid"` set. |

### Known Threat Patterns for {Rust CLI + Go fixture}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Supply-chain (typosquat/slop dependency) | Tampering | Versions verified on authoritative crates.io sparse index + Go proxy; deps named in project STACK.md/skill; commit `Cargo.lock`; consider `cargo-deny`/`cargo audit` in a later hardening phase (not required Phase 1). |
| `unsafe` memory bugs | Tampering/EoP | `unsafe_code = "forbid"` in `[workspace.lints.rust]`. |
| Panic-based DoS / unclear failure | DoS | RUST-04: no prod `unwrap`/`expect`/`panic`; clippy denies them; typed `CoreError`. |
| Malicious postinstall / build script | Tampering | Rust deps here have no build scripts of concern; Go `go build` runs no remote code beyond pinned modules. |
| Path traversal via user-supplied source dir | Tampering/Info-disclosure | Not exposed in Phase 1 (paths are `CARGO_MANIFEST_DIR`-relative constants). Flag for Phase 2 when `generate`/`inspect` accept user paths. |

**Security verdict for Phase 1:** No high/critical ASVS controls are triggered by this scaffolding phase. The only actionable Phase-1 security items are supply-chain hygiene (pinned versions ✓, committed lockfile, `unsafe_code = "forbid"` ✓) and the no-panic guarantee (clippy lints ✓) — all already part of the recommended setup. A deeper dependency-audit gate (`cargo audit`/`cargo deny`) is reasonable but belongs to Phase 5 hardening, not Phase 1.

## Sources

### Primary (HIGH confidence)
- crates.io sparse index (`https://index.crates.io/...`) — authoritative version data for clap 4.6.1, thiserror 2.0.18, anyhow 1.0.102, insta 1.48.0, serde 1.0.228, serde_json 1.0.150, serde_yaml 0.9.34+deprecated; insta dep graph (no serde_yaml).
- Go module proxy (`https://proxy.golang.org/...`) — gin-gonic/gin v1.12.0 (requires Go 1.25), google/uuid v1.6.0.
- Local toolchain probe — cargo/rustc 1.96.0, clippy 0.1.96, rustfmt 1.9.0, Go 1.26.2.
- `docs.rs/clap/4.6.1/clap/_derive` — global flags (`#[arg(global = true)]`), nested subcommands (`#[command(subcommand)]`).
- `doc.rust-lang.org/cargo/reference/workspaces.html` — `[workspace.lints]`/`[lints] workspace = true`, `[workspace.dependencies]` inheritance.
- `insta.rs/docs/advanced/` — CI behavior: `INSTA_UPDATE=auto` → `no` in CI; missing snapshot fails the test (red-by-design mechanism).
- Project canon: `.planning/phases/01-foundation-and-fixtures/01-CONTEXT.md` (D-01..D-16), `.planning/research/TARGET-API.md` (§4 type map, §5 pitfalls, §6 fixture plan), `.planning/REQUIREMENTS.md`, `.planning/ROADMAP.md`, `.planning/PROJECT.md`.
- `thoughts/skills/rust-best-practices/` — SKILL.md + chapters 2 (clippy/workspace lints), 4 (thiserror/anyhow boundaries), 5 (insta snapshots, test naming).

### Secondary (MEDIUM confidence)
- `insta.rs/docs/quickstart` — `cargo install cargo-insta`, YAML snapshot recommendation (cross-checked with skill ch.5).

### Tertiary (LOW confidence)
- None relied upon. (`serde_norway` as a future YAML-output alternative is noted but not recommended for Phase 1.)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all versions verified on authoritative registries (crates.io sparse index, Go proxy) and independently named in project STACK.md/skill.
- Architecture: HIGH — workspace lints/deps inheritance and clap derive patterns confirmed against official Cargo + clap docs; error/test patterns drawn directly from the project's own vendored skill.
- Pitfalls: HIGH — thiserror v2 / serde_yaml deprecation / insta CI behavior / `--locked` lockfile all verified against primary sources.

**Research date:** 2026-06-24
**Valid until:** 2026-07-24 (30 days; stable toolchain/crates). Re-verify `clap`, `thiserror`, `insta` patch versions and `notify` (currently RC) before Phase 4.
