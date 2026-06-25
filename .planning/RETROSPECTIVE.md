# gnr8 — Retrospective

## Milestone: v1.0 — PoC: Go → OpenAPI → Go SDK

**Shipped:** 2026-06-24
**Phases:** 5 | **Plans:** 14 | **Tasks:** 38 | **Commits:** 113 | **LOC:** ~10.2K Rust + ~3.5K Go

### What Was Built
A complete owned pipeline: a `goextract` Go sidecar (go/packages + go/types) extracts route/schema/handler
facts from a real-shaped Gin service into a deterministic, router-agnostic Rust `ApiGraph`; the graph
lowers to valid OpenAPI 3.1 and generates a compiling, httptest-verified Go SDK; a `.gnr8/` workspace adds
blake3 ownership tracking, no-op generation, loop-safe watch + latency, and a `doctor` health aggregator.

### What Worked
- **Contract-tests-first (red-by-design).** Phase 1 authored four failing snapshot tests against
  `NotYetImplemented` seams; Phases 2–3 flipped them green. This made "is the pipeline actually wired?"
  continuously verifiable and prevented stub drift.
- **Determinism as a first-class requirement** (sort everything, stable IDs) made no-op detection and
  watch loop-safety almost free in Phase 4 — the same output-exclusion logic served both.
- **go/types over tree-sitter** for Go analysis gave real type resolution, which the SDK/OpenAPI accuracy
  depended on. Validated the "official tooling for semantic truth" bet.
- **Adversarial code review per phase** caught the two most important real bugs: the hard-coded
  `HttpError` SDK error type (overfitting to the fixture) and the no-op gap (analyzing own output).

### What Was Inefficient
- Three separate code-fixer runs hit a worktree-cleanup confusion that briefly advanced the wrong branch;
  each was detected and corrected, but it cost verification cycles. Sequential non-worktree execution
  (used for executors) proved more robust for this workspace than the fixers' worktree isolation.
- One executor (04-03) died on an API socket error after committing all work but before its SUMMARY;
  the orchestrator reconstructed it from the committed code + green gates.

### Patterns Established
- **Pure-core / thin-shell split** (e.g. `plan_writes` truth table, watch event filter) — the testable
  decision logic lives in `gnr8-core` and is unit-tested without filesystem/subprocess; the binary holds
  only the I/O shell.
- **Exclude-own-output** is a recurring invariant: analysis, generation, watch, and doctor all must avoid
  ingesting gnr8's generated files. Centralized in `exclude_output_paths`.
- **Honest scope framing**: code-as-config knobs shipped statically; programmatic customization explicitly
  deferred to v2 rather than faked.

### Key Lessons
- The highest-value reviews were the ones that asked "does this generalize beyond the one fixture?" —
  the explicit "support this, but don't overfit" framing turned up the genuine product bugs.
- Red-by-design contract tests are a strong backbone for an incrementally-built pipeline.

### Cost Observations
- Fully autonomous run (`/gsd:autonomous`): per phase = discuss(--auto) → research → plan → plan-check →
  execute (per-plan agents) → code-review → verify → (gap-closure/fix when warranted).
- Model mix: Opus orchestrator + Opus subagents throughout (inherit).

## Cross-Milestone Trends

| Milestone | Phases | Plans | Tasks | Status |
|-----------|--------|-------|-------|--------|
| v1.0 | 5 | 14 | 38 | ✅ Shipped 2026-06-24 |
