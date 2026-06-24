---
phase: 03-openapi-and-go-sdk-generation
verified: 2026-06-24T21:40:00Z
status: passed
score: 19/19 must-haves verified
overrides_applied: 0
---

# Phase 3: OpenAPI And Go SDK Generation Verification Report

**Phase Goal:** Generate real artifacts from the graph: a valid OpenAPI document and a compiling Go SDK.
**Verified:** 2026-06-24T21:40:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

All four ROADMAP success criteria are observably true in the codebase, proven by a fully GREEN `make gates`
(exit 0) that runs the real Go toolchain (go 1.26.2): `lower::to_openapi` and `sdk::generate` are implemented
seams (no `NotYetImplemented`), the OpenAPI snapshot is a valid OpenAPI 3.1.0 document, the generated Go SDK
genuinely `go build`s and answers an httptest round-trip, and the 7 compatibility-gap diagnostics are surfaced.

### Observable Truths (ROADMAP Success Criteria)

| #   | Truth (Success Criterion) | Status | Evidence |
| --- | ------------------------- | ------ | -------- |
| SC1 | The fixture graph lowers to a valid OpenAPI document | ✓ VERIFIED | `snapshot_openapi` GREEN; `.snap` parses as valid OpenAPI 3.1.0 (Ruby psych load OK); `openapi: 3.1.0`, info/security/paths/components present |
| SC2 | OpenAPI output includes paths, operations, parameters, request bodies, responses, and component schemas | ✓ VERIFIED | `.snap` has 3 paths (`/goal/`, `/goal/list`, `/goal/{uuid}` with put+delete coexisting); operationId/summary/tags; path+query params with enum/required; requestBody $refs; responses by status (201/400/200/404); 9 component schemas; securitySchemes ApiKeyAuth + top-level security |
| SC3 | The generated Go SDK compiles and can call fixture operations through tests | ✓ VERIFIED | `tests/sdk_compile.rs` 3/3 PASS (ran for real, 1.98s, no skip): `generated_sdk_go_builds_clean` (real `go build ./...` exit 0 on hermetic zero-require module), `generated_sdk_passes_httptest_smoke` (NewClient → CreateGoal POST /goal/ + decoded CommandMessageWithUUID + DeleteGoal 404 → *APIError), `invalid_go_build_maps_to_go_build_error_not_panic` |
| SC4 | OpenAPI compatibility gaps are reported as diagnostics | ✓ VERIFIED | `snapshot_diagnostics` GREEN with exactly 7 WARN diagnostics: 3× float64→float32 narrowing, 1× free-form map (→additionalProperties:true), 3× untyped query param (cursor/page_size/aggregation), each with source provenance |

**Score:** 4/4 ROADMAP success criteria verified

### PLAN Must-Have Truths (plan-specific detail)

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | `to_openapi(&ApiGraph)` returns String OpenAPI 3.1.0 YAML (no NotYetImplemented) | ✓ VERIFIED | `lower/mod.rs:63` real body builds OpenApiDoc + `yaml::write`; lib test `to_openapi_*` pass |
| 2 | Four fixture paths keyed absolutely under `/goal` (deterministic join) | ✓ VERIFIED | `.snap` keys `/goal/`, `/goal/list`, `/goal/{uuid}`; `join_base` const `BASE_PATH="/goal"`; unit test `paths_are_keyed_absolutely_under_goal` PASS |
| 3 | Document has info/security/paths→ops/params/requestBody/responses/components.schemas+securitySchemes | ✓ VERIFIED | Full `.snap` read; all sections present and well-formed |
| 4 | OAPI-03 diagnostics surfaced (carried from graph, not re-derived, no panic) | ✓ VERIFIED | `snapshot_diagnostics` GREEN (7 WARNs); `lowering_succeeds_even_when_diagnostics_are_non_empty` PASS |
| 5 | Dangling $ref → typed `CoreError::Lowering` (no panic) | ✓ VERIFIED | `dangling_request_body_ref_returns_lowering_error` + `unknown_schema_type_kind_returns_lowering_error` PASS |
| 6 | `snapshot_openapi` GREEN against reviewed `.snap` | ✓ VERIFIED | Contract test PASS; not red-by-design (header confirms GREEN, calls live seam) |
| 7 | `to_openapi` byte-identical across two runs | ✓ VERIFIED | `to_openapi_is_byte_identical_across_two_runs` (determinism.rs) PASS |
| 8 | `sdk::generate` returns deterministic multi-file Go SDK bundle String (no NotYetImplemented) | ✓ VERIFIED | `sdk/mod.rs:30` real emit→gofmt→bundle pipeline; `generate_returns_ok_with_the_four_file_markers` PASS |
| 9 | client.go: Client + functional options (WithHTTPClient/WithAPIKey) + NewClient (SDK-01) | ✓ VERIFIED | `.snap` lines 15-44: `type Client`, `WithHTTPClient`, `WithAPIKey`, `NewClient(baseURL string, opts ...Option)` |
| 10 | Tag-grouped typed methods on *Client, ctx-first, path/query/body + decode (SDK-02) | ✓ VERIFIED | `.snap` goals.go: CreateGoal/ListGoals/DeleteGoal/UpdateGoal all `(ctx context.Context, ...)`; body marshal, query encode, X-API-Key, 2xx decode / non-2xx APIError |
| 11 | models.go: structs with json tags, optional pointers/omitempty, enum newtypes, uuid/time mapping (SDK-03) | ✓ VERIFIED | `.snap` models.go: `*float32 json:"...,omitempty"`, `*TargetDirection`, `time.Time`, uuid→string, `map[string]any`, TargetDirection newtype + const block |
| 12 | errors.go: typed APIError (status + decoded body) + JSON encode/decode in ops (SDK-04) | ✓ VERIFIED | `.snap` errors.go: `APIError{StatusCode,Message,Slug,Hints}` + `Error()` + `IsNotFound()`; ops decode error body into APIError |
| 13 | Every emitted Go file gofmt-clean; gofmt non-zero → `CoreError::GoFmt` (no panic) | ✓ VERIFIED | `sdk/gofmt.rs` discrete-arg subprocess; `invalid_go_maps_to_gofmt_error_not_panic` + `formats_misindented_go_and_is_idempotent` PASS |
| 14 | `snapshot_sdk` GREEN against reviewed `.snap` | ✓ VERIFIED | Contract test PASS; not red-by-design |
| 15 | `sdk::generate` byte-identical across two runs | ✓ VERIFIED | `sdk_generate_is_byte_identical_across_two_runs` + `generate_is_byte_identical_across_two_runs` PASS |
| 16 | sdk_compile.rs: hermetic temp dir + zero-require go.mod + `go build ./...` succeeds (SDK-05) | ✓ VERIFIED | go.mod = `module gnr8sdktest\n\ngo 1.26\n` (zero requires); GOPROXY=off; `generated_sdk_go_builds_clean` PASS |
| 17 | httptest smoke: Client + fixture op + method/path/body + decode + 4xx→APIError (SDK-05/SDK-04) | ✓ VERIFIED | `generated_sdk_passes_httptest_smoke` runs real `go test ./...`: asserts POST /goal/ + body + UUID decode; 404 → *APIError StatusCode==404 + IsNotFound() |
| 18 | `go build` non-zero → `CoreError::GoBuild` (no panic) | ✓ VERIFIED | `invalid_go_build_maps_to_go_build_error_not_panic` PASS |
| 19 | All four contract tests + sdk_compile in BLOCKING gates; non-blocking contract job retired (D-07) | ✓ VERIFIED | Makefile `gates` + ci.yml `gates` job run all four + sdk_compile; no `contract:` job; no active `continue-on-error: true` |

**Score:** 19/19 plan must-have truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `crates/gnr8-core/src/lower/model.rs` | Typed OpenAPI 3.1 structs | ✓ VERIFIED | exists, 181 lines, no issues |
| `crates/gnr8-core/src/lower/yaml.rs` | Deterministic YAML writer | ✓ VERIFIED | exists, 427 lines, key-ordered, tested |
| `crates/gnr8-core/src/lower/mod.rs` | to_openapi mapping + /goal join | ✓ VERIFIED | exists, 622 lines, real seam |
| `crates/gnr8-core/src/error.rs` | Lowering/SdkGen/GoFmt/GoBuild variants | ✓ VERIFIED | all 4 variants + Display tests pass |
| `tests/snapshots/snapshot_openapi__...snap` | Reviewed OpenAPI snapshot | ✓ VERIFIED | valid OpenAPI 3.1, 255 lines |
| `crates/gnr8-core/src/sdk/emit.rs` | Go emitters + computed imports | ✓ VERIFIED | exists, 1096 lines, 16 tests |
| `crates/gnr8-core/src/sdk/bundle.rs` | SdkBundle + framing | ✓ VERIFIED | exists, 177 lines, round-trip tested |
| `crates/gnr8-core/src/sdk/gofmt.rs` | gofmt subprocess | ✓ VERIFIED | exists, 139 lines, typed error |
| `crates/gnr8-core/src/sdk/mod.rs` | generate + write_to_dir | ✓ VERIFIED | exists, 315 lines, real seam + public string-based write_to_dir |
| `tests/snapshots/snapshot_sdk__...snap` | Reviewed SDK snapshot | ✓ VERIFIED | 4 framed files, all SDK constructs |
| `crates/gnr8-core/tests/sdk_compile.rs` | go build + httptest smoke (SDK-05) | ✓ VERIFIED | exists, 266 lines, 3 tests pass for real |
| `crates/gnr8-core/tests/determinism.rs` | to_openapi + generate two-run asserts | ✓ VERIFIED | 3 tests (graph/openapi/sdk) pass |
| `.github/workflows/ci.yml` | contract tests blocking; contract job removed | ✓ VERIFIED | gates job runs all + sdk_compile; no continue-on-error |
| `Makefile` | gates includes four + sdk_compile; contract retired | ✓ VERIFIED | gates target verified; contract target gone |

**14/14 artifacts VERIFIED** (gsd-sdk verify.artifacts: 03-01 5/5, 03-02 5/5, 03-03 4/4).

### Key Link Verification

| From | To | Status | Details |
| ---- | -- | ------ | ------- |
| lower/mod.rs | graph/mod.rs | ✓ WIRED | consumes &ApiGraph |
| lower/mod.rs | lower/yaml.rs | ✓ WIRED | yaml::write serialization |
| snapshot_openapi.rs | lower/mod.rs | ✓ WIRED | calls to_openapi |
| sdk/mod.rs | graph/mod.rs | ✓ WIRED | consumes &ApiGraph |
| sdk/mod.rs | sdk/gofmt.rs | ✓ WIRED | gofmt normalization |
| snapshot_sdk.rs | sdk/mod.rs | ✓ WIRED | calls sdk::generate |
| sdk_compile.rs | sdk/mod.rs | ✓ WIRED | generate + write_to_dir |
| sdk_compile.rs | `go build ./...` | ✓ WIRED | discrete-arg Command |
| ci.yml | sdk_compile.rs | ✓ WIRED | blocking gates runs --test sdk_compile |

**9/9 key links VERIFIED** (gsd-sdk verify.key-links: all three plans all_verified=true).

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Full blocking gate suite | `make gates` | exit 0; all tests `0 failed` | ✓ PASS |
| OpenAPI snapshot is valid OpenAPI 3.1 | ruby psych YAML.load_file | openapi=3.1.0, 3 paths, 9 schemas, ApiKeyAuth | ✓ PASS |
| SDK compiles + smoke (not skipped) | `cargo test --test sdk_compile -- --nocapture` | 3 passed, 1.98s, no "skipping" | ✓ PASS |
| Determinism (graph/openapi/sdk) | `cargo test --test determinism` | 3 passed | ✓ PASS |
| Format check | `cargo fmt --all -- --check` | exit 0 | ✓ PASS |
| Lint (deny warnings) | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 | ✓ PASS |
| No NotYetImplemented seam in lower/sdk | grep src/lower src/sdk | none (only Phase-1 doc/test refs in lib.rs/analyze) | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| OAPI-01 | 03-01 | OpenAPI writer emits valid document for fixture | ✓ SATISFIED | snapshot_openapi GREEN; valid OpenAPI 3.1 |
| OAPI-02 | 03-01 | Doc includes info/paths/ops/params/bodies/responses/schemas | ✓ SATISFIED | Full `.snap` structure verified |
| OAPI-03 | 03-01 | Lowering emits diagnostics for unrepresentable facts | ✓ SATISFIED | snapshot_diagnostics 7 WARNs; free-form map → additionalProperties:true |
| SDK-01 | 03-02 | Client with base URL + custom http.Client | ✓ SATISFIED | NewClient + WithHTTPClient + WithAPIKey |
| SDK-02 | 03-02 | Typed methods for operations | ✓ SATISFIED | ctx-first tag-grouped methods |
| SDK-03 | 03-02 | Generated request/response models | ✓ SATISFIED | models.go structs + enum newtype |
| SDK-04 | 03-02 | JSON encoding/decoding + typed API errors | ✓ SATISFIED | APIError + encode/decode in ops; 404 smoke path |
| SDK-05 | 03-03 | Generated SDK compiles + exercised by tests | ✓ SATISFIED | go build exit 0 + httptest go test pass |

**8/8 phase requirements SATISFIED. No orphaned requirements** — all 8 expected IDs (OAPI-01..03, SDK-01..05)
are declared across the three plans and verified.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| (none) | — | — | — | No debt markers (TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER) in any Phase-3 src/test file. All unwrap/expect/panic occurrences are inside `#[cfg(test)]`/`mod tests` blocks or scoped-allow test targets; production paths use typed `CoreError` (clippy -D warnings clean confirms). The `placeholder` token in emit.rs:628 is a path-param substitution variable, not a stub. |

### Human Verification Required

None. The phase produces runnable code and every success criterion was verified programmatically with the real
Go toolchain present (go 1.26.2): the OpenAPI artifact parses as valid OpenAPI 3.1, and the generated Go SDK was
actually compiled (`go build`) and exercised (`go test` httptest round-trip), not merely snapshotted. There are
no `<verify><human-check>` blocks deferred in the PLAN files.

### Gaps Summary

No gaps. All four ROADMAP success criteria and all 19 plan must-have truths are VERIFIED against the actual
codebase. `make gates` is fully GREEN (exit 0) with all four contract tests (snapshot_graph, snapshot_diagnostics,
snapshot_openapi, snapshot_sdk) plus sdk_compile in the blocking job; the non-blocking `contract` job and Makefile
target are retired with no active `continue-on-error: true` remaining (D-07 satisfied). The hardest bar, SDK-05,
is genuinely met: the SDK materializes to a hermetic stdlib-only zero-require module, `go build ./...` exits 0, and
an httptest smoke test constructs the Client and round-trips CreateGoal (request + response decode) plus a
404→`*APIError` path. Both `to_openapi` and `sdk::generate` are real implementations (no `NotYetImplemented` seams
remain in src/), deterministic (byte-identical two-run), with no production unwrap/expect/panic and clean
fmt-check + clippy -D warnings. No red-by-design contract tests remain.

---

_Verified: 2026-06-24T21:40:00Z_
_Verifier: Claude (gsd-verifier)_
