# Phase 2: Go Analysis And API Graph - Context

**Gathered:** 2026-06-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss --auto — Claude selected recommended defaults)

<domain>
## Phase Boundary

Build the native Go extraction path and produce an inspectable, router-agnostic API graph from the
Phase-1 Gin fixture (`fixtures/goalservice`). This phase implements the `gnr8-core` seams left stubbed
in Phase 1 — `analyze::build_graph` (turns the `snapshot_graph` contract test green) and
`diagnostics::collect` (turns `snapshot_diagnostics` green) — plus the `inspect routes|schemas|graph`
CLI reports. It STOPS before OpenAPI lowering and SDK generation (Phase 3): the deliverable is the
internal graph + diagnostics + inspect reports, not OpenAPI/SDK artifacts.

</domain>

<decisions>
## Implementation Decisions

### Go Parsing Strategy (GO-01, GO-02, GO-03, GO-05)
- **D-01:** Parse Go via a small **Go sidecar helper** that uses the official `golang.org/x/tools/go/packages`
  loader (with `go/ast` + `go/types`), NOT a pure-Rust parser. Rationale: PROJECT.md mandates "official
  language tooling where it provides semantic truth"; accurate type resolution (GO-03) and request/response
  schema inference (GO-05) require `go/types`, which tree-sitter/regex cannot provide. The target users are
  Go developers who already have the Go toolchain installed.
- **D-02:** The helper is a Go program in its own module in this repo (e.g. `goextract/`), invoked by the
  Rust `gnr8-core` analyzer as a subprocess. It emits a single **JSON facts document** (stable, sorted) on
  stdout that the Rust side deserializes (via `serde`). The JSON is the Rust↔Go contract boundary.
- **D-03:** Helper invocation: prefer running a prebuilt/located helper; for the PoC, invoking via the Go
  toolchain (e.g. `go run ./goextract <target>`) is acceptable. The analyzer must surface a clear typed
  diagnostic (not a panic) when the Go toolchain or target module is missing/unbuildable (GO-06).

### Router & Handler Recognition (GO-04, GO-05)
- **D-04:** Recognize the **Gin** call patterns from `.planning/research/TARGET-API.md`: `router.Group(prefix)`
  + `group.METHOD(path, handler)` registration (METHOD ∈ GET/POST/PUT/DELETE), middleware on a group →
  security/marker fact, `c.ShouldBindJSON(&t)` → request body type, `c.Param("x")` → path param,
  `c.Query("x")` → query param. Extraction produces router-agnostic facts (method, full path template,
  params, handler symbol, request type, response type+status, source span) — Gin specifics stay in the
  recognizer, NOT the graph (honors Phase-1 D-03).
- **D-05:** Response inference: from supported typed handler patterns (`c.JSON(http.StatusXxx, dtoValue)`),
  infer status→response-type. Where the handler builds responses dynamically or the type can't be resolved,
  emit a diagnostic rather than guessing (GO-05/GO-06).

### Type Mapping (GO-03)
- **D-06:** Map Go types to graph schema types per the TARGET-API.md table: primitives, `bool`, ints,
  `float64`; pointers/`omitempty` → optional; `[]T` → array; `map[string]T` → object/additionalProperties;
  named structs → schema ref; embedded structs → field flattening; type aliases → underlying; well-known
  `uuid.UUID` → string(uuid), `time.Time` → string(date-time); named-string-with-consts → enum. `json` tags
  drive field names; `binding:"required"` drives required. Unsupported/uncertain types → diagnostic.

### Graph Model & Stable IDs (GRAPH-01, GRAPH-02)
- **D-07:** The internal `ApiGraph` models: routes, operations, parameters (path/query), request bodies,
  responses (by status), schemas (with fields), generated-file placeholders (filled later phases), and
  **source provenance** (file + line span) on every node.
- **D-08:** Node IDs are **deterministic and stable across unchanged runs** (GRAPH-02): operation IDs derived
  from method + normalized path; schema IDs from package-qualified type name. All report/serialized output is
  sorted by a stable key so unchanged source ⇒ byte-identical output.

### Inspect Reports & Diagnostics (GRAPH-03, GO-06)
- **D-09:** `inspect routes`, `inspect schemas`, `inspect graph` render human-readable tables by default and
  machine JSON under the global `--json` flag (reuse Phase-1 CLI surface). Reports explain inferred facts and
  list diagnostics.
- **D-10:** Diagnostics carry a severity, a message, and a **source location** (file:line) for unsupported
  patterns (e.g. `map[string]any`, untyped `c.Query` params, dynamic responses). Unsupported/uncertain
  inference NEVER panics and NEVER silently drops — it produces a diagnostic (GO-06). `diagnostics::collect`
  output must match the Phase-1 `expected/diagnostics.txt` acceptance target in spirit (the `snapshot_diagnostics`
  test will lock the exact text).

### Claude's Discretion
- Exact `goextract` JSON schema shape, internal Rust graph struct layout, the deterministic ID hashing/format,
  table column choices, and whether the helper is `go run` vs a built binary cached under target/ — left to
  research/planning. Snapshot contents (graph/diagnostics) are authored to match the Phase-1 `expected/` targets.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Target shape & type mapping
- `.planning/research/TARGET-API.md` — Gin route/handler/DTO patterns, the Go→OpenAPI→Go-SDK type-mapping
  table (drives D-06), and the pitfalls→diagnostics list (drives D-10).
- `fixtures/goalservice/` — the actual input the analyzer runs on; `expected/diagnostics.txt` is the
  diagnostics acceptance target.

### Phase contract & seams
- `.planning/phases/01-foundation-and-fixtures/01-SUMMARY.md` family — the `gnr8-core` seam signatures
  (`analyze::build_graph(&str) -> Result<ApiGraph, CoreError>`, `diagnostics::collect(&str) -> Result<String, CoreError>`)
  this phase implements, and the `ApiGraph` placeholder to flesh out.
- `crates/gnr8-core/tests/snapshot_graph.rs`, `crates/gnr8-core/tests/snapshot_diagnostics.rs` — the two
  contract tests that must go GREEN this phase (remove red-by-design, add real `.snap`).
- `.planning/REQUIREMENTS.md` — GO-01..06, GRAPH-01..03 acceptance criteria.
- `.planning/PROJECT.md` — "official language tooling for semantic truth", graph-is-source-of-truth, code-first.
- `thoughts/skills/rust-best-practices/` — typed errors, no prod unwrap, subprocess handling, snapshot guidance.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `gnr8-core` `CoreError` (thiserror) — extend with analysis/subprocess/parse variants instead of new error types.
- `analyze::build_graph` + `diagnostics::collect` stubs + `graph::ApiGraph` placeholder — implement these.
- `gnr8` clap CLI with `inspect routes|schemas|graph` subcommands + `--json` — wire to the new analyzer.
- `serde`/`serde_json` already pinned — use for the Go helper JSON contract and report serialization.
- `insta` harness — the two contract tests flip from red-by-design to real snapshots.

### Established Patterns
- thiserror in lib / anyhow only in binary; no prod unwrap; clippy `-D warnings`; deterministic sorted output.
- Router-agnostic graph (Phase-1 D-03): Gin specifics confined to the recognizer.

### Integration Points
- New: `goextract/` Go helper module (its own go.mod). Rust analyzer shells out to it.
- The graph + diagnostics produced here are consumed by Phase 3 (`lower::to_openapi`, `sdk::generate`).
- Makefile/CI may need a `goextract` build/vet target.

</code_context>

<specifics>
## Specific Ideas

- Lean on `go/packages` (LoadMode with types+syntax) for real type info — the differentiator vs comment/regex
  extraction. Keep the helper small and single-purpose (facts extraction only; no OpenAPI knowledge in Go).
- Determinism is a first-class requirement (GRAPH-02): sort everything, derive IDs from stable source identity.

</specifics>

<deferred>
## Deferred Ideas

- OpenAPI lowering + Go SDK generation — Phase 3 (this phase stops at graph + diagnostics + inspect).
- Additional routers (chi/echo/net-http) — post-PoC; only the router-agnostic graph seam is reserved.
- Incremental/partial graph invalidation + watch — Phase 4.
- Deep handler-body interpretation beyond supported typed patterns — out of scope (diagnose instead).

</deferred>

---

*Phase: 02-go-analysis-and-api-graph*
*Context gathered: 2026-06-24 (auto mode)*
