# Phase 5: PoC Hardening And Demo - Context

**Gathered:** 2026-06-25
**Status:** Ready for planning
**Mode:** Auto-generated (discuss --auto — Claude selected recommended defaults)

<domain>
## Phase Boundary

Make the now-complete PoC coherent, measured, diagnosable, and ready for review. This final phase
delivers: a `doctor` command that aggregates and explains diagnostics + lifecycle issues, performance
benchmark evidence (cold / warm-no-op / single-file-edit), a documented reproducible demo (Go source
edit → updated OpenAPI + Go SDK), and a final milestone verification that all tests/snapshots/quality
gates pass. It adds NO new pipeline capability — it hardens, measures, documents, and verifies what
Phases 1–4 built. (2 plans, not 3.)

</domain>

<decisions>
## Implementation Decisions

### `doctor` Diagnostics Aggregation (HARD-01)
- **D-01:** Implement `gnr8 doctor` (the last skeletal CLI command) to aggregate and explain, in one
  place: (a) **unsupported route/schema patterns** from the analysis diagnostics (the 7 known ones:
  map[string]any free-form maps, float64 narrowing, untyped query params — with source file:line);
  (b) **stale outputs** via the ownership manifest / `plan_writes` dry-run (drift, user-edited generated
  files, missing outputs); (c) **lifecycle issues** (no `.gnr8/` initialized, missing/invalid config,
  Go toolchain absent, output paths overlapping inputs). Human-readable grouped report by default, machine
  `--json` under the global flag. Non-zero exit when actionable problems exist (so CI can gate on it).
- **D-02:** `doctor` REUSES existing machinery (diagnostics::collect, the manifest, plan_writes/check, the
  config loader) — it is an aggregating/explaining front-end, NOT new analysis. Each item carries a short
  explanation of WHY it's flagged and (where possible) how to address it.

### Performance Reporting / Benchmarks (HARD-03 → success criterion 3)
- **D-03:** Produce **benchmark numbers** for the three scenarios the PoC must measure: cold generation,
  warm no-op, and single-file edit — reusing the Phase-4 `LatencyReport`/timing. Keep it honest wall-clock
  around the real pipeline on the fixture. Mechanism: a reproducible benchmark path (e.g. `gnr8 generate`
  timing output and/or a small bench harness/script) that prints the three numbers; capture representative
  numbers into the demo/evidence docs. Do NOT over-engineer (criterion bench suite optional; honest
  measured numbers are the bar). PROJECT's "benchmark before optimizing" guardrail applies.

### Documented Reproducible Demo (HARD-02 → success criterion 1)
- **D-04:** Write a **documented demo** (a `docs/demo.md` and/or README section) that a developer can run
  from a fresh checkout: build `gnr8`, point it at the `fixtures/goalservice` Gin service (or a copied
  scratch dir), run `init` → `generate`, show the produced OpenAPI + Go SDK, then **edit a Go source file**
  (e.g. add a field / route) and show `generate`/`watch` updating only the affected outputs. Must be
  copy-pasteable and reproducible (exact commands, expected output shape). This is the headline "source
  edit → updated OpenAPI + SDK" story.

### Milestone Verification Evidence (HARD-03 → success criterion 4)
- **D-05:** Produce a final **verification/evidence** artifact confirming all v1 requirements are met and
  all tests/snapshots/Rust quality gates pass (`make check`/`make gates` green: fmt, clippy -D warnings,
  all tests, all 4 contract snapshots, sdk_compile, determinism, lifecycle/watch). This is the
  ready-for-review sign-off backing the milestone audit.

### Claude's Discretion
- Exact `doctor` report grouping/format, the benchmark harness shape (CLI timing vs script vs criterion),
  demo doc structure, and where evidence lives (a docs file vs the milestone audit) — left to
  research/planning within the decisions above.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements & prior work
- `.planning/REQUIREMENTS.md` — HARD-01/02/03; the full v1 requirement set for the final evidence.
- `.planning/PROJECT.md` — coherence, measured, diagnosable, ready-for-review; benchmark-before-optimize.
- `crates/gnr8-core/src/diagnostics/` + the analysis diagnostics — what `doctor` aggregates (HARD-01).
- `crates/gnr8-core/src/lifecycle/` (manifest, plan_writes/check, regenerate) — stale-output + drift detection for `doctor`.
- `crates/gnr8/src/watch.rs` (LatencyReport) — the timing reused for benchmark numbers (HARD-03 criterion 3).
- `crates/gnr8/src/cli.rs` — the `doctor` skeletal command arm to implement.
- `fixtures/goalservice/` — the demo subject (Gin service → OpenAPI + SDK).
- `Makefile` / `.github/workflows/ci.yml` — the gates the final evidence asserts green.
- `thoughts/skills/rust-best-practices/` — typed errors, no prod unwrap, benchmarking guidance.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `diagnostics::collect` + the graph diagnostics (Phase 2) — `doctor`'s unsupported-pattern source.
- The ownership manifest + `plan_writes`/`check` (Phase 4) — `doctor`'s stale-output/drift source.
- `LatencyReport` + Instant timing (Phase 4) — benchmark numbers source.
- The config loader + `workspace::init` detection (Phase 4) — `doctor`'s lifecycle-issue checks.
- `gnr8 doctor` skeletal CLI arm — implement its body (last unimplemented command).

### Established Patterns
- thiserror in lib / anyhow only in binary; no prod unwrap; clippy -D warnings; deterministic output.
- Diagnostics carry source provenance; human tables + `--json`; non-zero exit on actionable problems.

### Integration Points
- `doctor` is a read-only aggregator over existing subsystems — no new analysis/codegen.
- Demo docs + benchmark numbers + evidence feed the milestone audit (the lifecycle step after this phase).

</code_context>

<specifics>
## Specific Ideas

- `doctor` should make the PoC feel finished: one command that says "here's what I can't represent and why,
  here's what's stale, here's what's misconfigured." It is the diagnosability headline.
- The demo must be reproducible from a fresh checkout with exact commands — it's the review artifact.
- Benchmark numbers should be honest measured wall-clock, not aspirational; PROJECT forbids premature opt.

</specifics>

<deferred>
## Deferred Ideas

- Deep performance optimization (only if benchmarks prove need) — v2 (ADV-01).
- Additional routers / source languages / SDK targets — v2.
- Richer programmatic customization — v2 (ADV-02).
- This is the final milestone phase: anything beyond hardening/measuring/documenting/verifying is out of scope.

</deferred>

---

*Phase: 05-poc-hardening-and-demo*
*Context gathered: 2026-06-25 (auto mode)*
