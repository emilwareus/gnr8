# Research: the `.gnr8` boundary is not a boundary — a thin SDK + host-owned engine

Date: 2026-08-27 · Branch base: `origin/main` @ `be37c4c` · Workspace version `0.8.0`
(`Cargo.toml:8`)

Question:

> `gnr8 generate` compiles and runs a project-local Rust crate. What does that crate actually link,
> what does that cost, and can the user-authored Rust be moved behind a genuinely thin SDK while the
> heavy analysis/generation engine stays in the prebuilt host?

Everything under **Verified** was read in this checkout or measured on this machine. Everything under
**Recommendation** / **Open** is judgement, not measurement.

---

## 1. Verified: how the current execution path works

### 1.1 The four commands all funnel into one child process

| Command | Entry | Child call |
|---|---|---|
| `gnr8 generate` | `crates/gnr8/src/main.rs:340` `run_generate` | `child::run_child(&root, "__emit")` (`main.rs:360`) |
| `gnr8 check` | `crates/gnr8/src/main.rs:452` `run_check` | `child::run_child(&root, "__emit")` (`main.rs:472`) |
| `gnr8 watch` | `crates/gnr8/src/watch.rs:374` `regenerate_once` | `child::run_child(project_root, "__emit")` (`watch.rs:375`) |
| `gnr8 inspect <no path>` | `crates/gnr8/src/main.rs:2403` `inspect_graph` | `child::inspect_child(&root)` → `__inspect` (`child.rs:170`) |
| `gnr8 inspect <path>` | `main.rs:2405` | none — direct `gnr8::analyze::build_graph` |
| `gnr8 doctor` | `main.rs:2260` | `child::run_child` via `collect_sdk_readiness` |

`child::run_child` (`crates/gnr8/src/child.rs:62`) does exactly one thing:

```
cargo run --quiet --manifest-path <root>/.gnr8/Cargo.toml -- __emit
```

with `current_dir = project_root` (`child.rs:290-303`), then parses the child's **entire stdout** as
one `ArtifactBundle` JSON document (`child.rs:219`).

### 1.2 The child links the whole engine

`.gnr8/Cargo.toml` as scaffolded by `gnr8 init` (`crates/gnr8-core/src/workspace/mod.rs:277`) has one
dependency: the `gnr8` package, which **is** `crates/gnr8-core` (`crates/gnr8-core/Cargo.toml:2`,
`Cargo.toml:76`). Every example matches — e.g. `examples/bookstore/.gnr8/Cargo.toml:8`.

`crates/gnr8-core/src/lib.rs:39-53` exports `analyze`, `diagnostics`, `gosdk`, `graph`, `lifecycle`,
`lower`, `manifest`, `pysdk`, `resource`, `runner`, `sdk`, `tssdk`, `workspace`. That is 53,454 lines
of Rust (`find crates/gnr8-core/src -name '*.rs' | xargs wc -l`), including:

- `sdk/builtins.rs` — 9,845 lines
- `tssdk/emit.rs` — 5,414 · `gosdk/emit.rs` — 4,847 · `pysdk/emit.rs` — 4,696
- `sdk/openapi_source.rs` — 4,464 · `lifecycle/mod.rs` — 4,131 · `lower/mod.rs` — 2,483

…all of which is compiled into the project-local `.gnr8` crate whether or not the pipeline uses it.
The dependency closure is 59 crates (`cargo tree --manifest-path .gnr8/Cargo.toml`), including
`noyalib`, `cap-std`, `cap-fs-ext`, `rustix`, `fs2`, `same-file`, `blake3`, `toml`,
`unicode-normalization`, `unicode-casefold` — every one of them a *host* concern (OpenAPI parsing,
capability-relative filesystem writes, output locks, path identity).

So the existing "boundary" is a JSON document handed between two processes that link **the same
library**. The child is not a plugin against a stable SDK; it is a second copy of the engine that
happens to `println!` its result.

### 1.3 Measured cost of that choice

All numbers measured on this machine (Linux aarch64 sandbox, `cargo 1.97.1`, warm crates.io cache,
`--offline`, dev profile). Commands and raw values:

| Measurement | Command | Result |
|---|---|---|
| Cold build of the `.gnr8` child | `cargo build --offline --manifest-path .gnr8/Cargo.toml --target-dir <fresh>` | **19.4 s**, 60 compile units, **561 MB** target dir |
| `gnr8-core` alone, deps prebuilt | `cargo clean -p gnr8` then rebuild | **12.5 s** |
| Child rebuild after a `gnr8-core` touch | `touch crates/gnr8-core/src/lib.rs` + rebuild | 2.0 s |
| Child rebuild after a `.gnr8/src/main.rs` touch | `touch` + rebuild | 0.44 s |
| No-op `cargo build` of the child | rebuild, nothing changed | 0.10–0.11 s |
| Cold `gnr8 generate` (bookstore, `.gnr8/target` removed) | `gnr8 generate -v` | **33.0 s** total, pipeline 33.0 s |
| Warm `gnr8 generate` (bookstore) | `gnr8 generate --json` ×3 | 0.30 / 0.27 / 0.31 s |
| Warm `gnr8 check` (bookstore) | `gnr8 check --json` | 0.28 s |
| Host release build | `cargo build --release -p gnr8-cli --offline` | 37.9 s |

**The dominant single cost is `gnr8-core`: 12.5 s of the 19.4 s cold child build.** The remaining
6.9 s is its 58-crate dependency closure.

Projected floor for a thin SDK, measured directly with scratch crates:

| Dependency set | Cold build | Units |
|---|---|---|
| `serde` (derive) + `serde_json` | 4.7 s | 12 |
| + `blake3` | 4.5 s | 21 (blake3 compiles in parallel — no wall-clock cost) |
| + `unicode-normalization` + `unicode-casefold` | 5.4 s | 25 |

So `serde` is the floor (~4.7 s), `blake3` is free in wall-clock, and the two Unicode crates cost
~0.8 s.

### 1.4 The wire format today

`crates/gnr8-core/src/runner/mod.rs:33` — `PROTOCOL_VERSION: u32 = 5`. The child prints one
`ArtifactBundle` (`runner/mod.rs:66-129`) on stdout: artifacts, diagnostics, output anchors,
readiness targets, and **eleven cache fields** (`cache_input_roots`, `cache_input_stamps`,
`cache_config_stamps`, `cache_config_complete`, `cache_pipeline_stamps`, `cache_pipeline_roots`,
`cache_pipeline_complete`, `cache_tool_stamps`, `artifact_cache_key`, …).

Handshake: three environment variables carried host → child (`runner/mod.rs:38-40`,
`child.rs:352-365`) — `GNR8_HOST_PROTOCOL_VERSION`, `GNR8_HOST_CLI_VERSION`,
`GNR8_HOST_CAPABILITY_FINGERPRINT` — validated in `validate_host_handshake`
(`runner/mod.rs:174-221`). If **none** of the three is set the child accepts the run anyway
(`runner/mod.rs:180-183`), so a direct `cargo run -- __emit` is unauthenticated by design.

Framing: none. The child's stdout is trimmed and fed to `serde_json::from_str`
(`child.rs:220`). There is no length prefix, no digest, no size bound, and no timeout —
`Command::output()` blocks until the child exits (`child.rs:189`).

### 1.5 Two whole subsystems are unreachable

- `crates/gnr8-core/src/runner/mod.rs:35` — `pub const PRE_CHILD_NOOP_SUPPORTED: bool = false`.
  Every entry point into the pre-child no-op returns early on it (`crates/gnr8/src/main.rs:737`,
  `:801`, `:907`). No `VerifiedNoopStamp` is ever written, therefore none is ever read. The
  ~400-line stamp subsystem in `main.rs:668-1240` is dead code, and `run_generate` reports
  `hot no-op check: 0.0 ms` on every run (observed).
- `crates/gnr8-core/src/sdk/mod.rs:54` — `const ARTIFACT_CACHE_SUPPORTED: bool = false`, so
  `run_with_cache` computes `cache_key = None` unconditionally (`sdk/mod.rs:983`). Consequently
  `bundle.artifact_cache_key` is always `None`, so `cached_artifact_metadata`
  (`crates/gnr8/src/main.rs:637`), `ensure_bundle_artifacts` (`:645`), `verified_noop_outcome`
  (`:819`), `gnr8::lifecycle::plan_only_cached` and `regenerate_cached_with_anchors` are all
  unreachable in production.

The stated reason is in the constants' own doc comments: *"arbitrary Rust used to construct a
pipeline may read non-file runtime inputs that a host cannot prove unchanged."* That reasoning is
correct for the current design, where the host cannot see the pipeline at all until the child has
already run.

**Consequence: every `gnr8 generate` / `check` / `watch` tick invokes `cargo`.** Measured: 0.10 s of
the 0.30 s warm generate is `cargo`'s own no-op check.

### 1.6 What the child needs from the host today

Because extraction runs *in the child*, the host must hand it the sidecar resource root:
`GNR8_RESOURCE_DIR` (`crates/gnr8-core/src/resource.rs:22`, set at `child.rs:353`). The child then
spawns `go` / `python3` / `node` itself (`sdk/builtins.rs:112-170` for `GoGin`). So the project-local
crate is not just linked against the engine, it *drives the toolchains*.

### 1.7 The composition surface users actually write

`crates/gnr8-core/src/sdk/mod.rs:1767-1795` (`prelude`) exports 4 traits, `Pipeline`, `Cx`,
`Artifact(s)`, and 40+ built-in stage types. The built-ins are, without exception, **plain data
structs with builder methods plus one trait `impl` that calls into the engine**:

- `GoGin` (`builtins.rs:51`) → `impl Source` at `:112` → `crate::analyze::build_go_graph_*`
- `OpenApi31` (`:4575`) → `impl Target` at `:4611` → `crate::lower::build_openapi_doc`
- `GoSdk` (`:4850`) → `impl Target` at `:4963` → `crate::gosdk::generate`
- `SetTitle` (`:733`) → `impl Transform` at `:747` → `ir.title = …`

`grep 'dyn Fn|Box<dyn|Rc<|Arc<|fn('` over `builtins.rs` returns nothing: **no built-in holds a
closure or trait object.** Every one is serializable data. This is the single most important
structural fact for the redesign.

Custom stages are real and shipped: `examples/taskflow/.gnr8/src/main.rs` defines
`DropDebugRoutes: Transform` and `ApiMarkdown: Target`, mixed freely with built-ins.

### 1.8 Repository gates that constrain any change

- `Makefile:gates` — `cargo test -p gnr8`, `-p gnr8-cli`, plus 13 named contract/determinism tests.
- `Makefile:examples-check` — builds the release host and runs `generate --force` + `check` in all
  five examples, then `diff -ru` against a pre-run copy. This is the byte-identical determinism gate.
- `Makefile:invariants` → `scripts/check-invariants.sh`. Note `check_rule "compat/legacy/brownfield
  vocabulary in identifiers"` forbids declarations matching
  `(fn|mod|struct|enum|trait|const) [a-zA-Z_]*(compat|legacy|brownfield|migration|baseline)` and CLI
  flags `--(compat|legacy|migration|baseline)`. `thoughts/` is out of scope; `crates`, `docs`,
  `examples`, `Cargo.toml`, `Cargo.lock`, `.github` are in scope.
- `scripts/check-ci-budget.py` — every CI job must declare `timeout-minutes: <= 5` and no workflow
  may run `make check`.
- `Cargo.toml:27` — `unsafe_code = "forbid"` workspace-wide. **This rules out `pre_exec`**, so a
  POSIX process group cannot be created for the worker; only the direct child can be killed.
- `Cargo.toml:33-37` — `unwrap_used`/`expect_used`/`panic` are `deny` in production code.

---

## 2. Verified: where the boundary should be

The engine splits cleanly along one line: **who needs a toolchain or an emitter**.

| Concern | Needs | Belongs |
|---|---|---|
| `analyze/` (goextract/pyextract/tsextract drivers) | `go`, `python3`, `node`, resource dir | host |
| `sdk/openapi_source.rs` (`OpenApi` source) | `noyalib` | host |
| `lower/` (OpenAPI 3.1 YAML/JSON) | — | host (4,176 lines) |
| `gosdk/`, `pysdk/`, `tssdk/` | `gofmt` subprocess | host (15,000+ lines) |
| `lifecycle/`, `manifest/` | `cap-std`, `fs2`, `rustix`, `same-file` | host |
| `graph/` node types | serde only | **SDK** |
| `analyze/facts.rs` type vocabulary (`Type`, `Prim`, `WellKnown`, `Field`) | serde only | **SDK** (`graph/mod.rs:29` already re-exports it) |
| `sdk/` core types (`Cx`, `Artifact`, `Artifacts`, `FileStamp`, `ReadinessTarget`, the 4 traits, `Pipeline`) | serde only | **SDK** |
| built-in stage *structs + builders* | serde only | **SDK** |
| built-in stage *trait impls* | everything above | host |
| `graph/direction.rs`, `graph/projection.rs` | — | host (they are generation-time algorithms, `pub(crate)` already) |

The only Rust-language obstacle is the orphan rule: if `Source` and `GoGin` both live in the SDK
crate, `impl Source for GoGin` cannot live in the engine crate. Therefore **built-ins must not
implement the stage traits at all**; they must be declarations the host executes.

That in turn forces a decision about `Pipeline::transform(…)`'s parameter type, because a blanket
`impl<T: Transform> IntoStage for T` overlaps with per-built-in impls and Rust coherence rejects it
(no negative reasoning). See §4.1.

---

## 3. Verified: security posture as it stands

- **Native compilation and execution of repository Rust is unsandboxed, trusted-code execution.**
  `cargo run --manifest-path .gnr8/Cargo.toml` compiles and runs `build.rs`, proc macros, and the
  user's `main()` with the invoking user's full privileges. Nothing in the repo claims otherwise, and
  nothing in the repo prevents it either: there is no opt-out flag, no trust prompt, and no
  no-execute mode. `gnr8 inspect <path>` is the only pipeline-free command.
- **No frame bound.** `child.rs:189` `Command::output()` buffers unbounded stdout and stderr into
  memory. A runaway child can exhaust host memory.
- **No timeout.** A child that never exits hangs `gnr8` forever.
- **No process-tree bound.** A child that forks leaves orphans; `unsafe_code = "forbid"` blocks the
  usual `setsid`/`pre_exec` remedy.
- **No integrity check on the payload.** Truncated stdout surfaces as a `serde_json` parse error, not
  as a detected corruption.
- **Writes are already well defended.** `sdk/mod.rs:79` `portable_path_identity` rejects absolute
  paths, `..`, backslashes, control characters, Windows device names, trailing dot/space, >255-byte
  components, the `.gnr8` namespace, and case-fold aliases; `lifecycle/` writes through `cap-std` /
  `cap-fs-ext` capability-relative no-follow opens with `fs2` locks and atomic renames. **This is the
  part that must not regress.**
- **`.gnr8/Cargo.toml` is not validated before use.** `run_child_stdout` only checks
  `manifest.is_file()` (`child.rs:180`). A symlinked `.gnr8` or a manifest declaring arbitrary
  dependencies is built without comment.

---

## 4. Recommendation: a real boundary

### 4.1 Package graph

```
crates/gnr8-sdk/   package `gnr8`         published, thin. deps: serde, serde_json, blake3
crates/gnr8-core/  package `gnr8-engine`  publish = false, heavy. deps: gnr8 + all of today's
crates/gnr8/       package `gnr8-cli`     publish = false, bin `gnr8`. deps: gnr8-engine
```

Keeping the *published* name `gnr8` on the thin crate preserves `use gnr8::sdk::prelude::*;` and
`gnr8 = "=x.y.z"` in every `.gnr8/Cargo.toml`, which is the surface users and docs actually touch.
The heavy crate loses its crates.io identity; it is a host implementation detail from now on.

Built-ins move to the SDK as pure descriptors with `#[doc(hidden)] pub` fields (so the engine can
read them without 150 hand-written getters and without a second mirrored type definition), keeping
their existing builder methods as the documented API. The engine gains an `exec` module that matches
on the descriptor enums and calls the code that already exists.

**Coherence resolution.** `Pipeline::transform(impl Into<TransformStage>)`, with a `From` impl per
built-in and one for `Custom<T>`:

```rust
.transform(SetTitle::new("Bookstore API"))     // built-in: declaration, host-executed
.transform(Custom(DropDebugRoutes))            // your Rust: worker-executed
```

`Custom` is a one-word wrapper that also makes the host/worker split legible in the config file. The
alternative — a hidden `fn __builtin(&self) -> Option<Spec>` default method on each trait, with
built-ins implementing `apply()` as an unreachable error — preserves the exact call syntax but leaves
~40 unreachable method bodies in the public API and silently breaks any user who wraps a built-in.
Rejected for honesty.

### 4.2 Process protocol

The host stops using `cargo run` as the execution primitive:

1. `cargo build --quiet --locked --manifest-path .gnr8/Cargo.toml --target-dir .gnr8/target` — an
   explicit `--target-dir` so the host knows the binary's path deterministically.
2. Execute `.gnr8/target/debug/<package-name>` directly with `stdin`/`stdout` piped and
   `current_dir = project_root`. stdout is now *only* the protocol; cargo can never interleave.
3. Exchange length-prefixed, digest-checked frames until `Shutdown`.

Frame: `b"GN8F" ‖ u32_be(len) ‖ 32-byte BLAKE3(payload) ‖ payload(JSON)`. Both sides verify the
digest before parsing and reject `len > MAX_FRAME_BYTES`.

Session:

```
host → Hello   { protocol, host_version, capability_digest, project_root }
wkr  → Ready   { protocol, sdk_version, capability_digest, plan }
                 plan = ordered [Builtin(spec) | Custom{ index, label }] per stage kind
host → LoadSource { index }            → wkr  → Graph { graph }          (custom sources only)
host → Transform  { index, graph }     → wkr  → Graph { graph }          (custom transforms only)
host → Generate   { index, graph, artifacts } → wkr → Artifacts { … }    (custom targets only)
host → Post       { index, artifacts } → wkr  → Artifacts { … }          (custom posts only)
host → Shutdown                        → wkr exits 0
```

Built-in stages never cross the wire as work — only as declarations. A pipeline with no custom stages
does exactly one round trip (`Hello`/`Ready`) and then the host generates everything natively.

### 4.3 Why this is the right target, with numbers

Projected at design time, then re-measured on the shipped code (same machine, same method as §1.3):

| | before | projected | **measured after** |
|---|---|---|---|
| cold `.gnr8` build | 19.4 s, 60 units, 561 MB | ~7 s | **9.79 s, 23 units, 212 MB** |
| cold `gnr8 generate` | 33.0 s | — | **12.03 s** |
| warm `gnr8 generate` | 0.27–0.31 s | — | **0.054–0.069 s** |
| warm `gnr8 check` | 0.28 s | — | **0.049–0.051 s** |
| `.gnr8/Cargo.lock` | 59 packages | ~12 | **23 packages** |

The cold build came in above the ~7 s projection: the SDK crate carries more derived data types than
the estimate assumed (the graph plus ~47 declaration types), so its own compile is ~4 s rather than
~2 s. The warm path beat expectations, because cargo leaves it entirely — the worker binary is
content-addressed against every `.gnr8` input plus the host executable's own hash, so an unchanged
project skips `cargo` outright (proven by a sentinel `cargo` shim in
`crates/gnr8/tests/worker_contract.rs`).

- Version-bump rebuild: **12.5 s of `gnr8-core` → the SDK crate only**.
- The worker no longer needs `GNR8_RESOURCE_DIR`, `go`, `python3`, or `node`: extraction is the
  host's.

`scripts/bench.sh` — the repository's own harness, which scaffolds a fresh `.gnr8` over a scratch
copy of `fixtures/goalservice` — reports `cold=11921ms warm-no-op=2110ms single-file-edit=2374ms` on
the shipped code. Read that `warm-no-op` figure the way the harness defines it: it is the *second*
`generate`, which still pays a Go source-analysis miss because the `GoGin` source cache keys on the
compiled `goextract` binary and that binary is produced during the first run. A third `generate` in
the same tree reports `pipeline: 44 ms, total: 50 ms` — the same steady state the examples show.

### 4.4 What must NOT change

- Byte-identical generated output for all five examples (`make examples-check`).
- `portable_path_identity` and the `lifecycle` writer stay exactly where they are, host-side, and now
  additionally validate everything a worker returns.
- Custom `Source`/`Transform`/`Target`/`PostProcess` stay first-class.
- No TOML/YAML config, no foreign-tool coupling, no fallback chains (CLAUDE.md rules 0–4).

---

## 5. Alternatives considered and rejected

1. **Keep `cargo run`, just trim `gnr8-core` with cargo features.** A `default-features = false` SDK
   feature would still put 53 k lines of source in the dependency graph, still recompile on every
   version bump, and would create exactly the "two ways to build the same crate" branch rule 3
   forbids. Rejected.
2. **Dynamic plugins (`dylib`/FFI/ABI).** Explicitly forbidden by CLAUDE.md ("no dynamic plugin
   runtime … extension is compile-time only") and by `unsafe_code = "forbid"`. Rejected.
3. **WASM worker.** Would give real sandboxing, but users' custom stages could no longer call
   ordinary Rust crates or the filesystem, which is the entire point of code-as-config. Also a large
   new toolchain requirement. Rejected for now; noted as a future gate.
4. **Send the pipeline *to* the host as data and drop custom stages.** That is a config DSL wearing a
   Rust costume, and it deletes the product's differentiator. Rejected.
5. **Keep built-ins implementing the traits inside the SDK, with the engine injected via a
   registry/`dyn` handle.** Requires the SDK to define an "engine" trait the host implements and pass
   it through `Cx`; the built-ins would then be thin forwarders. This is workable but puts an
   engine-shaped abstract interface (≈40 methods) into the thin crate and makes the worker able to
   *request* engine work mid-stage, re-entrantly. More protocol surface, no user-visible gain.
   Rejected in favour of declarations.
6. **Keep the unreachable artifact/no-op caches and re-point them at the host.** Their inputs
   (`bundle.cache_*`) cease to exist. Reviving them is new behaviour, not preservation. They are
   removed; the host-side worker-build fingerprint replaces the only part that was load-bearing.

---

## 6. Security and trust implications of the new design

**Stated plainly: `.gnr8` is trusted code. Building and running it executes arbitrary Rust —
`build.rs`, proc macros, and `main()` — with the invoking user's privileges. gnr8 does not sandbox
it and will not claim to.** What the design adds is *consent and containment*, not a sandbox:

- `--no-build` — never invoke `cargo`. Requires an existing worker binary whose fingerprint matches.
- `--no-execute` — never build *and* never run the worker. Pipeline commands fail with a typed error
  naming the trust boundary; `gnr8 inspect <path>` still works.
- Pre-build manifest validation: `.gnr8` and `.gnr8/Cargo.toml` must be real (non-symlink) entries
  inside the project root; the manifest must parse; it must depend on `gnr8`; it must not depend on
  `gnr8-engine`/`gnr8-core`; a `gnr8` pin below the first SDK version is the old contract and is
  rejected with upgrade instructions **before** anything is compiled.
- Frames are length-prefixed, BLAKE3-digest-checked, and bounded (`MAX_FRAME_BYTES`).
- Worker stderr is drained by a bounded reader and truncated with an explicit marker.
- A wall-clock timeout kills the worker; **honest limitation:** `unsafe_code = "forbid"` prevents
  creating a process group, so only the direct worker process is killed. Grandchildren the user's own
  code spawns are not tracked. This must be documented, not glossed.
- Every artifact the worker returns is re-validated host-side with `portable_path_identity` before it
  reaches `lifecycle`, so a hostile or buggy worker cannot widen the write surface.
- The worker binary is required to live under `.gnr8/target/`, resolved and checked against the
  project root.

---

## 7. Open decisions

1. **Version.** `0.9.0` (breaking, unstable minor). Cargo treats `0.x.y → 0.x.(y+1)` as compatible,
   so the minor must move (`CHANGELOG.md:6-8`).
2. **`gnr8-engine` publication.** Not published. If someone later needs the engine as a library the
   decision can be revisited; nothing in the CLI needs it published.
3. **Artifact round-trip cost for custom targets.** The full artifact set crosses the wire when a
   custom target or post-processor exists, because `Target::generate(&ir, &mut out, …)` may
   `overlay`/`rewrite` a previous target's files. For the five examples this is <200 KB. Not
   optimised now; measured, not assumed.
4. **Multi-source merge.** Still one source per pipeline (`sdk/mod.rs:920-940`). Unchanged here.
5. **Reviving a host-side no-op cache.** Now *possible* for all-declaration pipelines because the host
   sees the plan, but it is new behaviour and is deliberately out of scope. Removing the dead code is
   in scope.
6. **Windows.** All measurements are Linux. The design uses no new platform APIs beyond
   `std::process`, but nothing here proves Windows behaviour; CI is `ubuntu-latest` only
   (`.github/workflows/ci.yml`).
