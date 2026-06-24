---
phase: 02-go-analysis-and-api-graph
plan: 01
subsystem: api
tags: [go, go-packages, go-types, go-ast, rust, serde, subprocess, json-contract, thiserror]

# Dependency graph
requires:
  - phase: 01-foundation-and-fixtures
    provides: "gnr8-core CoreError + analyze::build_graph seam; goalservice Gin fixture (DTOs + expected/); red-by-design contract tests; fmt/clippy/test + go-fixture CI gates"
provides:
  - "goextract/ Go sidecar module (own go.mod, go 1.26, golang.org/x/tools v0.46.0) loading targets with go/packages LoadAllSyntax+NeedModule"
  - "DTO struct/field/tag extraction: json field names, binding:required, pointer/omitempty optionality, embedded flattening, named-string enum const sets, well-known uuid.UUID/time.Time, source spans"
  - "float64-narrowing (x3) + free-form-map (x1) WARN diagnostics with field identity + file:line"
  - "Deterministic sorted JSON facts document on stdout (sort-before-marshal; empty slices as [])"
  - "Rust analyze::facts serde mirror (GoFacts + children) with deny_unknown_fields on every struct"
  - "Rust analyze::helper subprocess driver (run_goextract) mapping every failure to typed CoreError"
  - "CoreError::{GoToolchainMissing, HelperExit, FactsParse} typed variants (no-panic library boundary)"
  - "Makefile goextract-build target + blocking CI goextract job (CGO_ENABLED=0, go 1.26)"
affects: [02-02-routes-handlers, 02-03-apigraph-inspect, 03-openapi-sdk-generation]

# Tech tracking
tech-stack:
  added:
    - "golang.org/x/tools/go/packages v0.46.0 (Go helper, official Go team)"
    - "serde_json (gnr8-core dependency, for facts deserialize + FactsParse source)"
  patterns:
    - "Rust<->Go JSON facts contract: Go sorts+marshals, Rust deserializes with deny_unknown_fields"
    - "Sort-before-marshal determinism (GRAPH-02): never range a Go map into output order"
    - "Subprocess failures -> typed CoreError via ?; binary name parameterized for toolchain-missing tests"
    - "Forward-contract surface (RouteFact/helper) lands a wave before its consumer; scoped #![allow(dead_code)] until 02-03 wires it"

key-files:
  created:
    - "goextract/go.mod, goextract/go.sum"
    - "goextract/main.go"
    - "goextract/internal/load/load.go"
    - "goextract/internal/types/extract.go"
    - "goextract/internal/types/extract_test.go"
    - "goextract/internal/diag/diag.go"
    - "goextract/internal/facts/facts.go"
    - "goextract/internal/facts/facts_test.go"
    - "crates/gnr8-core/src/analyze/facts.rs"
    - "crates/gnr8-core/src/analyze/helper.rs"
  modified:
    - "crates/gnr8-core/src/error.rs"
    - "crates/gnr8-core/src/analyze/mod.rs"
    - "crates/gnr8-core/src/lib.rs"
    - "crates/gnr8-core/Cargo.toml"
    - "Cargo.lock"
    - "Makefile"
    - ".github/workflows/ci.yml"

key-decisions:
  - "Extraction scope = named types declared in the target MODULE whose struct (or embedded struct) carries a json: tag; excludes server/wiring structs (HttpServer) and the fixture's expected/ acceptance-snapshot packages"
  - "Schema IDs are module-relative package-qualified names (e.g. internal/common/dto.CreateGoalInput); enum/struct named-type fields lower to a ref to that id"
  - "Diagnostic messages keep machine-stable rule + field identity + declared type (e.g. *float64, map[string]any); canonical rendered wording is reconciled in 02-03"
  - "goextract dir resolved from Rust as concat!(env!(CARGO_MANIFEST_DIR), /../../goextract); go invoked as discrete Command args (no shell, T-02-01)"
  - "Routes key is always an empty non-nil slice this plan (02-02 fills it) so the JSON schema is stable"

patterns-established:
  - "JSON facts schema field names (snake_case) are the Rust<->Go contract; Go json tags == Rust serde fields exactly"
  - "Every facts DTO uses #[serde(deny_unknown_fields)]; SourceSpan also derives Serialize for the 02-03 graph"
  - "Test binary-name parameterization (run_goextract_with) to test the toolchain-missing path without mutating PATH"

requirements-completed: [GO-01, GO-02, GO-03, GO-06]

# Metrics
duration: 14min
completed: 2026-06-24
---

# Phase 2 Plan 01: goextract Go Sidecar + Rust Facts Contract Summary

**A go/packages-based `goextract` helper that emits a deterministic sorted JSON facts document (8 DTO schemas + TargetDirection enum + float64/free-form-map diagnostics) for the goalservice fixture, plus the Rust serde mirror, a typed-error subprocess driver, and the Makefile/CI gate.**

## Performance

- **Duration:** 14 min
- **Started:** 2026-06-24T17:02:15Z
- **Completed:** 2026-06-24T17:16:21Z
- **Tasks:** 3
- **Files modified:** 18 (10 created, 8 modified)

## Accomplishments

- `goextract/` Go module loads a target with `go/packages` in `LoadAllSyntax|NeedModule` mode and extracts every reachable DTO struct: json field names, `binding:"required"`, pointer/`omitempty` optionality, embedded-struct flattening (`CommandMessageWithUUID` promotes `message`), named-string enum const sets (`TargetDirection` -> sorted `["gte","lte"]`), well-known `uuid.UUID` -> string(uuid) / `time.Time` -> string(date-time), arrays/refs, and a source span on every node.
- Emits the float64-narrowing (CreateGoalInput/UpdateGoalInput/GoalResponse `.TargetValue` `*float64`) x3 and free-form-map (`GoalResponse.Metadata` `map[string]any`) x1 WARN diagnostics with field identity + `file:line`.
- One stable, sorted JSON facts document on stdout: schemas by id, fields by json name, enum values + diagnostics sorted; empty slices serialize as `[]`. Two `go run` invocations on the unchanged fixture are byte-identical (GRAPH-02).
- Rust `analyze::facts` serde mirror (`GoFacts` + `RouteFact`/`ParamFact`/`ResponseFact`/`SchemaFact`/`FieldFact`/`SchemaType`/`TypeRef`/`DiagnosticFact`/`SourceSpan`), all with `#[serde(deny_unknown_fields)]`, round-trips real helper-shaped JSON and rejects unknown fields.
- Rust `analyze::helper::run_goextract` runs `go run . <dir>` from the `goextract` dir, mapping spawn failure -> `GoToolchainMissing`, non-zero exit -> `HelperExit{code,stderr}`, bad JSON -> `FactsParse` — all via `?`, zero prod unwrap/expect/panic.
- `CoreError` gained the three typed variants with no-panic Display tests; `make goextract-build` + a blocking CI `goextract` job (CGO_ENABLED=0, go 1.26) gate the helper.

## Task Commits

Each task was committed atomically:

1. **Task 1: Scaffold goextract module + loader + struct/type/enum extractor** - `5a0a30c` (feat)
2. **Task 2: Extended CoreError + Rust serde facts DTOs + subprocess driver** - `8fdd0d4` (feat)
3. **Task 3: Wire goextract into Makefile + CI; determinism test** - `1a05058` (chore)

_Task 1 and Task 2 are TDD tasks; tests and implementation landed in one commit each (test + impl co-developed against the live fixture / sample JSON)._

## Files Created/Modified

- `goextract/go.mod` / `go.sum` - standalone module `github.com/gnr8/goextract`, go 1.26, x/tools v0.46.0 (committed for reproducible CI)
- `goextract/main.go` - CLI: `goextract <target-dir>` -> sorted JSON on stdout; errors on stderr + exit 1
- `goextract/internal/load/load.go` - `packages.Load` wrapper (LoadAllSyntax+NeedModule); per-package errors surfaced as `LoadError` (GO-06)
- `goextract/internal/types/extract.go` - struct/field/tag extraction, type mapping (well-known/enum/ref/array/map), embedded flattening, scoping
- `goextract/internal/diag/diag.go` - WARN accumulator: `Floatf`, `FreeFormMap`, `Warn` carrying rule + identity + file:line
- `goextract/internal/facts/facts.go` - JSON DTOs + `Marshal` (sort-before-encode, empty slices as `[]`)
- `goextract/internal/{types,facts}/*_test.go` - extraction behaviors + determinism/sort/empty-slice tests
- `crates/gnr8-core/src/error.rs` - `GoToolchainMissing`/`HelperExit`/`FactsParse` + Display tests
- `crates/gnr8-core/src/analyze/facts.rs` - serde mirror (deny_unknown_fields) + round-trip tests
- `crates/gnr8-core/src/analyze/helper.rs` - subprocess driver + toolchain-missing/path tests
- `crates/gnr8-core/src/analyze/mod.rs` - declare `pub(crate) mod facts/helper`; `build_graph` stays NotYetImplemented
- `crates/gnr8-core/src/lib.rs` - exhaustive `NotYetImplemented` match via let-else
- `crates/gnr8-core/Cargo.toml` / `Cargo.lock` - add `serde_json` dependency
- `Makefile` - `goextract-build` target + `.PHONY` + `check` dep
- `.github/workflows/ci.yml` - blocking `goextract` job (CGO_ENABLED=0, go 1.26)

## JSON Facts Schema (the 02-02 / 02-03 contract)

Top level `GoFacts`: `module` (string), `routes` (array, empty this plan), `schemas` (array), `diagnostics` (array).
- `SchemaFact`: `id`, `name`, `kind` ("object"|"enum"), `fields` (array), `enum_values` (array), `span`.
- `FieldFact`: `json_name`, `required`, `optional`, `schema`, `description`, `example`.
- `SchemaType`: `kind` (string|integer|number|boolean|array|object|ref), `format`, `items`, `ref_id`, `additional_properties`.
- `DiagnosticFact`: `severity`, `message`, `file`, `line`. `SourceSpan`: `file`, `start_line`, `end_line`.
- `RouteFact`/`ParamFact`/`ResponseFact`/`TypeRef` are defined (Rust + Go) but unpopulated until 02-02.

Schema id format: module-relative package path + `.` + type name (e.g. `internal/common/dto.CreateGoalInput`). Named struct/enum field types lower to `{kind:"ref", ref_id:<schema id>}`.

## Decisions Made

- **Extraction scope:** a named type is a DTO schema iff it is declared in the target module AND its struct (or an embedded struct) carries a `json:` tag. This naturally captures the 8 `dto.*` structs, treats `TargetDirection` (named string + const set) as an enum, and excludes the `HttpServer` wiring struct.
- **Excluded `expected/` packages:** the fixture's `expected/sdk` package is a hand-authored Phase-3 SDK acceptance snapshot that re-declares DTO names; analysis skips any package with an `expected/` path segment so it does not double the schema set. (See Deviation 1.)
- **Diagnostic wording:** the helper emits a stable rule + field identity + *declared* Go type (e.g. `(*float64)`, `(map[string]any)`); the canonical rendered text that must reconcile with `expected/diagnostics.txt` is finalized on the Rust side in 02-03.
- **`NeedModule` added to the load mode:** required so `pkg.Module.Main` identifies the target module path (used for scoping + schema-id derivation). The plan named `LoadAllSyntax`; `NeedModule` is an additive superset. (See Deviation 2.)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Excluded the fixture's `expected/sdk` package from extraction**
- **Found during:** Task 1 (extractor verification)
- **Issue:** `go/packages` loads `fixtures/goalservice/expected/sdk`, a hand-authored Phase-3 SDK snapshot that re-declares DTO type names (CreateGoalInput, GoalResponse, ...) plus extra SDK types (Client, APIError, ListGoalsParams). Scanning it produced far more than the expected 8 object schemas and duplicated ids.
- **Fix:** `isTargetPackage` skips any package whose module-relative path contains an `expected/` segment — generated/expected output is never analyzer input.
- **Files modified:** `goextract/internal/types/extract.go`
- **Verification:** `go run . ../fixtures/goalservice` yields exactly 9 schemas (8 object + 1 enum); `TestExtractObjectAndEnumCounts` asserts the exact set.
- **Committed in:** `5a0a30c` (Task 1 commit)

**2. [Rule 3 - Blocking] Added `packages.NeedModule` to the load mode**
- **Found during:** Task 1 (0 schemas extracted on first run)
- **Issue:** With `LoadAllSyntax` alone, `pkg.Module` was nil, so the target-module path could not be resolved and every package failed the scoping check -> 0 schemas.
- **Fix:** Added `packages.NeedModule` to the load bitset (additive to `LoadAllSyntax`).
- **Files modified:** `goextract/internal/load/load.go`
- **Verification:** loader now reports `Module.Main` with path `github.com/gnr8/gnr8-fixtures/goalservice`; extraction succeeds.
- **Committed in:** `5a0a30c` (Task 1 commit)

**3. [Rule 2 - Missing Critical] Render the DECLARED field type in float64 diagnostics**
- **Found during:** Task 1 (diagnostic-message identity)
- **Issue:** The float64 diagnostic initially hardcoded `*float64` for the type; a non-pointer `float64` field would have been mislabeled, and the type rendering must be correct for the 02-03 wording reconciliation.
- **Fix:** Threaded a `mapCtx` carrying the field's as-written type string through the recursive `mapType` walk so float64 + free-form-map diagnostics render the declared type (`*float64`, `map[string]any`) and the field's own file:line.
- **Files modified:** `goextract/internal/types/extract.go`
- **Verification:** emitted messages show `(*float64)` and `(map[string]any)`, matching the identity in `expected/diagnostics.txt`; `TestFloat64Diagnostics`/`TestGoalResponseWellKnownAndFreeFormMap` pass.
- **Committed in:** `5a0a30c` (Task 1 commit)

**4. [Rule 3 - Blocking] Scoped `#![allow(dead_code)]` on the forward-contract Rust modules**
- **Found during:** Task 2 (clippy `-D warnings`)
- **Issue:** `analyze::facts` DTOs and `analyze::helper::run_goextract` have no production caller until 02-03 wires `build_graph`, so `dead_code` warnings would fail the `-D warnings` clippy gate.
- **Fix:** Added a module-level `#![allow(dead_code)]` with an explanatory comment to `facts.rs` and `helper.rs` (the surface is exercised by unit tests now and consumed by 02-03). This is a deliberate forward-contract, not unused code.
- **Files modified:** `crates/gnr8-core/src/analyze/facts.rs`, `crates/gnr8-core/src/analyze/helper.rs`
- **Verification:** `cargo clippy --all-targets --all-features --locked -- -D warnings` is clean.
- **Committed in:** `8fdd0d4` (Task 2 commit)

---

**Total deviations:** 4 auto-fixed (3 blocking, 1 missing-critical)
**Impact on plan:** All four were necessary for correctness or to satisfy the existing quality gates; the schema set, diagnostics, and determinism all match the plan's behavior exactly. No scope creep — routes/handlers (02-02) and the ApiGraph/inspect (02-03) were not touched and the contract tests remain red-by-design.

## Authentication Gates

None - no external services or credentials involved.

## Known Stubs (intentional, forward contract)

- `GoFacts.routes` is always an empty `[]` and `RouteFact`/`ParamFact`/`ResponseFact`/`TypeRef` are defined but unpopulated. This is intentional: 02-02 implements Gin route/handler recognition and fills `routes`. The empty-but-present key keeps the JSON schema stable. Not goal-blocking for 02-01 (the plan's goal is DTO/schema extraction + the contract surface).

## Issues Encountered

None - the only friction was the `expected/sdk` package and the nil `Module` (both handled as deviations above).

## User Setup Required

None - no external service configuration required. Developers need the Go toolchain (`go 1.26`) installed; a missing toolchain now surfaces as `CoreError::GoToolchainMissing`, not a panic.

## Next Phase Readiness

- **Ready for 02-02:** the JSON facts schema, schema-id format, `goextract`-dir resolution, the three `CoreError` variants, and the determinism discipline are all locked. 02-02 extends `goextract` with route/handler recognition (filling `routes`) and the Rust mirror is already shaped for it.
- **Ready for 02-03:** `analyze::facts::GoFacts` + `analyze::helper::run_goextract` are the consumable contract; 02-03 implements `build_graph` (flipping `snapshot_graph` + `snapshot_diagnostics` green) and reconciles the canonical diagnostics wording.
- `snapshot_graph` and `snapshot_diagnostics` remain RED-by-design (build_graph still returns NotYetImplemented), as the plan requires.

---
*Phase: 02-go-analysis-and-api-graph*
*Completed: 2026-06-24*

## Self-Check: PASSED

- All 11 created files verified present on disk (`[ -f ]`).
- All 3 task commits verified in `git log` (`5a0a30c`, `8fdd0d4`, `1a05058`).
- Plan `<verification>` re-run green: `goextract` build/vet/test, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test -p gnr8-core --lib` (10 pass), `make goextract-build`, two `go run` invocations byte-identical.
- `snapshot_graph` + `snapshot_diagnostics` confirmed still RED-by-design (expected).
