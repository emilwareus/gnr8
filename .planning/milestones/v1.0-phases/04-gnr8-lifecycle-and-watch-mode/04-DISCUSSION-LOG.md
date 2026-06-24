# Phase 4: .gnr8 Lifecycle And Watch Mode - Discussion Log (Auto Mode)

> Audit trail only. Decisions in 04-CONTEXT.md.

**Date:** 2026-06-24 · **Mode:** discuss --auto (recommended defaults; grounded in PROJECT/REQUIREMENTS/ROADMAP + Phase-3 generation entry points).

## Gray Areas & Auto-Selected Decisions
- **.gnr8 layout** → `gnr8 init` scaffolds checked-in config+customization vs gitignored cache/manifest; idempotent; auto `.gnr8/.gitignore`. Outputs go to user-configured project paths, tracked (not hidden in .gnr8).
- **Code-as-config (WS-03)** → honor "no YAML-as-main-UX + no dynamic plugins": PoC scopes customization to documented knobs (inputs/outputs/module path/naming overrides) in a checked-in config read statically; full programmatic customization is a documented v2 direction (ADV-02), NOT faked. (No overclaiming.)
- **Ownership (WS-04)** → manifest with content hashes; warn+skip on user-edited generated files, overwrite only under --force. Headline "no silent clobbering".
- **No-op (WATCH-01)** → content-hash outputs; skip write if byte-identical (builds on Phase 2-3 determinism).
- **Watch (WATCH-02)** → `notify` crate; debounce; ignore own outputs to avoid loops; regen only changed.
- **Latency (WATCH-03)** → report cold / warm no-op / single-file-edit timings (human + --json where it fits).

## Corrections
None — autonomous run, all recommended defaults accepted.
