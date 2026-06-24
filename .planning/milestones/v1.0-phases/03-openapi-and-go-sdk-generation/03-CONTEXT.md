# Phase 3: OpenAPI And Go SDK Generation - Context

**Gathered:** 2026-06-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss --auto — Claude selected recommended defaults)

<domain>
## Phase Boundary

Generate real artifacts from the Phase-2 `ApiGraph`: a valid **OpenAPI 3.1** document and a **compiling
Go SDK**. Implements the two remaining `gnr8-core` seams — `lower::to_openapi` (flips the `snapshot_openapi`
contract test green) and `sdk::generate` (flips `snapshot_sdk` green) — plus generated-SDK compile/smoke
tests and end-to-end artifact snapshots. This phase consumes the graph; it does NOT add `.gnr8/` workspace
lifecycle or watch mode (Phase 4). After this phase, all four contract tests are green and the core
source→OpenAPI→Go-SDK loop works end-to-end on the fixture.

</domain>

<decisions>
## Implementation Decisions

### OpenAPI Lowering (OAPI-01, OAPI-02)
- **D-01:** Lower `ApiGraph` → an **OpenAPI 3.1.0** document modeled as **typed Rust structs** (serde-serializable),
  covering the needed subset: `info`, `paths` → operations (operationId, summary, tags, security), `parameters`
  (path + query, with enum/required), `requestBody`, `responses` by status code, `components.schemas` (with
  field types/required/refs), and `components.securitySchemes` (the API-key scheme from annotation facts).
  Emit as **YAML** (primary, for the `insta` snapshot + reconciling with `fixtures/goalservice/expected/openapi.yaml`)
  and support JSON form. Deterministic: sorted keys / stable ordering.
- **D-02:** Reuse the Phase-2 graph's type mapping (uuid→string/uuid, time.Time→string/date-time, pointer/omitempty→
  optional, enums→string enum, nested→`$ref`, embedded→flattened, `[]T`→array, `map[string]T`→object). The graph
  already carries these facts; lowering is graph→OpenAPI-shape, not re-analysis.

### OpenAPI Compatibility Diagnostics (OAPI-03)
- **D-03:** When a graph fact cannot be represented cleanly in the OpenAPI 3.1 target, emit a **diagnostic** (not a
  silent drop / not a panic): e.g. `map[string]any` free-form maps, float64→float32 narrowing risk carried into the
  SDK, untyped query params. Carry forward / reconcile the Phase-2 diagnostics where they pertain to lowering; add
  lowering-specific ones. Diagnostics keep source provenance.

### Go SDK Shape (SDK-01, SDK-02, SDK-03, SDK-04)
- **D-04:** Generate a single Go SDK **package** matching Phase-1 `expected/sdk/` (D-05 from Phase 1): a **`Client`**
  constructed with **functional options** (base URL + custom `*http.Client`) (SDK-01); **tag-grouped typed operation
  methods** with `context.Context` as the first arg (SDK-02); generated **request/response model structs** (SDK-03);
  JSON encode/decode + a **typed API error** carrying status + decoded error body (SDK-04). Idiomatic Go — NOT the
  verbose openapi-generator builder pattern.
- **D-05:** Code generation is done in **Rust** (deterministic string emission; sorted; produces gofmt-clean Go — or
  the build/test step runs through code that compiles regardless). No heavy template-engine dependency; a small
  internal templating approach is fine. Output is stable across unchanged runs.

### Generated-SDK Compile & Smoke Tests (SDK-05)
- **D-06:** `sdk::generate(&ApiGraph)` returns a deterministic representation (the seam returns `String`; model the
  multi-file SDK as a stable serialized bundle for the `snapshot_sdk` snapshot). A separate writer materializes the
  files to a temp dir with a `go.mod`; a Rust test runs **`go build`** on it (SDK-05 compiles) and a **smoke test**
  that constructs the `Client` and calls a fixture operation against an `httptest`-style stub, asserting request
  shape + response decode (SDK-05 "can call fixture operations through tests").

### Snapshots & Gates
- **D-07:** Flip `snapshot_openapi` + `snapshot_sdk` from red-by-design to **real `insta` snapshots** (review the
  generated artifacts match the fixture); reconcile with the hand-authored `expected/openapi.yaml` + `expected/sdk/`
  reference targets. **Promote all four contract tests to the blocking CI gate** (the non-blocking `contract` job is
  now empty / removed — Open Q1 option d's final state). End-to-end: cold `generate` produces both artifacts.

### Claude's Discretion
- Exact Rust OpenAPI struct layout, the SDK bundle serialization format for the snapshot, the templating mechanism,
  temp-dir/go.mod scaffolding for the compile test, and table/field ordering — left to research/planning. Snapshot
  contents are authored to reflect the real generated output (reconciled with the expected/ reference targets).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Target artifact shapes
- `fixtures/goalservice/expected/openapi.yaml` — the OpenAPI 3.1 acceptance-target shape `to_openapi` reconciles with.
- `fixtures/goalservice/expected/sdk/{client,goals,models,errors}.go` — the Go SDK acceptance-target shape `generate` reconciles with.
- `.planning/research/TARGET-API.md` — Go→OpenAPI→Go-SDK type-mapping table (§4) + pitfalls→diagnostics (§5: float64 narrowing, additionalProperties, etc.) that drive D-03.

### Graph input & seams
- `crates/gnr8-core/src/graph/mod.rs` — the `ApiGraph` (Phase-2 output) this phase consumes; do not re-analyze.
- `crates/gnr8-core/src/lower/mod.rs`, `crates/gnr8-core/src/sdk/mod.rs` — the seams to implement (signatures return `Result<String, CoreError>`).
- `crates/gnr8-core/tests/snapshot_openapi.rs`, `crates/gnr8-core/tests/snapshot_sdk.rs` — the two contract tests to flip GREEN.
- `.planning/REQUIREMENTS.md` — OAPI-01..03, SDK-01..05.
- `.planning/phases/02-go-analysis-and-api-graph/02-03-SUMMARY.md` — how the graph is shaped + serialized.
- `.planning/PROJECT.md` — owned pipeline, graph-is-source-of-truth, OpenAPI-is-an-artifact, code-first.
- `thoughts/skills/rust-best-practices/` — typed errors, no prod unwrap, subprocess (go build) handling, snapshots.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `ApiGraph` (Phase 2) — fully populated with routes/operations/params/bodies/responses/schemas/provenance + diagnostics.
- `CoreError` (thiserror) — extend with lowering/sdk/go-build variants.
- `lower::to_openapi` + `sdk::generate` stubs — implement these (currently `NotYetImplemented`).
- `serde`/`serde_json` + insta (yaml) already pinned — use for OpenAPI structs + snapshots. `std::process::Command` for `go build`.
- The Phase-2 Go-toolchain subprocess pattern (helper invocation, typed-error mapping) — reuse for the SDK `go build` compile test.

### Established Patterns
- thiserror in lib / anyhow only in binary; no prod unwrap; clippy `-D warnings`; deterministic sorted output; insta snapshots.
- Diagnostics carry source provenance and never panic/drop (GO-06 pattern → OAPI-03).

### Integration Points
- `gnr8 generate` (CLI) wires to `to_openapi` + `sdk::generate` to emit artifacts (full surface may land here or Phase 4/5).
- Compile test needs the Go toolchain (available in CI, as in Phase 2).
- Output of this phase is the foundation for Phase 4's `.gnr8/` lifecycle + watch (no-op detection on these artifacts).

</code_context>

<specifics>
## Specific Ideas

- The generated Go SDK MUST genuinely compile and be exercised (SDK-05) — a real `go build` + httptest smoke test,
  not just a string snapshot. This is the phase's hardest acceptance bar.
- Keep OpenAPI as an *artifact* serialized from typed structs; the graph stays the source of truth (PROJECT constraint).
- Determinism: identical graph ⇒ byte-identical OpenAPI + SDK output.

</specifics>

<deferred>
## Deferred Ideas

- `.gnr8/` workspace init, generated-file ownership tracking, no-op detection, watch mode — Phase 4.
- `doctor` diagnostics aggregation, perf benchmarks, demo docs — Phase 5.
- OpenAPI 3.0 downstream-generator compatibility mode — future (behind diagnostics).
- TypeScript/Python SDK targets — v2 (out of scope).

</deferred>

---

*Phase: 03-openapi-and-go-sdk-generation*
*Context gathered: 2026-06-24 (auto mode)*
