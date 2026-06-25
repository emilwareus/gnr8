---
phase: 03-openapi-and-go-sdk-generation
fixed_at: 2026-06-24T20:10:33Z
review_path: .planning/phases/03-openapi-and-go-sdk-generation/03-REVIEW.md
iteration: 1
findings_in_scope: 5
fixed: 5
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-06-24T20:10:33Z
**Source review:** .planning/phases/03-openapi-and-go-sdk-generation/03-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope (BLOCKER + WARNINGs): 5
- Fixed: 5
- Skipped: 0
- INFO findings (IN-01..IN-04): out of scope (`critical_warning`), all already documented as deliberate PoC choices in REVIEW.md — see "Deferred / Out of scope" below.

**Guiding principle applied:** every fix derives behaviour from the graph so the SDK is correct for an
arbitrary Gin service, not just `fixtures/goalservice`. Each fix is locked by a focused unit test that
runs the emitter on a *synthetic* graph whose shape differs from the fixture (error type not named
`HttpError`, body-less 201/204, typed query params, escaped path args, mismatched path tokens).

## Fixed Issues

### CR-01 (BLOCKER): SDK error-decode hard-codes a Go type named `HttpError`

**Files modified:** `crates/gnr8-core/src/sdk/emit.rs`, `crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap`
**Commit:** a918f38
**Applied fix:** Added `error_model_of(op, graph)` which resolves the operation's lowest-status non-2xx
response body `$ref` to its actual graph schema (mirroring `success_of`). The non-2xx branch now emits
`var apiErr <resolved-name>` instead of a literal `HttpError`, and copies into `APIError` only the
fields (`Message`/`Slug`/`Hints`) the resolved error struct actually declares (so it never reads a
field the user type lacks). When an operation declares no typed error body — or the error struct shares
none of those fields — the SDK decodes into a generator-owned anonymous struct exposing exactly the
fields `APIError` consumes, so the SDK never references a type the graph does not define.

**Generality test:** `error_decode_uses_the_graphs_error_model_name_not_a_hardcoded_httperror` builds a
synthetic graph whose error model is named `ApiError` and asserts the generated Go emits
`var apiErr ApiError` and does NOT contain `HttpError`. Plus
`error_decode_falls_back_to_an_anonymous_struct_when_no_error_response_exists` and
`error_decode_only_copies_fields_the_error_model_declares`.

**Fixture snapshot change (reviewed):** ONE block changed — `ListGoals` (which declares only a `200`
response, no error response) now decodes into the anonymous fallback struct instead of the fabricated
`var apiErr HttpError`. This is the correct, principled behaviour: the old code invented a dependency on
`HttpError` for an operation whose contract never mentioned an error body. `CreateGoal`/`DeleteGoal`/
`UpdateGoal` (which DO declare a `400 -> HttpError`) stay byte-identical. The SDK still `go build`s and
the httptest 404 smoke (`*APIError`) still passes.

### WR-01: Body-less 2xx response defaulted success status to 200

**Files modified:** `crates/gnr8-core/src/sdk/emit.rs`
**Commit:** d612157
**Applied fix:** `Success.model` is now `Option<String>` and `success_of` returns the first 2xx
response's real status even when it carries no body. The dispatch compares `resp.StatusCode != <real
status>` instead of defaulting to 200, so a body-less `201`/`204` is no longer mis-classified as an
error. `has_decode`/`return_model` derive from `model.is_some()`.

**Generality test:** `body_less_201_compares_against_its_real_status_not_a_default_200` and
`body_less_204_compares_against_its_real_status`.

**Fixture snapshot:** byte-identical (the fixture's body-less paths return 200, so the comparison is
unchanged).

### WR-02: Query-param encoding assumed every query param is a Go `string`

**Files modified:** `crates/gnr8-core/src/sdk/emit.rs`
**Commit:** 52681bd
**Applied fix:** `query_string_expr` coerces a query value to a string at the `q.Set` call site based on
its resolved Go type — `string` passes through unchanged, `int64`→`strconv.FormatInt`,
`float32`→`strconv.FormatFloat`, `bool`→`strconv.FormatBool`, `time.Time`→`.Format(time.RFC3339)`; any
other shape (slice/struct) is a typed `CoreError::SdkGen` rather than non-compiling Go. The required
`strconv`/`time` imports are computed from the file's query params (`query_imports`) and unioned into
the import block.

**Generality test:** `non_string_query_params_are_converted_to_string_with_strconv` (int64 + bool) and a
regression guard `string_query_params_emit_no_conversion_and_no_strconv_import`. The strconv conversions
were independently confirmed to compile as valid Go.

**Fixture snapshot:** byte-identical (all fixture query params are strings → identity conversion, no
extra import).

### WR-03: `emit_url` silently dropped path-param args on a token/param mismatch

**Files modified:** `crates/gnr8-core/src/sdk/emit.rs`, `crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap`
**Commit:** d334f1d (combined with WR-04 — same `emit_url` rewrite + same snapshot regeneration)
**Applied fix:** `path_tokens` extracts the `{...}` tokens from the absolute path; `emit_url` now asserts
the token set equals the declared path-param set and returns `CoreError::SdkGen` on any mismatch,
turning a silent runtime `%!s(MISSING)` (or an unused arg) into a typed generation-time error.
Interpolation now iterates the path tokens in path order so the `Sprintf` verbs and args always line up.

**Generality test:** `mismatched_path_token_and_param_is_a_typed_error` (path templates `{uuid}` but the
param is named `id`).

### WR-04: Path-param values interpolated into the URL with no escaping

**Files modified:** `crates/gnr8-core/src/sdk/emit.rs`, `crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap`
**Commit:** d334f1d (combined with WR-03)
**Applied fix:** Each interpolated path value is wrapped in `url.PathEscape(...)`, and `net/url` is added
to the operations file's import set whenever any op in the file has a path param, so a path value
containing `/`, `?`, `#`, `%`, or `..` can never restructure the request URL. To avoid the local URL
variable shadowing the freshly-imported `url` package, the local was renamed `url` → `reqURL`.

**Generality test:** `templated_path_escapes_each_arg_and_imports_net_url`.

**Fixture snapshot change (reviewed):** the two templated-path ops (`DeleteGoal`, `UpdateGoal`) now emit
`fmt.Sprintf("/goal/%s", url.PathEscape(uuid))`; the file imports `net/url`; every op's local URL var is
`reqURL`. The SDK still `go build`s and the httptest smoke still passes (`PathEscape("missing-uuid")`
== `missing-uuid`, so the path assertion is unchanged).

## Deferred / Out of scope

The four INFO findings are outside the `critical_warning` fix scope AND are explicitly recorded in
REVIEW.md as deliberate, non-defect PoC choices. None is a generality *bug* (each compiles and round-
trips today); they are documented here rather than fixed:

- **IN-01 (query-param enum not carried into the SDK):** the value is still a `string` that compiles and
  round-trips; emitting a newtype+const block is a feature-parity nicety, not a correctness break.
- **IN-02 (`title`/`version`/`BASE_PATH` hard-coded + duplicated):** the single-group `/goal` PoC scope
  is acknowledged (D-02); lifting the prefix into one shared source is a refactor, not a correctness bug.
- **IN-03 (`unique_temp_dir` PID+nanos key):** test-only hygiene note; distinct labels make collision
  impossible in practice.
- **IN-04 (`bundle::parse` drops pre-marker text):** the writer never emits leading text, so the round-
  trip is exact today; latent only under a hypothetical future banner.

## Gate status

`make gates` — GREEN (exit 0):
- gnr8-core lib: 73 passed (incl. 12 new generality tests for CR-01/WR-01..WR-04)
- gnr8 CLI: 9 passed
- contract snapshots (graph/diagnostics/openapi/sdk): 4 passed, fixture SDK reviewed-regenerated
- determinism (graph + OpenAPI + SDK byte-identical): 3 passed
- sdk_compile (go build + httptest smoke, incl. 4xx `*APIError`): 3 passed
- `cargo fmt --all -- --check`: clean
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: clean
- no production `unwrap`/`expect`/`panic` introduced (every new path returns `CoreError::SdkGen`)

---

_Fixed: 2026-06-24T20:10:33Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
