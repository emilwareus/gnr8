# Phase 4: `.gnr8` Lifecycle And Watch Mode - Research

**Researched:** 2026-06-24
**Domain:** Rust CLI lifecycle (project scaffolding, content-hash idempotency, filesystem watching/debounce, latency reporting)
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** `gnr8 init` scaffolds a project-local `.gnr8/` workspace with a clear split: **checked-in
  customization** (config + user-editable customization files) vs **git-ignored lifecycle** (cache,
  the ownership manifest, generated output staging). `init` writes a `.gnr8/.gitignore` that ignores
  the lifecycle paths so the split is automatic. `init` is idempotent (re-running doesn't clobber user edits).
- **D-02:** Generated SDK/OpenAPI **outputs** are written to project-relative paths the user configures
  (e.g. `sdk/` + `openapi.yaml`), tracked for ownership (D-04), NOT hidden inside `.gnr8/`. `.gnr8/` holds
  config + customization + cache/manifest only.
- **D-03:** Honor PROJECT constraints ("code-as-config under `.gnr8/`; YAML/TOML/JSON is not the main UX"
  AND "no dynamic plugin runtime"). PoC realizes this **without** a dynamic plugin loader: the checked-in
  customization is a real, user-editable source-of-truth the engine reads **statically**. Configurable
  knobs = source input dir(s), OpenAPI output path, SDK output path + Go module path, naming overrides
  (operation/type name remaps). The format is a minimal checked-in config file chosen by research — an
  explicit PoC stand-in, NOT the long-term UX. **Full programmatic ("through code") customization of
  routing recognition / transport / emitters is a documented v2 direction (ADV-02)**, not built here.
  Phase 4 scopes WS-03 to the documented knobs + naming overrides + a reserved seam, and says so plainly.
- **D-04:** Maintain an **ownership manifest** (e.g. `.gnr8/cache/manifest.json`) recording every generated
  file with a content hash from the last generation. Before overwriting a tracked file, compare on-disk
  content to the recorded hash: if a user hand-edited a generated file (hash mismatch, not produced by gnr8),
  do NOT silently clobber — emit a diagnostic / require explicit `--force` (warn + skip by default, overwrite
  under `--force`). Deleting a file from config drops it from the manifest. Core "no silent clobbering" guarantee.
- **D-05:** Content-hash each output; if regeneration produces **byte-identical** content to what's on disk
  (and recorded), **skip the write** (no mtime churn). Report counts (written / unchanged). Builds on the
  Phases 2–3 determinism guarantee.
- **D-06:** Implement `gnr8 watch` with the **`notify`** crate watching the configured Go source dir(s).
  **Debounce** rapid duplicate events. **Ignore changes to gnr8's own generated outputs** (consult the
  ownership manifest / output paths) to avoid regeneration loops. On a supported source change, re-run the
  pipeline and write only changed outputs (reuse no-op detection).
- **D-07:** Report **latency** for three scenarios (WATCH-03): cold generation, warm no-op, single-file edit.
  Print human-readable timings (and machine `--json` where it fits the existing CLI surface).

### Claude's Discretion

- Exact config file format/name, manifest schema, hash algorithm, debounce interval, `notify` watcher
  config (recursive/poll fallback), and the precise `init` directory tree — left to research/planning,
  within the decisions above.

### Deferred Ideas (OUT OF SCOPE)

- `doctor` diagnostics aggregation, perf benchmark suite, demo docs, milestone verification — **Phase 5**.
- Full programmatic customization (routing/transport/emitter overrides "through code") — **v2 (ADV-02)**.
- Deeper incremental/partial graph invalidation beyond file-level no-op — **v2 (ADV-01)**, only if benchmarks prove need.
- Additional routers / languages — post-PoC.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| **WS-01** | `gnr8 init` scaffolds a project-local `.gnr8/` workspace with user-owned generator code | §1 `.gnr8/` layout + idempotent init algorithm; §6 no new runtime crate needed (std `fs`) |
| **WS-02** | `.gnr8/` separates checked-in customization from ignored cache/output lifecycle files | §1 directory tree + the auto-written `.gnr8/.gitignore` contents |
| **WS-03** | Users can customize source inputs, routing recognition, OpenAPI output, SDK output, naming, transport through code | §2 honest scope: TOML config holds the documented knobs (inputs/outputs/module-path/naming overrides) read statically; routing/transport/emitter overrides = reserved seam, deferred to v2 (ADV-02). No overclaiming. |
| **WS-04** | Generated-file ownership tracked well enough to avoid silently clobbering user-owned files | §3 manifest schema (path → blake3 hash + provenance) + warn+skip/`--force` write algorithm |
| **WATCH-01** | No-op generation avoids rewriting unchanged outputs | §3 write decision function: byte-identical ⇒ skip; leans on Phase 2–3 determinism (D-05) |
| **WATCH-02** | Watch reacts to source changes, debounces duplicate events, avoids loops from generated files | §4 `notify-debouncer-full` + path-filtering against output set/manifest (loop-safety) |
| **WATCH-03** | Reports cold generation, warm no-op, and single-file edit latency for fixture services | §5 `std::time::Instant` wall-clock around the pipeline + write; human + `--json` reporting |

This section is required because requirement IDs were provided. The planner maps each ID to a plan.
</phase_requirements>

## Summary

Phase 4 wraps the now-proven Phase-3 pipeline (`analyze::build_graph` → `lower::to_openapi` + `sdk::generate`
→ `sdk::write_to_dir`) in a project lifecycle: a `.gnr8/` workspace, ownership-aware writes, no-op detection,
and a debounced watch loop. The pipeline is already deterministic (Phase-3 SUMMARY proves `to_openapi` and
`sdk::generate` are byte-identical across two runs), which is the single fact that makes no-op detection
(WATCH-01) correct rather than heuristic: identical source ⇒ identical bytes ⇒ skip the write.

**The three falsifiable guarantees** to test hard are: (1) **no silent clobbering** — a user-edited generated
file is detected (recorded hash ≠ on-disk hash) and skipped with a diagnostic unless `--force`; (2) **no-op =
no write** — a second `generate` over unchanged source writes zero files and touches zero mtimes; (3) **watch
doesn't loop** — gnr8's own writes to the configured output paths are filtered out of the watch event stream
so regeneration never re-triggers itself. Each maps to a pure, testable decision function plus a thin I/O shell.

The only non-trivial new runtime dependency is the file watcher. **Recommend `notify-debouncer-full` 0.7.0**
(which re-exports and pins `notify ^8.2.0`), because it solves the macOS FSEvents "double-fire" and rename-pair
problems that a hand-rolled debounce would have to re-implement, and its MSRV (1.85) exactly matches the project
floor. For content hashing, **recommend `blake3` 1.8.5** for fast, collision-resistant, stable digests (std's
`DefaultHasher` is explicitly unsuitable — its output is not stable across Rust releases and it is not designed
for content identity). For the config format, **recommend `toml` 1.1** as the PoC stand-in — framed honestly in
docs as a temporary surface, not the long-term code-as-config UX.

**Primary recommendation:** Build a pure-core / thin-shell split. Put `Workspace` (init/layout), `Config`
(TOML parse), `Manifest` (load/save/diff), and a pure `plan_writes(outputs, manifest, on_disk) -> WritePlan`
decision function in `gnr8-core`; put the `notify` watch loop and `Ctrl-C` handling in the `gnr8` binary.
Test the decision function exhaustively in-process; smoke the watch shell with one timing-tolerant integration test.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| `.gnr8/` scaffolding (`init`) | `gnr8-core` (`workspace`) | `gnr8` binary (CLI arm) | Pure filesystem layout + idempotency logic is library-testable; binary only dispatches |
| Config parse (TOML → typed `Config`) | `gnr8-core` (`config`) | — | Typed deserialization with `CoreError::Config` belongs in the library (RUST-04) |
| Ownership manifest (load/save/diff) | `gnr8-core` (`manifest`) | — | Hashing + state diff is pure logic; no I/O orchestration concerns leak to the binary |
| Write decision (no-op / clobber / overwrite) | `gnr8-core` (`plan_writes`) | — | A pure function over (new bytes, recorded hash, on-disk bytes) — the heart of WS-04/WATCH-01 |
| Materializing writes to disk | `gnr8-core` (`apply_writes`) | — | Extends the existing `sdk::write_to_dir` path-safety pattern |
| Watch loop + debounce | `gnr8` binary | `gnr8-core` (reuses `plan_writes`) | `notify` event handling + signal handling is binary-boundary concern (anyhow lives here, D-09) |
| Latency measurement | `gnr8` binary | `gnr8-core` (returns counts) | `Instant` wall-clock wraps the binary's call into core; core returns a `GenerateOutcome` with counts |
| Ctrl-C shutdown | `gnr8` binary | — | Signal handling is inherently a binary concern |

**Tier-correctness note for the planner:** the watch *loop* and signal handling are the ONLY parts that
belong in the binary. Everything else (layout, config, manifest, the write decision) is pure core logic and
must be unit-testable without spawning a watcher or touching `notify`. This split is what makes WATCH-02
testable (you test the decision, you smoke the shell).

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `notify-debouncer-full` | 0.7.0 | Cross-platform FS watching + production-grade debounce | Official `notify-rs` debouncer; re-exports `notify ^8.2.0`; merges rename pairs, dedups create/modify, collapses dir-removal — solves macOS FSEvents double-fire that a hand-roll would re-implement. MSRV 1.85 = project floor. [VERIFIED: crates.io registry + docs.rs] |
| `blake3` | 1.8.5 | Content hashing for the ownership manifest + no-op detection | Fast, collision-resistant, stable digest. std `DefaultHasher` output is NOT stable across Rust releases (unsuitable for a persisted manifest). 30.9M recent downloads. [VERIFIED: crates.io registry] |
| `toml` | 1.1 | Parse the checked-in `.gnr8/config.toml` (PoC config stand-in) | Idiomatic Rust TOML deserializer; integrates with the already-pinned `serde` derive. Framed as a PoC surface, not the long-term UX (D-03). 159M recent downloads. [VERIFIED: crates.io registry] |
| `serde` + `serde_json` | 1.0 (already pinned) | Manifest (de)serialization (JSON) + config struct derives | Already a workspace dependency; manifest is `manifest.json` for human-diffability + tooling. [VERIFIED: workspace Cargo.toml] |
| `thiserror` | 2.0 (already pinned) | New typed `CoreError` variants (workspace/config/manifest/io) | Established library-error pattern (D-09 / RUST-04). [VERIFIED: workspace Cargo.toml] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ctrlc` | 3.5.2 | Graceful `Ctrl-C` shutdown of the watch loop | If the std-only `AtomicBool` + handler approach (see §4) proves insufficient. Optional — prefer std first. 17.7M recent downloads. [VERIFIED: crates.io registry] |
| `notify` | 8.2.0 | The watcher itself | Pulled in transitively by `notify-debouncer-full` (`notify ^8.2.0`); only add a direct line if you bypass the debouncer. [VERIFIED: sparse index — debouncer-full 0.7.0 deps on `notify ^8.2.0`] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `notify-debouncer-full` 0.7.0 | `notify-debouncer-mini` 0.7.0 | Mini is lighter (just time-bucketing, MSRV 1.77) but does NOT match rename pairs, dedup create/modify, or collapse directory-removal events. For a Go *source* watch where a single editor save can fire create+modify+rename (atomic-save editors), full's dedup materially reduces redundant regenerations. Mini is acceptable if the planner wants the smallest surface; **full is recommended.** |
| `notify-debouncer-full` | raw `notify` 8.2.0 + hand-rolled debounce | Hand-rolling means re-implementing FSEvents double-fire coalescing and timer-bucketing correctly. The CONTEXT explicitly names `notify` + debounce (D-06); a debouncer crate from the same `notify-rs` org is the lower-risk, less-code path. Hand-roll only if a debouncer crate is rejected at package-legitimacy review (it passed — see audit). |
| `blake3` | std `std::hash::DefaultHasher` (SipHash) | DefaultHasher is a *hashmap* hasher: its algorithm/seed are not guaranteed stable across Rust versions, so a manifest written by one toolchain could mis-compare under another. Disqualified for a *persisted* content-identity manifest. |
| `blake3` | `sha2` 0.11 (SHA-256) | SHA-256 is equally stable and collision-resistant; blake3 is faster and the API is a one-liner (`blake3::hash(bytes)`). Either is defensible; **blake3 recommended** for speed on large SDK bundles. `sha2` is a fine fallback if the planner prefers a FIPS-family algorithm. |
| `blake3` | a raw byte comparison only (no hash) | For no-op you *could* just compare new bytes to on-disk bytes (no hash needed at all). But the manifest needs a compact persisted fingerprint to detect *user edits between runs* (WS-04) without re-reading every prior output — so a stored hash is required. Hash for the manifest; you may still short-circuit no-op with a direct byte compare. |
| `toml` config | a checked-in `.rs` / Rust-DSL file read "statically" | A real Rust file cannot be read "statically" without either compiling it (a build step / plugin = forbidden by PROJECT) or parsing Rust syntax (huge surface). TOML is the honest PoC compromise: declarative, user-editable, no dynamic execution. Document it as the stand-in. |
| `ctrlc` crate | std `signal` handling via a shared `AtomicBool` | The std approach (set a flag in a signal handler / break the recv loop on channel disconnect) avoids a dependency. Prefer std; reach for `ctrlc` only if cross-platform signal correctness becomes fiddly. |

**Installation (per crate, in the owning crate's `Cargo.toml`):**
```bash
# gnr8-core (library): hashing + config + new error variants (thiserror/serde already present)
cargo add --package gnr8-core blake3@1.8 toml@1.1
# gnr8 (binary): the watcher lives at the binary boundary alongside anyhow (D-09)
cargo add --package gnr8 notify-debouncer-full@0.7
# optional, only if std signal handling proves insufficient:
# cargo add --package gnr8 ctrlc@3.5
```

> **Pin via `[workspace.dependencies]`** to match the existing convention (root `Cargo.toml` is the single
> source of truth for versions). Add `blake3`, `toml`, `notify-debouncer-full` (and optionally `ctrlc`) there,
> then reference with `{ workspace = true }` in the member crates.

**Version verification (run 2026-06-24 against crates.io API + sparse index):**
```
notify                 max_stable: 8.2.0   (newest 9.0.0-rc.4 is a release CANDIDATE — do NOT use)
notify-debouncer-full  max_stable: 0.7.0   (newest 0.8.0-rc.2 is rc — do NOT use); deps notify ^8.2.0; MSRV 1.85
notify-debouncer-mini  max_stable: 0.7.0   deps notify ^8.2.0; MSRV 1.77
blake3                 max_stable: 1.8.5
sha2                   max_stable: 0.11.0
toml                   max_stable: 1.1.2+spec-1.1.0
ctrlc                  max_stable: 3.5.2
```
Project toolchain is rustc 1.96.0; MSRV floor is 1.85 — every recommended crate's MSRV (≤1.85) is satisfied.

## Package Legitimacy Audit

> slopcheck 0.6.1 installed and run against a probe `Cargo.toml` with `--ecosystem crates.io`. All clean.

| Package | Registry | Age | Downloads (recent) | Source Repo | slopcheck | Disposition |
|---------|----------|-----|--------------------|-------------|-----------|-------------|
| `notify` | crates.io | 11 yrs (since 2014) | 32.3M | github.com/notify-rs/notify | [OK] | Approved (transitive via debouncer) |
| `notify-debouncer-full` | crates.io | 3+ yrs | 2.9M | github.com/notify-rs/notify | [OK] | **Approved (primary)** |
| `notify-debouncer-mini` | crates.io | 3+ yrs | 3.4M | github.com/notify-rs/notify | [OK] | Approved (alternative) |
| `blake3` | crates.io | 5+ yrs | 30.9M | github.com/BLAKE3-team/BLAKE3 | [OK] | **Approved (primary hash)** |
| `sha2` | crates.io | 9+ yrs | 179.7M | github.com/RustCrypto/hashes | [OK] | Approved (hash fallback) |
| `toml` | crates.io | 9+ yrs | 159.0M | github.com/toml-rs/toml | [OK] | **Approved (config)** |
| `ctrlc` | crates.io | 8+ yrs | 17.7M | github.com/Detegr/rust-ctrlc | [OK] | Approved (optional) |

**Packages removed due to slopcheck [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** none. All seven are long-lived, high-download, source-backed crates
from well-known orgs (`notify-rs`, `BLAKE3-team`, `RustCrypto`, `toml-rs`). No `postinstall`-equivalent risk
in the Cargo ecosystem for these (no build-script supply-chain red flags noted). No `checkpoint:human-verify`
gating required by the legitimacy protocol — but see the cross-crate version-compat note in §6.

## Architecture Patterns

### System Architecture Diagram

```
                            gnr8 binary (anyhow boundary, D-09)
  ┌──────────────────────────────────────────────────────────────────────────────┐
  │  CLI dispatch (cli.rs / main.rs)                                                │
  │     init │ generate │ watch │ check                                             │
  └───┬───────────┬───────────────┬──────────────────┬─────────────────────────────┘
      │           │               │                  │
      │ init      │ generate      │ watch            │ check
      ▼           ▼               ▼                  ▼
  ┌────────┐  ┌──────────────────────────────────────────────────────────────────┐
  │workspace│ │  load Config (.gnr8/config.toml)  ──►  resolve input/output paths │
  │ ::init  │ └───────────────┬──────────────────────────────────────────────────┘
  │(idempot)│                 │
  └────────┘                  ▼
                    gnr8-core PIPELINE (Phase 2–3, deterministic)
              build_graph(input) ─► to_openapi(graph)  ─┐
                                  └► sdk::generate(graph)─┤  new output bytes (per path)
                                                          ▼
                          ┌───────────────────────────────────────────────┐
            manifest.json │  plan_writes(new_outputs, manifest, on_disk)   │  ← PURE FN (core)
            (cache, load) │   per file → Written | Unchanged(no-op)        │     heart of WS-04/WATCH-01
                          │            | UserEdited(warn+skip) | Forced    │
                          └──────────────────────┬────────────────────────┘
                                                 ▼
                          apply_writes(plan, --force)  ─► disk (user-config paths)
                                                 ▼
                          update manifest (path → blake3 hash) ─► .gnr8/cache/manifest.json
                                                 ▼
                          GenerateOutcome { written, unchanged, skipped }  ─► report (human/--json)

   WATCH path only ─ the binary's loop:
     notify-debouncer-full(input dirs, RecursiveMode::Recursive, debounce ~200ms)
        ──► DebouncedEvent batch ──► FILTER OUT events whose path ∈ output set / manifest  (loop-safety, D-06)
        ──► if any *source* path changed: time the generate pipeline above, print latency (WATCH-03)
        ──► repeat until Ctrl-C (AtomicBool flag / channel disconnect) ──► clean exit
```

The reader can trace the primary use case: a source edit enters via `notify`, is debounced, filtered against
output paths (so gnr8's own writes never re-trigger), runs the deterministic pipeline, the *pure* `plan_writes`
decides per-file Written/Unchanged/UserEdited, only changed files hit disk, the manifest updates, and a latency
line prints.

### Recommended Project Structure

```
crates/gnr8-core/src/
├── workspace/mod.rs     # .gnr8/ layout constants, init (idempotent scaffold), .gitignore body  (WS-01/02)
├── config/mod.rs        # typed Config (serde+toml), load_or_default, path resolution            (WS-03)
├── manifest/mod.rs      # Manifest schema (path→entry), load/save (serde_json), blake3 hashing    (WS-04)
├── lifecycle/mod.rs     # plan_writes (pure decision fn) + apply_writes + GenerateOutcome counts  (WS-04/WATCH-01)
├── error.rs             # + Workspace / Config / Manifest / Io CoreError variants                 (D-09)
├── lower/  sdk/  graph/  analyze/   # unchanged Phase 2–3 pipeline (consumed, not modified)

crates/gnr8/src/
├── cli.rs               # add --force to generate/check; watch flags (debounce ms, --json already global)
├── main.rs              # wire init/generate/check arms to core; OWN the watch loop + Ctrl-C
└── watch.rs (new)       # notify-debouncer-full shell: build debouncer, filter, call core, time, report

crates/gnr8-core/tests/
├── lifecycle.rs (new)   # plan_writes truth table; manifest round-trip; init idempotency (hermetic temp dir)
crates/gnr8/tests/
└── watch_smoke.rs (new) # one timing-tolerant integration test: edit a temp file → one regen, no loop
```

### `.gnr8/` Directory Tree (D-01, D-02 — recommended)

```
<project-root>/
├── .gnr8/
│   ├── config.toml          # CHECKED IN — the PoC code-as-config surface (inputs/outputs/naming)  (WS-03)
│   ├── .gitignore           # CHECKED IN — auto-written; ignores the lifecycle subdir              (WS-02)
│   └── cache/               # GIT-IGNORED — lifecycle state
│       └── manifest.json    #   ownership manifest (path → blake3 hash + provenance)               (WS-04)
├── openapi.yaml             # GENERATED OUTPUT — user-configured path, tracked in manifest (D-02)
└── sdk/                     # GENERATED OUTPUT — user-configured dir, each file tracked (D-02)
    ├── client.go  errors.go  goals.go  models.go
```

**Auto-written `.gnr8/.gitignore` body (exact contents `init` should emit):**
```gitignore
# gnr8 lifecycle state — regenerated, do not commit.
/cache/
```
Rationale: the `.gitignore` lives *inside* `.gnr8/`, so its patterns are relative to `.gnr8/`. Ignoring
`/cache/` (leading slash = anchored to this dir) hides the manifest + any future cache while keeping
`config.toml` and the `.gitignore` itself checked in. This realizes D-01's "automatic split" with one file.
Generated *outputs* (`openapi.yaml`, `sdk/`) live OUTSIDE `.gnr8/` (D-02) and are intentionally **committed**
by the user (they are the product); they are tracked in the manifest for ownership, not gitignored.

### Pattern 1: Idempotent init (WS-01) — write-if-absent, never clobber

**What:** `init` creates each workspace file only if it does not already exist; an already-initialized
workspace is a successful no-op with a clear message, not an error and not an overwrite.
**When to use:** the `gnr8 init` command body.
**Example:**
```rust
// gnr8-core/src/workspace/mod.rs  — pattern, not final code
use std::path::Path;

/// Outcome of `init` so the CLI can report created vs already-present without re-reading disk.
pub struct InitOutcome {
    pub created: Vec<String>,   // relative paths newly written
    pub skipped: Vec<String>,   // relative paths that already existed (left untouched)
}

/// Scaffold .gnr8/ idempotently: create the dir tree, then write each file ONLY if absent.
///
/// # Errors
/// Returns [`CoreError::Workspace`] if a directory cannot be created or a file cannot be written.
pub fn init(root: &Path) -> Result<InitOutcome, crate::CoreError> {
    let gnr8 = root.join(".gnr8");
    let cache = gnr8.join("cache");
    create_dir_all(&cache)?;                       // mkdir -p — idempotent by nature
    let mut outcome = InitOutcome { created: vec![], skipped: vec![] };
    write_if_absent(&gnr8.join("config.toml"), DEFAULT_CONFIG_TOML, &mut outcome)?;
    write_if_absent(&gnr8.join(".gitignore"),  GITIGNORE_BODY,      &mut outcome)?;
    Ok(outcome)
}

/// Write `body` to `path` only if it does not exist; record created/skipped. Never clobbers user edits.
fn write_if_absent(path: &Path, body: &str, out: &mut InitOutcome) -> Result<(), crate::CoreError> {
    if path.exists() {
        out.skipped.push(rel(path));
        return Ok(());                              // idempotent: re-running init preserves edits (D-01)
    }
    std::fs::write(path, body).map_err(|e| crate::CoreError::Workspace {
        message: format!("failed to write {}: {e}", path.display()),
    })?;
    out.created.push(rel(path));
    Ok(())
}
```
**Note on TOCTOU:** `path.exists()` then `write` has a benign race for an interactive CLI (single user,
single invocation). For strictness you can use `OpenOptions::new().write(true).create_new(true)` which
atomically fails with `AlreadyExists` if the file appears — treat that error as "skipped". Prefer
`create_new` over `exists()` for the write-if-absent guarantee.

### Pattern 2: The pure write-decision function (WS-04 + WATCH-01 core) — `plan_writes`

**What:** a pure function that, given the freshly generated bytes per output path, the recorded manifest,
and what is currently on disk, classifies each file. This is the single most important testable unit in the phase.
**When to use:** every `generate` and every watch-triggered regeneration.
**Example:**
```rust
// gnr8-core/src/lifecycle/mod.rs  — pattern, not final code
/// Per-file classification produced by [`plan_writes`].
pub enum WriteAction {
    /// New file (not in manifest) or recorded file whose bytes changed → write it.
    Write,
    /// On-disk bytes == new bytes (and == recorded hash) → skip the write (no-op, WATCH-01 / D-05).
    Unchanged,
    /// On-disk hash != recorded hash → a human edited a generated file. Warn + skip unless --force (WS-04 / D-04).
    UserEdited,
}

pub struct PlannedFile { pub path: String, pub action: WriteAction, pub new_bytes: Vec<u8>, pub new_hash: String }
pub struct WritePlan   { pub files: Vec<PlannedFile> }

/// Decide what to do with each generated output. PURE: no I/O — the caller supplies on-disk bytes.
/// `on_disk` is None when the file is absent. `manifest` is the previous-run record.
pub fn plan_writes(
    new_outputs: &[(String, Vec<u8>)],          // (path, freshly generated bytes) — deterministic (Phase 2–3)
    manifest: &Manifest,
    on_disk: &dyn Fn(&str) -> Option<Vec<u8>>,  // injected reader → trivially mockable in tests
) -> WritePlan {
    let mut files = Vec::new();
    for (path, new_bytes) in new_outputs {
        let new_hash = blake3_hex(new_bytes);
        let action = match (on_disk(path), manifest.recorded_hash(path)) {
            // file absent on disk → always write (fresh generation or user deleted it)
            (None, _) => WriteAction::Write,
            // present, and its CURRENT hash != what we last wrote → user hand-edited it → protect it
            (Some(disk), Some(recorded)) if blake3_hex(&disk) != recorded => WriteAction::UserEdited,
            // present, matches what we last wrote, and new == disk → byte-identical → no-op skip
            (Some(disk), Some(_)) if disk == *new_bytes => WriteAction::Unchanged,
            // present, gnr8-owned, but content changed → write the update
            (Some(_), Some(_)) => WriteAction::Write,
            // present but NOT in manifest → gnr8 has never written here → treat as a user file → protect
            (Some(_), None) => WriteAction::UserEdited,
        };
        files.push(PlannedFile { path: path.clone(), action, new_bytes: new_bytes.clone(), new_hash });
    }
    WritePlan { files }
}
```
**Why this design is robust (the "user edit vs gnr8 edit" question, D-04):** gnr8 only considers a file
"its own" if (a) it is in the manifest AND (b) the on-disk hash still equals what gnr8 last recorded. The
instant a human edits a tracked file, the on-disk hash diverges from the recorded hash and it is reclassified
`UserEdited` → warn+skip. A path that is NOT in the manifest is treated as a user file even if it sits at an
output location (defense against clobbering a pre-existing hand-written `openapi.yaml`). `--force` is applied
by `apply_writes` (overwrite `UserEdited` anyway), NOT by `plan_writes`, so the classification stays pure and
the force policy stays in one place.

### Pattern 3: apply_writes — only changed files hit disk, then update the manifest

**What:** the impure half. Iterates the `WritePlan`, writes `Write` (and, under `--force`, `UserEdited`)
files, skips `Unchanged`/`UserEdited`, then records new hashes and drops manifest entries for paths no
longer produced (D-04: "deleting a file from config drops it from the manifest").
**Example pattern:**
```rust
pub struct GenerateOutcome { pub written: Vec<String>, pub unchanged: Vec<String>, pub skipped: Vec<String> }

pub fn apply_writes(plan: &WritePlan, manifest: &mut Manifest, force: bool)
    -> Result<GenerateOutcome, crate::CoreError>
{
    let mut out = GenerateOutcome::default();
    for f in &plan.files {
        match f.action {
            WriteAction::Write => { fs_write_safe(&f.path, &f.new_bytes)?; manifest.record(&f.path, &f.new_hash); out.written.push(f.path.clone()); }
            WriteAction::Unchanged => { out.unchanged.push(f.path.clone()); }                  // no write, no mtime churn
            WriteAction::UserEdited if force => { fs_write_safe(&f.path, &f.new_bytes)?; manifest.record(&f.path, &f.new_hash); out.written.push(f.path.clone()); }
            WriteAction::UserEdited => { out.skipped.push(f.path.clone()); }                     // warn at CLI layer
        }
    }
    manifest.prune_to(&plan.files);   // drop entries for paths no longer generated (D-04)
    Ok(out)
}
```
Reuse the **`sdk::write_to_dir` path-safety pattern** (reject empty / `/` / `\` / `..` names) when writing —
Phase 3 already established that defense-in-depth for program-controlled paths (T-03-03). Output paths here
come from user config, so path validation matters *more*: resolve them against the project root and reject
escapes.

### Pattern 4: Loop-safe watch (WATCH-02 / D-06) — filter own writes by path

**What:** `notify` (and every FS watcher) reports *all* changes under the watched tree, including writes gnr8
itself just made. There is **no built-in "ignore my own writes"** [VERIFIED: notify 8.2.0 docs + web search].
You prevent the regeneration loop by (a) watching only the configured *source* dirs, not the output dirs,
and (b) filtering each debounced event batch to drop any path that is an output path / in the manifest.
**Example pattern:**
```rust
// gnr8/src/watch.rs — pattern, not final code
let output_set: HashSet<PathBuf> = config.output_paths();   // openapi.yaml + sdk/*  (absolute, canonicalized)
let debouncer = new_debouncer(Duration::from_millis(200), None, move |res: DebounceEventResult| {
    let Ok(events) = res else { return; /* log errors, don't crash the loop */ };
    let touched_source = events.iter().any(|ev| {
        ev.paths.iter().any(|p| {
            let p = canonicalize(p);
            !is_under_any(&p, &output_set)          // DROP gnr8's own output writes → loop-safe (D-06)
              && is_go_source(&p)                   // only react to *.go edits (supported source)
        })
    });
    if touched_source { tx.send(()).ok(); }          // coalesced "regenerate" signal
})?;
debouncer.watch(&config.input_dir, RecursiveMode::Recursive)?;   // recursive watch of SOURCE dirs only
```
Two-layer defense: **don't watch the output dirs at all** (primary), and **filter by output-path membership**
(belt-and-braces, since a user may configure outputs *inside* a watched source tree). `notify-debouncer-full`
additionally dedups create/modify and merges rename pairs, so atomic-save editors (write temp → rename) don't
double-fire.

### Anti-Patterns to Avoid

- **`std::hash::DefaultHasher` for the persisted manifest:** its output is a hashmap hash, not stable across
  Rust releases — a manifest written under one toolchain could mis-classify under another. Use `blake3`/`sha2`.
- **Watching the output directories:** guarantees a regeneration loop on macOS FSEvents. Watch source dirs only.
- **Putting watch-loop logic in `gnr8-core`:** the loop + `notify` + signal handling belong at the binary
  boundary (D-09). Core exposes a *pure* `plan_writes` and a `regenerate(config) -> GenerateOutcome`; the
  binary times and loops. Keeps core testable without spawning watchers.
- **`unwrap()` on `notify`/`Instant`/`recv` in production:** every recommended crate's fallible call must
  flow through `?` into a typed error or be logged-and-continued in the loop (RUST-04, clippy `-D warnings`).
- **Comparing mtimes for no-op:** mtime is not content identity (and the goal is to *avoid* mtime churn).
  Compare bytes / hashes (D-05).
- **Hand-rolling debounce when a same-org debouncer exists and passed legitimacy review:** re-implements
  FSEvents coalescing for no benefit.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cross-platform FS watching | A per-OS `inotify`/`FSEvents`/`kqueue` wrapper | `notify` 8.2.0 (via `notify-debouncer-full`) | Three OS backends, 11 yrs of edge-case fixes, 32M downloads. Re-implementing is months of platform bugs. |
| Debounce + rename-pair coalescing | A timer-bucket + manual rename matching | `notify-debouncer-full` 0.7.0 | Merges rename from/to, dedups create+modify, collapses dir-removal, tracks file IDs across platforms. |
| Stable content hashing | A custom checksum or `DefaultHasher` | `blake3` 1.8.5 (or `sha2`) | Collision resistance + cross-toolchain stability are non-negotiable for a persisted manifest. |
| TOML parsing | A hand-written config parser | `toml` 1.1 + `serde` derive | Spec-compliant parsing, typed errors, zero custom lexer. |
| Graceful Ctrl-C (if std insufficient) | Raw `sigaction` FFI | `ctrlc` 3.5.2 | Cross-platform signal handling done correctly; but try the std `AtomicBool` approach first. |
| SDK file writing with path safety | New disk-writer | Extend existing `sdk::write_to_dir` pattern | Phase 3 already established frame-name path-safety (T-03-03); reuse it for output writes. |

**Key insight:** the only genuinely hard problem in this phase is cross-platform watch + debounce, and the
`notify-rs` org already owns it. Everything else (init, manifest, no-op) is small *pure* logic built on std
`fs` — keep it in core, keep it testable, and lean on the Phase 2–3 determinism guarantee so no-op detection
is a byte compare, not a heuristic.

## Runtime State Inventory

> This is a greenfield-feature phase (new lifecycle on top of a working pipeline), NOT a rename/refactor/
> migration. There is no pre-existing renamed string, datastore, OS-registration, or build artifact being
> migrated. The new state this phase *creates* (the manifest) is covered below for completeness.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None being migrated. This phase *introduces* `.gnr8/cache/manifest.json` (new state, not migrated). | Manifest is created on first `generate`; absent-manifest = treat all outputs as fresh (graceful). |
| Live service config | None — gnr8 has no external service, daemon, or remote config. | None. |
| OS-registered state | None — `gnr8 watch` is a foreground process (no daemon/launchd/systemd registration in PoC). Ctrl-C exits it. | None (deferring any "run as service" to post-PoC). |
| Secrets/env vars | None — no secrets. `GOPROXY=off`/`GOFLAGS` env are set only inside Phase-3 test subprocesses, untouched here. | None. |
| Build artifacts | New runtime crates land in `Cargo.lock`. The Phase-3 hermetic-temp pattern (no `tempfile` crate) is the precedent for tests. | Commit `Cargo.lock` updates; no stale artifact carries an old name. |

**Verified by:** reading `crates/gnr8-core/src/*` (no datastore, no daemon), `Cargo.toml` files (deps enumerated),
and the Phase-3 SUMMARY (hermetic temp-dir test pattern, no persistent state).

## Common Pitfalls

### Pitfall 1: Watch regeneration loop on macOS (FSEvents reports gnr8's own writes)
**What goes wrong:** gnr8 writes `sdk/models.go`, FSEvents reports the write, the watcher regenerates, writes
again — infinite loop, pegged CPU.
**Why it happens:** no FS watcher has a built-in "ignore my own writes" [VERIFIED: notify docs + search];
FSEvents is especially eager (batches + double-fires).
**How to avoid:** (1) watch only the configured *source* dirs, never output dirs; (2) filter each debounced
batch to drop any path that is an output path or in the manifest; (3) use `notify-debouncer-full` to coalesce.
**Warning signs:** `watch` never goes idle after the first generation; repeated identical latency lines with no
source edit. Add a smoke test that edits one source file once and asserts exactly one regeneration.

### Pitfall 2: Atomic-save editors fire create+rename+modify, causing duplicate regenerations
**What goes wrong:** vim/IntelliJ/VS Code save a Go file by writing a temp file then renaming over the target;
a naive watcher sees 2–3 events and regenerates 2–3 times.
**Why it happens:** atomic save is a multi-syscall dance the FS reports as multiple events.
**How to avoid:** `notify-debouncer-full` matches rename pairs and dedups create/modify within the debounce
window — this is the concrete reason to choose *full* over *mini*. Use a debounce window ≥150ms (recommend 200ms).
**Warning signs:** N regenerations per single save in the smoke test.

### Pitfall 3: Flaky timing-based watch tests
**What goes wrong:** a test that "edit file, sleep 100ms, assert regenerated" passes locally, flakes in CI
(slower FS, longer FSEvents latency, debounce coalescing).
**Why it happens:** FS event delivery latency is non-deterministic and platform-dependent.
**How to avoid:** **test the pure core, smoke the shell.** Unit-test `plan_writes` and the manifest exhaustively
(no timing). For the one watch integration test: use a generous timeout (e.g. poll for up to 5s with a
`recv_timeout`), assert "≥1 and exactly the expected file set regenerated," not exact timing, and gate it so it
can be `#[ignore]`d or skipped where FS events are unreliable. Mirror the Phase-3 graceful-skip precedent.
**Warning signs:** intermittent CI red on the watch test only.

### Pitfall 4: `DefaultHasher` (or any unstable hash) in the persisted manifest
**What goes wrong:** the manifest written by toolchain A mis-compares under toolchain B → false "user edited"
warnings or false no-ops.
**Why it happens:** `std::hash::DefaultHasher` is a hashmap hasher with no cross-version stability guarantee.
**How to avoid:** use `blake3` (or `sha2`). Store the hex digest in the manifest. Both are stable forever.
**Warning signs:** ownership warnings appear after a `rustup update` with no actual file edits.

### Pitfall 5: Clobbering a pre-existing hand-written output on first run
**What goes wrong:** a user already has an `openapi.yaml`; the first `gnr8 generate` overwrites it silently.
**Why it happens:** on first run the manifest is empty, so a naive writer treats every output path as gnr8-owned.
**How to avoid:** `plan_writes` classifies a path that is **present on disk but absent from the manifest** as
`UserEdited` (protect it) — see Pattern 2's `(Some(_), None) => UserEdited` arm. gnr8 only owns paths it has
recorded writing. The user opts in with `--force`.
**Warning signs:** a user's pre-existing spec vanishes after the first `generate`.

### Pitfall 6: Output paths configured *inside* a watched source tree
**What goes wrong:** a user sets `sdk/` under a directory that is also a watched Go source root → outputs land
inside the watch tree → loop risk even with "watch source dirs only."
**Why it happens:** user config is free-form; nothing forces outputs outside source.
**How to avoid:** keep the output-path filter (Pattern 4 belt-and-braces) even though you watch source dirs;
optionally emit a diagnostic if an output path resolves under a watched input dir.
**Warning signs:** the loop reappears despite watching only source dirs.

## Code Examples

### Hashing a generated output (blake3)
```rust
// Source: docs.rs/blake3 (1.8.5) — blake3::hash returns a Hash; to_hex() gives a stable lowercase digest.
fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()   // 64-char lowercase hex; stable across toolchains
}
```

### Constructing the debouncer (notify-debouncer-full 0.7.0)
```rust
// Source: docs.rs/notify-debouncer-full/0.7.0 — new_debouncer(Duration, Option<FileIdCache>, callback).
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use notify::RecursiveMode;
use std::time::Duration;

let mut debouncer = new_debouncer(
    Duration::from_millis(200),        // debounce window (recommend 150–250ms)
    None,                              // default file-ID cache
    move |result: DebounceEventResult| {
        match result {
            Ok(events) => { /* filter output paths, signal regenerate */ }
            Err(errors) => { /* log, do NOT panic — keep the loop alive */ }
        }
    },
)?;                                    // ? into the binary's anyhow boundary
debouncer.watch(source_dir, RecursiveMode::Recursive)?;   // recursive watch of SOURCE only
// Keep `debouncer` alive for the lifetime of the watch loop; dropping it stops watching.
```

### Recommended raw watcher (only if NOT using a debouncer)
```rust
// Source: docs.rs/notify/8.2.0 — recommended_watcher picks FSEvents (macOS) / inotify (Linux) / ReadDirectoryChangesW (Win).
use notify::{recommended_watcher, RecursiveMode, Watcher, Event};
use std::sync::mpsc;
let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
let mut watcher = recommended_watcher(tx)?;
watcher.watch(std::path::Path::new(source_dir), RecursiveMode::Recursive)?;
// PollWatcher fallback for network/edge filesystems is available via notify::Config + PollWatcher.
```

### Latency measurement (std::time::Instant, WATCH-03)
```rust
// Source: std::time::Instant — monotonic wall-clock around the pipeline + write.
let t0 = std::time::Instant::now();
let outcome = gnr8_core::lifecycle::regenerate(&config, force)?;   // build_graph → lower/sdk → plan/apply
let elapsed = t0.elapsed();
// Human:
println!("regenerated in {:.1?}  ({} written, {} unchanged)", elapsed, outcome.written.len(), outcome.unchanged.len());
// --json (fits the existing global --json flag; serialize a small struct):
#[derive(serde::Serialize)]
struct LatencyReport<'a> { scenario: &'a str, millis: u128, written: usize, unchanged: usize }
```
Measure the three WATCH-03 scenarios honestly as wall-clock around the same `regenerate` call: **cold** = first
run (empty manifest, all files written); **warm no-op** = immediate re-run with no source change (all
`Unchanged`, zero writes); **single-file edit** = one source file changed under watch (subset regenerated). No
synthetic instrumentation — just `Instant::now()` / `.elapsed()` around the real call.

### Recommended `Config` serde struct (PoC TOML stand-in, WS-03)
```rust
// gnr8-core/src/config/mod.rs — the documented knobs ONLY (D-03). Reserved seam for v2, not faked.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]   // reject typos / unsupported keys with a clear error (V5 input validation)
pub struct Config {
    /// Go source input directory(ies) to analyze (relative to project root).
    pub inputs: Vec<String>,
    pub output: OutputConfig,
    #[serde(default)]
    pub naming: NamingOverrides,    // operation/type name remaps — the one customization knob built here
    // NOTE: routing-recognition / transport / emitter customization "through code" is v2 (ADV-02).
    // It is deliberately NOT a field here — adding an empty stub would overclaim. Document the seam in prose.
}
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    pub openapi: String,            // e.g. "openapi.yaml"
    pub sdk_dir: String,            // e.g. "sdk"
    pub go_module: String,          // Go module path for the generated SDK, e.g. "github.com/acme/svc/sdk"
}
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingOverrides {
    #[serde(default)]
    pub operations: std::collections::BTreeMap<String, String>,   // operation_id remaps (sorted = deterministic)
    #[serde(default)]
    pub types: std::collections::BTreeMap<String, String>,        // schema/type name remaps
}
```
**Default `config.toml` body that `init` writes (checked in):**
```toml
# gnr8 PoC configuration (code-as-config stand-in — see docs; NOT the long-term UX).
inputs = ["."]                          # Go source dir(s) to analyze

[output]
openapi   = "openapi.yaml"              # OpenAPI artifact path (project-relative)
sdk_dir   = "sdk"                       # generated Go SDK directory
go_module = "example.com/yourservice/sdk"   # Go module path for the generated SDK

# [naming.operations]                   # optional: remap operation ids, e.g.
# goalUuidPut = "UpdateGoal"
# [naming.types]                        # optional: remap generated type names, e.g.
# CreateGoalInput = "NewGoal"
```

### Recommended manifest schema (WS-04)
```jsonc
// .gnr8/cache/manifest.json — git-ignored. Sorted keys for deterministic diffs (mirrors graph sorting policy).
{
  "version": 1,
  "files": [
    { "path": "openapi.yaml",   "hash": "blake3:<64-hex>", "source": "openapi"  },
    { "path": "sdk/client.go",  "hash": "blake3:<64-hex>", "source": "sdk"      },
    { "path": "sdk/models.go",  "hash": "blake3:<64-hex>", "source": "sdk"      }
  ]
}
```
```rust
// gnr8-core/src/manifest/mod.rs — typed, serde_json (already pinned). Vec<Entry> sorted by path = byte-stable.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Manifest { pub version: u32, pub files: Vec<ManifestEntry> }
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ManifestEntry { pub path: String, pub hash: String, pub source: String }
```
The `source` field carries provenance ("openapi" | "sdk") so `doctor` (Phase 5) and diagnostics can attribute
a stale/edited file to its generator. `version` future-proofs the schema. Sort `files` by `path` before writing
(consistent with the graph's sorted-collection determinism policy, GRAPH-02).

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `notify` 4.x `DebouncedEvent` enum + built-in `watcher_with_debounce` | `notify` 5+/8.x emits a flat `Event`/`EventKind`; debounce moved to **separate** `notify-debouncer-{mini,full}` crates | notify 5.0 (2022) | Old `docs.rs/notify/4.0.x/DebouncedEvent` examples are stale — use a debouncer crate, not a built-in. |
| `notify` 6.x | `notify` 8.2.0 stable (9.0 is rc only) | 2024–2026 | Pin 8.2.0; do NOT pull the `9.0.0-rc.4` release candidate. |
| Hashing with ad-hoc checksums | `blake3`/`sha2` for content identity | — | Stable, fast, collision-resistant; std `DefaultHasher` is not for persistence. |

**Deprecated/outdated:**
- **`notify::DebouncedEvent` (notify 4.x):** removed in notify 5+. Search results surface 4.0.x docs first —
  ignore them. The current debounce API is `notify_debouncer_full::{new_debouncer, DebouncedEvent}`.
- **`notify` 9.0.0-rc.x and `notify-debouncer-full` 0.8.0-rc.x:** release candidates, not stable. Use 8.2.0 / 0.7.0.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | 200ms is a good default debounce window | §4, Pitfall 2 | Low — tunable; too low = duplicate regens, too high = laggy feel. Make it a config/flag knob with a 200ms default. |
| A2 | A foreground `gnr8 watch` (no daemon) is the intended PoC scope | Runtime State Inventory | Low — matches "fast regeneration loop" goal; daemonization is clearly post-PoC. Confirm no daemon expectation. |
| A3 | TOML is acceptable as the PoC config stand-in given "YAML/TOML/JSON not the main UX" | §2, WS-03 | Medium — PROJECT says TOML/JSON "must not be the main customization surface." This is honored by framing TOML as a temporary PoC knob-holder with the real customization (code) deferred to v2. The planner/discuss should confirm TOML (vs a minimal bespoke format) is acceptable as the explicit stand-in. |
| A4 | `--force` is the chosen override verb (vs `--overwrite`) for clobbering user-edited files | §3, D-04 | Low — D-04 says "explicit `--force`"; naming is cosmetic. |
| A5 | std `AtomicBool` + channel-disconnect is sufficient for Ctrl-C; `ctrlc` crate is optional | §4, Supporting | Low — std approach is standard; `ctrlc` is a clean fallback if cross-platform signal correctness is fiddly. |
| A6 | The manifest lives at `.gnr8/cache/manifest.json` (git-ignored) | §1, D-04 | Low — D-04 explicitly suggests this path. |

**Note:** A3 is the one assumption that touches a PROJECT constraint and should be confirmed before it becomes
a locked decision — the rest are low-risk discretion items the planner can adopt directly.

## Open Questions

1. **Debounce window default + configurability**
   - What we know: 150–250ms is the typical range; `notify-debouncer-full` needs a `Duration`.
   - What's unclear: whether to expose it as a `watch --debounce-ms` flag, a config key, or a fixed constant.
   - Recommendation: ship a fixed 200ms default; add a `--debounce-ms` flag only if the smoke test or demo
     shows the default misbehaving. Don't over-configure the PoC.

2. **Which outputs does a single-file edit regenerate? (WATCH-03 "single-file edit" scenario)**
   - What we know: the pipeline currently regenerates the *whole* graph → whole OpenAPI + whole SDK bundle;
     no-op then skips unchanged files at write time.
   - What's unclear: whether "single-file edit latency" means "re-run full pipeline, write only changed files"
     (correct for the PoC) or "partial graph invalidation" (explicitly v2 / ADV-01).
   - Recommendation: measure single-file-edit latency as **full re-run + no-op-filtered writes** (the honest
     PoC behavior). Partial/incremental invalidation is deferred (ADV-01) — do not build it here. Report the
     wall-clock of the full re-run; the "only changed files written" is the no-op win, not a partial-graph win.

3. **`check` command semantics (WS-04-adjacent)**
   - What we know: the CLI has a `check` arm ("Verify generated outputs are up to date").
   - What's unclear: exact pass/fail contract.
   - Recommendation: `check` = run `plan_writes` in dry-run mode; exit non-zero if any file would be `Write`
     (outputs stale) or `UserEdited` (drifted). This reuses the exact same pure decision function as `generate`
     — zero new logic, and it gives Phase 5's `doctor` a building block. Worth a small task in 04-02 or 04-03.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | Whole phase | ✓ | rustc 1.96.0 / cargo 1.96.0 | — (project floor MSRV 1.85; all crates' MSRV ≤1.85) |
| Go toolchain (`go`/`gofmt`) | The wrapped Phase 2–3 pipeline (`build_graph`, `gofmt` in `sdk::generate`) | ✓ (per Phase-3 SUMMARY; CI uses go 1.26) | go 1.26 in CI | Tests skip gracefully when absent (established pattern); `generate`/`watch` surface `CoreError::GoToolchainMissing` at runtime |
| `notify` OS backend | `gnr8 watch` | ✓ (macOS FSEvents present; Linux inotify / Win ReadDirectoryChangesW on those platforms) | notify 8.2.0 picks per-OS | `PollWatcher` (`notify::Config` + `PollWatcher`) for network/edge FS |
| crates.io access (build-time) | Adding `notify-debouncer-full`/`blake3`/`toml` | ✓ (dev); CI must allow fetch | — | Vendoring if CI is offline (not expected) |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** the watch backend has a `PollWatcher` fallback for filesystems where
native events are unavailable (network mounts, some containers). The Go toolchain has a graceful-skip path in
tests but is a hard requirement at runtime (unchanged from Phases 2–3).

## Validation Architecture

> `workflow.nyquist_validation` is `true` in config.json — this section is required.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` (libtest) + `insta` 1.48 for snapshots (already pinned) |
| Config file | none (Cargo-native); `cargo test` / `make gates` / `make check` orchestrate |
| Quick run command | `cargo test -p gnr8-core --lib` (pure logic: workspace/config/manifest/plan_writes) |
| Full suite command | `make check` (fmt-check + clippy -D warnings + full test suite + Go fixture build/vet) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| WS-01 | `init` scaffolds `.gnr8/` tree | unit (hermetic temp dir) | `cargo test -p gnr8-core --test lifecycle init_scaffolds_workspace` | ❌ Wave 0 |
| WS-01 | `init` is idempotent (re-run preserves user edits) | unit | `cargo test -p gnr8-core --test lifecycle init_is_idempotent` | ❌ Wave 0 |
| WS-02 | `.gnr8/.gitignore` ignores `cache/`, keeps config | unit | `cargo test -p gnr8-core --test lifecycle gitignore_splits_lifecycle` | ❌ Wave 0 |
| WS-03 | TOML config parses the documented knobs; rejects unknown keys | unit | `cargo test -p gnr8-core --lib config::tests` | ❌ Wave 0 |
| WS-03 | naming overrides remap operation/type names | unit/snapshot | `cargo test -p gnr8-core --test lifecycle naming_overrides_apply` | ❌ Wave 0 |
| WS-04 | user-edited generated file → `UserEdited` (warn+skip) | unit | `cargo test -p gnr8-core --test lifecycle user_edit_is_protected` | ❌ Wave 0 |
| WS-04 | `--force` overwrites a user-edited file | unit | `cargo test -p gnr8-core --test lifecycle force_overwrites_user_edit` | ❌ Wave 0 |
| WS-04 | pre-existing un-tracked output is protected on first run | unit | `cargo test -p gnr8-core --test lifecycle untracked_output_protected` | ❌ Wave 0 |
| WS-04 | deleting a file from config drops its manifest entry | unit | `cargo test -p gnr8-core --test lifecycle manifest_prunes_dropped` | ❌ Wave 0 |
| WATCH-01 | second generate over unchanged source writes 0 files | unit | `cargo test -p gnr8-core --test lifecycle noop_second_run_writes_nothing` | ❌ Wave 0 |
| WATCH-01 | no-op does not change file mtime | unit | `cargo test -p gnr8-core --test lifecycle noop_preserves_mtime` | ❌ Wave 0 |
| WATCH-02 | output-path events are filtered (no loop) | unit (pure filter fn) | `cargo test -p gnr8 watch::tests::output_paths_filtered` | ❌ Wave 0 |
| WATCH-02 | one source edit → exactly one regeneration | integration (smoke, timing-tolerant) | `cargo test -p gnr8 --test watch_smoke single_edit_one_regen -- --ignored` | ❌ Wave 0 |
| WATCH-03 | latency report emits cold/no-op/single-edit timings + `--json` shape | unit (report struct) + smoke | `cargo test -p gnr8-core --lib lifecycle::tests::outcome_counts` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core --lib` (sub-second pure-logic tests) + `cargo clippy --all-targets --locked -- -D warnings`
- **Per wave merge:** `make gates` (lib + bin + the four contract snapshots + determinism + sdk_compile + the new `lifecycle` test)
- **Phase gate:** `make check` green before `/gsd:verify-work` (adds fmt-check + Go fixture build/vet; CI mirrors it)

### Wave 0 Gaps
- [ ] `crates/gnr8-core/tests/lifecycle.rs` — covers WS-01/02/03/04 + WATCH-01 + WATCH-03 counts (hermetic temp dir, no `tempfile` crate — reuse Phase-3 `std::env::temp_dir()` + PID/nanosecond unique subdir precedent)
- [ ] `crates/gnr8/tests/watch_smoke.rs` — covers WATCH-02 (one timing-tolerant smoke; `#[ignore]`-able / graceful-skip)
- [ ] `crates/gnr8/src/watch.rs` unit tests — the pure output-path filter fn (WATCH-02 loop-safety) tested without a real watcher
- [ ] `make gates` + CI `gates` job: add `--test lifecycle` to the blocking set (mirror how Phase 3 added `sdk_compile`)
- [ ] Framework install: none — libtest + `insta` already present.

## Security Domain

> `security_enforcement: true`, ASVS level 1. This phase reads user config + watches/writes files — the
> relevant surface is input validation and path handling, not auth/session/crypto.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth surface (local CLI). |
| V3 Session Management | no | No sessions. |
| V4 Access Control | no | No multi-user/permission surface; operates on the invoking user's files. |
| V5 Input Validation | **yes** | `toml` + `serde` with `#[serde(deny_unknown_fields)]` rejects malformed/typo'd config; output paths resolved against project root and rejected if they escape (`..`, absolute outside root) — extends the `sdk::write_to_dir` frame-name safety (T-03-03). |
| V6 Cryptography | **yes (non-secret)** | `blake3`/`sha2` used as a *content fingerprint*, NOT a security primitive — no secret, no auth. Never hand-roll a checksum (the only "crypto" rule that applies). |

### Known Threat Patterns for {Rust CLI + filesystem + config}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via config output path (`sdk_dir = "../../etc"`) | Tampering / Elevation | Canonicalize output paths and reject any that escape the project root; reuse Phase-3 reject-`..`/separator pattern. |
| Symlink in an output path redirects a write outside the project | Tampering | Resolve symlinks (`canonicalize`) before writing; validate the resolved path is under the project root. |
| Watch loop pegs CPU (self-triggering writes) | Denial of Service | Filter own-write paths (Pattern 4) + debounce — a correctness *and* availability control. |
| Malformed/huge config or manifest crashes the CLI | Denial of Service | Typed `CoreError` (never `unwrap`/panic, RUST-04); `deny_unknown_fields`; treat a corrupt manifest as "regenerate from scratch" rather than panicking. |
| Manifest tampering masks a stale/edited file | Tampering | Low risk (local, user-owned). The hash is integrity-by-comparison, not anti-tamper; `check` re-derives from disk so a tampered manifest at worst forces a regeneration. |

No high-severity findings; nothing blocks at `security_block_on: high`. The two controls that matter are
**output-path validation** (V5/traversal) and **watch-loop filtering** (DoS) — both already designed into the
patterns above.

## Sources

### Primary (HIGH confidence)
- crates.io API (`/api/v1/crates/<name>`) — max_stable versions, download counts, repos for notify, notify-debouncer-full/mini, blake3, sha2, toml, ctrlc (queried 2026-06-24)
- crates.io sparse index (`index.crates.io`) — exact dep constraints + MSRV: notify-debouncer-full 0.7.0 → `notify ^8.2.0`, MSRV 1.85; notify-debouncer-mini 0.7.0 → MSRV 1.77; notify 8.2.0 MSRV 1.77, macos_fsevent default
- docs.rs/notify-debouncer-full/0.7.0 — `new_debouncer(Duration, Option<FileIdCache>, callback)` API, dedup/rename-pair behavior vs mini
- docs.rs/notify/8.2.0 — `recommended_watcher`, `RecursiveMode`, `Config`/`PollWatcher`, macOS FSEvents default
- slopcheck 0.6.1 scan (crates.io ecosystem) — all 7 candidate crates `[OK]`
- Local codebase: `crates/gnr8-core/src/{lower,sdk,graph,analyze,error,lib}.rs`, `crates/gnr8/src/{main,cli}.rs`, `Cargo.toml` files, `Makefile`, `.planning/{PROJECT,REQUIREMENTS,ROADMAP,config.json}`, `03-03-SUMMARY.md`, rust-best-practices SKILL.md + chapter_04.md
- rustc/cargo 1.96.0 verified locally

### Secondary (MEDIUM confidence)
- WebSearch: notify FSEvents double-fire / no built-in "ignore own writes" — corroborated across notify docs, dev.to, and rust forum threads (the design implication is the same across all)
- WebSearch: EventKind Create/Modify/Remove variants + debouncer event filtering patterns

### Tertiary (LOW confidence)
- None relied upon. (notify 4.x `DebouncedEvent` docs surfaced in search but were explicitly identified as stale and excluded.)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every crate version verified against crates.io API + sparse index; MSRV/compat confirmed; slopcheck clean.
- Architecture: HIGH — the pure-core/thin-shell split, `plan_writes` truth table, and loop-safe watch design follow directly from the existing deterministic pipeline (Phase-3 SUMMARY) and notify's documented behavior.
- Pitfalls: HIGH (watch loop, debounce, hash stability — all corroborated by docs + the codebase's own determinism guarantee); MEDIUM on exact debounce tuning (A1, tunable).

**Research date:** 2026-06-24
**Valid until:** 2026-07-24 (30 days — notify/blake3/toml are stable; re-verify only if notify 9.0 leaves rc or debouncer-full 0.8 stabilizes)

## RESEARCH COMPLETE
