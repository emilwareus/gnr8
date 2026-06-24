# Feature Research

**Generated:** 2026-06-24
**Source material:** `thoughts/FEATURE.md`, `thoughts/ROADMAP.md`, `thoughts/research/*`

## Accepted First-Slice Features

- Native code-first API extraction from Go source.
- Internal API graph as the source of truth.
- OpenAPI generated as an artifact.
- Go SDK generated from the internal graph.
- Code-as-config in a project-local `.gnr8/` workspace.
- CLI-first UX with `init`, `generate`, `watch`, `check`, `inspect`, and `doctor` style commands.
- Comments only as escape hatches.
- Realistic fixture suite for route extraction, schema extraction, diagnostics, OpenAPI, and SDK output.
- Watch mode and incremental generation as first-class product promises.

## PoC Feature Scope

The proof of concept should include:

- One or two Go router styles, probably `net/http` plus `chi`.
- Struct extraction with JSON tags.
- Basic Go type mapping.
- Direct route registration recognition.
- Simple handler contract inference.
- Stable API graph report.
- Valid OpenAPI output.
- Usable Go SDK output.
- Diagnostics for unsupported inference.
- No-op generation skip or unchanged-file write avoidance.
- Watch mode with measured latency.

## Deferred Features

These should remain visible but outside the PoC:

- TypeScript source frontend.
- Python source frontend.
- Rust source frontend.
- TypeScript SDK target.
- Python SDK target.
- Rust SDK target.
- Dynamic plugin loading.
- Macro-heavy extension APIs.
- Graph database.
- Full Go framework coverage.
- Full OpenAPI 3.2 coverage.
- Arbitrary handler body interpretation.

## User Experience Feature Direction

The desired UX is similar to `polint`: scaffold editable project-local code, let the user own customization, and keep lifecycle artifacts in a tool folder.

Candidate `.gnr8/` responsibilities:

- User-owned generator Rust code.
- Fixtures and snapshots for local generation behavior.
- Cache and last-run reports.
- Generated-file ownership metadata.
- Diagnostics and drift reports.

YAML/TOML/JSON should not become the primary customization model.

