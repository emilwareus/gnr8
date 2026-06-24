# Phase 4: `.gnr8` Lifecycle And Watch Mode - Context

**Gathered:** 2026-06-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss --auto — Claude selected recommended defaults)

<domain>
## Phase Boundary

Prove the code-as-config user workflow and fast regeneration loop on top of the now-working Phase-3
pipeline (graph → OpenAPI + Go SDK). This phase delivers: `gnr8 init` scaffolding a `.gnr8/` workspace,
generated-file ownership tracking (no silent clobbering), no-op generation (skip unchanged outputs), and
watch mode (regenerate on supported Go source edits, debounced, loop-safe, with latency reporting). It wires
the full `gnr8 generate` / `gnr8 watch` / `gnr8 init` / `gnr8 check` surface to the real pipeline. It does
NOT add `doctor` aggregation, perf benchmark suites, or demo docs (Phase 5).

</domain>

<decisions>
## Implementation Decisions

### `.gnr8/` Workspace Layout & Lifecycle (WS-01, WS-02)
- **D-01:** `gnr8 init` scaffolds a project-local `.gnr8/` workspace with a clear split: **checked-in
  customization** (the config + user-editable customization files) vs **git-ignored lifecycle** (cache,
  the ownership manifest, and — depending on D-02 — generated output staging). `init` writes a `.gnr8/.gitignore`
  that ignores the lifecycle paths so the split is automatic. `init` is idempotent (re-running doesn't clobber
  user edits).
- **D-02:** Generated SDK/OpenAPI **outputs** are written to project-relative paths the user configures (e.g.
  `sdk/` + `openapi.yaml`), tracked for ownership (D-04), NOT hidden inside `.gnr8/`. `.gnr8/` holds config +
  customization + cache/manifest only.

### Code-as-Config Customization (WS-03)
- **D-03:** Honor the PROJECT constraints ("code-as-config under `.gnr8/`; YAML/TOML/JSON is not the main UX"
  AND "no dynamic plugin runtime"). The PoC realizes this **without** a dynamic plugin loader by making the
  checked-in customization a real, user-editable source-of-truth that the engine reads **statically**:
  configurable knobs = source input dir(s), OpenAPI output path, SDK output path + Go module path, and
  naming overrides (e.g. operation/type name remaps). The format is a minimal checked-in config file chosen by
  research — but it is explicitly a PoC stand-in, NOT positioned as the long-term UX. **Full programmatic
  ("through code") customization of routing recognition / transport / emitters is a documented v2 direction**
  (REQUIREMENTS v2 ADV-02), not built here. Phase 4 scopes WS-03 to the documented knobs + naming overrides +
  a reserved seam, and says so plainly (no overclaiming).

### Generated-File Ownership Tracking (WS-04)
- **D-04:** Maintain an **ownership manifest** (e.g. `.gnr8/cache/manifest.json`) recording every generated
  file with a content hash from the last generation. Before overwriting a tracked file, compare on-disk content
  to the recorded hash: if a user has hand-edited a generated file (hash mismatch and not produced by gnr8),
  do NOT silently clobber — emit a diagnostic / require an explicit `--force` (decision: warn + skip by default,
  overwrite under `--force`). Deleting a file from config drops it from the manifest. This is the core "avoid
  silent user-file clobbering" guarantee.

### No-Op Generation (WATCH-01)
- **D-05:** Content-hash each output; if regeneration produces **byte-identical** content to what's on disk
  (and recorded), **skip the write** (no mtime churn). Report counts (written / unchanged). Builds directly on
  the determinism guarantee from Phases 2–3 (identical graph ⇒ identical output ⇒ no-op).

### Watch Mode (WATCH-02, WATCH-03)
- **D-06:** Implement `gnr8 watch` with the **`notify`** crate watching the configured Go source dir(s). **Debounce**
  rapid duplicate events (coalesce a burst into one regeneration). **Ignore changes to gnr8's own generated
  outputs** (consult the ownership manifest / output paths) to avoid regeneration loops. On a supported source
  change, re-run the pipeline and write only changed outputs (reuse no-op detection).
- **D-07:** Report **latency** for the three scenarios the PoC must measure (WATCH-03): cold generation, warm
  no-op, and single-file edit. Print human-readable timings (and machine `--json` where it fits the existing
  CLI surface).

### Claude's Discretion
- Exact config file format/name, manifest schema, hash algorithm, debounce interval, `notify` watcher config
  (recursive/poll fallback), and the precise `init` directory tree — left to research/planning, within the
  decisions above.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Constraints & prior pipeline
- `.planning/PROJECT.md` — code-as-config under `.gnr8/`, NOT YAML-as-main-UX, NO dynamic plugin runtime, simpler-is-better.
- `.planning/REQUIREMENTS.md` — WS-01..04, WATCH-01..03 (this phase); ADV-02 (v2 richer extension APIs — the deferred customization).
- `crates/gnr8-core/src/{lower,sdk}/` — `to_openapi` + `generate` + `write_to_dir` (the Phase-3 generation entry points watch/no-op/ownership wrap).
- `crates/gnr8/src/{main.rs,cli.rs}` — the `init`/`generate`/`watch`/`check` CLI commands to wire to the real pipeline.
- `.planning/phases/03-openapi-and-go-sdk-generation/03-03-SUMMARY.md` — how generated artifacts are materialized (write_to_dir, bundle).
- `thoughts/skills/rust-best-practices/` — typed errors, no prod unwrap, filesystem + watcher handling, benchmark-before-optimize.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `lower::to_openapi` + `sdk::generate` + `sdk::write_to_dir` (Phase 3) — the generation the lifecycle wraps.
- Determinism guarantee (Phases 2–3) — identical graph ⇒ identical output, which makes no-op detection trivial/correct.
- `CoreError` (thiserror) — extend with workspace/manifest/watch/IO variants.
- `gnr8` clap CLI with `init`/`generate`/`watch`/`check` skeleton commands — implement their bodies.
- `serde`/`serde_json` pinned — use for the config + ownership manifest.

### Established Patterns
- thiserror in lib / anyhow only in binary; no prod unwrap; clippy `-D warnings`; deterministic output.
- Diagnostics carry provenance and never panic/drop.

### Integration Points
- New runtime dep likely needed: a file-watcher (`notify`) — first non-trivial new crate; research to pin version + justify.
- `.gnr8/` is created in the user's project root; outputs go to user-configured project paths.
- Latency reporting feeds Phase 5's benchmark/demo evidence.

</code_context>

<specifics>
## Specific Ideas

- The headline guarantee is "**no silent clobbering**" (WS-04) + "**no-op = no write**" (WATCH-01) + "**watch
  doesn't loop**" (WATCH-02). These are the falsifiable behaviors to test hard.
- Be honest about code-as-config scope: build the workspace + lifecycle + the documented knobs/naming overrides;
  do NOT fake a full programmatic-customization plugin system (out of scope; PROJECT forbids dynamic plugins).

</specifics>

<deferred>
## Deferred Ideas

- `doctor` diagnostics aggregation, perf benchmark suite, demo docs, milestone verification — Phase 5.
- Full programmatic customization (routing/transport/emitter overrides "through code") — v2 (ADV-02).
- Deeper incremental/partial graph invalidation beyond file-level no-op — v2 (ADV-01), only if benchmarks prove need.
- Additional routers / languages — post-PoC.

</deferred>

---

*Phase: 04-gnr8-lifecycle-and-watch-mode*
*Context gathered: 2026-06-24 (auto mode)*
