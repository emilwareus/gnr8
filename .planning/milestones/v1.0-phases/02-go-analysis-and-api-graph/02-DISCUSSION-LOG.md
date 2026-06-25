# Phase 2: Go Analysis And API Graph - Discussion Log (Auto Mode)

> Audit trail only. Decisions live in 02-CONTEXT.md.

**Date:** 2026-06-24 · **Mode:** discuss --auto (recommended defaults, grounded in PROJECT.md, REQUIREMENTS.md, ROADMAP.md, TARGET-API.md, Phase-1 SUMMARY/seams)

## Gray Areas & Auto-Selected Decisions
- **Go parsing strategy** → Go sidecar helper using official `go/packages`+`go/types`, emitting JSON. (alt: pure-Rust tree-sitter — rejected: no type resolution, fails GO-03/GO-05)
- **Helper packaging** → `goextract/` Go module invoked as subprocess; JSON facts = Rust↔Go contract.
- **Router recognition** → Gin patterns from TARGET-API.md → router-agnostic route facts (Gin stays in recognizer).
- **Type mapping** → per TARGET-API.md table (primitives/pointers/slices/maps/structs/aliases/uuid/time/enums).
- **Graph + stable IDs** → ApiGraph(routes/ops/params/bodies/responses/schemas/provenance); deterministic IDs, sorted output (GRAPH-02).
- **Inspect + diagnostics** → routes|schemas|graph human tables + --json; diagnostics with file:line; never panic/drop (GO-06).

## Corrections
None — autonomous run, all recommended defaults accepted.
