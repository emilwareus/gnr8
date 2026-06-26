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

## Milestone: v2.0 — Multi-language: TypeScript & Python (parse + generate)

**Shipped:** 2026-06-26
**Phases:** 6 | **Plans:** 19 | **Tasks:** 43

### What Was Built
The `ApiGraph` IR proven a true language-neutral narrow waist: a closed neutral `Type` enum + one JSON
facts contract; `pyextract` (stdlib `ast`, FastAPI full + Flask typed-envelope) and `tsextract` (the
user's own `typescript` Compiler API, NestJS) static sidecars; dependency-free `PySdk` (urllib/@dataclass)
and `TsSdk` (fetch/interfaces) targets; FastAPI + NestJS `.gnr8/` examples with real committed output; an
honest per-language `docs/USAGE.md` envelope; doctor/check/watch generalized to the source language.

### What Worked
- **Red-by-design snapshots (Phase 1) as the acceptance contract.** Authoring intended-green fixture
  snapshots up front meant each extractor phase had a precise, byte-exact target it flipped green with
  ZERO snapshot edits — the cleanest possible "definition of done."
- **The twin pattern.** PySdk/TsSdk cloned the gosdk structure; pyextract/tsextract cloned goextract's
  role. Pattern-mapper + the prior SUMMARYs made each phase mostly a structural port with a small
  genuinely-new core (symbol table; Compiler-API traversal; per-language Type mapping).
- **Hermetic generate-and-run / typecheck tests caught real codegen bugs** that string-only unit tests
  missed (Python f-string SyntaxError + forward-ref NameError; TS namespace-prefix + escaping + unquoted
  keys). The "does it actually compile/run" gate is load-bearing.
- **Adversarial code-review + auto-fix per phase** surfaced rule-3 fallbacks and edge-case codegen defects
  on shapes the fixtures didn't exercise, before they shipped.

### What Was Inefficient
- **The gsd-planner truncated ROADMAP.md to a single phase TWICE** (it Writes the whole file). Caught and
  reverted both times from git, but it cost recovery cycles. Lesson: verify the 6-phase ROADMAP after every
  planner run; the planner should patch, not rewrite, ROADMAP.md.
- **Vendoring `typescript` (23MB) in git was the wrong first call.** Corrected mid-run to "resolve the
  user's own typescript from the target project" — consistent with how go/python toolchains are found.
  Lesson: a sidecar's language toolchain is an environment prerequisite, never a shipped artifact.
- **Session limits interrupted long phases.** Safe-resume (all work committed atomically) made recovery
  clean, but per-plan executor agents are the right granularity to keep blast radius small.

### Patterns Established
- One deterministic `detect_language` decision routes to the right sidecar (no fallback); the same neutral
  facts contract flows through the SAME `build_graph → lower → SDK` path with no per-language branch.
- "Required user toolchain, not shipped" is the rule-2-clean way to depend on a language's own compiler.
- Fixture-line reconciliation (blank lines / non-fact comments only) anchors spans/diagnostics to the
  authoritative committed snapshot without editing the snapshot.

### Key Lessons
- Snapshots-as-spec + hermetic execution gates make multi-language codegen tractable and honest.
- Keep ROADMAP edits surgical; never let an agent rewrite the whole roadmap file.
- Toolchain = environment dependency; ship zero OSS.

### Cost Observations
- Fully autonomous run; Opus orchestrator + Opus subagents throughout (inherit).
- Per phase: discuss(--auto) → research → pattern-map → plan → plan-check → execute (per-plan agents) →
  code-review → auto-fix → verify → complete. ~3–4 code-review BLOCKER/WARNING findings fixed per phase.

## Cross-Milestone Trends

| Milestone | Phases | Plans | Tasks | Status |
|-----------|--------|-------|-------|--------|
| v1.0 | 5 | 14 | 38 | ✅ Shipped 2026-06-24 |
| v2.0 | 6 | 19 | 43 | ✅ Shipped 2026-06-26 |
