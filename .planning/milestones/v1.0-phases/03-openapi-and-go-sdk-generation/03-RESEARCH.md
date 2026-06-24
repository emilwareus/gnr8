# Phase 3: OpenAPI And Go SDK Generation - Research

**Researched:** 2026-06-24
**Domain:** Deterministic artifact generation in Rust — OpenAPI 3.1 lowering + Go SDK codegen from a typed API graph, with a real `go build` + httptest compile/smoke gate
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Lower `ApiGraph` → an **OpenAPI 3.1.0** document modeled as **typed Rust structs** (serde-serializable),
  covering the needed subset: `info`, `paths` → operations (operationId, summary, tags, security), `parameters`
  (path + query, with enum/required), `requestBody`, `responses` by status code, `components.schemas` (with
  field types/required/refs), and `components.securitySchemes` (the API-key scheme from annotation facts).
  Emit as **YAML** (primary, for the `insta` snapshot + reconciling with `fixtures/goalservice/expected/openapi.yaml`)
  and support JSON form. Deterministic: sorted keys / stable ordering.
- **D-02:** Reuse the Phase-2 graph's type mapping (uuid→string/uuid, time.Time→string/date-time, pointer/omitempty→
  optional, enums→string enum, nested→`$ref`, embedded→flattened, `[]T`→array, `map[string]T`→object). The graph
  already carries these facts; lowering is graph→OpenAPI-shape, not re-analysis.
- **D-03:** When a graph fact cannot be represented cleanly in the OpenAPI 3.1 target, emit a **diagnostic** (not a
  silent drop / not a panic): e.g. `map[string]any` free-form maps, float64→float32 narrowing risk carried into the
  SDK, untyped query params. Carry forward / reconcile the Phase-2 diagnostics where they pertain to lowering; add
  lowering-specific ones. Diagnostics keep source provenance.
- **D-04:** Generate a single Go SDK **package** matching Phase-1 `expected/sdk/` (D-05 from Phase 1): a **`Client`**
  constructed with **functional options** (base URL + custom `*http.Client`) (SDK-01); **tag-grouped typed operation
  methods** with `context.Context` as the first arg (SDK-02); generated **request/response model structs** (SDK-03);
  JSON encode/decode + a **typed API error** carrying status + decoded error body (SDK-04). Idiomatic Go — NOT the
  verbose openapi-generator builder pattern.
- **D-05:** Code generation is done in **Rust** (deterministic string emission; sorted; produces gofmt-clean Go — or
  the build/test step runs through code that compiles regardless). No heavy template-engine dependency; a small
  internal templating approach is fine. Output is stable across unchanged runs.
- **D-06:** `sdk::generate(&ApiGraph)` returns a deterministic representation (the seam returns `String`; model the
  multi-file SDK as a stable serialized bundle for the `snapshot_sdk` snapshot). A separate writer materializes the
  files to a temp dir with a `go.mod`; a Rust test runs **`go build`** on it (SDK-05 compiles) and a **smoke test**
  that constructs the `Client` and calls a fixture operation against an `httptest`-style stub, asserting request
  shape + response decode (SDK-05 "can call fixture operations through tests").
- **D-07:** Flip `snapshot_openapi` + `snapshot_sdk` from red-by-design to **real `insta` snapshots** (review the
  generated artifacts match the fixture); reconcile with the hand-authored `expected/openapi.yaml` + `expected/sdk/`
  reference targets. **Promote all four contract tests to the blocking CI gate** (the non-blocking `contract` job is
  now empty / removed — Open Q1 option d's final state). End-to-end: cold `generate` produces both artifacts.

### Claude's Discretion

- Exact Rust OpenAPI struct layout, the SDK bundle serialization format for the snapshot, the templating mechanism,
  temp-dir/go.mod scaffolding for the compile test, and table/field ordering — left to research/planning. Snapshot
  contents are authored to reflect the real generated output (reconciled with the expected/ reference targets).

### Deferred Ideas (OUT OF SCOPE)

- `.gnr8/` workspace init, generated-file ownership tracking, no-op detection, watch mode — Phase 4.
- `doctor` diagnostics aggregation, perf benchmarks, demo docs — Phase 5.
- OpenAPI 3.0 downstream-generator compatibility mode — future (behind diagnostics).
- TypeScript/Python SDK targets — v2 (out of scope).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| OAPI-01 | The OpenAPI writer emits a valid document for the fixture service. | §1 typed-struct model + §2 graph→OpenAPI mapping; serialize via hand-rolled stable YAML emitter (§1). Validity verified by reconciling with `expected/openapi.yaml`. |
| OAPI-02 | The document includes info, paths, operations, parameters, request bodies, responses, and component schemas. | §1 struct set enumerates every required node; §2 maps each `ApiGraph` node to its OpenAPI counterpart. |
| OAPI-03 | Lowering emits diagnostics when the target cannot represent a graph fact cleanly. | §3 — reuse Phase-2 graph diagnostics (already 7 lines, byte-locked) + identify which are lowering-relevant; do NOT re-derive, surface them through the lowering path. |
| SDK-01 | Go SDK includes a usable client with base URL and custom `http.Client` support. | §4 — `Client` + functional options (`WithHTTPClient`, `WithAPIKey`, `NewClient`), matches `expected/sdk/client.go`. |
| SDK-02 | The SDK exposes typed methods for generated operations. | §4 — tag-grouped methods, `ctx` first, path/query/body handling; matches `expected/sdk/goals.go`. |
| SDK-03 | The SDK includes generated request and response models. | §4 — model structs with json tags, optional pointers, enums, uuid/time mapping; matches `expected/sdk/models.go`. |
| SDK-04 | The SDK handles JSON encoding/decoding and typed API errors. | §4 — `encoding/json` marshal/decode + `APIError` type; matches `expected/sdk/errors.go`. |
| SDK-05 | The generated SDK compiles and is exercised by fixture tests. | §5 (bundle/writer split) + §6 (temp-dir `go.mod` + `go build` + httptest smoke). SDK is stdlib-only → hermetic, no network. |
</phase_requirements>

## Summary

Phase 3 turns the byte-stable Phase-2 `ApiGraph` into two real artifacts: an OpenAPI 3.1.0 YAML document and a
compiling, idiomatic Go SDK. The graph already carries every fact needed (operations sorted by `(path, method)`,
schemas sorted by id, params/fields/responses/enums all pre-sorted, `SchemaType` with `kind/format/items/ref_id/
additional_properties`, plus 7 byte-locked diagnostics). Lowering is therefore a pure **graph → artifact-shape
transformation** with no re-analysis — exactly as D-02 requires. The hard acceptance bar is SDK-05: the generated Go
must genuinely `go build` and answer an `httptest` round-trip, not merely match a string snapshot.

The strongest, lowest-risk recommendation is to **hand-roll** both emitters. For OpenAPI: a small set of serde-derived
Rust structs plus a **deterministic YAML writer**. A crate (`openapiv3`, `okapi`, `utoipa`) is a poor fit here — they
model OpenAPI 3.0.x not 3.1's JSON-Schema-2020-12 alignment, they pull large dependency trees, and `serde_yaml` is
unmaintained/deprecated and is NOT in the current dependency tree (insta vendors its own internal YAML serializer
for `.snap` framing — that is not a reusable public emitter). A ~150-line key-ordered YAML writer that walks the
typed structs gives byte-exact control to reconcile with the hand-authored `expected/openapi.yaml` and guarantees
determinism without adding a heavy dependency, which D-01/D-05 and the PROJECT "no heavy crates" constraint demand.
For the Go SDK: `format!`-based emission into a stable multi-file bundle, then run the real `gofmt` binary over each
file (the Go toolchain is already a hard project dependency) so the output is gofmt-clean by construction rather than
by hand-counting spaces.

**Primary recommendation:** Hand-roll typed OpenAPI 3.1 structs + a deterministic key-ordered YAML writer (no new
crates); emit the Go SDK with `format!`-based templating piped through the real `gofmt`; model the SDK as a stable
file-marker bundle `String` for the `snapshot_sdk` snapshot with a separate disk writer; gate SDK-05 with a hermetic
stdlib-only `go.mod` (no external requires → no network) + `go build ./...` + an `httptest` smoke test, all mapping
failures to a new typed `CoreError` variant. Split into 3 plans: 03-01 (OpenAPI), 03-02 (Go SDK), 03-03 (compile/
smoke + e2e snapshots + CI promotion).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| OpenAPI 3.1 document modeling | `gnr8-core` lib (`lower`) | — | Pure typed-struct transform of the in-memory graph; no I/O, no subprocess. |
| OpenAPI YAML serialization | `gnr8-core` lib (`lower`) | — | Deterministic string emission from typed structs; library-owned for testability. |
| OpenAPI compatibility diagnostics | `gnr8-core` lib (`diagnostics` + `lower`) | — | Diagnostics already collected in Phase 2; lowering surfaces the subset it cares about. No new analysis tier. |
| Go SDK model/client/op/error codegen | `gnr8-core` lib (`sdk`) | — | Deterministic Rust string emission; library-owned so the contract test can snapshot it. |
| gofmt normalization | `gnr8-core` lib (`sdk`) + Go toolchain subprocess | — | `gofmt` binary is the formatting source of truth; invoked via `std::process::Command` like Phase-2's `go run`. |
| SDK materialization to disk | Rust **test** harness (`tests/`) | `sdk` writer fn | The disk writer is a library fn; the temp-dir scaffolding + `go build` invocation live in an integration test. |
| `go build` + httptest smoke | Rust **test** harness (`tests/`) + Go toolchain | — | Compile/exercise is a test-tier concern; failures map to typed errors, never panics. |
| CLI `generate` wiring (artifact emission) | `gnr8` binary (`main`) | `gnr8-core` seams | Binary is the anyhow boundary; calls `to_openapi`/`generate` and writes files. Full surface may be Phase 4/5. |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `serde` | 1.0 (pinned, workspace) | Derive on the typed OpenAPI structs (forward-compat if a JSON form is ever serialized via `serde_json`) | Already pinned + used across the graph; zero new dependency. `[VERIFIED: Cargo.toml workspace.dependencies]` |
| `serde_json` | 1.0 (pinned, workspace) | Optional JSON form of OpenAPI (D-01 "support JSON form"); also the existing facts parser | Already pinned; JSON output is one `serde_json::to_string_pretty` call if a JSON variant is wanted. `[VERIFIED: Cargo.toml]` |
| `thiserror` | 2.0 (pinned, workspace) | New `CoreError` variants for lowering / SDK / go-build failures | Already the crate-level error mechanism (RUST-04). `[VERIFIED: Cargo.toml]` |
| `insta` | 1.48, `yaml` feature (pinned, workspace, dev-dep) | `assert_snapshot!` for the two contract snapshots (plain-text, not yaml-redacted) | Already the snapshot tool; the two contract tests use `assert_snapshot!`. `[VERIFIED: snapshot_openapi.rs:23 + Cargo.toml]` |
| `std::process::Command` | std | `gofmt` normalization + `go build` compile gate | Same subprocess pattern as Phase-2 `run_goextract`. `[VERIFIED: helper.rs]` |
| Go toolchain (`go`, `gofmt`) | 1.26.x | Compile the generated SDK + format it | Already a hard project dependency (CI sets up `go-version: '1.26'`). `[VERIFIED: go version go1.26.2 on PATH; ci.yml setup-go]` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tempfile` | 3.x | Hermetic temp dir for the SDK compile test (auto-cleanup, unique path, parallel-safe) | RECOMMENDED for the compile test as a **dev-dependency only**. Alternative: `std::env::temp_dir()` + a manually-generated unique subdir, which avoids any new dependency. Decide in 03-03. `[ASSUMED]` pending slopcheck (see Audit). |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled OpenAPI structs | `openapiv3` crate | Models OpenAPI **3.0.x**, not 3.1's JSON-Schema-2020-12 (`type` arrays, `$schema`, no `nullable` keyword). Would fight the 3.1.0 target. Large dep tree. NOT RECOMMENDED. `[CITED: github.com/glademiller/openapiv3]` |
| Hand-rolled OpenAPI structs | `okapi` / `utoipa` | `utoipa` is annotation-macro-driven (you decorate Rust types to *produce* OpenAPI) — wrong direction; we lower a graph, not annotate Rust types. `okapi` is rocket-ecosystem-coupled and 3.0-centric. NOT RECOMMENDED. `[CITED: docs.rs/utoipa]` |
| Hand-rolled YAML writer | `serde_yaml` | **Deprecated/unmaintained** (archived 2024), NOT in the current lockfile, and gives weak control over key ordering / block-vs-flow style needed to match `expected/openapi.yaml` byte-for-byte. NOT RECOMMENDED. `[CITED: github.com/dtolnay/serde-yaml — archived]` |
| Hand-rolled YAML writer | `serde_norway` (maintained serde_yaml fork) | A live fork exists, but adds a dependency for ~150 lines of writer we can fully control, and still needs ordering shims (`IndexMap`) to match the fixture. Lower-risk to hand-roll. NOT RECOMMENDED for the PoC. `[ASSUMED]` |
| `format!` + real `gofmt` | A Rust template engine (`askama`, `tera`, `minijinja`) | D-05 forbids a heavy template engine. `format!` + `gofmt` is deterministic, dependency-free, and the real Go formatter guarantees canonical output. RECOMMENDED. `[VERIFIED: CONTEXT.md D-05]` |

**Installation:**
```bash
# No new RUNTIME crate. Only a possible dev-dependency for the compile test:
cargo add --dev tempfile -p gnr8-core   # OPTIONAL — or use std::env::temp_dir() to avoid it entirely
```

**Version verification (run during planning before locking the stack):**
```bash
cargo search tempfile        # confirm current 3.x and that it is the maintained crate
# OpenAPI/YAML crates are intentionally NOT added — verification confirms the recommendation to avoid them.
```

## Package Legitimacy Audit

> This phase adds **zero new runtime crates**. The only candidate is `tempfile` as an optional **dev-dependency**,
> and it can be avoided entirely with `std`. slopcheck could not be installed/run in this research session, so the
> single candidate is tagged `[ASSUMED]` and the planner must gate it behind a `checkpoint:human-verify` task (or
> drop it in favor of `std::env::temp_dir()`).

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `serde` | crates.io | mature | very high | github.com/serde-rs/serde | n/a (already pinned) | Approved (existing) |
| `serde_json` | crates.io | mature | very high | github.com/serde-rs/json | n/a (already pinned) | Approved (existing) |
| `thiserror` | crates.io | mature | very high | github.com/dtolnay/thiserror | n/a (already pinned) | Approved (existing) |
| `insta` | crates.io | mature | very high | github.com/mitsuhiko/insta | n/a (already pinned) | Approved (existing) |
| `tempfile` | crates.io | mature (well-known) | very high | github.com/Stebalien/tempfile | not run | Flagged — planner adds checkpoint:human-verify OR uses `std` fallback |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none (slopcheck unavailable; `tempfile` tagged `[ASSUMED]` per protocol)

*slopcheck was unavailable at research time, so `tempfile` is `[ASSUMED]`; the planner must gate it behind a
`checkpoint:human-verify` task before install, OR choose the zero-dependency `std::env::temp_dir()` path.*

## Architecture Patterns

### System Architecture Diagram

```
                         crates/gnr8-core (library)
  build_graph(fixture)
        │  (Phase 2 — already implemented; returns ApiGraph)
        ▼
   ┌──────────┐
   │ ApiGraph │  module + operations[] + schemas[] + diagnostics[]   (byte-stable, pre-sorted)
   └────┬─────┘
        │
        ├──────────────────────────────► lower::to_openapi(&graph) ──► OpenApiDoc (typed structs)
        │                                       │                            │
        │                                       │  (join /goal prefix,       │ yaml::write(&doc)
        │                                       │   map SchemaType→Schema,   ▼  (deterministic, key-ordered)
        │                                       │   collect securitySchemes)  String  ──► snapshot_openapi (insta)
        │                                       │                                 │
        │                                       └──► surface lowering diagnostics ┘  (reuse Phase-2 set, OAPI-03)
        │
        └──────────────────────────────► sdk::generate(&graph) ──► SdkBundle (files: models/client/<tag>/errors)
                                                │                          │
                                                │  format!-based emit      │ bundle.to_string()  (file-marker frames)
                                                │  per file, then          ▼
                                                │  gofmt(file)        String ──► snapshot_sdk (insta)
                                                │
                                                └──► sdk::write_to_dir(&bundle, path)  (separate disk writer)
                                                            │
                                                            ▼
                                       tests/sdk_compile.rs (integration / test tier)
                                         temp dir + generated go.mod (stdlib-only, NO requires)
                                         ├─► go build ./...               (SDK-05: compiles)
                                         └─► httptest stub + Client call  (SDK-05: exercised)
                                                 asserts method/path/body + decode + typed APIError
```

### Recommended Project Structure
```
crates/gnr8-core/src/
├── lower/
│   ├── mod.rs            # to_openapi(&ApiGraph) -> Result<String, CoreError>  (the seam)
│   ├── model.rs          # typed OpenAPI 3.1 structs (OpenApiDoc, PathItem, Operation, Schema, ...)
│   └── yaml.rs           # deterministic key-ordered YAML writer over the typed model
├── sdk/
│   ├── mod.rs            # generate(&ApiGraph) -> Result<String, CoreError>  + write_to_dir(...)
│   ├── bundle.rs         # SdkBundle (ordered Vec<SdkFile{name, contents}>) + to_string() framing
│   ├── emit.rs           # format!-based emitters: models / client / operations(by tag) / errors
│   └── gofmt.rs          # gofmt(&str) -> Result<String, CoreError>  (subprocess, like helper.rs)
└── error.rs              # + Lowering / SdkGen / GoFmt / GoBuild variants

crates/gnr8-core/tests/
├── snapshot_openapi.rs   # EXISTS — flip GREEN (accept reviewed .snap)
├── snapshot_sdk.rs       # EXISTS — flip GREEN (accept reviewed .snap)
└── sdk_compile.rs        # NEW — materialize SDK to temp dir, go build, httptest smoke (SDK-05)
```

### Pattern 1: Typed OpenAPI model with a deterministic key-ordered YAML writer
**What:** Define plain Rust structs mirroring the OpenAPI 3.1 subset, then walk them with a hand-written writer that
emits keys in a **fixed, spec-conventional order** (e.g. `openapi`, `info`, `security`, `paths`, `components`) rather
than relying on serde's struct-field order through a YAML library you do not control.
**When to use:** When the output must be byte-reconcilable with a hand-authored fixture (here, `expected/openapi.yaml`)
and determinism is a hard requirement (GRAPH-02 carries into the artifacts).
**Example (illustrative struct shape — not from a crate):**
```rust
// Source: hand-rolled per CONTEXT D-01; OpenAPI 3.1.0 field set per spec.openapis.org/oas/v3.1.0
pub struct OpenApiDoc {
    pub openapi: &'static str,            // "3.1.0"
    pub info: Info,                       // title, version, description
    pub security: Vec<SecurityRequirement>, // top-level: [{ApiKeyAuth: []}]
    pub paths: Vec<(String, PathItem)>,   // Vec (not a map) so ordering is explicit + sorted
    pub components: Components,            // schemas: Vec<(id, Schema)>, security_schemes: Vec<...>
    pub diagnostics: Vec<Diagnostic>,     // OAPI-03 carry-through (not serialized into the doc)
}
```
> Use `Vec<(String, T)>` for every map-like construct (`paths`, `schemas`) so insertion/sort order is explicit and
> deterministic; never serialize a `HashMap` (the same rule the graph already follows — graph/mod.rs module doc).

### Pattern 2: OpenAPI 3.1 specifics that differ from 3.0 (do not get these wrong)
**What:** 3.1.0 aligns with JSON Schema 2020-12.
- `openapi: 3.1.0` (string).
- **Nullable:** there is **no `nullable: true`** in 3.1. Optionality is expressed by *absence from `required`*
  (which is exactly what `expected/openapi.yaml` does — optional fields simply aren't listed in `required`). The
  fixture does NOT use `type: [T, "null"]` arrays; it relies on `required` omission. Match the fixture: **do not emit
  `nullable` and do not emit `type` arrays** — keep `type: string` etc. and drive optionality via the `required` list.
- **`$ref`:** JSON-pointer form `'#/components/schemas/Name'`, quoted in YAML (the fixture quotes it).
- **`additionalProperties: true`** for free-form maps (`map[string]any` → `GoalResponse.metadata`), per TARGET-API §5.1.
- **`format`** is an annotation keyword (`uuid`, `date-time`, `int64`) — emit alongside `type`.
**When to use:** Always, for this target. Confirmed by reading every line of `expected/openapi.yaml`.

### Pattern 3: Go SDK emission via `format!` then real `gofmt`
**What:** Build each Go file's text with `format!`/`writeln!` into a `String`, then pipe it through the `gofmt`
binary so indentation/import-grouping/alignment are canonical. Never hand-align Go by counting tabs.
**When to use:** All generated `.go` files. The Go toolchain is already required, so `gofmt` is free.
**Example (subprocess shape, mirrors helper.rs):**
```rust
// Source: same pattern as crates/gnr8-core/src/analyze/helper.rs run_goextract
fn gofmt(src: &str) -> Result<String, CoreError> {
    use std::io::Write;
    let mut child = std::process::Command::new("gofmt")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;
    child.stdin.take().expect("piped").write_all(src.as_bytes())  // expect only in non-prod? -> use ?-friendly take
        .map_err(|source| CoreError::GoFmt { source: source.to_string() })?;
    let out = child.wait_with_output().map_err(/* ... */)?;
    // non-zero status => CoreError::GoFmt with stderr; else String::from_utf8(out.stdout)
}
```
> Caveat (RUST-04): the `.expect("piped")` above must be replaced with a `let Some(mut stdin) = child.stdin.take()
> else { return Err(...) }` pattern in production code — no `expect` in lib paths. Flagged here so the planner makes
> it explicit.

### Anti-Patterns to Avoid
- **Pulling an OpenAPI crate (`openapiv3`/`okapi`/`utoipa`):** wrong spec version (3.0) or wrong direction (annotation→spec). Adds heavy deps the PROJECT forbids.
- **Using `serde_yaml`:** deprecated/unmaintained, not in the lockfile, weak key-ordering control.
- **Serializing a `HashMap` anywhere:** breaks determinism (the graph deliberately never does this).
- **Hand-formatting Go whitespace:** brittle; let `gofmt` own it.
- **Re-deriving diagnostics in the lowering layer:** Phase 2 already produced the 7 byte-locked diagnostics; OAPI-03 surfaces the relevant subset, it does not recompute them.
- **`go.mod` with external `require`s in the compile test:** would force a network fetch in CI. The SDK is stdlib-only — keep it that way.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Go source formatting | A Rust Go-pretty-printer | the real `gofmt` binary (subprocess) | Canonical output, zero drift, already on PATH. |
| Compiling the generated SDK | A Go-AST type checker in Rust | `go build ./...` subprocess | Only the real compiler proves SDK-05. |
| HTTP round-trip in the smoke test | A custom mock server | Go's `net/http/httptest` (in the generated test, run by `go test`) | Stdlib, hermetic, idiomatic; no Rust HTTP stack needed. |
| JSON encode/decode in the SDK | A custom marshaller | Go stdlib `encoding/json` (in generated code) | Matches `expected/sdk` (json tags drive it). |
| Diagnostics text | New lowering-time diagnostic strings | The Phase-2 `diagnostics::collect` output (already byte-locked) | OAPI-03 reuses the existing 7-line set; avoids divergence. |

**Key insight:** The "generator" in this phase is a deterministic string transform. The genuinely hard/edge-case-rich
work (Go formatting, Go compilation, HTTP serving, JSON (de)serialization) is delegated to the Go toolchain and Go
stdlib *inside the generated artifact* — Rust never reimplements any of it. This keeps the Rust surface small and the
correctness bar (does it compile + answer a request?) owned by real tools.

## Runtime State Inventory

> This is a code/artifact-generation phase, not a rename/refactor/migration. No stored data, live-service config, or
> OS-registered state is renamed. Inventory included for completeness; all categories verified empty.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — no datastore keys/IDs are renamed; the graph is in-memory. | none |
| Live service config | None — no external service config touched. | none |
| OS-registered state | None — no scheduled tasks/services. | none |
| Secrets/env vars | None — the `WithAPIKey` option is generated SDK *code*; no real secret is stored or read by gnr8. | none |
| Build artifacts | The compile test creates a **transient** temp dir (auto-cleaned); no persistent build artifact is added to the repo. The two `.snap` files ARE new committed artifacts (expected). | Commit the two reviewed `.snap` files. |

**Nothing found in categories Stored data / Live service config / OS-registered state / Secrets — verified by reading
the graph (in-memory), the SDK shape (stdlib-only), and CONTEXT (no service wiring this phase).**

## Common Pitfalls

### Pitfall 1: The graph `path` is group-relative; the OpenAPI document needs the absolute `/goal/...` path
**What goes wrong:** The graph stores `path` as `/`, `/list`, `/{uuid}` (group-relative) with the dynamic `"/" +
basePath` prefix deliberately NOT folded (graph/mod.rs:46-52 + 02-03 SUMMARY key-decisions). But `expected/openapi.yaml`
keys are absolute: `/goal/`, `/goal/list`, `/goal/{uuid}`. If lowering emits the group-relative path the snapshot will
not match the fixture.
**Why it happens:** The base prefix is a runtime string the Go helper cannot constant-fold; 02-03 explicitly deferred
joining it to "Phase-3 lowering."
**How to avoid:** In `to_openapi`, join the service base prefix (`/goal`) with each operation's `path`, normalizing
the slash (`/goal` + `/` → `/goal/`, `/goal` + `/list` → `/goal/list`, `/goal` + `/{uuid}` → `/goal/{uuid}`).
**OPEN QUESTION (see §Open Questions):** where the `/goal` prefix comes from deterministically — the graph carries
`module` and per-op `router_path` but not an explicit service base path. Likely derived from the fixture's single
group, or a fixed lowering input. Must be resolved in 03-01 planning.
**Warning signs:** Snapshot diff shows `/` vs `/goal/` path keys.

### Pitfall 2: `expected/openapi.yaml` is a *reference target*, not the literal snapshot — reconcile, don't blind-copy
**What goes wrong:** The fixture YAML was hand-authored "scoped and reviewable, not exhaustive" (its header) and
carries explanatory NOTE comments (e.g. on `targetValue`, `metadata`). The generated output will differ in incidental
ways (comment lines, description presence, `info.description`). Treating the fixture as the literal expected snapshot
will cause churn.
**Why it happens:** D-07 says "reconcile with the hand-authored reference," not "byte-equal it."
**How to avoid:** Author the `.snap` from the **real generated output**, review it against the fixture for *semantic*
equivalence (same paths, operations, schemas, required lists, refs, security), then accept. This is exactly the
process 02-03 used for `snapshot_graph` (SUMMARY "Issues Encountered": author from real output, review, rename
`.snap.new`→`.snap`, strip `assertion_line`). `cargo-insta` is NOT installed in this environment — use that manual flow.
**Warning signs:** Trying to make the generator emit the fixture's `#` comments.

### Pitfall 3: Generated Go that snapshots fine but does not compile (SDK-05's whole point)
**What goes wrong:** A string snapshot can be "correct-looking" yet not compile — unused imports, an unreferenced
type, a method on a type in another file the bundle didn't include, wrong package clause.
**Why it happens:** The snapshot and the compile gate are independent; passing one does not imply the other.
**How to avoid:** Make `go build ./...` (03-03) a **blocking** test, run on the *materialized* files (not the bundle
string). Generate only the imports actually used (the model file needs `time` only if a `time.Time` field exists;
`errors.go` needs `fmt`; operation files need `context`, `net/http`, `encoding/json`, `bytes`). `gofmt` removes
formatting issues but NOT unused imports — `go build` will still fail on an unused import, so import selection must be
computed from the emitted content, or use `goimports` (note: `goimports` is not stdlib — prefer computing imports).
**Warning signs:** `go build` errors like `imported and not used` or `undefined: X`.

### Pitfall 4: Non-deterministic ordering leaking into the SDK or OpenAPI output
**What goes wrong:** Iterating a `HashMap`, or emitting operations/fields in arrival order, produces output that
differs run-to-run → snapshot flaps + violates the idempotent-generation requirement (TARGET-API §5.6).
**Why it happens:** Easy to reach for a map when grouping operations by tag.
**How to avoid:** The graph is already fully sorted (operations by `(path, method)`, schemas by id, fields by json
name, params by name). Preserve that order; when grouping operations by tag for the SDK, sort tags lexically and
operations within a tag by their existing graph order. Use `Vec<(K, V)>` not `HashMap`. Add a determinism assertion
(two `generate` runs byte-identical) mirroring `tests/determinism.rs`.
**Warning signs:** `tests/determinism.rs`-style two-run check fails; `.snap` changes with no source change.

### Pitfall 5: `go build` test is non-hermetic / needs network in CI
**What goes wrong:** If the generated `go.mod` `require`s gin/uuid/etc., `go build` fetches from the module proxy →
slow, flaky, and fails in a sandboxed/offline CI.
**Why it happens:** Reflexively copying the fixture's `go.mod` (which DOES require gin).
**How to avoid:** The generated SDK imports **only Go stdlib** (`context`, `net/http`, `encoding/json`, `bytes`,
`fmt`, `time` — confirmed by reading all four `expected/sdk/*.go`). So the compile-test `go.mod` needs only `module
<name>` + `go 1.26` and **zero `require`s** → fully offline, no `go.sum`. Set `GOFLAGS=-mod=mod` is unnecessary; even
`GOPROXY=off` will work since nothing is fetched. This is the single biggest hermeticity win.
**Warning signs:** Compile test hits the network / fails with `dial tcp` in CI.

### Pitfall 6: A panic or `unwrap` in the lowering/SDK/subprocess path (RUST-04 / clippy `-D warnings`)
**What goes wrong:** `unwrap_used`/`expect_used`/`panic` are `deny` workspace-wide; any production-path `expect`
(e.g. on `child.stdin.take()`, `String::from_utf8`, a missing schema ref) fails the gate or, worse, panics at runtime.
**Why it happens:** Subprocess plumbing and ref resolution are full of `Option`/`Result` that tempt `unwrap`.
**How to avoid:** New typed `CoreError` variants for every failure mode (`Lowering`, `SdkGen`, `GoFmt`, `GoBuild`);
use `let Some(..) = .. else { return Err(..) }` and `?` throughout (skill ch.4). A dangling `$ref` (a `request_body`/
`response.body` whose `ref_id` is not among `schemas`) must be a typed `CoreError::Lowering`, not an `unwrap`.
**Warning signs:** clippy `unwrap_used`/`expect_used`/`panic` errors; a `.expect(` in `src/lower` or `src/sdk`.

## Code Examples

### Determinism + subprocess pattern to reuse (verified, in-repo)
```rust
// Source: crates/gnr8-core/src/analyze/helper.rs (run_goextract) — reuse this exact shape for gofmt + go build
let output = std::process::Command::new("go")
    .args(["build", "./..."])
    .current_dir(&sdk_dir)
    .output()
    .map_err(|source| CoreError::GoToolchainMissing { source })?;
if !output.status.success() {
    return Err(CoreError::GoBuild {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
}
```

### SDK bundle → single deterministic String (file-marker framing) for the snapshot (D-06)
```rust
// Source: hand-rolled per CONTEXT D-06. One stable, reviewable String the snapshot can lock; the
// SAME framing the disk writer parses to materialize files. File order is fixed + sorted.
// Recommended frame marker (greppable, unambiguous, never appears in gofmt'd Go):
//
//   // ==== gnr8:file client.go ====
//   <gofmt'd contents of client.go>
//   // ==== gnr8:file errors.go ====
//   <gofmt'd contents of errors.go>
//   // ==== gnr8:file goals.go ====   (one file per tag, tags sorted)
//   <...>
//   // ==== gnr8:file models.go ====
//   <...>
//
// write_to_dir(&bundle, dir) writes each frame to dir/<name>; the snapshot asserts the whole String.
```

### httptest smoke test (generated Go, run by `go test` in the temp dir) — illustrative shape
```go
// Source: Go stdlib net/http/httptest pattern; exercises SDK-05 "call fixture operations through tests".
func TestCreateGoalSmoke(t *testing.T) {
    srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        if r.Method != http.MethodPost || r.URL.Path != "/goal/" { t.Fatalf("bad request %s %s", r.Method, r.URL.Path) }
        w.WriteHeader(http.StatusCreated)
        _ = json.NewEncoder(w).Encode(CommandMessageWithUUID{Message: "ok", UUID: "abc"})
    }))
    defer srv.Close()
    c := NewClient(srv.URL)
    out, err := c.CreateGoal(context.Background(), CreateGoalInput{Name: "x"})
    if err != nil || out.UUID != "abc" { t.Fatalf("got %+v err %v", out, err) }
}
```
> The smoke test can either be (a) emitted as a `*_test.go` file in the bundle and run by `go test ./...`, or
> (b) hand-written into the temp dir by the Rust test harness. (a) keeps it deterministic/snapshot-able; (b) keeps
> the bundle pure-SDK. Recommend **(b)**: write a fixed smoke `*_test.go` from the Rust harness so the SDK bundle
> snapshot stays "production SDK only" and the test file isn't part of the reviewed artifact. Decide in 03-03.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `serde_yaml` for YAML in Rust | Hand-rolled writer / maintained forks (`serde_norway`) | serde_yaml archived 2024 | Don't depend on serde_yaml; control YAML emission directly. |
| OpenAPI 3.0 `nullable: true` | 3.1 drops `nullable`; optionality via `required` omission (and optionally `type: [T,"null"]`) | OpenAPI 3.1.0 (2021), JSON Schema 2020-12 | The fixture uses `required`-omission only — emit accordingly. |
| openapi-generator builder-pattern Go SDK | Idiomatic functional-options + ctx-first methods | the gnr8 thesis (D-04) | Generated Go is hand-written-quality, not verbose. |

**Deprecated/outdated:**
- `serde_yaml`: archived/unmaintained — avoid.
- `openapiv3` crate: 3.0.x only — wrong target version for a 3.1.0 document.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `tempfile` is the maintained, legitimate temp-dir crate (slopcheck not run) | Standard Stack / Audit | Low — it is well-known; planner gates with checkpoint or uses `std::env::temp_dir()` (no dep). |
| A2 | `serde_norway` is the live serde_yaml fork | Alternatives | Low — only relevant if hand-rolled YAML is rejected; recommendation is to hand-roll, so unused. |
| A3 | The `/goal` absolute path prefix is derivable deterministically in lowering (single fixture group) | Pitfall 1 / Open Questions | MEDIUM — if the prefix source is ambiguous, 03-01 needs a decision on where the base path comes from. |
| A4 | The generated SDK imports only Go stdlib (no gin/uuid) so the compile-test go.mod needs no requires | Pitfall 5 | Low — VERIFIED by reading all four `expected/sdk/*.go` (imports: net/http, time, context, fmt only). |
| A5 | `assert_snapshot!` plain-text (not yaml) is the right insta macro for both contract tests | Standard Stack | Low — VERIFIED: both existing contract tests already call `insta::assert_snapshot!`. |

**Most load-bearing assumption: A3** (path-prefix derivation) — flagged for the planner to resolve in 03-01.

## Open Questions

1. **Where does the absolute `/goal` base-path prefix come from during lowering?**
   - What we know: graph `path` is group-relative (`/`, `/list`, `/{uuid}`); `expected/openapi.yaml` keys are absolute
     (`/goal/`, `/goal/list`, `/goal/{uuid}`). 02-03 SUMMARY explicitly says "Phase-3 lowering joins it." The graph
     carries `module` and per-op `router_path` but no explicit service base path.
   - What's unclear: the deterministic *source* of `/goal` — derived from the fixture's single route group, the package
     path, or a fixed lowering parameter.
   - Recommendation: 03-01 first task should read the goextract output for the fixture (`cargo run -p gnr8 -- inspect
     routes fixtures/goalservice`) to confirm what fact (if any) carries the group prefix, then either (a) lower from a
     known constant for the single-group PoC, or (b) add the base path to the graph in a small graph-side change. Prefer
     (a) for the PoC to avoid reshaping the Phase-2 graph.

2. **Should the smoke `*_test.go` be part of the snapshot-ed SDK bundle, or written separately by the Rust harness?**
   - What we know: D-06 wants the bundle to represent "the SDK"; the smoke test exercises it.
   - Recommendation: write the smoke test separately from the Rust harness (keep the bundle = production SDK only).
     Decide in 03-03.

3. **`description` / `info.description` fidelity:** the fixture includes `info.description` and per-field `description`s.
   - What we know: the graph carries `Operation.summary`, `Param.description`, `Field.description`/`example`, but there
     is no `info.description` source in the graph.
   - Recommendation: `info.title` = graph `module` leaf (or a fixed `goalservice`), `info.version` fixed (`0.1.0`);
     omit or fix `info.description`. Reconcile the `.snap` to whatever is actually emitted (Pitfall 2). Resolve in 03-01.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `go` (toolchain) | SDK-05 `go build`; `gofmt` | ✓ | 1.26.2 (CI pins 1.26) | none — already a hard project dep; missing → `CoreError::GoToolchainMissing` (clean exit, no panic) |
| `gofmt` | SDK formatting | ✓ | ships with `go` (1.26.2) | If absent, emit hand-formatted Go (riskier) — but it ships with `go`, so N/A |
| `cargo` / `rustc` | build/test | ✓ | 1.96.0 (MSRV 1.85) | none |
| `cargo-insta` CLI | snapshot review (convenience) | ✗ | — | Manual flow: run test → review `.snap.new` → rename to `.snap`, strip `assertion_line` (the 02-03 process) |
| network (module proxy) | NOT needed (SDK is stdlib-only) | n/a | — | go.mod has zero requires → `GOPROXY=off` safe |

**Missing dependencies with no fallback:** none (the only hard dep, the Go toolchain, is present and CI-provisioned).
**Missing dependencies with fallback:** `cargo-insta` — use the documented manual accept flow (already proven in 02-03).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `insta` 1.48 (snapshots) + Go `testing`/`httptest` (inside the generated SDK compile test) |
| Config file | none — insta config is in `Cargo.toml` dev-deps; `INSTA_UPDATE=no` enforced via `CI=true` |
| Quick run command | `cargo test -p gnr8-core --lib` (unit tests for lower/sdk emitters) |
| Full suite command | `cargo test -p gnr8-core` then `cd fixtures/goalservice && go build ./...` (mirrors `make check`) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| OAPI-01 | Valid OpenAPI doc emitted for fixture | snapshot (integration) | `cargo test -p gnr8-core --test snapshot_openapi` | ✅ (flip GREEN) |
| OAPI-02 | Doc has info/paths/ops/params/bodies/responses/schemas | snapshot (covered by OAPI-01 content) | `cargo test -p gnr8-core --test snapshot_openapi` | ✅ |
| OAPI-02 | YAML writer ordering/types are correct | unit | `cargo test -p gnr8-core --lib lower::` | ❌ Wave 0 (`src/lower/yaml.rs` tests) |
| OAPI-03 | Lowering surfaces compatibility diagnostics | unit + reuse snapshot_diagnostics | `cargo test -p gnr8-core --lib lower:: && cargo test -p gnr8-core --test snapshot_diagnostics` | ✅ (diagnostics) / ❌ Wave 0 (lowering-surface test) |
| SDK-01 | Client + functional options compile/behave | go compile + smoke | `cargo test -p gnr8-core --test sdk_compile` | ❌ Wave 0 (new `tests/sdk_compile.rs`) |
| SDK-02 | Typed tag-grouped ops, ctx first | go compile + smoke | `cargo test -p gnr8-core --test sdk_compile` | ❌ Wave 0 |
| SDK-03 | Generated model structs | snapshot + go build | `cargo test -p gnr8-core --test snapshot_sdk` | ✅ (flip GREEN) |
| SDK-04 | JSON (de)code + typed APIError | go smoke (asserts decode + 4xx→APIError) | `cargo test -p gnr8-core --test sdk_compile` | ❌ Wave 0 |
| SDK-05 | Generated SDK compiles + is exercised | go build + httptest | `cargo test -p gnr8-core --test sdk_compile` | ❌ Wave 0 |
| (cross) | Deterministic output (idempotent gen) | unit | `cargo test -p gnr8-core` (extend `tests/determinism.rs` or add two-run asserts) | ✅ extend |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core --lib` (fast emitter unit tests) + `cargo clippy --all-targets --all-features --locked -- -D warnings`
- **Per wave merge:** `cargo test -p gnr8-core` (includes the four contract snapshots once green) + `cd fixtures/goalservice && go build ./...`
- **Phase gate:** full suite green + `tests/sdk_compile.rs` green (go build + smoke) before `/gsd:verify-work`; promote `snapshot_openapi` + `snapshot_sdk` (+ the new `sdk_compile`) into the blocking CI `gates` job and retire the non-blocking `contract` job (D-07).

### Wave 0 Gaps
- [ ] `crates/gnr8-core/src/lower/yaml.rs` unit tests — covers OAPI-02 (writer ordering, `$ref` quoting, `additionalProperties`, no `nullable`)
- [ ] `crates/gnr8-core/src/lower/mod.rs` unit tests — covers OAPI-01/OAPI-03 (path-prefix join, dangling-ref → typed error, diagnostic surfacing)
- [ ] `crates/gnr8-core/src/sdk/emit.rs` unit tests — covers SDK-01..04 emitter fragments (model field rendering, optional→pointer, enum const block, ctx-first signature)
- [ ] `crates/gnr8-core/tests/sdk_compile.rs` — NEW integration test, covers SDK-05 (temp dir + `go.mod` + `go build` + httptest smoke); also asserts SDK-04 typed-error path (4xx → `APIError`)
- [ ] Extend `crates/gnr8-core/tests/determinism.rs` (or add module-local two-run asserts) — covers idempotent generation for both artifacts
- [ ] Framework install: none — `insta`, `go`, `gofmt` all present; optional `tempfile` dev-dep (or `std::env::temp_dir()`)

## Security Domain

> `security_enforcement: true`, `security_asvs_level: 1`, `security_block_on: high`. This phase generates code +
> spawns the Go toolchain; the relevant surface is subprocess hygiene and untrusted-input handling, not auth.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | The SDK *emits* a `WithAPIKey`/`X-API-Key` option (generated code), but gnr8 itself authenticates nothing. |
| V3 Session Management | no | No sessions in the generator. |
| V4 Access Control | no | No access control surface. |
| V5 Input Validation | yes | The graph is the input; lowering must reject malformed facts (dangling `$ref`, unknown `SchemaType.kind`) with a typed error, not a panic. The facts mirror already uses `#[serde(deny_unknown_fields)]`. |
| V6 Cryptography | no | No crypto in the generator (the SDK's API key is a header value, plaintext by design — same as the fixture). |
| V12/V5 (Command injection / subprocess) | yes | `gofmt`/`go build` must be invoked with **discrete args**, never a shell string (the existing `run_goextract` pattern: T-02-01). Temp paths are program-controlled, not user input. |

### Known Threat Patterns for {Rust generator + Go subprocess}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Command injection via shell interpolation in `gofmt`/`go build` | Tampering / EoP | Discrete `Command::args([...])`, no `sh -c`; reuse helper.rs pattern (T-02-01). `[VERIFIED: helper.rs comment]` |
| Malformed/forward-incompatible graph facts panicking lowering | DoS | Typed `CoreError` on every failure path (dangling ref, unknown kind); `deny_unknown_fields` already on the facts mirror (T-02-05). |
| Temp-dir path traversal / collision in the compile test | Tampering | Use `tempfile` (unique, auto-cleaned) OR a uniquely-named subdir under `std::env::temp_dir()`; write only program-generated filenames. |
| Generated SDK leaking the API key into logs | Info disclosure | Out of scope for the PoC SDK shape (matches fixture); note for Phase 5 hardening if surfaced. |
| Non-hermetic `go build` reaching the network (supply-chain exposure) | Tampering | stdlib-only `go.mod` (zero requires); `GOPROXY=off`-safe (Pitfall 5). |

**security_block_on: high — no high-severity issue identified in this phase's surface.** The two real controls are
(1) discrete-arg subprocess invocation (already the house pattern) and (2) typed-error-not-panic on malformed graph
input (already the house pattern). Both carry forward from Phase 2 with no new mechanism.

## Sources

### Primary (HIGH confidence)
- In-repo `fixtures/goalservice/expected/openapi.yaml` — the exact OpenAPI 3.1.0 target shape (paths, schemas, security, additionalProperties, required-omission-for-optional).
- In-repo `fixtures/goalservice/expected/sdk/{client,goals,models,errors}.go` — the exact Go SDK target (imports confirm stdlib-only).
- In-repo `crates/gnr8-core/src/graph/mod.rs` + `analyze/facts.rs` — the consumed `ApiGraph`/`SchemaType` field set + determinism contract.
- In-repo `crates/gnr8-core/src/analyze/helper.rs` — the verified subprocess pattern to reuse for `gofmt`/`go build`.
- In-repo `crates/gnr8-core/tests/{snapshot_openapi,snapshot_sdk}.rs` — confirm `assert_snapshot!` (plain text), red-by-design wiring.
- In-repo `.planning/phases/02-go-analysis-and-api-graph/02-03-SUMMARY.md` — graph shape, stable ids, the manual `.snap` accept flow, the deferred path-prefix decision, the gates/contract CI split.
- In-repo `.planning/research/TARGET-API.md` §4 (type map) + §5 (pitfalls→diagnostics).
- `thoughts/skills/rust-best-practices/` ch.4 (typed errors, no prod unwrap) + ch.5 (snapshot/test discipline).
- Local environment probes — `go version go1.26.2`, `gofmt` on PATH, `serde_yaml` NOT in `Cargo.lock`, `cargo 1.96.0`.

### Secondary (MEDIUM confidence)
- OpenAPI 3.1.0 spec semantics (no `nullable`, JSON-Schema-2020-12 alignment, `$ref` form) — cross-checked against the fixture's actual content. `[CITED: spec.openapis.org/oas/v3.1.0]`

### Tertiary (LOW confidence)
- `serde_yaml` archived / `serde_norway` fork status — training knowledge, not re-verified this session (does not affect the recommendation to hand-roll). `[ASSUMED]`
- `tempfile` legitimacy — well-known crate, slopcheck not run this session. `[ASSUMED]`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — zero new runtime crates; everything (serde/insta/thiserror/Command/go/gofmt) is already in-repo and probed.
- Architecture: HIGH — both seam signatures, the graph shape, the target artifacts, and the subprocess pattern are read directly from the repo.
- Pitfalls: HIGH — the load-bearing ones (group-relative path, reference-vs-snapshot, hermetic go.mod, determinism, no-panic) are grounded in the actual code/fixtures/SUMMARY, not inferred.
- Open question A3 (path prefix) is the one genuine MEDIUM — flagged for 03-01 to resolve before locking the OpenAPI snapshot.

**Research date:** 2026-06-24
**Valid until:** 2026-07-24 (stable; in-repo facts won't drift, Go 1.26/Rust 1.96 are pinned)

## Recommended Plan Split (3 plans)

- **03-01 — OpenAPI lowering + validation snapshot (OAPI-01, OAPI-02, OAPI-03):** typed OpenAPI 3.1 structs
  (`lower/model.rs`), deterministic key-ordered YAML writer (`lower/yaml.rs`), `to_openapi` graph→doc mapping incl.
  the `/goal` base-path join (resolve Open Q1 first), diagnostic surfacing (reuse Phase-2 set), unit tests, flip
  `snapshot_openapi` GREEN (author-from-real-output → review-vs-fixture → accept).
- **03-02 — Go SDK models/client/operations/errors (SDK-01..04):** `format!` emitters (`sdk/emit.rs`) for model
  structs / functional-options Client / tag-grouped ctx-first ops / typed `APIError`, `gofmt` normalization
  (`sdk/gofmt.rs`), `SdkBundle` + file-marker `to_string()` (`sdk/bundle.rs`), `write_to_dir`, unit tests, flip
  `snapshot_sdk` GREEN.
- **03-03 — Compile/smoke tests + e2e artifact snapshots + CI promotion (SDK-05, D-07):** `tests/sdk_compile.rs`
  (temp dir + stdlib-only `go.mod` + `go build ./...` + httptest smoke asserting method/path/body + decode + 4xx→
  `APIError`), new `CoreError::{GoBuild, GoFmt, Lowering, SdkGen}` variants, determinism assertion for both artifacts,
  promote `snapshot_openapi`/`snapshot_sdk`/`sdk_compile` into the blocking CI `gates` job and remove the non-blocking
  `contract` job + update `Makefile`.

## RESEARCH COMPLETE

**Phase:** 3 - OpenAPI And Go SDK Generation
**Confidence:** HIGH

### Key Findings
- **Hand-roll, don't pull a crate.** No OpenAPI/YAML crate fits OpenAPI 3.1.0 cleanly (`openapiv3` is 3.0; `utoipa` is annotation-driven, wrong direction; `serde_yaml` is deprecated and not in the lockfile). A ~150-line key-ordered YAML writer over typed serde structs gives byte-exact control + determinism with zero new runtime deps — matching D-01/D-05 and the PROJECT "no heavy crates" constraint.
- **The compile test is fully hermetic.** All four `expected/sdk/*.go` import only Go stdlib, so the SDK-05 `go.mod` needs zero `require`s → `go build` never touches the network (`GOPROXY=off`-safe). This removes the biggest CI-flakiness risk.
- **Two load-bearing reconciliation facts.** (1) The graph `path` is group-relative; lowering must join the absolute `/goal` prefix (deferred from 02-03) — the one open question (A3). (2) `expected/openapi.yaml` is a *reference target*, not the literal `.snap`; author the snapshot from real output and review for semantic equivalence (the proven 02-03 manual flow, since `cargo-insta` is absent).
- **Reuse the house patterns.** `gofmt`/`go build` go through the exact discrete-arg `Command` pattern from `helper.rs` (no shell, T-02-01); every failure maps to a new typed `CoreError` variant (no prod `unwrap`/`expect`, RUST-04); ordering stays `Vec<(K,V)>` not `HashMap` (the graph's determinism rule).
- **3-plan split is clean:** 03-01 OpenAPI, 03-02 Go SDK, 03-03 compile/smoke + e2e snapshots + CI promotion (retire the non-blocking `contract` job, promote all four contract tests to blocking per D-07).

### File Created
`/Users/emilwareus/conductor/workspaces/gnr8/tripoli-v7/.planning/phases/03-openapi-and-go-sdk-generation/03-RESEARCH.md`

### Confidence Assessment
| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | Zero new runtime crates; all tooling probed in-repo (serde/insta/thiserror/Command/go 1.26.2/gofmt). |
| Architecture | HIGH | Seam signatures, graph shape, target artifacts, and subprocess pattern all read directly from the repo. |
| Pitfalls | HIGH | Grounded in actual code/fixtures/SUMMARY (group-relative path, reference-vs-snapshot, hermetic go.mod, determinism, no-panic). |

### Open Questions
- **A3 (MEDIUM):** Where the absolute `/goal` base-path prefix comes from deterministically during lowering — the graph stores group-relative paths and deferred this to Phase 3 but carries no explicit service base path. 03-01's first task must resolve it (recommend lowering from a known constant for the single-group PoC rather than reshaping the Phase-2 graph).
- Minor: smoke `*_test.go` placement (in-bundle vs harness-written) and `info.description` fidelity — both resolvable at planning time.

### Ready for Planning
Research complete. The planner can now create the three PLAN.md files (03-01 / 03-02 / 03-03).
