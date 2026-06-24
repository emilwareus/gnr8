---
phase: 03-openapi-and-go-sdk-generation
reviewed: 2026-06-24T00:00:00Z
depth: deep
files_reviewed: 16
files_reviewed_list:
  - .github/workflows/ci.yml
  - Makefile
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/lower/mod.rs
  - crates/gnr8-core/src/lower/model.rs
  - crates/gnr8-core/src/lower/yaml.rs
  - crates/gnr8-core/src/sdk/bundle.rs
  - crates/gnr8-core/src/sdk/emit.rs
  - crates/gnr8-core/src/sdk/gofmt.rs
  - crates/gnr8-core/src/sdk/mod.rs
  - crates/gnr8-core/tests/determinism.rs
  - crates/gnr8-core/tests/sdk_compile.rs
  - crates/gnr8-core/tests/snapshot_openapi.rs
  - crates/gnr8-core/tests/snapshot_sdk.rs
  - crates/gnr8-core/tests/snapshots/snapshot_openapi__goalservice_openapi.snap
  - crates/gnr8-core/tests/snapshots/snapshot_sdk__goalservice_sdk.snap
findings:
  critical: 1
  warning: 4
  info: 4
  total: 9
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-06-24
**Depth:** deep
**Files Reviewed:** 16
**Status:** issues_found

## Summary

Phase 3 lowers the Phase-2 API graph to an OpenAPI 3.1 document (`lower/`) and generates a
gofmt-clean Go SDK (`sdk/`), promoting all four contract snapshots plus a real `go build` + httptest
smoke gate into the blocking CI/Makefile set. The mechanical quality is high and the stated guardrails
hold: clippy `-D warnings` is clean, no production `unwrap`/`expect`/`panic` exists anywhere in the
changed `gnr8-core` source (verified by an AST-scope scan), the `gofmt`/`go build` subprocesses use
discrete args with no shell (ASVS L1), `write_to_dir` carries a real path-traversal guard, the YAML
writer and SDK emission are byte-deterministic (`Vec<(K,V)>` everywhere, no `HashMap` iteration), the
OpenAPI is correct 3.1 (required-omission optionality, quoted JSON-pointer `$ref`, no `nullable`/`null`
array form), and the `/goal` base-path join is correct (no `/goal//list`, no dropped prefix). The
sdk_compile test is genuinely hermetic (zero-`require` go.mod, `GOPROXY=off`) and exercises a real
round-trip + a 4xx `*APIError` path.

The review nonetheless surfaces **one BLOCKER** that the fixture masks: the operations emitter
hard-codes the error-decode type to a Go type literally named `HttpError` (with `Slug`/`Hints`/`Message`
fields), ignoring the per-operation error model the graph already carries. This compiles only because
the goalservice fixture happens to define exactly that type — I empirically confirmed that renaming the
error model produces `undefined: HttpError` and a failed `go build`. Several WARNINGs flag related
codegen-generality gaps (success-status default, non-string query params, ignored query-param enums)
that are latent for the current single-fixture extractor but will produce wrong or non-compiling Go the
moment the graph carries a shape the fixture does not.

No `<structural_findings>` block was provided, so this report contains only narrative findings.

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: SDK error-decode path hard-codes a Go type named `HttpError`, ignoring the graph's per-operation error model — non-compiling Go for any real graph whose error type is named otherwise

**File:** `crates/gnr8-core/src/sdk/emit.rs:593-602` (`emit_request_dispatch`)
**Issue:**
The non-2xx branch of every generated method is emitted with a fixed type name and fixed field set:

```rust
writeln!(body, "if resp.StatusCode != {success_status} {{").map_err(sink)?;
writeln!(body, "var apiErr HttpError").map_err(sink)?;          // <-- hard-coded type name
writeln!(body, "_ = json.NewDecoder(resp.Body).Decode(&apiErr)").map_err(sink)?;
writeln!(body, "return out, &APIError{{").map_err(sink)?;
writeln!(body, "StatusCode: resp.StatusCode,").map_err(sink)?;
writeln!(body, "Message: apiErr.Message,").map_err(sink)?;       // <-- hard-coded field
writeln!(body, "Slug: apiErr.Slug,").map_err(sink)?;             // <-- hard-coded field
writeln!(body, "Hints: apiErr.Hints,").map_err(sink)?;           // <-- hard-coded field
```

The operation's actual non-2xx response bodies (`op.responses` — e.g. the fixture's `400 -> HttpError`,
`404 -> HttpError`) are never read here; the generator throws away the error-model information the graph
already resolved during lowering and substitutes a literal `HttpError`. Two failure modes for a
real-world graph (not the fixture):

1. **No type named `HttpError`** → the emitted `goals.go` references an undefined identifier and
   `go build` fails. Empirically verified: taking the committed fixture SDK and renaming only the
   `type HttpError struct` declaration to `type ApiProblem struct` yields
   `./goals.go:34:14: undefined: HttpError` (four occurrences) and a failed build.
2. **A type named `HttpError` exists but lacks `Slug` and/or `Hints`** → the `apiErr.Slug` /
   `apiErr.Hints` field accesses fail to compile (`apiErr.Slug undefined`).

The fixture compiles purely by coincidence: its error DTO is named exactly `HttpError` and carries
exactly `Message`/`Slug`/`Hints` (see `snapshot_sdk__goalservice_sdk.snap:275-279`). The sdk_compile
gate therefore cannot catch this — it only ever exercises that one fixture. This is the central
codegen-correctness defect for the phase's explicit "would the generated SDK be correct for real-world
graphs?" bar; it is a hard-coded generator assumption, not a fixture limitation.

**Fix:**
Derive the error model per operation from the graph instead of hard-coding it. Resolve the body `$ref`
of the operation's first non-2xx response (mirroring the existing `success_of`/`body_model_of`
resolvers), and emit that resolved Go type name (falling back to inline-decoding only the fields
`APIError` actually consumes when no typed error body is present). Sketch:

```rust
// Resolve the (lowest-status) non-2xx error model for this op, like success_of for 2xx.
fn error_model_of(op: &Operation, graph: &ApiGraph) -> Result<Option<String>, CoreError> {
    for resp in &op.responses {
        if !(200..300).contains(&resp.status) {
            if let Some(body) = &resp.body {
                let model = graph.schemas.iter().find(|s| s.id == body.ref_id)
                    .ok_or_else(|| CoreError::SdkGen { message: format!(
                        "operation '{}' error response references dangling $ref '{}'",
                        op.id, body.ref_id) })?;
                return Ok(Some(model.name.clone()));
            }
        }
    }
    Ok(None)
}
```

Then emit `var apiErr <resolved-name>` (and only reference fields the resolved struct actually has — or
decode into a small generator-owned anonymous struct so the SDK never depends on a field shape the user
type may not provide). At minimum, if the design intends a single fixed error envelope, that envelope
must be generated by the SDK itself (in `errors.go`) rather than depending on a user model named
`HttpError`.

## Warnings

### WR-01: Success-status comparison uses a default of 200 for body-less 2xx responses, so a 201/204-only operation treats its own success as an error

**File:** `crates/gnr8-core/src/sdk/emit.rs:474, 593` (`emit_operation` / `emit_request_dispatch`)
**Issue:**
`success_of` returns `Ok(None)` for an operation whose first 2xx response has no body (early `return
Ok(None)` at `emit.rs:361-363`). The caller then defaults the success status:

```rust
let success_status = success.as_ref().map_or(200, |s| s.status);   // emit.rs:474
```

and the dispatch emits `if resp.StatusCode != {success_status} { ...APIError... }`. For an operation
that genuinely succeeds with a body-less **201 or 204**, the generated client compares against `200`,
so the real success status falls into the non-2xx error branch and the method returns a spurious
`*APIError`. The fixture never hits this (its body-less paths still return 200), so the gate passes. The
single-2xx-with-body case is correct; the defect is specifically the no-2xx-body branch defaulting to
200 instead of the response's actual status.

**Fix:**
Have `success_of` (or a sibling) return the chosen 2xx status even when there is no body, so
`success_status` reflects the real code:

```rust
// Track the first 2xx status regardless of whether it carries a body.
for resp in &op.responses {
    if (200..300).contains(&resp.status) {
        let model = match &resp.body { Some(b) => Some(resolve(b)?), None => None };
        return Ok(Some(Success { status: resp.status, model }));  // model now Option
    }
}
```

and treat the success branch as "2xx" generally rather than exact-status when the spec allows a range.

### WR-02: Query-param encoding assumes every query param is a Go `string`; a non-string typed query param emits non-compiling `q.Set(string, <non-string>)`

**File:** `crates/gnr8-core/src/sdk/emit.rs:567-575` (query encoding) + `emit.rs:662` (params struct)
**Issue:**
`emit_params_struct` types each query field via `go_type(&p.schema, false, graph)` — so an `integer`
query param becomes `int64`, a `number` becomes `float32`, etc. The encoder then emits
`q.Set("name", params.Field)` (required) or `q.Set("name", *params.Field)` (optional). Go's
`url.Values.Set` takes `(string, string)`, so any non-string query field produces a compile error
(`cannot use params.Field (variable of type int64) as string value`). This is latent today only because
the goextract helper currently hard-types every `c.Query(...)` param as `string`
(`goextract/internal/handlers/handlers.go:389`), but the codegen makes the assumption implicitly with no
guard — and the project's stated direction (more routers / typed query binding) will produce typed query
params.

**Fix:**
When the query field is not a Go `string`, convert at the call site (e.g. `strconv.FormatInt`,
`strconv.FormatFloat`, `fmt.Sprintf("%v", ...)`) and add the corresponding import, or restrict
`emit_params_struct` to `string` and return a `CoreError::SdkGen` for an unsupported non-string query
param so the failure is typed rather than a downstream `go build` break.

### WR-03: `emit_url` silently drops path-param args whose `{placeholder}` is absent from the path, risking a `fmt.Sprintf` verb/arg mismatch

**File:** `crates/gnr8-core/src/sdk/emit.rs:625-640` (`emit_url`)
**Issue:**
`emit_url` only appends a `Sprintf` argument when `format_str.contains(&placeholder)`:

```rust
for p in path_params {
    let placeholder = format!("{{{p}}}");
    if format_str.contains(&placeholder) { /* replace + push arg */ }
}
```

Conversely, a `{name}` token present in the path with no matching path param leaves a literal `%s` in
the format string with no argument supplied — `fmt.Sprintf` then renders `%!s(MISSING)` into the URL at
runtime (a silent wrong-URL bug, not a compile error). The opposite skew (a path param not in the path)
leaves an unused method argument (benign in Go). The fixture's params and paths are perfectly aligned so
neither skew triggers, but the emitter trusts that alignment without asserting it.

**Fix:**
Validate that the set of `{...}` tokens in the absolute path exactly equals the set of path-param names,
returning `CoreError::SdkGen` on any mismatch before emitting the `Sprintf` call; this turns a silent
runtime `%!s(MISSING)` into a typed generation error.

### WR-04: Path-param values are interpolated into the URL with no escaping (`fmt.Sprintf("/goal/%s", uuid)`)

**File:** `crates/gnr8-core/src/sdk/emit.rs:634-639` (`emit_url`)
**Issue:**
The generated method builds the request URL by raw `fmt.Sprintf` interpolation of the caller-supplied
path argument: `url := c.baseURL + fmt.Sprintf("/goal/%s", uuid)`. A path-param value containing `/`,
`?`, `#`, `%`, or `..` is injected verbatim into the path, which can change the effective route
(`uuid = "../admin"` → `/goal/../admin`, or `uuid = "x?y=z"` injecting a query) on the generated SDK's
outgoing request. Path params should be percent-encoded via `url.PathEscape`. This is an SDK-side
request-shaping correctness/robustness issue rather than a server vulnerability, but it is the kind of
escaping defect the codegen review is meant to flag, and it affects every templated path the SDK emits.

**Fix:**
Escape each path argument before interpolation, e.g. emit `url.PathEscape(uuid)` (adding `net/url` to
the computed import set), so a path value can never restructure the request URL:

```go
url := c.baseURL + fmt.Sprintf("/goal/%s", url.PathEscape(uuid))
```

## Info

### IN-01: Query-param enum constraint present in the OpenAPI is dropped in the Go SDK

**File:** `crates/gnr8-core/src/sdk/emit.rs:656-668` (`emit_params_struct`)
**Issue:**
For the `aggregation` query param the OpenAPI emits a closed value set
(`enum: [avg, count, max, min, sum]`, `snapshot_openapi__...snap:46`), but the SDK params struct types
it as a bare `Aggregation string` (`snapshot_sdk__...snap:120`) — `emit_params_struct` calls `go_type`,
which never inspects `param.enum_values`. The two artifacts therefore disagree on the contract: the spec
constrains the value, the SDK does not. This is a known-PoC feature-parity gap (the value is still a
`string`, so it compiles and round-trips), not a correctness break, but it is worth recording since the
lowering path does honor param enums (`lower/mod.rs:183-191`) and the SDK path does not.
**Fix:** Optionally generate a string-newtype + const block for enum-valued query params (as
`emit_enum` already does for schema enums) and type the param field as that newtype.

### IN-02: `lower/mod.rs` hard-codes the document `title`/`version` and a single `BASE_PATH`, duplicated again in the SDK emitter

**File:** `crates/gnr8-core/src/lower/mod.rs:43, 100-105` and `crates/gnr8-core/src/sdk/emit.rs:406`
**Issue:**
`title: "goalservice"`, `version: "0.1.0"`, and `BASE_PATH = "/goal"` are baked into lowering, and the
same `/goal` base path is independently re-declared in `sdk/emit.rs` (`const BASE_PATH: &str = "/goal"`).
Two copies of the same magic constant in two modules can drift; both are also unrelated to the analyzed
graph (`graph.module` is `github.com/acme/svc`, not `goalservice`). The module docs acknowledge the
single-group PoC scope (D-02), so this is recorded as INFO, not a defect.
**Fix:** Lift the base path (and ideally title/version) into one shared source — e.g. derive from the
graph or a single `pub(crate) const` consumed by both `lower` and `sdk` — so the OpenAPI and SDK can
never disagree on the prefix.

### IN-03: `unique_temp_dir` keys only on PID + nanosecond timestamp; two same-PID calls in the same nanosecond would collide

**File:** `crates/gnr8-core/tests/sdk_compile.rs:43-53`
**Issue:**
The temp-dir name is `gnr8-sdk-compile-{label}-{pid}-{nanos}`. Distinct test functions use distinct
`label`s so they cannot collide, and each function calls it once, so in practice this is safe. It is
flagged only as a latent hygiene note: if the helper were ever reused twice within one process at
nanosecond resolution, `create_dir_all` would succeed on an existing dir and the two runs could
interleave files. Not a current defect.
**Fix:** Add a monotonic per-process counter (e.g. an `AtomicU64`) to the directory name if the helper
is ever called more than once per label.

### IN-04: `bundle::parse` silently discards any text before the first frame marker

**File:** `crates/gnr8-core/src/sdk/bundle.rs:71-89` (`parse`)
**Issue:**
`parse` ignores all lines until the first marker (`current` is `None`). The doc comment notes "there is
none in practice", and `to_string` never emits leading text, so the round-trip is exact today. Recorded
only because `write_to_dir` relies on `parse` for the on-disk materialization: if a future change ever
prepended a banner/license header before the first marker, that header would be silently dropped from
both the bundle round-trip and the written files with no error. Not a current defect.
**Fix:** Either assert the bundle begins with a marker, or attach pre-marker text to a sentinel so it is
never silently lost.

---

_Reviewed: 2026-06-24_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
