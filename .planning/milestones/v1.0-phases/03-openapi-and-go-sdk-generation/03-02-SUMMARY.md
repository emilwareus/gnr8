---
phase: 03-openapi-and-go-sdk-generation
plan: 02
subsystem: api
tags: [rust, go-sdk, codegen, gofmt, subprocess, insta, determinism, format-macro]

# Dependency graph
requires:
  - phase: 02-go-analysis-and-api-graph
    provides: "byte-stable ApiGraph (operations/schemas/params/responses, all pre-sorted) that the SDK emitters consume directly (no re-analysis)"
  - phase: 03-openapi-and-go-sdk-generation
    provides: "03-01 added the SdkGen/GoFmt CoreError variants this plan consumes (no error.rs edit) and the manual insta-accept flow reused here"
provides:
  - "sdk::generate(&ApiGraph) -> Result<String, CoreError> — a deterministic, gofmt-clean multi-file Go SDK bundle String (no longer NotYetImplemented)"
  - "format!-based Go emitters (sdk/emit.rs): models (json tags, optional pointers/omitempty, enum newtypes, uuid/time/float32 mapping, computed imports), functional-options Client, tag-grouped ctx-first ops, typed APIError"
  - "gofmt subprocess driver (sdk/gofmt.rs): discrete-arg Command, stdin pipe, typed CoreError::GoFmt on non-zero exit, no .expect on stdin.take()"
  - "SdkBundle + file-marker framing (sdk/bundle.rs): stable `// ==== gnr8:file <name> ====` frames, round-trippable, byte-identical across runs"
  - "write_to_dir(&SdkBundle, &Path) — materializes the same framing for 03-03's compile test"
  - "snapshot_sdk flipped GREEN with a reviewed .snap reconciled with expected/sdk/*.go"
affects: [03-03-compile-smoke, 04-gnr8-workspace-lifecycle]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "format!/writeln!-based Go emission (no template engine, D-05) with per-file import sets computed from emitted content (Pitfall 3 — gofmt does not drop unused imports)"
    - "Real gofmt binary owns Go formatting; Rust never hand-aligns Go (discrete-arg subprocess mirroring analyze/helper.rs)"
    - "Multi-file SDK serialized as one file-marker-framed String (D-06) so a single insta snapshot locks the whole SDK; same framing parsed by write_to_dir"
    - "Vec<(K,V)> everywhere (tags grouped + sorted, ops in graph order) — never a HashMap — for byte-stable output; two-run determinism unit-tested"
    - "Typed CoreError on every un-representable fact (dangling $ref, unknown kind); no prod unwrap/expect/panic, including let-Some on child.stdin.take()"

key-files:
  created:
    - crates/gnr8-core/src/sdk/emit.rs
    - crates/gnr8-core/src/sdk/gofmt.rs
    - crates/gnr8-core/src/sdk/bundle.rs
    - crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap
  modified:
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/tests/snapshot_sdk.rs

key-decisions:
  - "Tag grouping: an operation's tag is its first (sorted) tag; an untagged op inherits the lexically-first tag in the graph (else the package name). The fixture's two tagged (Goals) + two untagged ops thus land in one goals.go, matching expected/sdk/goals.go."
  - "Optional struct fields: pointer-wrap only Go value types (number/bool/int/time.Time) when optional; strings/slices/maps are already nilable so they stay plain with omitempty (matches expected/sdk/models.go). Optional QUERY params are always *T so the SDK can distinguish unset from empty."
  - "Path-param Go arg names are lower_camel with the first word fully lower-cased (uuid, not the initialism-aware uUID) so the generated code is idiomatic and compiles."
  - "models.go field optionality follows the GRAPH (source of truth), not expected/sdk's illustrative comments: a field that is required=false AND optional=false (e.g. CreateGoalInput.Description) emits no omitempty (Pitfall 2 — reconcile, not byte-copy)."
  - "SdkBundle::to_string() is implemented via std::fmt::Display so the D-06 to_string() contract comes from the blanket ToString impl (avoids the should_implement_trait lint on an inherent to_string)."

patterns-established:
  - "Go SDK emission = deterministic Rust string transform; the hard work (formatting, eventually compiling) is delegated to the Go toolchain (gofmt now, go build in 03-03)"
  - "Per-task scoped #[allow(dead_code)] for cross-task wiring, removed in the task that consumes the code (no genuinely-dead code masked — clippy stays green after removal)"

requirements-completed: [SDK-01, SDK-02, SDK-03, SDK-04]

# Metrics
duration: 14min
completed: 2026-06-24
---

# Phase 3 Plan 2: Go SDK Codegen + Snapshot Summary

**`sdk::generate` emits a deterministic, gofmt-clean Go SDK from the Phase-2 ApiGraph — a functional-options `Client`, tag-grouped `context.Context`-first operation methods with path/query/body handling and a typed `APIError`, and model structs with json tags/optional pointers/enum newtypes/uuid·time·float32 mapping — serialized as one file-marker-framed String that flips `snapshot_sdk` GREEN and `go build`s clean.**

## Performance

- **Duration:** 14 min
- **Started:** 2026-06-24T18:58:01Z
- **Completed:** 2026-06-24T19:12:02Z
- **Tasks:** 3
- **Files modified:** 6 (4 created, 2 modified)

## Accomplishments
- `sdk::generate(&ApiGraph) -> Result<String, CoreError>` implemented as a pure graph→Go transform: emit each file with `format!`/`writeln!` (no template engine, D-05), pipe each through the real `gofmt`, and frame them into a single deterministic `SdkBundle` String (D-06).
- Four idiomatic Go files reconciled with `expected/sdk/`: `client.go` (functional-options `Client` + `NewClient`/`WithHTTPClient`/`WithAPIKey`, SDK-01), `goals.go` (tag-grouped `CreateGoal`/`ListGoals`/`DeleteGoal`/`UpdateGoal`, ctx-first, with body marshal + query encode + path interpolation + `X-API-Key` + 2xx decode / non-2xx `APIError`, SDK-02), `models.go` (request/response structs + `TargetDirection` enum newtype, json tags, optional pointers, uuid→string/date-time→time.Time/number→*float32/[]T/map[string]any, SDK-03), `errors.go` (typed `APIError` with status + decoded body + `Error()`/`IsNotFound()`, SDK-04).
- Per-file import sets are **computed** from emitted content (Pitfall 3): `time` only when a `time.Time` field exists; ops need bytes/context/encoding-json/fmt/net-http; errors need fmt — so `go build` will not fail on unused imports.
- `gofmt` subprocess driver normalizes every file (discrete-arg `Command`, source on stdin, `let Some(..) else` on `stdin.take()` — no `.expect`); a non-zero exit maps to `CoreError::GoFmt`, a missing binary to `CoreError::GoToolchainMissing` — never a panic.
- `snapshot_sdk` flipped red→GREEN with a reviewed `.snap` authored from real output; the generated SDK was independently verified to `go build` AND `go vet` clean in a hermetic stdlib-only temp module (de-risks 03-03's compile gate).
- Determinism unit-tested (two `generate` runs byte-identical); the bundle framing round-trips and is byte-identical across runs. No regression: `snapshot_graph`/`snapshot_diagnostics`/`snapshot_openapi`/`determinism` stay GREEN.

## Task Commits

Each task was committed atomically (each developed RED→GREEN, squashed to one feat commit):

1. **Task 1: Go SDK emitters (sdk/emit.rs) — models / client / ops / errors + computed imports** - `002ea29` (feat)
2. **Task 2: gofmt subprocess (sdk/gofmt.rs) + SdkBundle file-marker framing (sdk/bundle.rs) + write_to_dir** - `143552d` (feat)
3. **Task 3: Wire sdk::generate (emit→gofmt→bundle) and flip snapshot_sdk GREEN** - `c2c6d9e` (feat)

**Plan metadata:** committed with this SUMMARY + STATE.md + ROADMAP.md.

## Files Created/Modified
- `crates/gnr8-core/src/sdk/emit.rs` (created) — `format!`-based emitters: `emit_models`/`emit_client`/`emit_operations`/`emit_errors`, the `go_type` type-mapper (TARGET-API §4), the `exported`/`lower_camel` Go-name casers (with Go initialisms), per-file computed imports, and 16 unit tests.
- `crates/gnr8-core/src/sdk/gofmt.rs` (created) — `gofmt(&str) -> Result<String, CoreError>` subprocess (discrete args, stdin pipe, typed errors) + 3 tests (idempotence, invalid-Go→GoFmt, missing-binary→GoToolchainMissing).
- `crates/gnr8-core/src/sdk/bundle.rs` (created) — `SdkBundle`/`SdkFile` + `Display` (file-marker framing) + `parse` (shared with `write_to_dir`) + 3 round-trip/determinism tests.
- `crates/gnr8-core/src/sdk/mod.rs` (modified) — `generate` (emit→gofmt→bundle), `group_by_tag`, `write_to_dir`, and 4 generate behavior tests.
- `crates/gnr8-core/tests/snapshot_sdk.rs` (modified) — header flipped red-by-design → GREEN contract test.
- `crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap` (created) — reviewed SDK bundle snapshot reconciled with `expected/sdk/*.go`.

## Decisions Made
- **Tag grouping rule (untagged → default tag):** Untagged operations inherit the lexically-first tag present anywhere in the graph (else the package name). Deterministic and keeps the single-resource fixture's four operations in one `goals.go`, matching `expected/sdk/goals.go`. Multi-tag services fan out to one `<tag>.go` per sorted tag.
- **Pointer-wrapping discipline:** Only Go value types (`number`/`bool`/`int64`/`time.Time`) become `*T` when optional; strings/slices/maps stay plain (already nilable) — matches `expected/sdk/models.go`. Optional **query** params are an exception: always `*T` so the encoder distinguishes unset from empty.
- **Idiomatic path-param args:** `lower_camel` lower-cases the first word fully so a `uuid` path param is `uuid string`, not the initialism-aware `uUID` — keeps the generated code idiomatic and `gofmt`-clean.
- **Graph is the source of truth for optionality (Pitfall 2):** `CreateGoalInput.Description` (graph: required=false, optional=false) emits no `omitempty`, even though the illustrative fixture shows one. The `.snap` was authored from real output and reviewed for semantic equivalence, not byte-copied.
- **`to_string` via `Display`:** Implemented `std::fmt::Display` for `SdkBundle` so the D-06 `to_string()` contract is the blanket-impl one (avoids clippy's `should_implement_trait` on an inherent `to_string`).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Non-idiomatic `uUID` path-param argument name**
- **Found during:** Task 3 (reviewing the real generated `goals.go` before accepting the snapshot)
- **Issue:** The initialism-aware `exported("uuid")` returns `UUID`; the original `lower_camel` only lower-cased the first char, yielding `uUID string` for the path argument — valid Go but not idiomatic and a mismatch with `expected/sdk/goals.go`'s `uuid string`.
- **Fix:** Rewrote `lower_camel` to fully lower-case the first word and exported-case subsequent words (`uuid`→`uuid`, `goalId`→`goalID`, `page_size`→`pageSize`); added a `lower_camel` unit test.
- **Files modified:** crates/gnr8-core/src/sdk/emit.rs
- **Verification:** `lower_camel` test passes; the regenerated `.snap` shows `uuid string`; the materialized SDK `go build`s + `go vet`s clean.
- **Committed in:** c2c6d9e (Task 3 commit)

**2. [Rule 3 - Blocking] Clippy `-D warnings` fixes in new code**
- **Found during:** Tasks 1–3
- **Issue:** `-D warnings` flagged: `format_push_string` (using `push_str(&format!())` in the file header builder), `too_many_lines` on `emit_operation`, `doc_markdown` (unbacktick'd `APIError`/`TargetDirection` in docs), `should_implement_trait` (an inherent `SdkBundle::to_string`), `doc-comment unbalanced backticks` (raw `` ` `` inside `` `json:...` `` doc examples), and cross-task `dead_code`.
- **Fix:** Switched to `write!`/`writeln!`; split `emit_operation` into an `emit_request_dispatch` helper; backticked the doc terms and rephrased the json-tag doc examples; implemented `Display` instead of an inherent `to_string`; used scoped `#[allow(dead_code)]` for cross-task wiring (removed in the consuming task, leaving clippy green).
- **Files modified:** crates/gnr8-core/src/sdk/{emit.rs, bundle.rs, gofmt.rs, mod.rs}, crates/gnr8-core/tests/snapshot_sdk.rs
- **Verification:** `cargo clippy --all-targets --all-features --locked -- -D warnings` clean; `cargo fmt --all -- --check` clean.
- **Committed in:** 002ea29, 143552d, c2c6d9e (task commits)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking). **Impact on plan:** Both were necessary for code quality and the workspace lint gate; no scope creep — the generated SDK is idiomatic and compiles, and all gates pass.

## Issues Encountered
- **No `cargo-insta` in the environment.** Used the documented manual insta accept flow (`INSTA_UPDATE=new` → review `.snap.new` vs `expected/sdk/*.go` → strip `assertion_line` → rename to `.snap`), the same flow 02-03/03-01 used. Additionally materialized the framed bundle into a hermetic stdlib-only temp module and ran `go build`/`go vet` to confirm the generated SDK genuinely compiles before accepting the snapshot. Resolved within the task.
- **Benign pre-commit hook noise.** A repo-root pre-commit hook prints `go: go.mod file not found` (it runs `go` where there is no module); it does not block the commit and is unrelated to these Rust-only changes.

## User Setup Required
None - no external service configuration required (the Go toolchain, used by `gofmt` and the snapshot test's `build_graph`, is already a hard project dependency at go 1.26).

## Next Phase Readiness
- **Ready for 03-03 (compile/smoke + CI promotion):** `sdk::generate` returns a stable bundle; `write_to_dir` materializes the four stdlib-only files; the generated SDK was verified to `go build`/`go vet` clean, so 03-03's hermetic temp-dir `go build` + `httptest` smoke (SDK-05) has a known-good input. `CoreError::GoBuild` is already defined (03-01). All four contract tests are now GREEN and ready to be promoted into the blocking CI gate (the last red-by-design test, `snapshot_sdk`, is green).
- No blockers.

## Self-Check: PASSED

- Created files exist: `crates/gnr8-core/src/sdk/emit.rs`, `crates/gnr8-core/src/sdk/gofmt.rs`, `crates/gnr8-core/src/sdk/bundle.rs`, `crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap`, `.planning/phases/03-openapi-and-go-sdk-generation/03-02-SUMMARY.md` — all FOUND.
- Task commits exist: `002ea29`, `143552d`, `c2c6d9e` — all FOUND.
- `snapshot_sdk` GREEN; `snapshot_graph`/`snapshot_diagnostics`/`snapshot_openapi`/`determinism` GREEN (no regression); 64 lib tests pass; generated SDK `go build`s + `go vet`s clean; `cargo fmt --check` + `cargo clippy -D warnings` clean.

---
*Phase: 03-openapi-and-go-sdk-generation*
*Completed: 2026-06-24*
