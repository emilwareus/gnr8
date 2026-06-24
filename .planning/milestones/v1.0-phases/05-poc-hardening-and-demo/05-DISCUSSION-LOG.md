# Phase 5: PoC Hardening And Demo - Discussion Log (Auto Mode)

> Audit trail only. Decisions in 05-CONTEXT.md.

**Date:** 2026-06-25 · **Mode:** discuss --auto (recommended defaults; grounded in PROJECT/REQUIREMENTS/ROADMAP + Phases 1-4 subsystems).

## Gray Areas & Auto-Selected Decisions
- **doctor (HARD-01)** → aggregate unsupported patterns (file:line) + stale outputs (manifest/plan_writes drift) + lifecycle issues (no .gnr8/missing config/no Go toolchain/output-input overlap); human + --json; non-zero exit on actionable problems. Reuses existing machinery (read-only aggregator).
- **Benchmarks (criterion 3)** → honest wall-clock cold/warm-no-op/single-file-edit reusing Phase-4 LatencyReport; reproducible; no over-engineering.
- **Demo (HARD-02)** → docs/demo.md reproducible from fresh checkout: build → init → generate → show OpenAPI+SDK → edit Go source → regen updates affected outputs.
- **Evidence (HARD-03)** → final verification artifact: all v1 reqs met + make check/gates green (fmt/clippy/tests/4 snapshots/sdk_compile/determinism/lifecycle).

## Corrections
None — autonomous run, all recommended defaults accepted.
