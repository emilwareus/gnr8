# Implementation plan: thin SDK + host-owned engine + framed worker protocol

Companion to [`2026-08-27-thin-sdk-worker-boundary.md`](2026-08-27-thin-sdk-worker-boundary.md).
Target release: **0.9.0** — a deliberate, breaking, unstable-minor change. There is no second
backend, no compatibility mode, no automatic fallback, and no deprecation window.

---

## 0. The one-sentence contract

> The prebuilt `gnr8` host analyzes source, builds the canonical `ApiGraph`, executes every built-in
> stage natively, and hands only the user's own Rust stages to a small project-local worker that
> links nothing but the thin `gnr8` SDK; the worker answers over a length-prefixed, digest-checked
> frame protocol; the host owns the cache, the filesystem, edit protection, lifecycle and rendering.

---

## 1. Package graph and dependency boundaries

| Path | Package | Publish | Dependencies |
|---|---|---|---|
| `crates/gnr8-sdk/` | `gnr8` | **yes** | `serde`, `serde_json`, `blake3` — nothing else, ever |
| `crates/gnr8-core/` | `gnr8-engine` | no | `gnr8` + everything it has today |
| `crates/gnr8/` | `gnr8-cli` (bin `gnr8`) | no | `gnr8-engine`, `clap`, `anyhow`, `notify-debouncer-full`, `ctrlc` |

Enforcement (all three are gates, not conventions):

1. `crates/gnr8-sdk/Cargo.toml` lists exactly three dependencies. A new one is a review event.
2. A new test `crates/gnr8-sdk/tests/thin_boundary.rs` parses `Cargo.toml` and asserts the exact
   dependency set, so adding `noyalib`/`cap-std`/etc. fails CI.
3. `scripts/check-invariants.sh` gains a rule: no file under `crates/gnr8-sdk/src` may name
   `analyze`, `lower`, `gosdk`, `pysdk`, `tssdk`, `lifecycle`, `manifest`, `resource`, or
   `openapi_source`.

### What moves to the SDK

| From | To | Notes |
|---|---|---|
| `gnr8-core/src/graph/mod.rs` (data) | `gnr8-sdk/src/graph/mod.rs` | `direction.rs`, `projection.rs` stay in the engine |
| `gnr8-core/src/analyze/facts.rs` (type vocabulary + fact DTOs) | `gnr8-sdk/src/facts.rs` | `graph` already re-exports `Type`/`Prim`/`WellKnown`/`Field` from it |
| `gnr8-core/src/sdk/mod.rs` core types | `gnr8-sdk/src/sdk/mod.rs` | `Cx`, `Artifact`, `Artifacts`, `ArtifactOwnership`, `ArtifactRewrite`, `ArtifactMetadata`, `FileStamp`, `ReadinessTarget`, `ReadinessKind`, the 4 traits, `Pipeline`, `prelude` |
| `gnr8-core/src/sdk/builtins.rs` struct defs + builders | `gnr8-sdk/src/sdk/builtins.rs` | descriptors only; `#[doc(hidden)] pub` fields |
| `gnr8-core/src/sdk/{docs,layout,model,model_style}.rs` | `gnr8-sdk/src/sdk/…` | pure config data |
| `gnr8-core/src/diagnostics/mod.rs` | stays | host rendering |
| `gnr8-core/src/error.rs` `CoreError` | split | `gnr8::Error` (SDK, the variants a stage author constructs) + `gnr8_engine::CoreError` (full set, `From<gnr8::Error>`) |

### What stays in the engine

`analyze/`, `lower/`, `gosdk/`, `pysdk/`, `tssdk/`, `lifecycle/`, `manifest/`, `resource/`,
`workspace/`, `diagnostics/`, `graph/{direction,projection}`, the artifact-path portability rules,
and the new `exec/` + `worker/` (host side) modules.

### What is deleted

The two unreachable subsystems (research §1.5) and everything that only existed to feed them:

- `runner::PRE_CHILD_NOOP_SUPPORTED`, `ArtifactBundle` and all eleven `cache_*` fields,
  `runner::{emit,inspect}`, `capability_fingerprint`'s three env vars.
- `sdk::{ARTIFACT_CACHE_SUPPORTED, artifact_cache_key, load_artifact_cache, load_artifact_cache_files,
  load_artifact_cache_metadata, save_artifact_cache, save_artifact_metadata_cache,
  discard_artifact_cache, artifact_cache_exists, cleanup_artifact_cache_temporary_files,
  ArtifactCache, ArtifactMetadataCache, cache_config_input_stamps, cache_tool_input_stamps,
  cache_config_inputs_complete, ArtifactCacheInputs, Pipeline::{cache_input_roots,
  cache_input_stamps, artifact_cache_inputs, run_for_emit}}`.
- The `verified_noop_*` / `Fast*Stamp` subsystem in `crates/gnr8/src/main.rs:668-1240` and
  `regenerate_bundle` / `plan_bundle` / `cached_artifact_metadata` / `ensure_bundle_artifacts`.
- `lifecycle::{plan_only_cached, regenerate_cached_with_anchors, plan_metadata_writes,
  recover_cached_output_transactions}` **only if** nothing else uses them after the rewrite; verified
  during implementation, not assumed.
- The `Source::verified_noop_input_files`, `Transform::verified_noop_input_*`,
  `Target::{cache_input_files, verified_noop_input_*}`, `PostProcess::{cache_key_fragment,
  verified_noop_input_*}` trait methods — they exist solely to feed the deleted caches.

---

## 2. Snapshot / IR representation

The IR crossing the wire is the existing `ApiGraph`, serialized with the existing serde derives. No
second representation, no lossy projection: the worker sees exactly what an in-process transform sees
today.

- Sent as compact JSON inside a frame payload.
- The frame carries `BLAKE3(payload)`; both sides verify before parsing.
- The `Artifacts` set crosses as `Vec<Artifact>` (path, text, producer, ownership, rewrite chain) so
  a custom target can legally `overlay`/`rewrite` a previous target's file.
- Ordering is unchanged: `Artifacts` keeps its `Vec` sorted by path on both sides;
  `Artifacts::from_files` re-sorts on receipt.

---

## 3. Host ↔ worker process protocol

### 3.1 Framing

```
frame := MAGIC(4) ‖ len:u32be ‖ digest:[u8;32] ‖ payload[len]
MAGIC  = b"GN8F"
digest = blake3(payload)
payload = compact JSON of one protocol message
```

`MAX_FRAME_BYTES = 64 * 1024 * 1024`. A larger `len`, a bad magic, or a digest mismatch is a typed
`Protocol` error naming which check failed. Reads are exact-length (`read_exact`), so a truncated
pipe is an error, never a partial parse.

`crates/gnr8-sdk/src/protocol/mod.rs`:

- `pub const PROTOCOL_VERSION: u32 = 6;`
- `pub const MAX_FRAME_BYTES: usize`
- `pub fn write_frame<W: Write>(w: &mut W, msg: &impl Serialize) -> Result<(), Error>`
- `pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, Error>`
- `pub enum HostMessage { Hello, LoadSource, Transform, Generate, Post, Shutdown }`
- `pub enum WorkerMessage { Ready, Graph, Artifacts, Failed }`
- `pub fn capability_digest(sdk_version: &str) -> String`

### 3.2 Session

```
host → Hello { protocol, host_version, capability_digest, project_root }
wkr  → Ready { protocol, sdk_version, capability_digest, plan }
        plan.sources/transforms/targets/posts : Vec<PlanStage>
        PlanStage = Builtin(BuiltinX) | Custom { index, label }
```

Then, only for `Custom` stages, in pipeline order:

```
host → LoadSource { index }                    → Graph { graph }
host → Transform  { index, graph }             → Graph { graph }
host → Generate   { index, graph, artifacts }  → Artifacts { artifacts }
host → Post       { index, artifacts }         → Artifacts { artifacts }
host → Shutdown                                → (worker exits 0)
```

`Failed { message }` may replace any worker reply; the host turns it into a typed
`CoreError::WorkerRun`.

### 3.3 Handshake and version/ABI negotiation

- `Hello.protocol` must equal the worker's `PROTOCOL_VERSION` — else the worker replies `Failed` and
  exits 2.
- `Hello.host_version` must equal the SDK's `CARGO_PKG_VERSION` (exact-version contract, as today).
- `capability_digest` = `blake3("gnr8-sdk:{version};protocol:{n};frames:1;plan:1")`. Both sides
  compute it; a mismatch is fatal on both.
- The worker refuses to run at all without a valid `Hello` frame on stdin. Unlike today there is no
  "no handshake ⇒ run anyway" branch: running the worker binary by hand prints usage to stderr and
  exits 2.

### 3.4 Bounds

| Bound | Value | Enforcement |
|---|---|---|
| frame size | 64 MiB | `read_frame` rejects before allocating |
| stderr capture | 1 MiB, then truncated with a marker | bounded reader thread in the host |
| wall clock | 300 s default per session, `GNR8_WORKER_TIMEOUT_SECS` to override | host watchdog kills the child |
| process tree | direct child only | **documented limitation** — `unsafe_code = "forbid"` blocks `pre_exec`/`setsid` |
| stdout | protocol only | worker's own `println!` is impossible: `run` takes stdout exclusively and the SDK exposes no print helper |

---

## 4. Custom stage semantics

Unchanged in substance; changed in spelling.

```rust
Pipeline::new()
    .source(GoGin::new().inputs(["."]))          // declaration → host
    .transform(SetTitle::new("Taskflow API"))    // declaration → host
    .transform(Custom(DropDebugRoutes))          // your Rust  → worker
    .target(OpenApi31::new().to("openapi.yaml")) // declaration → host
    .target(Custom(ApiMarkdown { … }))           // your Rust  → worker
    .post(Header::generated())                   // declaration → host
```

- `pub struct Custom<T>(pub T);` in `gnr8::sdk`, re-exported from the prelude.
- `impl<T: Source + 'static> From<Custom<T>> for SourceStage` (and the three siblings).
- `impl From<GoGin> for SourceStage` … one per built-in, generated by a local `macro_rules!`.
- `Pipeline::{source,transform,target,post}` take `impl Into<XStage>`.
- Trait signatures keep their shape but return `gnr8::Error`:
  `Source::load(&self, cx) -> Result<ApiGraph, Error>`, `Transform::apply(&self, ir, cx)`,
  `Target::{generate, producer, output_anchors, readiness_targets}`, `PostProcess::{run, producer}`.
  The cache-declaration methods listed in §1 are removed.
- `Target::output_anchors()` / `readiness_targets()` for **custom** targets are collected at plan
  time (the worker calls them while building `Ready`), so the host has them before any generation.

---

## 5. Diagnostics and errors

- `gnr8::Error` (SDK): `Config`, `Io`, `Generation`, `ArtifactOwnership`, `Protocol`. Marked
  `#[non_exhaustive]` so future variants are not a breaking change (the current `CoreError` is not,
  and `0.8.0`'s only breaking change was exactly that).
- `gnr8_engine::CoreError` keeps every existing variant, gains `WorkerRun` and `WorkerBuild`
  (replacing `ChildRun`'s two roles), and gains `From<gnr8::Error>`.
- Worker-side failures are transported as `Failed { message }` and re-raised host-side as
  `CoreError::WorkerRun` with the worker's stderr appended, exactly as `ChildRun` does today.
- `Diagnostic` is unchanged and still travels on the graph.

---

## 6. Build fingerprint, cache and artifact layout

`.gnr8/` layout after this change:

```
.gnr8/
  Cargo.toml          committed   dependency: gnr8 = "=<version>"
  Cargo.lock          committed   ~12 crates
  src/main.rs         committed
  .gitignore          committed
  README.md           committed
  target/             ignored     cargo output; worker binary at target/debug/<pkg>
  cache/
    worker.json       ignored     the build fingerprint stamp
    source/…          ignored     existing per-source graph cache (GoGin), now host-side
  manifest.json       (existing ownership manifest — unchanged)
```

**Worker build fingerprint** (`gnr8-engine::worker::fingerprint`):

```
blake3(
  "gnr8-worker-v1\n"
  ‖ host_exe_content_hash ‖ "\n"        # covers the SDK source in a path-dep dev build
  ‖ PROTOCOL_VERSION ‖ "\n"
  ‖ capability_digest ‖ "\n"
  ‖ for each file under .gnr8 except target/ and cache/, sorted by relative path:
        rel_path ‖ "\0" ‖ blake3(content) ‖ "\n"
)
```

`.gnr8/cache/worker.json` records `{ fingerprint, binary_path, binary_len, binary_hash }`.

Decision rule — exactly one, no fallback:

> If `worker.json` exists, its `fingerprint` equals the freshly computed one, and the recorded binary
> still hashes to `binary_hash`, the recorded binary **is** the build output; run it. Otherwise run
> `cargo build` and rewrite the stamp.

This is what makes "unchanged input does not invoke Cargo" provable, and it is a content-addressed
identity, not a heuristic.

Invalidation matrix (each is a test):

| Change | Rebuild? | Re-run worker? | Regenerate? |
|---|---|---|---|
| nothing | no | yes | no (all outputs unchanged) |
| `.gnr8/src/main.rs` | yes | yes | as the new pipeline says |
| `.gnr8/Cargo.toml` / `Cargo.lock` | yes | yes | as the new pipeline says |
| host binary replaced/upgraded | yes | yes | as the new pipeline says |
| project source file | no | yes | yes |
| generated output hand-edited | no | yes | protected unless `--force` |
| worker binary deleted | yes | yes | no |
| worker binary tampered with | yes | yes | no |

---

## 7. Offline and no-Cargo operation

- `cargo build` is invoked with `--locked` so a committed `.gnr8/Cargo.lock` is authoritative and no
  network resolution happens for a lock that is already complete.
- `--offline` is added when `GNR8_CARGO_OFFLINE=1`. (Single knob, single source; no precedence
  chain.)
- `gnr8 --no-build <cmd>` never invokes `cargo`; a missing/stale worker is a typed error naming the
  fingerprint mismatch.
- `gnr8 --no-execute <cmd>` never builds and never runs the worker. `generate`/`check`/`watch`/
  `inspect` (pipeline form) / `doctor` fail with a typed error that states plainly that `.gnr8` is
  trusted Rust; `gnr8 inspect routes <path>` still works because it never touches `.gnr8`.
- `gnr8 init` and `gnr8 guide` never build or execute anything.

---

## 8. Path, symlink and process safety

Pre-build validation (`gnr8-engine::worker::validate_workspace`), all before any process starts:

1. `<root>/.gnr8` exists, is a directory, and `symlink_metadata` says it is **not** a symlink.
2. `<root>/.gnr8/Cargo.toml` is a regular file, not a symlink.
3. The manifest parses as TOML and has `[package].name` that is a valid Cargo package name.
4. `[dependencies]` contains `gnr8`.
5. `[dependencies]` contains **neither** `gnr8-engine` nor `gnr8-core` → typed error.
6. If the `gnr8` requirement is an exact pin (`=X.Y.Z` or `X.Y.Z`) below `0.9.0` → the pre-SDK
   contract; typed error with the exact upgrade steps.
7. The resolved worker binary path is `<root>/.gnr8/target/debug/<package-name>` and must, after
   `canonicalize`, still start with the canonicalized `<root>/.gnr8/target`.

Post-run validation:

8. Every artifact returned by the worker goes through `portable_path_identity` host-side before it
   reaches `lifecycle` (§3 of the research: this is the write-surface guard).
9. `lifecycle`'s existing `cap-std` / `cap-fs-ext` / `fs2` / atomic-rename writer is untouched.

Honest limitations, documented in `docs/code-as-config.md` and the CHANGELOG:

- Compiling and running `.gnr8` executes arbitrary Rust (`build.rs`, proc macros, `main`) with the
  user's privileges. **It is not sandboxed.**
- Only the direct worker process is killed on timeout; grandchildren are not tracked.

---

## 9. Phases, files, symbols

### Phase 1 — create the SDK crate (mechanical move)

- `crates/gnr8-sdk/Cargo.toml` — package `gnr8`, deps `serde`, `serde_json`, `blake3`.
- `crates/gnr8-sdk/src/lib.rs` — `pub mod error; pub mod facts; pub mod graph; pub mod protocol;
  pub mod sdk; pub mod worker; pub use sdk::prelude;`
- Move `facts.rs`, `graph/mod.rs` (data), `sdk/{mod,builtins,docs,layout,model,model_style}.rs`
  descriptor halves.
- `crates/gnr8-core` becomes package `gnr8-engine`, gains `gnr8 = { workspace = true }`, and shims:
  `pub use gnr8::graph::*` inside its own `graph` module (which keeps `direction`/`projection`),
  `pub use gnr8::sdk::{…}` inside its own `sdk` module, `pub mod facts` re-export in `analyze`.
  This keeps ~2,000 existing `crate::graph::…` / `crate::sdk::…` paths compiling.

### Phase 2 — descriptors and execution split

- SDK: built-in structs get `#[doc(hidden)] pub` fields, `Serialize`/`Deserialize`, and
  `From<X> for XStage`.
- SDK: `sdk::stage` — `SourceStage`, `TransformStage`, `TargetStage`, `PostStage`, `Custom<T>`,
  `BuiltinSource`, `BuiltinTransform`, `BuiltinTarget`, `BuiltinPost` (serde-tagged enums).
- Engine: `exec/mod.rs` — `load_source`, `apply_transform`, `generate_target`, `run_post`,
  `builtin_output_anchors`, `builtin_readiness_targets`; one `match` per enum, each arm calling the
  code that used to be in the trait `impl`.
- Engine: delete the `impl Source/Transform/Target/PostProcess for <builtin>` blocks.

### Phase 3 — protocol + worker runtime

- SDK: `protocol/mod.rs` (framing, messages, digests, bounds).
- SDK: `worker/mod.rs` — `pub fn run(pipeline: Pipeline) -> ExitCode`, the stdin/stdout loop.
- Engine: `worker/mod.rs` (host side) — `Worker::start`, `Worker::plan`, `Worker::load_source`,
  `Worker::transform`, `Worker::generate`, `Worker::post`, `Worker::shutdown`, plus
  `build::ensure_worker`, `fingerprint`, `validate_workspace`.
- Engine: `pipeline/mod.rs` (host side) — `run_pipeline(root, &plan, worker, options) ->
  PipelineOutcome { artifacts, diagnostics, output_anchors, readiness_targets }` and
  `build_ir(root, &plan, worker)` for `inspect`.

### Phase 4 — CLI rewiring

- `crates/gnr8/src/child.rs` → `crates/gnr8/src/worker.rs` (host driver glue + trust flags).
- `main.rs`: `run_generate`/`run_check`/`run_inspect`/`run_doctor` call the new engine entry points;
  delete `regenerate_bundle`, `plan_bundle`, `cached_artifact_metadata`, `ensure_bundle_artifacts`,
  the `VerifiedNoopStamp` subsystem and the `Fast*Stamp` helpers.
- `watch.rs`: `regenerate_once` uses the new path; **one worker session per tick** (a persistent
  worker across ticks would have to be invalidated on `.gnr8` edits, which is what the fingerprint
  already does at build time — not worth the state).
- `cli.rs`: global `--no-build`, `--no-execute`.

### Phase 5 — scaffold, examples, docs, release metadata

- `workspace/mod.rs`: `cargo_toml_body` emits `gnr8 = "=<version>"` (packaged) or the path dep to
  `crates/gnr8-sdk`; `main_rs_body` unchanged in shape; `readme_body` mentions the worker.
- `gnr8 init --upgrade`: rewrite `.gnr8/Cargo.toml`'s dependency line, delete a stale
  `.gnr8/Cargo.lock`, print the exact `src/main.rs` edits (`Custom(...)` wrapping). Never touches
  user Rust.
- All five `examples/*/.gnr8/{Cargo.toml,Cargo.lock,src/main.rs}` updated; `examples/taskflow` gains
  `Custom(...)`.
- `docs/`: `code-as-config.md`, `extensibility.md`, `pipeline/configuration.md`,
  `operations/artifacts-and-ci.md`, `reference/public-api.md`, `cli/commands.md`, `USAGE.md`,
  `AGENT-USAGE.md`, `install.md`.
- `scripts/package-release.sh`: ship `crates/gnr8-sdk` (not `crates/gnr8-core`) in `share/gnr8`;
  `resource.rs` validation list updated.
- `Cargo.toml` workspace version → `0.9.0`; `CHANGELOG.md` gains a `### Breaking` section.

### Phase 6 — tests

| Area | Test |
|---|---|
| thin boundary | `gnr8-sdk/tests/thin_boundary.rs` — exact dependency set; no engine module names |
| framing | `gnr8-sdk/src/protocol` unit tests — round trip, bad magic, bad digest, oversize, truncation |
| plan | `gnr8-sdk/src/sdk` unit tests — built-in → descriptor, `Custom` → custom, order preserved |
| worker loop | `gnr8-sdk/tests/worker_session.rs` — drive `worker::run` over in-memory pipes |
| host driver | `gnr8-engine` unit tests — handshake mismatch, `Failed`, timeout, oversize frame |
| manifest safety | `gnr8-engine/tests/worker_workspace.rs` — symlinked `.gnr8`, missing dep, engine dep, old pin, escaping binary path |
| old contract rejected | same file — a `0.8`-pinned manifest fails **before** cargo runs |
| fingerprint | `gnr8-cli/tests/worker_cache.rs` — cargo invoked once, then never; each row of §6's matrix |
| determinism | existing `determinism.rs`, `snapshot_*` unchanged and green |
| e2e | `gnr8-cli/tests/generate_e2e.rs` rewritten for the new scaffold; new `custom_stage_e2e.rs` proving a custom `Transform` + `Target` round-trips |
| examples | `make examples-check` byte-identical |

### Phase 7 — verification and performance gates

Run, and report only what is actually measured:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p gnr8 && cargo test -p gnr8-engine && cargo test -p gnr8-cli
make invariants
make examples-check
make goextract-build pyextract-test tsextract-test action-test
```

Plus a re-run of every measurement in research §1.3 on the new code.

---

## 10. Ships now vs. deliberately deferred

**Ships now:** the whole boundary — thin SDK crate, descriptors, engine-side execution, framed
digest-checked protocol, host-owned worker build + fingerprint cache, trust flags, manifest/path
validation, `init --upgrade`, all five examples, docs, release metadata, and the test matrix above.

**Deliberately deferred, with the reason:**

1. **A host-side no-op cache that skips the worker entirely.** Now *possible* for pipelines with no
   custom stages, but it is new behaviour with a real correctness surface (a `main()` may read the
   environment while composing the pipeline). Out of scope; the dead code that pretended to do this
   is removed rather than revived.
2. **Persistent worker across `watch` ticks.** One session per tick is simpler and already cheap
   (exec of a prebuilt binary). Revisit only with a measurement showing it matters.
3. **Sandboxing (WASM or OS-level).** Would change what user stages can do. Recorded as a future
   gate; until then the docs say plainly that `.gnr8` is trusted code.
4. **Publishing `gnr8-engine`.** Nothing needs it.
5. **Multi-source merge.** Unrelated and still one source per pipeline.
6. **Windows/macOS verification.** CI is `ubuntu-latest`; no claim is made about other platforms
   beyond "the code uses only `std::process` and existing cross-platform crates".

---

## 11. As built: where the implementation diverged from this plan

Recorded after the fact, so the plan is evidence of what was decided *and* of what changed.

1. **`SdkModel` stayed in the engine and left the prelude.** §1 planned to move `sdk/model.rs` to the
   SDK. It cannot go: `SdkModel::from_graph` needs `graph::projection::for_generation` and
   `sdk::emit_common`, both host-only, and an inherent impl must live with its type. It is the SDK
   emitters' internal model, so the engine is the right home — but that is a public-API removal, and
   the CHANGELOG says so.
2. **The SDK has four dependencies, not three.** `thiserror` was added: typed errors are the
   repository's standing convention (RUST-04), and its proc-macro closure is already pulled in by
   `serde`'s derive, so it costs one compile unit.
3. **The thin-boundary rule is a test, not a grep.** §1 planned a rule in
   `scripts/check-invariants.sh`. `crates/gnr8-sdk/tests/thin_boundary.rs` does it better: it can
   distinguish a `crate::analyze` path from prose that merely names the engine, and it asserts the
   exact dependency set rather than a denylist.
4. **`pipeline::{InProcessRunner, run_in_process, build_ir_in_process}` were added.** The engine's own
   contract tests hold a `Pipeline` value and need to run it without spawning a process. They use the
   same `pipeline::run` with a `StageRunner` that executes stages in-process, so there is one ordering
   implementation, not two. The CLI never uses it.
5. **`media_example` and `validate_metadata_value` became public SDK helpers.** Both are needed by the
   declaration builders *and* by the host executor; one definition beside the declarations is the
   rule-3 answer.
6. **Two guards were added during self-review, beyond the plan:**
   - the resolved worker binary path is checked level by level with `symlink_metadata`, because
     `.gnr8/target` is excluded from the build fingerprint and a symlinked build directory would
     otherwise redirect execution without appearing as a changed input;
   - a worker reply that drops an artifact an earlier stage produced is rejected, because the host
     would then treat that file as stale and delete it. In-process this is impossible (`Artifacts`
     has no removal API); across a wire it is not.
7. **`GNR8_WORKER_TIMEOUT_SECS` rejects a malformed value** rather than silently reverting to the
   default — a silent fallback is exactly what rule 3 forbids.
8. **The removed lifecycle entry points were confirmed dead and deleted**, along with their tests:
   `plan_only_cached`, `regenerate_cached_with_anchors`, `plan_metadata_writes`,
   `recover_cached_output_transactions` and four private helpers. Crash recovery is unaffected —
   `regenerate_with_anchors` calls `recover_abandoned_generations_locked` on its own.
9. **`PROTOCOL_VERSION` restarted at 1**, not 6. §3.1 proposed continuing the old bundle's numbering.
   It is a different protocol with a different framing, a different message set and a different
   magic; continuing someone else's counter would imply a lineage that does not exist.
10. **The message names are `ApplyTransform` / `GenerateTarget` / `RunPost`**, not `Transform` /
   `Generate` / `Post` — the shorter names collided with the trait names in every `use` site.
11. **Not done, and not claimed:** Windows and macOS behaviour is unverified (CI is `ubuntu-latest`);
   the host-side no-op cache remains deliberately out of scope; `gnr8-engine` is not published.
