# Phase 5: PoC Hardening And Demo - Research

**Researched:** 2026-06-25
**Domain:** CLI diagnostics aggregation, honest wall-clock benchmarking, reproducible demo docs, milestone evidence (Rust workspace)
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**`doctor` Diagnostics Aggregation (HARD-01)**
- **D-01:** Implement `gnr8 doctor` (the last skeletal CLI command) to aggregate and explain, in one place: (a) **unsupported route/schema patterns** from the analysis diagnostics (the 7 known ones: `map[string]any` free-form maps, float64 narrowing, untyped query params — with source `file:line`); (b) **stale outputs** via the ownership manifest / `plan_writes` dry-run (drift, user-edited generated files, missing outputs); (c) **lifecycle issues** (no `.gnr8/` initialized, missing/invalid config, Go toolchain absent, output paths overlapping inputs). Human-readable grouped report by default, machine `--json` under the global flag. Non-zero exit when actionable problems exist (so CI can gate on it).
- **D-02:** `doctor` REUSES existing machinery (`diagnostics::collect`, the manifest, `plan_writes`/`check`, the config loader) — it is an aggregating/explaining front-end, NOT new analysis. Each item carries a short explanation of WHY it's flagged and (where possible) how to address it.

**Performance Reporting / Benchmarks (HARD-03 → success criterion 3)**
- **D-03:** Produce **benchmark numbers** for the three scenarios the PoC must measure: cold generation, warm no-op, and single-file edit — reusing the Phase-4 `LatencyReport`/timing. Keep it honest wall-clock around the real pipeline on the fixture. Mechanism: a reproducible benchmark path (e.g. `gnr8 generate` timing output and/or a small bench harness/script) that prints the three numbers; capture representative numbers into the demo/evidence docs. Do NOT over-engineer (criterion bench suite optional; honest measured numbers are the bar). PROJECT's "benchmark before optimizing" guardrail applies.

**Documented Reproducible Demo (HARD-02 → success criterion 1)**
- **D-04:** Write a **documented demo** (a `docs/demo.md` and/or README section) that a developer can run from a fresh checkout: build `gnr8`, point it at the `fixtures/goalservice` Gin service (or a copied scratch dir), run `init` → `generate`, show the produced OpenAPI + Go SDK, then **edit a Go source file** (e.g. add a field / route) and show `generate`/`watch` updating only the affected outputs. Must be copy-pasteable and reproducible (exact commands, expected output shape). This is the headline "source edit → updated OpenAPI + SDK" story.

**Milestone Verification Evidence (HARD-03 → success criterion 4)**
- **D-05:** Produce a final **verification/evidence** artifact confirming all v1 requirements are met and all tests/snapshots/Rust quality gates pass (`make check`/`make gates` green: fmt, clippy -D warnings, all tests, all 4 contract snapshots, sdk_compile, determinism, lifecycle/watch). This is the ready-for-review sign-off backing the milestone audit.

### Claude's Discretion
- Exact `doctor` report grouping/format, the benchmark harness shape (CLI timing vs script vs criterion), demo doc structure, and where evidence lives (a docs file vs the milestone audit) — left to research/planning within the decisions above.

### Deferred Ideas (OUT OF SCOPE)
- Deep performance optimization (only if benchmarks prove need) — v2 (ADV-01).
- Additional routers / source languages / SDK targets — v2.
- Richer programmatic customization — v2 (ADV-02).
- This is the final milestone phase: anything beyond hardening/measuring/documenting/verifying is out of scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HARD-01 | `doctor` or equivalent diagnostics summarize unsupported route patterns, stale outputs, and lifecycle issues. | §"`doctor` Aggregation" — three data sources already exist (`diagnostics::collect`/`build_graph` graph diagnostics; `lifecycle::plan_only` drift; config/init/Go-toolchain lifecycle checks). Recommended report grouping, `--json` shape, exit-code policy below. |
| HARD-02 | A documented demo shows Go source changing, OpenAPI updating, and Go SDK output updating. | §"Demo Doc" — exact copy-pasteable command sequence from a fresh checkout against a SCRATCH COPY of `fixtures/goalservice` (the fixture has an `expected/` dir and is a CI-gated module, so it must not be mutated in place). Edit-a-field flow + diff-the-outputs walkthrough. |
| HARD-03 | All PoC tests, snapshots, and Rust quality gates pass before the milestone is considered complete. | §"Benchmark Mechanism" (criterion 3) + §"Final Evidence" (criterion 4) — `make check`/`make gates` already encode the full gate; evidence enumerates v1 req → where-satisfied and asserts gates green. |
</phase_requirements>

## Summary

Phase 5 is a **wiring + documentation** phase, not a feature phase. Every capability `doctor` needs already exists and is exercised by green tests: the analysis diagnostics (`diagnostics::collect` renders 7 `WARN` lines from `goextract`; `build_graph(...).diagnostics` carries the same as structured `Diagnostic { severity, message, file, line }`), the stale/drift signal (`lifecycle::plan_only` → `WritePlan` with `WriteAction::{Write, UserEdited, Unchanged}` and `has_drift()`), and the lifecycle checks (`.gnr8/` presence, `config::load` parse, Go-toolchain presence via the `CoreError::GoToolchainMissing` path, and the Phase-4 input/output overlap logic in `exclude_output_paths`/`is_under_output`). The `doctor` body is the **last unimplemented CLI arm** — today `dispatch()` returns `gnr8_core::not_yet("doctor", 5)`. The job is to replace that stub with a read-only aggregator that calls these existing functions, groups their results, prints a human report (or `--json`), and exits non-zero when actionable problems exist — exactly mirroring the established `check` exit-code + `--json` discipline.

The benchmark requirement (criterion 3) is **already 80% built**: `watch.rs` times every `lifecycle::regenerate` with `Instant::now()/elapsed()` and emits a `LatencyReport { scenario, millis, written, unchanged }` — and `gnr8 watch` already runs the COLD scenario at startup (`watch::cold_regenerate`). The remaining gap is producing the three numbers (cold / warm-no-op / single-file-edit) **reproducibly without an interactive watch session**. The least-effort honest option is a `scripts/bench.sh` that drives the real binary on a scratch fixture copy (`generate` cold → `generate` again for warm-no-op → edit one file → `generate` for single-file-edit), parsing `--json` output; a one-shot timing flag on `generate` is the cleaner alternative but is a small new CLI surface (flag it). `cargo bench`/criterion is explicitly NOT recommended — it benchmarks the in-process `regenerate` and would pull a dev-dependency, while the PoC's "honest wall-clock around the real pipeline" bar wants the end-to-end binary including the `go run` subprocess cost.

The demo (HARD-02) and evidence (criterion 4) are pure docs/verification. The single load-bearing gotcha: `fixtures/goalservice` is a **CI-gated Go module with an `expected/` golden dir** and is also the default `inspect` target — the demo must `cp -R` it to a scratch dir (or `git clone`/checkout into `/tmp`) and run `gnr8 init`/`generate` THERE, so the demo never dirties the committed fixture or its snapshots. `make check`/`make gates` already encode the complete v1 gate; the evidence artifact runs them and maps each of the 37 v1 requirements to where it is satisfied.

**Primary recommendation:** Implement `gnr8 doctor` as a read-only aggregator in the binary (`main.rs` `run_doctor` + a small `doctor` render module), reusing `build_graph().diagnostics` + `lifecycle::plan_only` + `config::load` + a Go-presence probe; mirror the `check` `--json`/exit-code pattern (0 = healthy, 1 = actionable). Produce benchmark numbers with a `scripts/bench.sh` driving the real binary on a scratch fixture copy and reading `gnr8 generate --json`/`gnr8 watch --json`. Write `docs/demo.md` against a scratch copy of the fixture. Generate the evidence by running `make check` and enumerating v1 req → satisfied-by. Two plans: **05-01** (doctor + benchmark — code) and **05-02** (demo + evidence — docs/verification).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| `doctor` aggregation + report rendering | Binary (`gnr8/src`) | — | Reuses core read-only functions; the report shape + exit-code policy is a CLI/UX concern, mirroring `run_check`/`run_inspect`. No new core analysis (D-02). |
| Analysis diagnostics source (7 unsupported patterns) | Core (`gnr8-core/src/analyze` + `graph`) | Core `diagnostics` | Already produced by `build_graph(...).diagnostics` (structured) and `diagnostics::collect` (rendered text). `doctor` only READS these. |
| Stale/drift detection source | Core (`gnr8-core/src/lifecycle`) | Core `manifest` | `lifecycle::plan_only` + `WritePlan::has_drift()` already classify outputs (the exact dry-run `check` uses). `doctor` reuses it verbatim. |
| Lifecycle checks (init/config/toolchain/overlap) | Binary probe + Core loaders | Core `config`, `workspace`, `analyze::helper` | `.gnr8/` presence + Go probe are filesystem/subprocess facts the binary owns; config validity is `config::load`; overlap reuses `is_under_output`. |
| Benchmark driver (3 scenarios) | Script (`scripts/`) over the binary | Binary `watch::LatencyReport` timing | Honest end-to-end wall-clock wants the real binary incl. `go run`; a shell script is the lightest reproducible harness reusing existing `--json` timing. |
| Demo + evidence docs | Docs (`docs/`) | Makefile gates | Pure documentation/verification; `make check` is the gate the evidence asserts. |

## Standard Stack

**No new runtime crates are expected this phase.** `doctor` is built entirely from `std` + crates already in the dependency tree (`serde`, `serde_json`, `gnr8-core`). The benchmark harness is a shell script (no crate). The demo + evidence are Markdown. This aligns with the PROJECT "no dynamic plugin runtime / simpler is better" constraint and the prior phases' "no new Rust crates" research stance (see `render.rs` doc comment).

### Core (already present — reused, not added)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `serde` / `serde_json` | 1.0 | `doctor --json` serialization (mirrors `run_check`'s inline `#[derive(Serialize)]` report) | Already the project's `--json` mechanism across `inspect`/`generate`/`check`/`watch`. |
| `clap` (derive) | 4.6 | `Doctor` variant already parsed in `cli.rs`; global `--json`/`-v` already wired | The CLI surface is defined; only the dispatch body is missing. |
| `gnr8-core` | path | `build_graph`, `lifecycle::plan_only`, `config::load`, `analyze::helper` (Go probe), `manifest::load` | The read-only subsystems `doctor` aggregates (D-02). |

### Supporting (tooling, not crates)
| Tool | Purpose | When to Use |
|------|---------|-------------|
| `bash` + `jq` (or pure `grep`) | `scripts/bench.sh` parses `gnr8 ... --json` timing | The reproducible 3-number benchmark harness. Prefer pure-`bash`/`grep` over a `jq` dependency if avoidable (capture `millis`). |
| `cp -R` / `git` | Make a scratch copy of `fixtures/goalservice` for the demo + bench | Keep the committed fixture + its `expected/` golden dir + CI module pristine. |
| `make check` / `make gates` | The full v1 gate the evidence asserts green | Final evidence (criterion 4). |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `scripts/bench.sh` driving the binary | `cargo bench` + `criterion` (dev-dependency) | criterion benchmarks the in-process `regenerate`, NOT the end-to-end binary incl. the `go run` subprocess; it adds a dev-dependency + a `[[bench]]` target + a statistically-rigorous-but-slow harness that overshoots the "honest measured wall-clock, do not over-engineer" bar (D-03). Recommend AGAINST. If chosen, it is **dev-only** (no runtime surface). |
| `scripts/bench.sh` | A `gnr8 generate --timing` one-shot flag (new CLI surface) | Cleaner UX (one binary, no shell) and reuses `LatencyReport`, but adds a flag + tests + a new `--json` field path; the script reuses what already exists with zero new surface. Either is acceptable; the script is lower-risk for a final hardening phase. **Flag the choice for the planner.** |
| Aggregating in the binary | A new `gnr8_core::doctor` module | D-02 says `doctor` is an aggregating FRONT-END, not new analysis. The render/exit-policy is CLI UX (like `render.rs`/`run_check`), so the binary is the right tier. A thin core helper is acceptable ONLY if it stays read-only and adds no analysis. |

**Installation:** None — no new dependencies. (If the planner chooses criterion despite the recommendation: `cargo add --dev criterion` in `crates/gnr8` and a `[[bench]]` target — dev-only, gated behind `make` not `make gates`.)

**Version verification:** No new packages to verify. Existing pinned versions (clap 4.6, serde/serde_json 1.0, blake3 1.8, toml 1.1, notify-debouncer-full 0.7, ctrlc 3.5) were verified against the crates.io sparse index in prior phases (`Cargo.toml` header notes "verified 2026-06-24") and are unchanged.

## Package Legitimacy Audit

> No external packages are installed this phase. The `doctor` command, benchmark script, demo, and evidence introduce **zero new dependencies** (std + already-vendored crates + bash + Markdown only).

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| _(none — no new packages)_ | — | — | — | — | — | N/A |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*If the planner chooses the criterion alternative (NOT recommended), `criterion` must pass the Package Legitimacy Gate before being added as a dev-dependency — it is a well-known, multi-year, high-download crate (`github.com/bheisler/criterion.rs`), but verify with `cargo search criterion` + slopcheck at plan time rather than trusting this note.*

## Architecture Patterns

### System Architecture Diagram

```
                          gnr8 doctor  [--json]
                                 │
                                 ▼
                    ┌────────────────────────────┐
                    │  run_doctor (binary, NEW)   │   read-only aggregator
                    │  — collects, never analyzes │
                    └────────────┬───────────────┘
                                 │ calls existing core (no new analysis, D-02)
        ┌────────────────────────┼─────────────────────────┬───────────────────────┐
        ▼                        ▼                         ▼                       ▼
  LIFECYCLE CHECKS         ANALYSIS DIAGS            STALE/DRIFT OUTPUTS      (optional) MANIFEST
  ───────────────         ─────────────             ─────────────────       ────────────────
  • .gnr8/ present?       build_graph(input)         lifecycle::plan_only     manifest::load
    (fs probe)             → .diagnostics            → WritePlan               (provenance per file)
  • config::load ok?        (7 WARN, file:line)       → Write   = stale
  • Go toolchain?         [reuses goextract via       → UserEdited = drift
    (probe `go`/            analyze::helper]          → Unchanged = clean
     GoToolchainMissing)                              → has_drift()
  • inputs ⊄ outputs?
    (is_under_output)
        │                        │                         │                       │
        └────────────────────────┴────────────┬────────────┴───────────────────────┘
                                               ▼
                                  ┌─────────────────────────┐
                                  │  DoctorReport (grouped)  │
                                  │  group → items[] +       │
                                  │  why + how-to-fix        │
                                  └────────────┬────────────┘
                                               │
                          ┌────────────────────┴────────────────────┐
                          ▼                                          ▼
                  human grouped report                       --json (serde_json)
                  (writeln! into String,                     pretty record
                   like render.rs)                                  │
                          └────────────────────┬─────────────────────┘
                                               ▼
                              exit 0 (healthy) | exit 1 (actionable)
                              [mirrors run_check exit policy]
```

**Trace the primary use case:** A developer runs `gnr8 doctor` in a project. `run_doctor` probes the four lifecycle facts (is `.gnr8/` there? does `config.toml` parse? is `go` runnable? do inputs overlap outputs?), then — if config loads — runs `build_graph` over the configured input to harvest the structured `diagnostics` (the 7 unsupported patterns with `file:line`) and runs `lifecycle::plan_only` to classify each output as stale/drifted/clean. It groups these into a `DoctorReport`, renders a human table (or `--json`), and exits non-zero if any actionable problem exists (missing init, invalid config, missing toolchain, stale/drifted outputs). The analysis diagnostics are INFORMATIONAL (the fixture intentionally has 7 — they explain "what I can't represent and why") and should NOT by themselves force a non-zero exit; only true misconfiguration/staleness should (see Pitfall 1).

### Recommended Project Structure
```
crates/gnr8/src/
├── main.rs           # add run_doctor(json) + dispatch arm; mirrors run_check/run_inspect
├── doctor.rs         # NEW: DoctorReport types + grouping + human/JSON render (like render.rs)
│                     #      pure-ish: takes already-collected facts, formats them
└── cli.rs            # unchanged — `Doctor` variant + global --json already exist

scripts/
└── bench.sh          # NEW: drives the real binary on a scratch fixture copy → 3 numbers

docs/
├── demo.md           # NEW: copy-pasteable fresh-checkout → source-edit → updated outputs
├── benchmarks.md     # NEW (or a section in demo.md/evidence): captured representative numbers
└── poc-contract.md   # existing

.planning/phases/05-.../   # evidence artifact may live here OR in docs/ (D-05 discretion)
```

### Pattern 1: Read-only aggregator mirroring `run_check`
**What:** `run_doctor` collects facts from existing read-only functions, formats a grouped report, and exits non-zero on actionable problems — structurally identical to `run_check` (which partitions a `WritePlan` into stale/drifted/clean, emits `--json` or a human report, and `std::process::exit(1)` on drift).
**When to use:** The whole `doctor` command.
**Example:**
```rust
// Source: pattern lifted from crates/gnr8/src/main.rs run_check (lines 184-238) + run_inspect (277-294)
fn run_doctor(json: bool) -> anyhow::Result<()> {
    let (root, gnr8_dir) = project_paths()?;

    // 1. Lifecycle checks (each is a fact, not an analysis):
    let initialized = gnr8_dir.is_dir();                       // .gnr8/ present?
    let config = gnr8_core::config::load(&gnr8_dir);           // Result — Err == invalid/missing config
    let go_present = probe_go();                               // spawn `go version`, catch the spawn error

    // 2. If config is valid, harvest analysis diagnostics + drift (reuse, no new analysis):
    let (diagnostics, drift) = if let Ok(cfg) = &config {
        let graph = gnr8_core::analyze::build_graph(/* resolved input */).ok();
        let plan  = gnr8_core::lifecycle::plan_only(&root, cfg).ok();
        (graph.map(|g| g.diagnostics), plan)                  // both Option — toolchain-missing degrades gracefully
    } else { (None, None) };

    // 3. Build the grouped report; decide the exit code.
    let report = doctor::DoctorReport::assemble(initialized, &config, go_present, diagnostics, drift);
    if json { println!("{}", serde_json::to_string_pretty(&report)?); }
    else    { print!("{}", report.render_human()); }

    if report.has_actionable_problem() { std::process::exit(1); } // mirrors run_check
    Ok(())
}
```

### Pattern 2: Go-toolchain probe (reuse the existing failure mode)
**What:** Detect a missing Go toolchain the same way the analyzer already does — attempt to spawn `go` and treat the spawn `io::Error` as "absent". The existing `helper::run_goextract_with` already returns `CoreError::GoToolchainMissing` on spawn failure; `doctor` either reuses that path (a failed `build_graph` whose error is `GoToolchainMissing`) or does a cheap dedicated `Command::new("go").arg("version").output()` probe.
**When to use:** The lifecycle "Go toolchain absent" check.
**Example:**
```rust
// Source: pattern from crates/gnr8-core/src/analyze/helper.rs run_goextract_with (lines 72-78)
fn probe_go() -> bool {
    std::process::Command::new("go").arg("version").output().is_ok()
    // .is_ok() == the binary spawned; a non-zero exit still means `go` exists. Matches the
    // GoToolchainMissing semantics (spawn failure == absent), never panics.
}
```

### Pattern 3: Input/output overlap check (reuse `is_under_output` semantics)
**What:** The Phase-4 gap-fix already encodes "is this path one of gnr8's own outputs?" in `lifecycle::is_under_output` (and `exclude_output_paths`). The lifecycle "output paths overlapping inputs" check is the same boundary question: does a configured `input` dir sit under (or equal) a configured output (`output.sdk_dir` / `output.openapi`), which would make gnr8 try to analyze its own generated SDK? `is_under_output` is currently private; either expose a tiny read-only predicate or replicate the separator-boundary string check in `doctor`.
**When to use:** The lifecycle "inputs overlap outputs" diagnostic.

### Pattern 4: Benchmark via the real binary on a scratch copy
**What:** Drive the three scenarios end-to-end through the installed binary, reusing the `--json` timing already emitted, on a throwaway copy of the fixture so nothing committed is mutated.
**When to use:** Producing the cold / warm-no-op / single-file-edit numbers (criterion 3).
**Example:**
```bash
# Source: composed from existing binary behavior — watch::cold_regenerate + run_generate --json + LatencyReport
set -euo pipefail
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
cp -R fixtures/goalservice "$WORK/svc"
cargo build --release -p gnr8
GNR8="$(pwd)/target/release/gnr8"
cd "$WORK/svc"
"$GNR8" init
# COLD: first generate (empty manifest → everything written). Time the wall clock.
t0=$(date +%s%N); "$GNR8" generate >/dev/null; cold_ms=$(( ($(date +%s%N)-t0)/1000000 ))
# WARM NO-OP: immediate re-generate (manifest hits → 0 written). WATCH-01 no-op path.
t0=$(date +%s%N); "$GNR8" generate >/dev/null; warm_ms=$(( ($(date +%s%N)-t0)/1000000 ))
# SINGLE-FILE EDIT: touch one Go source field, regenerate.
#   (append a field to a DTO struct — see demo.md for the exact edit)
t0=$(date +%s%N); "$GNR8" generate >/dev/null; edit_ms=$(( ($(date +%s%N)-t0)/1000000 ))
echo "cold=${cold_ms}ms warm-no-op=${warm_ms}ms single-file-edit=${edit_ms}ms"
```
*Note: the script can either time externally (shown) or parse `gnr8 watch --json`'s `LatencyReport.millis` for the in-pipeline measurement. External timing includes process startup + `go run` cost (the honest end-to-end number); `LatencyReport` isolates `regenerate`. Capture and label BOTH if cheap; otherwise pick the end-to-end number and say so.*

### Anti-Patterns to Avoid
- **Re-running analysis inside `doctor`:** D-02 forbids new analysis. Call `build_graph`/`plan_only` ONCE each and read their results; do not re-implement diagnostics or drift detection.
- **Forcing non-zero exit on informational diagnostics:** The fixture intentionally emits 7 `WARN`s (unsupported patterns are EXPECTED, not failures). A non-zero exit on those would make `doctor` permanently red on the demo subject. Reserve non-zero for actionable misconfiguration/staleness (see Pitfall 1).
- **Mutating the committed fixture in the demo/bench:** `fixtures/goalservice` has an `expected/` golden dir and is a CI-gated Go module + the default `inspect` target. Running `gnr8 init`/`generate` in it would write `.gnr8/`, `openapi.yaml`, `sdk/*.go` into the tree and could dirty git / snapshots. ALWAYS use a scratch copy.
- **Asserting exact benchmark numbers in a test:** Wall-clock is environment-dependent. Capture REPRESENTATIVE numbers into docs; never gate CI on an absolute latency threshold (it would flake). (No latency assertion exists today — keep it that way.)
- **Heredoc/`cat <<EOF` to write the new files:** Use the editor's Write tool, per project + harness convention. (Applies to the implementing agent.)

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Collecting unsupported-pattern diagnostics | A new scanner inside `doctor` | `build_graph(input).diagnostics` (structured `Diagnostic{severity,message,file,line}`) or `diagnostics::collect` (rendered text) | Already produced + snapshot-locked (7 WARN lines); D-02 forbids new analysis. |
| Detecting stale/drifted outputs | A bespoke hash/diff in `doctor` | `lifecycle::plan_only` → `WritePlan` + `has_drift()` + per-file `WriteAction` | This is the EXACT dry-run `gnr8 check` uses; reuse it verbatim for consistency. |
| Content hashing for drift | `std::hash::DefaultHasher` or a hand-rolled checksum | `manifest::blake3_hex` (already in the manifest layer) | DefaultHasher is not stable across toolchains (documented Pitfall 4 in manifest/mod.rs); already solved. |
| Config validity check | A custom TOML reader | `config::load` (returns typed `CoreError::Config` on missing/invalid/typo'd keys via `deny_unknown_fields`) | One source of truth; the error message already points at `gnr8 init`. |
| Go-toolchain detection | Parsing `$PATH` / `which` | Spawn `go version` (or reuse `GoToolchainMissing` from a failed `build_graph`) | Mirrors the existing `helper.rs` spawn-error semantics; portable, no panic. |
| Latency timing | A new timer abstraction | `std::time::Instant` + the existing `LatencyReport` shape | `watch.rs` already times every regeneration and emits the 4-field record the bench tooling relies on (see `latency_report_json_field_set` test). |
| Benchmark statistics | criterion's full harness | Honest wall-clock (`Instant`/`date +%s%N`) on the fixture | D-03: "do NOT over-engineer; honest measured numbers are the bar." |

**Key insight:** Every input `doctor` and the benchmark need is already computed by a green, tested subsystem. The phase's risk is NOT building new logic — it is (a) choosing the right grouping/exit policy so `doctor` is useful without crying wolf, and (b) keeping the demo/bench hermetic so they never touch committed state.

## Runtime State Inventory

> This phase is additive (new command body, new scripts, new docs). It renames/migrates nothing. Inventory included for completeness; nothing requires migration.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — `doctor` is read-only; the demo/bench write `.gnr8/`, `openapi.yaml`, `sdk/*.go` ONLY into a scratch copy of the fixture. | None (verified: no datastore, no persistent IDs renamed). |
| Live service config | None — no external services (no n8n/Datadog/etc.). CI workflow + Makefile are checked-in and updated in-repo if a `doctor`/bench gate is added. | If a `doctor` smoke or bench step is added to CI/Makefile, edit `.github/workflows/ci.yml` + `Makefile` in-repo (code edit). |
| OS-registered state | None — no Task Scheduler / launchd / systemd / pm2 registrations. | None. |
| Secrets/env vars | None — no secrets. CI uses only `actions/setup-go` (go-version '1.26') + `dtolnay/rust-toolchain`. | None. |
| Build artifacts | The demo/bench produce `target/release/gnr8` and scratch-dir outputs — all ephemeral/git-ignored. No stale egg-info/binary names. | None (scratch dir is `mktemp -d`, removed on exit). |

**Nothing found requiring migration:** verified by grep + reading all of `crates/`, `Makefile`, `ci.yml`. This phase adds files; it does not rename or move persistent state.

## Common Pitfalls

### Pitfall 1: `doctor` exit code crying wolf on expected diagnostics
**What goes wrong:** `doctor` exits non-zero because the fixture emits 7 `WARN` analysis diagnostics, making `doctor` permanently red on the very subject the demo showcases (and breaking any CI gate on it).
**Why it happens:** Conflating INFORMATIONAL diagnostics (unsupported patterns are an expected, documented PoC limitation — `expected/diagnostics.txt` is an acceptance target) with ACTIONABLE problems (missing init, invalid config, missing toolchain, stale/drifted outputs).
**How to avoid:** Define the exit policy explicitly: **exit 0** when the only findings are analysis `WARN`s (informational); **exit 1** when an actionable lifecycle/staleness problem exists (`.gnr8/` missing, `config::load` failed, Go toolchain absent, any output stale `Write`/drifted `UserEdited`). Document this in the report header ("N informational diagnostics; M actionable problems"). This mirrors `run_check`, which exits non-zero only on `has_drift()`, not on clean-but-present outputs.
**Warning signs:** `gnr8 doctor` returns 1 on a freshly-generated, correctly-configured project.

### Pitfall 2: Demo / benchmark dirties the committed fixture
**What goes wrong:** Running `gnr8 init`/`generate` directly in `fixtures/goalservice` writes `.gnr8/`, `openapi.yaml`, `sdk/*.go` into the committed tree, dirtying git, possibly colliding with `expected/`, and breaking the `go-fixture` CI job (which runs `go build ./...` over the whole module — generated SDK files would be compiled too).
**Why it happens:** The fixture is the natural demo subject, so it's tempting to run in place.
**How to avoid:** `cp -R fixtures/goalservice "$WORK/svc"` (or checkout into `mktemp -d`) and run everything in the scratch copy. The demo doc's FIRST step after building must be "make a scratch copy." Note the fixture's `expected/` dir is golden output, NOT a gnr8 run.
**Warning signs:** `git status` shows untracked `fixtures/goalservice/.gnr8/` or modified fixture files after running the demo.

### Pitfall 3: Benchmark numbers presented as asserted/exact rather than representative
**What goes wrong:** Numbers get baked into a test or a contract as hard thresholds, then flake on slower CI / faster dev machines; or a single run's noise is presented as "the" latency.
**Why it happens:** Treating wall-clock like a deterministic snapshot.
**How to avoid:** Capture representative numbers (a few runs, note the machine), label them clearly as environment-dependent and reproducible-via-`scripts/bench.sh`, and NEVER assert an absolute threshold in CI. The cold number is dominated by `go run` compiling the helper (and the SDK `go build`/`gofmt` subprocesses) — call that out so the cold/warm gap is understood, not mistaken for a gnr8 inefficiency.
**Warning signs:** A test compares `millis < SOME_CONSTANT`; a doc states one number with no machine/context.

### Pitfall 4: `doctor` panics or hard-errors when the Go toolchain is absent
**What goes wrong:** `doctor` calls `build_graph`, which returns `GoToolchainMissing`, and an unwrap/`?` propagation turns a diagnosable condition into a crash or an unhelpful exit — when "Go toolchain absent" is precisely one of the lifecycle issues `doctor` is supposed to REPORT.
**Why it happens:** Treating `build_graph`'s error as fatal instead of as a finding.
**How to avoid:** Probe Go presence FIRST as a reportable fact; gate the `build_graph`/`plan_only` calls behind it (or treat their `Err` as a graceful `None` and report "diagnostics unavailable: Go toolchain missing"). RUST-04 forbids prod `unwrap`/`expect`/`panic`; the binary's anyhow boundary should still surface a clean message if something truly unexpected fails.
**Warning signs:** `gnr8 doctor` on a machine without `go` prints a stack-ish error or exits via the anyhow path instead of a "Go toolchain: NOT FOUND — install Go and ensure it's on PATH" line.

### Pitfall 5: `doctor` analyzes the wrong input directory (cwd vs project root)
**What goes wrong:** `doctor` analyzes the process cwd or the fixture default instead of the configured `inputs`, producing diagnostics for the wrong code.
**Why it happens:** `inspect` defaults to the fixture (`DEFAULT_INSPECT_TARGET`); naively copying that into `doctor` would point it at the fixture, not the user's project.
**How to avoid:** `doctor` operates on the CURRENT PROJECT (like `generate`/`check`/`watch`): resolve `config.inputs[0]` against `project_root` (the `build_outputs`/`run_check` pattern), NOT the inspect fixture default. Reuse `project_paths()` + `config::load`.
**Warning signs:** `doctor` reports the goalservice diagnostics when run in an unrelated project.

### Pitfall 6: Evidence artifact asserts gates green without actually running them
**What goes wrong:** The evidence doc lists "all gates pass" from memory/assumption rather than a fresh `make check` run, so it drifts from reality (e.g. a snapshot quietly broke).
**Why it happens:** Treating the evidence as prose rather than a verification.
**How to avoid:** The evidence MUST be generated from an actual `make check` (or `make gates`) run captured at verification time, plus a per-requirement table mapping each of the 37 v1 reqs to where-satisfied (file/test). The plan's verification step should re-run the gate, not trust the artifact. Note: `make check`/`make gates` require the Go toolchain (the suite invokes `go run`/`gofmt`/`go build`) — the evidence run environment must have it (it's present here: go 1.26.2).
**Warning signs:** Evidence claims green but `make check` was never run in the same session.

## Code Examples

### `doctor --json` shape (recommended)
```jsonc
// Recommended DoctorReport JSON — mirrors run_check's inline CheckReport derive(Serialize) shape.
// "healthy" is the top-level boolean a CI gate reads; groups carry items with why + how-to-fix.
{
  "healthy": false,
  "lifecycle": {
    "initialized": true,
    "config_valid": true,
    "go_toolchain": true,
    "inputs_overlap_outputs": false
  },
  "outputs": {
    "stale":   ["sdk/client.go"],          // WriteAction::Write   — run `gnr8 generate`
    "drifted": ["openapi.yaml"],           // WriteAction::UserEdited — hand-edited; --force to overwrite
    "unchanged": ["sdk/models.go", "sdk/errors.go", "sdk/goals.go"]
  },
  "diagnostics": [                          // informational; do NOT force non-zero on these alone
    { "severity": "WARN",
      "message": "free-form map field: GoalResponse.Metadata (map[string]any) lowers to additionalProperties: true",
      "file": "internal/common/dto/goal.go", "line": 62,
      "why": "untyped maps cannot be represented as a typed schema",
      "fix": "use a typed struct or accept additionalProperties: true" }
    // ... up to the 7 known patterns
  ],
  "summary": { "actionable_problems": 2, "informational_diagnostics": 7 }
}
```

### Human report grouping (recommended)
```text
# Source: grouping mirrors render.rs append_diagnostics + run_check human output
gnr8 doctor — project health

LIFECYCLE
  ✓ .gnr8/ initialized
  ✓ config.toml valid
  ✗ Go toolchain: NOT FOUND — install Go and ensure `go` is on PATH
  ✓ inputs do not overlap outputs

OUTPUTS (2 actionable)
  stale   sdk/client.go      — out of date; run `gnr8 generate`
  drifted openapi.yaml       — hand-edited since gnr8 wrote it; `gnr8 generate --force` to overwrite

DIAGNOSTICS (7 informational — expected PoC limitations)
  WARN  float64 -> float32 narrowing: CreateGoalInput.TargetValue (...) (internal/common/dto/goal.go:NN)
  WARN  free-form map field: GoalResponse.Metadata (map[string]any) ... (internal/common/dto/goal.go:62)
  ... (untyped query params x3, float64 x3, free-form map x1)

2 actionable problem(s) found.   # exit 1
```

### Exit-code policy (the contract)
```
exit 0  →  healthy: no actionable lifecycle problem AND no stale/drifted output.
           (analysis WARN diagnostics may be present — they are informational.)
exit 1  →  actionable: .gnr8/ missing, OR config invalid, OR Go toolchain absent,
           OR any output stale (Write) / drifted (UserEdited), OR inputs overlap outputs.
           Mirrors run_check (std::process::exit(1) on has_drift), so CI can gate on `gnr8 doctor`.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `doctor` arm returns `not_yet("doctor", 5)` | `doctor` aggregates existing read-only subsystems | This phase (05) | Last unimplemented command; completes RUST-02's command surface. |
| Latency reported only interactively via `watch` | A reproducible `scripts/bench.sh` produces the 3 numbers headlessly | This phase (05) | Satisfies WATCH-03's "reports cold/warm/single-file latency" as a captured artifact, not just live output. |
| Demo lives in maintainers' heads | `docs/demo.md` reproducible from fresh checkout | This phase (05) | The headline review artifact (HARD-02 / Definition of Done). |

**Deprecated/outdated:** Nothing. This phase only fills the last seam and adds docs.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | A shell `scripts/bench.sh` is the least-effort honest harness (vs. a `generate --timing` flag or criterion). | Standard Stack / Pattern 4 | Low — planner may prefer a `--timing` flag; both reuse `LatencyReport`. Either satisfies D-03. Flagged as discretion. |
| A2 | `doctor` should NOT exit non-zero on analysis `WARN`s alone (only on actionable lifecycle/staleness). | Pitfall 1 / Exit policy | Medium — if the user WANTS `doctor` to be strict about unsupported patterns, the policy differs. This is a UX call worth confirming; the recommended policy keeps `doctor` green on the demo subject. |
| A3 | The evidence artifact may live in `docs/` (e.g. `docs/evidence.md` or a section in `demo.md`); the milestone audit consumes it. | Final Evidence | Low — D-05 leaves location to discretion ("a docs file vs the milestone audit"). |
| A4 | `is_under_output` (currently private in `lifecycle`) is the right reuse for the inputs-overlap-outputs check; exposing a tiny read-only predicate or replicating the string check is acceptable. | Pattern 3 | Low — it's a pure separator-boundary string check; replication is trivial and tested precedent exists. |
| A5 | No CI change is strictly required (doctor/bench need not be CI gates); evidence asserts the EXISTING `make check` gate. | Environment / Final Evidence | Low — adding a `doctor` smoke to CI is optional hardening, not required by HARD-01/03. |

**If this table feels long:** these are genuine discretion points (D-03/D-05 explicitly defer harness shape + evidence location) plus one UX policy (A2) worth a one-line confirmation in plan-check/discuss.

## Open Questions (RESOLVED)

> RESOLVED in the plans: exit policy = informational WARNs exit 0 (has_actionable_problem); benchmark = scripts/bench.sh; evidence = docs/evidence.md.

1. **`doctor` exit policy on unsupported-pattern diagnostics (A2).**
   - What we know: the fixture intentionally emits 7 `WARN`s; `run_check` exits non-zero only on drift, not on present-but-clean outputs.
   - What's unclear: whether the user wants `doctor` strictly green (informational WARNs → exit 0) or strict (any WARN → exit 1).
   - Recommendation: Informational WARNs → exit 0; actionable lifecycle/staleness → exit 1. Document the chosen policy in the report header and the plan. Confirm with the user if a stricter posture is desired.

2. **Benchmark measurement boundary: end-to-end binary vs in-pipeline `LatencyReport`.**
   - What we know: external timing (`date`/`Instant` around the process) includes startup + `go run` cost; `LatencyReport.millis` isolates `regenerate`.
   - What's unclear: which number is "the" headline.
   - Recommendation: capture BOTH if cheap (label them); otherwise report the end-to-end number and state the cold number is dominated by `go run`/`go build` subprocess cost.

3. **One-shot `generate --timing` flag vs `scripts/bench.sh` (A1).**
   - What we know: both reuse `LatencyReport`; the flag is a new CLI surface (+tests), the script reuses what exists.
   - What's unclear: planner/user preference.
   - Recommendation: `scripts/bench.sh` for lowest risk in a final hardening phase. If the planner prefers the flag, keep it tiny and dev-facing.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Go toolchain (`go`) | `doctor` analysis + demo + bench + `make check`/`gates` (invoke `go run`/`gofmt`/`go build`) | ✓ | go1.26.2 (matches CI's '1.26') | None — but `doctor` REPORTS its absence rather than crashing (Pitfall 4). Demo/bench require it. |
| `cargo` / `rustc` | Build the binary, run gates | ✓ | cargo 1.96.0 / rustc 1.96.0 (≥ MSRV 1.85) | None needed. |
| `bash` + `mktemp` + `cp` | `scripts/bench.sh`, demo scratch copy | ✓ (darwin/standard) | — | `git clone`/`git archive` to a temp dir as an alternative scratch mechanism. |
| `jq` | (optional) parse `--json` `millis` in bench | unknown | — | Use pure `bash`/`grep`/`sed` on the JSON, or external `date` timing — no `jq` dependency required. |

**Missing dependencies with no fallback:** none (Go + cargo present).
**Missing dependencies with fallback:** `jq` (avoid it; use `grep`/`date`).

## Validation Architecture

> `workflow.nyquist_validation: true` — section included.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `cargo test`; `insta` 1.48 (yaml) for snapshots; Go `go test` for fixture/helper modules |
| Config file | none (cargo-native); snapshots in `crates/gnr8-core/tests/snapshots/` |
| Quick run command | `cargo test -p gnr8` (binary unit/CLI-parse tests, fast) |
| Full suite command | `make check` (fmt-check + clippy -D + `cargo test --all-features` + go fixture/helper build/vet/test) — or `make gates` for the blocking subset |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HARD-01 | `doctor` exits 0 healthy, 1 on actionable problem; `--json` shape stable | unit (binary) | `cargo test -p gnr8 doctor` | ❌ Wave 0 (new `doctor` tests) |
| HARD-01 | `doctor` reports the 7 analysis diagnostics for the fixture | integration (graceful-skip if no Go) | `cargo test -p gnr8 doctor -- --include-ignored` (or a graph-or-skip test like `render.rs`) | ❌ Wave 0 |
| HARD-01 | `doctor` reports stale/drift via `plan_only` | unit | `cargo test -p gnr8 doctor` | ❌ Wave 0 |
| HARD-02 | Demo commands reproduce source-edit → updated outputs | manual-only (doc walkthrough) + optional smoke | run `docs/demo.md` steps on a scratch copy; optional `scripts/bench.sh`/demo smoke in CI | ❌ Wave 0 (doc) |
| HARD-03 | All gates green (fmt/clippy/tests/4 snapshots/sdk_compile/determinism/lifecycle) | gate | `make check` (and `make gates`) | ✅ exists (Makefile + ci.yml) |
| HARD-03 | Evidence enumerates 37 v1 reqs → satisfied-by | verification (doc generated from a real gate run) | `make check` + the evidence table | ❌ Wave 0 (doc) |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8` (fast binary tests incl. the new `doctor` tests) + `cargo clippy --all-targets --all-features --locked -- -D warnings`.
- **Per wave merge:** `make gates` (the blocking set; mirrors CI).
- **Phase gate:** `make check` green (full local gate incl. Go fixture/helper build/vet/test) before `/gsd:verify-work`; the evidence artifact is generated from THIS run.

### Wave 0 Gaps
- [ ] `crates/gnr8/src/doctor.rs` tests — `DoctorReport` assembly, grouping, exit-policy decision (pure, no Go needed), and `--json` field-set stability (mirror `watch.rs`'s `latency_report_json_field_set` test).
- [ ] `crates/gnr8/src/main.rs` `run_doctor` — wire + a graceful-skip integration test for the fixture path (pattern from `render.rs` `graph_or_skip`).
- [ ] No new framework install needed (cargo + insta + go already present).
- [ ] (Optional, if planner adds a CI smoke) a `doctor`/bench step in `Makefile`/`ci.yml`.

## Security Domain

> `security_enforcement: true` — section included. This phase is read-only aggregation + docs + a shell script; the attack surface is small, and prior phases already hardened the underlying functions.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth surface (local CLI). |
| V3 Session Management | no | No sessions. |
| V4 Access Control | no | No multi-user access control. |
| V5 Input Validation | yes | `doctor` reads config via `config::load` (`deny_unknown_fields` → typed `CoreError::Config`, already hardened). The bench script's only "input" is the scratch dir path it creates (`mktemp -d`); no untrusted input parsed. `--json` output is serde-serialized, not string-concatenated. |
| V6 Cryptography | no | `doctor` reuses `blake3_hex` only as a non-secret content fingerprint (explicitly NOT a security primitive, per manifest docs); no new crypto. |
| V12 Files & Resources | yes | The demo/bench write ONLY into a scratch copy (`mktemp -d` / `cp -R`), never the committed fixture. The existing `safe_output_path` traversal guard (rejects `..`/absolute/root) protects any generate run. `doctor` itself writes nothing. |

### Known Threat Patterns for {Rust CLI + Go subprocess + shell bench}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Subprocess argument injection (spawning `go`) | Tampering | Pass `go`/`version`/target as DISCRETE `Command` args, never `sh -c` (existing `helper.rs` discipline; the `doctor` Go probe follows it). |
| `doctor` panicking on missing toolchain / corrupt manifest | Denial of Service | Graceful degradation — treat `build_graph`/`plan_only` `Err` as a reportable `None`; `manifest::load` already returns empty default on corrupt/absent (no panic, RUST-04). |
| Bench/demo mutating committed state | Tampering | Operate exclusively in a `mktemp -d` scratch copy with a `trap rm -rf` cleanup; never run `init`/`generate` in `fixtures/goalservice` (Pitfall 2). |
| Path traversal via config output paths during a demo generate | Tampering | Already mitigated by `lifecycle::safe_output_path` (rejects `..`/absolute/Windows-prefix components) — unchanged this phase. |
| `--json` output enabling downstream injection | Tampering | serde_json serialization (no manual string building) — same as existing `--json` paths. |

## Sources

### Primary (HIGH confidence — read this session)
- `crates/gnr8/src/cli.rs` — the `Doctor` command variant + global `--json`/`-v` (already parsed; body missing).
- `crates/gnr8/src/main.rs` — `dispatch()` returns `not_yet("doctor", 5)`; `run_check`/`run_generate`/`run_inspect`/`project_paths` (the patterns `run_doctor` mirrors).
- `crates/gnr8/src/watch.rs` — `LatencyReport { scenario, millis, written, unchanged }`, `Instant` timing, `cold_regenerate`, `latency_report_json_field_set` test (the benchmark timing source).
- `crates/gnr8-core/src/diagnostics/mod.rs` — `collect()` renders the 7 canonical `WARN` lines with `file:line`.
- `crates/gnr8-core/src/graph/mod.rs` — `ApiGraph.diagnostics: Vec<Diagnostic{severity,message,file,line}>` (structured diagnostics carried on every `build_graph`).
- `crates/gnr8-core/src/analyze/mod.rs` + `analyze/helper.rs` — `build_graph`, the `go run` subprocess driver, `CoreError::GoToolchainMissing` spawn-failure semantics (Go-presence reuse).
- `crates/gnr8-core/src/lifecycle/mod.rs` — `plan_only`, `WritePlan::has_drift`, `WriteAction::{Write,UserEdited,Unchanged}`, `is_under_output`/`exclude_output_paths` (drift + overlap reuse), `safe_output_path` (traversal guard).
- `crates/gnr8-core/src/config/mod.rs` — `config::load` (typed `CoreError::Config`, `deny_unknown_fields`).
- `crates/gnr8-core/src/manifest/mod.rs` — `blake3_hex`, graceful-degradation load, `ManifestEntry.source` provenance.
- `crates/gnr8-core/src/workspace/mod.rs` — `init` scaffold (`.gnr8/` presence semantics).
- `Makefile` + `.github/workflows/ci.yml` — the exact gate (`make check`/`make gates`) the evidence asserts green.
- `fixtures/goalservice/` (incl. `expected/`) — the demo subject + golden dir (the "don't mutate in place" constraint).
- `.planning/REQUIREMENTS.md` — the 37 v1 requirements the evidence enumerates; HARD-01/02/03.
- `.planning/phases/05-poc-hardening-and-demo/05-CONTEXT.md` — locked D-01..D-05.
- `thoughts/skills/rust-best-practices/SKILL.md` + chapter_03 (performance: "don't guess, measure"; `--release`; cargo bench is optional) + chapter_05 (snapshot/insta).
- Tool probes: `go version` → go1.26.2; `cargo`/`rustc` 1.96.0; `.planning/config.json` (nyquist + security enforcement both true).

### Secondary (MEDIUM confidence)
- (none required — all claims grounded in the codebase read this session)

### Tertiary (LOW confidence — flagged for validation)
- (none)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new packages; every reused function read directly this session and confirmed exercised by green tests.
- Architecture: HIGH — `doctor` mirrors the existing, tested `run_check`/`run_inspect` patterns; benchmark reuses the existing `LatencyReport` path; demo/evidence are docs over an existing gate.
- Pitfalls: HIGH — derived from concrete codebase facts (fixture `expected/` dir + CI module, `run_check` exit semantics, `GoToolchainMissing` path, `safe_output_path`).

**Research date:** 2026-06-25
**Valid until:** 2026-07-25 (stable — internal-codebase-bound; no fast-moving external dependency). Re-verify only if `lifecycle`/`watch`/`diagnostics`/`config` signatures change before planning.

## RESEARCH COMPLETE

**Phase:** 5 - PoC Hardening And Demo
**Confidence:** HIGH

### Key Findings
- `gnr8 doctor` is the last unimplemented CLI arm (`dispatch()` → `not_yet("doctor", 5)`); every input it needs already exists and is green-tested: `build_graph(...).diagnostics` (7 structured WARNs with file:line), `lifecycle::plan_only` + `WritePlan::has_drift()` (stale/drift, the exact `check` dry-run), `config::load` (typed config validity), and a `go version` probe / `GoToolchainMissing` reuse (toolchain presence). It should mirror `run_check`'s `--json` + exit-code discipline (0 healthy, 1 actionable) and treat analysis WARNs as INFORMATIONAL (exit 0) to avoid crying wolf on the fixture's expected 7.
- Benchmarks are ~80% built: `watch.rs` already times `regenerate` into `LatencyReport{scenario,millis,written,unchanged}` and runs the cold scenario at startup. Recommend a `scripts/bench.sh` driving the real `--release` binary on a SCRATCH copy of the fixture to produce cold / warm-no-op / single-file-edit numbers reproducibly; criterion is over-engineering (and dev-only if used).
- The headline demo + bench MUST run on a `cp -R` / `mktemp -d` scratch copy of `fixtures/goalservice` — it has an `expected/` golden dir and is a CI-gated Go module + the default `inspect` target; running `init`/`generate` in place would dirty git/snapshots and break the `go-fixture` CI job.
- The full v1 gate already exists as `make check`/`make gates` (fmt, clippy -D, all tests, 4 contract snapshots, sdk_compile, determinism, lifecycle). The evidence (criterion 4) is generated by running that gate and mapping each of the 37 v1 reqs → where-satisfied; Go toolchain (go1.26.2) is present so the gate runs locally.
- The 2-plan split is CONFIRMED: **05-01** = doctor command + benchmark harness (code; new `doctor.rs` + `run_doctor` in `main.rs` + `scripts/bench.sh`), **05-02** = `docs/demo.md` + final evidence artifact (docs/verification over the existing gate). The plans are file-disjoint (code vs docs) and can be sequenced 05-01 → 05-02 (the demo references `doctor`).

### File Created
`/Users/emilwareus/conductor/workspaces/gnr8/tripoli-v7/.planning/phases/05-poc-hardening-and-demo/05-RESEARCH.md`

### Confidence Assessment
| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | No new packages; all reused functions read this session, confirmed green-tested. |
| Architecture | HIGH | `doctor` mirrors tested `run_check`/`run_inspect`; benchmark reuses `LatencyReport`. |
| Pitfalls | HIGH | Grounded in concrete codebase facts (fixture `expected/`+CI module, exit semantics, toolchain path). |

### Open Questions
- `doctor` exit policy on the 7 informational WARNs (recommend exit 0; confirm if a stricter posture is wanted).
- Benchmark measurement boundary (end-to-end binary vs in-pipeline `LatencyReport.millis`) — recommend capturing/labeling both.
- Bench harness shape (`scripts/bench.sh` vs a `generate --timing` flag) — recommend the script; D-03 leaves it to discretion.

### Ready for Planning
Research complete. Planner can now create 05-01 (doctor + benchmark) and 05-02 (demo + evidence) PLAN.md files.
