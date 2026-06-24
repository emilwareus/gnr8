# gnr8 v1.0 milestone evidence — ready-for-review sign-off

**Milestone:** v1.0 (Go source → OpenAPI + Go SDK PoC)
**Captured:** 2026-06-25
**Environment:** macOS (darwin/arm64), cargo 1.96.0 / rustc 1.96.0 (MSRV ≥ 1.85), go1.26.2

This is the final HARD-03 verification artifact. It is **generated from a real `make check` run** in
this session (not from memory) and maps **every v1 requirement** to the concrete file/test where each
is satisfied. `make check` is the full local gate and mirrors CI.

> **Note on the requirement count.** `.planning/REQUIREMENTS.md` describes "37 v1 requirements", but
> its own "## v1 Requirements" checklist enumerates **38** distinct IDs (POC×3, RUST×4, FIX×4, GO×6,
> GRAPH×3, OAPI×3, SDK×5, WS×4, WATCH×3, HARD×3 = 38) — the headline "37" is an off-by-one in the
> planning summary. This document maps **all 38 actual v1 requirement IDs** (the set difference against
> `.planning/REQUIREMENTS.md` is empty), so coverage is complete regardless of the label.

---

## Gate Results — `make check` GREEN

`make check` = `fmt-check clippy test fixture-build goextract-build` — the complete v1 gate. Run live
this session; **exit code 0**. Every sub-gate passed:

| Gate | Command | Result |
|------|---------|--------|
| **fmt-check** | `cargo fmt --all -- --check` | ✅ PASS (no diff) |
| **clippy** (`-D warnings`) | `cargo clippy --all-targets --all-features --locked -- -D warnings` | ✅ PASS (0 warnings; `--locked` requires a committed, current `Cargo.lock`) |
| **test** | `cargo test --all-features` | ✅ PASS — see breakdown below |
| **fixture-build** | `cd fixtures/goalservice && go build ./... && go vet ./...` | ✅ PASS |
| **goextract-build** | `cd goextract && go build ./... && go vet ./... && go test ./...` | ✅ PASS |

### `cargo test --all-features` breakdown (all green, 0 failed)

| Test binary / suite | Result | Covers |
|---------------------|--------|--------|
| `gnr8` bin unittests (`src/main.rs`) | **25 passed**, 1 ignored | CLI parse, `render`, `watch` (incl. `latency_report_json_field_set`), and the **9 `doctor::tests`** (exit-policy truth table + `--json` field set) |
| `gnr8` `watch_smoke` integration | 0 passed, **1 ignored** | timing-dependent FS-event smoke (`#[ignore]`d by design; loop-safety covered deterministically by the pure `watch::tests`) |
| `gnr8-core` lib unittests (`src/lib.rs`) | **82 passed** | error display, analyze/facts, graph, lower (OpenAPI), sdk emit/gofmt/bundle, lifecycle/manifest, config, diagnostics |
| `determinism` (`tests/determinism.rs`) | **3 passed** | graph + OpenAPI + SDK byte-identical across two runs (GRAPH-02) |
| `lifecycle` (`tests/lifecycle.rs`) | **22 passed** | manifest round-trip, `plan_writes` truth table, no-op skip, naming-override `$ref` rewrites, traversal guard, idempotent init |
| `sdk_compile` (`tests/sdk_compile.rs`) | **3 passed** | generated SDK `go build` + `httptest` smoke + go-build-error mapping (SDK-05) |
| `snapshot_diagnostics` | **1 passed** | diagnostics match `expected/` golden |
| `snapshot_graph` | **1 passed** | graph report matches `expected/` golden |
| `snapshot_openapi` | **1 passed** | OpenAPI matches `expected/` golden |
| `snapshot_sdk` | **1 passed** | Go SDK matches `expected/` golden |
| `gnr8_core` doc-tests | 0 passed, 1 ignored | (doc example marked `ignore`) |

The blocking subset (`make gates` — lib + bin tests + the 4 contract snapshots + determinism +
`sdk_compile` + `lifecycle`) is a strict subset of the above and is green as part of this run.

> **Methodology note (Pitfall 6):** these statuses were captured from an actual `make check`
> invocation in this session, not asserted from memory. Re-run `make check` to reproduce — it is the
> authoritative gate, and it must exit 0 for the milestone to be considered complete.

---

## Requirement Traceability — all v1 requirements satisfied (38 / 38)

Every v1 requirement, mapped to the phase that delivered it and the concrete artifact (file/test)
where it is satisfied. Cross-referenced against `.planning/REQUIREMENTS.md` "## Traceability". (See the
count note above: the REQUIREMENTS.md checklist contains 38 IDs; the "37" headline is an off-by-one.)

| ID | Phase | Satisfied by (file / test) |
|----|-------|----------------------------|
| **POC-01** | 1 | `docs/poc-contract.md` (scope locked: Go source → OpenAPI → Go SDK) |
| **POC-02** | 1 | `docs/poc-contract.md` (router set, OpenAPI 3.1 target, Go SDK shape, `.gnr8/` layout documented) |
| **POC-03** | 1 | `docs/poc-contract.md` + `.planning/REQUIREMENTS.md` "## Out of Scope" (explicit non-goals) |
| **RUST-01** | 1 | `Cargo.toml` workspace + `crates/gnr8` (thin CLI bin) + `crates/gnr8-core` (lib modules) |
| **RUST-02** | 1 | `crates/gnr8/src/cli.rs` (`init`/`generate`/`watch`/`check`/`inspect`/`doctor`); `doctor` body in `crates/gnr8/src/doctor.rs` + `run_doctor` |
| **RUST-03** | 1 | `make check` (fmt-check + clippy `-D warnings` + `cargo test`) — green this session |
| **RUST-04** | 1 | `crates/gnr8-core/src/error.rs` (`thiserror` typed `CoreError`); clippy denies `unwrap_used`/`expect_used`/`panic` workspace-wide; `anyhow` confined to `crates/gnr8/src/main.rs` |
| **FIX-01** | 1 | `fixtures/goalservice/` (realistic Gin CRUD + list-with-filters module) |
| **FIX-02** | 1 | `fixtures/goalservice/internal/common/dto/goal.go` (path params, bodies, JSON tags, optional fields, package boundaries, `map[string]any` unsupported pattern) |
| **FIX-03** | 1 | `crates/gnr8-core/tests/snapshot_{graph,diagnostics,openapi,sdk}.rs` + `fixtures/goalservice/expected/` golden |
| **FIX-04** | 1 | contract tests were red-by-design until implemented (Phase 1→3 flip); now `crates/gnr8-core/tests/snapshot_*.rs` green |
| **GO-01** | 2 | `goextract/internal/load/` + `crates/gnr8-core/src/analyze/` (discovers packages/files for configured inputs) |
| **GO-02** | 2 | `goextract/internal/facts/` + `crates/gnr8-core/src/analyze/facts.rs` (structs, fields, JSON tags, source spans) |
| **GO-03** | 2 | `goextract/internal/types/` (primitives, pointers, slices, maps, named structs, aliases, `time.Time`, `uuid`) |
| **GO-04** | 2 | `goextract/internal/routes/` + `internal/handlers/` (Gin call patterns → method/path/family/handler/span) |
| **GO-05** | 2 | `goextract/internal/handlers/` (request/response schema inference for typed handlers) |
| **GO-06** | 2 | `crates/gnr8-core/src/diagnostics/` + `goextract/internal/diag/` (unsupported → diagnostics, not panics); `crates/gnr8-core/tests/snapshot_diagnostics.rs` |
| **GRAPH-01** | 2 | `crates/gnr8-core/src/graph/mod.rs` (routes, operations, params, bodies, responses, schemas, files, provenance) |
| **GRAPH-02** | 2 | `crates/gnr8-core/tests/determinism.rs` (`build_graph_is_byte_identical_across_two_runs`) + sorted serialization in `graph/mod.rs` |
| **GRAPH-03** | 2 | `crates/gnr8/src/render.rs` + `inspect routes\|schemas\|graph` (table + `--json`) |
| **OAPI-01** | 3 | `crates/gnr8-core/src/lower/mod.rs` (`to_openapi`) + `crates/gnr8-core/tests/snapshot_openapi.rs` |
| **OAPI-02** | 3 | `crates/gnr8-core/src/lower/` (info, paths, operations, params, bodies, responses, component schemas) |
| **OAPI-03** | 3 | `crates/gnr8-core/src/lower/` diagnostics (e.g. free-form map → `additionalProperties: true`) |
| **SDK-01** | 3 | `crates/gnr8-core/src/sdk/emit/` → generated `sdk/client.go` (base URL + `WithHTTPClient`) |
| **SDK-02** | 3 | `crates/gnr8-core/src/sdk/emit/` → generated `sdk/goals.go` (ctx-first typed operation methods) |
| **SDK-03** | 3 | `crates/gnr8-core/src/sdk/emit/` → generated `sdk/models.go` (request/response models + enums) |
| **SDK-04** | 3 | generated `sdk/errors.go` (typed `*APIError`) + JSON encode/decode in `sdk/goals.go` |
| **SDK-05** | 3 | `crates/gnr8-core/tests/sdk_compile.rs` (hermetic `go build` + `httptest` smoke) |
| **WS-01** | 4 | `crates/gnr8-core/src/workspace/mod.rs` (`init` scaffolds `.gnr8/` config + cache) |
| **WS-02** | 4 | `crates/gnr8-core/src/workspace/mod.rs` (`GITIGNORE_BODY` splits checked-in `config.toml` from ignored `/cache/`) |
| **WS-03** | 4 | `crates/gnr8-core/src/config/mod.rs` (typed `Config`: inputs/outputs/go-module/naming knobs, `deny_unknown_fields`) |
| **WS-04** | 4 | `crates/gnr8-core/src/manifest/mod.rs` (blake3 ownership manifest) + `lifecycle::plan_writes` (no silent clobber) |
| **WATCH-01** | 4 | `crates/gnr8-core/src/lifecycle/mod.rs` (`plan_writes` no-op skip) + `tests/lifecycle.rs::noop_second_run_writes_nothing` |
| **WATCH-02** | 4 | `crates/gnr8/src/watch.rs` (debounced FS events, output-path drop / loop-safety) + `watch::tests` |
| **WATCH-03** | 4 | `crates/gnr8/src/watch.rs` (`LatencyReport` cold/no-op/single-edit) + `scripts/bench.sh` (captured numbers below) |
| **HARD-01** | 5 | `crates/gnr8/src/doctor.rs` + `run_doctor` (unsupported patterns + stale/drift + lifecycle, human + `--json`, exit 0/1) |
| **HARD-02** | 5 | `docs/demo.md` (reproducible fresh-checkout source-edit → updated OpenAPI + SDK on a scratch fixture copy) |
| **HARD-03** | 5 | **this document** + `make check` green (captured above) + the 37-row traceability table |

**Coverage:** 38 / 38 v1 requirements satisfied (the complete `.planning/REQUIREMENTS.md` v1 set; the
"37" in the REQUIREMENTS.md coverage note is an off-by-one). 0 unmapped, 0 pending.

---

## Benchmarks (WATCH-03)

Captured this session via `scripts/bench.sh`, which drives the **release binary** end-to-end on a
`mktemp -d` scratch copy of `fixtures/goalservice` (the committed fixture is never mutated). Three
representative runs:

| Run | cold | warm-no-op | single-file-edit |
|-----|------|------------|------------------|
| 1 | 731 ms | 695 ms | 714 ms |
| 2 | 759 ms | 766 ms | 729 ms |
| 3 | 720 ms | 726 ms | 723 ms |

**These numbers are REPRESENTATIVE and environment-dependent — never asserted as thresholds and never
gated in CI** (Pitfall 3). The three scenarios cluster closely (~700–770 ms) because every run is
dominated by the Go subprocess cost — `go run` to compile the `goextract` helper, then `gofmt` and
`go build` of the SDK — not by gnr8's own analysis/codegen. Reproduce with:

```bash
bash scripts/bench.sh
```

The benchmark applies exactly the single-field `CreateGoalInput.BenchField` edit documented in
`docs/demo.md` step 7 for the single-file-edit scenario.

---

## Sign-off

All **v1 requirements** are satisfied (all 38 IDs in the `.planning/REQUIREMENTS.md` checklist mapped
above to concrete files/tests; the REQUIREMENTS.md "37" headline is an off-by-one), and the full local
gate **`make check` is green** (fmt-check, clippy `-D warnings`, the complete test suite — lib + bin +
the 4 contract snapshots + determinism + `sdk_compile` + `lifecycle`, fixture-build, goextract-build),
captured live this session with exit code 0. Benchmark numbers for cold / warm-no-op / single-file
edit are recorded as representative, environment-dependent, reproducible-via-`scripts/bench.sh`.

**The v1.0 milestone is ready for review.**
