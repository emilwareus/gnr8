---
phase: 05-typescript-target-tssdk
verified: 2026-06-26T00:40:00Z
status: passed
score: 3/3 must-haves verified
has_blocking_gaps: false
overrides_applied: 0
re_verification:
  previous_status: null
  previous_score: null
---

# Phase 5: TypeScript Target — `TsSdk` Verification Report

**Phase Goal:** A developer can generate a dependency-free TypeScript SDK from the neutral IR and prove it type-checks, completing the fourth language path (NestJS source → TS SDK) as a pure IR→string twin.
**Verified:** 2026-06-26T00:40:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth (ROADMAP Success Criterion) | Status | Evidence |
|---|-----------------------------------|--------|----------|
| 1 | A developer can generate a dependency-free TypeScript SDK from the IR (built-in `fetch`, typed `interface` models + string-literal-union enums, a typed `ApiError`, a configurable `Client`). | ✓ VERIFIED | Independently materialized the real 4-file SDK from the `nestjs-bookstore` IR. `models.ts` emits `export interface BookDto {...}`, string-literal-union enums (`export type BookFormat = "hardcover" \| "paperback"`, `sort?: "asc" \| "desc" \| null`), and named unions (`export type BookOrError = BookDto \| OutOfStockDto`). `errors.ts` emits `export class ApiError extends Error` with `isNotFound()`. `client.ts` emits `export class Client` with `fetch?: typeof fetch` injectable transport and `this.fetchFn = opts.fetch ?? fetch`. `ts_type` (emit.rs:166–235) is an EXHAUSTIVE 9-variant `Type` match with NO `_=>` catch-all; inline `Type::Object`/dangling `Named` → typed `CoreError::SdkGen`. |
| 2 | The generated TS SDK type-checks under `tsc --noEmit` in a hermetic test, with zero runtime dependencies (no axios). | ✓ VERIFIED | `cargo test -p gnr8-core --test tssdk_compile` ran in MY process: 3/3 PASS, including `generated_sdk_typechecks_with_vendored_tsc` which runs `node <vendored tsc> --noEmit --strict --target es2022 --module esnext --moduleResolution bundler --lib es2022,dom <files>` (exit 0) using ONLY the vendored typescript. Independent banned-import grep over the real generated `.ts` found ONLY relative intra-SDK imports (`./errors`, `./models`) — zero `axios`/`node-fetch`/`@types`/`from "http"`. Invalid-TS test captures a typed `CoreError::GoBuild`, not a panic. |
| 3 | A developer adds the TS SDK to a `.gnr8/` Pipeline via a `TsSdk` `Target` built-in, and the output is byte-identical across repeated runs. | ✓ VERIFIED | `TsSdk` struct + `impl Target` (builtins.rs:655–740) wired to `crate::tssdk::generate` + `crate::tssdk::split_bundle`; package via `sdk_package(&self.module)`, base path via `ir.base_path` (single source, rule 3 — no second derivation). Unconfigured → typed `CoreError::Config`; unsafe frame name → `CoreError::SdkGen`; `output_anchors()` returns the dir. Re-exported from `sdk::prelude` (mod.rs:339). Determinism asserted by `generated_sdk_is_byte_identical_across_two_runs` (PASS) and the Target's byte-identical test. |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/gnr8-core/src/tssdk/bundle.rs` | SdkBundle/SdkFile framing, `MARKER_PREFIX` | ✓ VERIFIED | Present (6957 B); `// ==== gnr8:file ` marker; tests in `#[cfg(test)]` (line 101). |
| `crates/gnr8-core/src/tssdk/emit.rs` | exhaustive `ts_type` + 5 emitters + escaping helpers | ✓ VERIFIED | Present (75 KB); `ts_type`, `ts_primitive`, `ts_string_literal` (CR-01), `is_ident` (CR-02), `emit_models/errors/client/operations/index`. No `_=>` in `ts_type`. |
| `crates/gnr8-core/src/tssdk/mod.rs` | `generate`+`split_bundle`+`write_to_dir` path guard | ✓ VERIFIED | Present (12.5 KB); fixed file order client/errors/index/models; path-safety guard; tests in `#[cfg(test)]` (line 117). |
| `crates/gnr8-core/src/lib.rs` | `pub mod tssdk;` | ✓ VERIFIED | Line 18. |
| `crates/gnr8-core/src/sdk/builtins.rs` | `struct TsSdk` + `impl Target` | ✓ VERIFIED | Lines 655–740; wired to `crate::tssdk::`; `sdk_package`/`ir.base_path`. |
| `crates/gnr8-core/src/sdk/mod.rs` | `TsSdk` in prelude | ✓ VERIFIED | Line 339 (alphabetized re-export). |
| `crates/gnr8-core/tests/tssdk_compile.rs` | hermetic tsc typecheck + banned-import grep + skip guard | ✓ VERIFIED | Present (11.8 KB); `es2022,dom`, vendored tsc path, discrete `Command` args, `TypeScriptToolchainMissing`/`GoBuild`. 3/3 tests pass. |
| `Makefile` | `tssdk_compile` in check gate | ✓ VERIFIED | `cargo test ... --test tssdk_compile` on the `test` line; `check` depends on `test`. `make check` exit 0. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| tssdk/mod.rs | tssdk/emit.rs | generate calls emit_* | ✓ WIRED | `emit::emit_client/operations/errors/index/models` invoked in fixed order. |
| tssdk/emit.rs | graph::Type | exhaustive `ts_type` match | ✓ WIRED | All 9 variants explicit (173–220), no catch-all. |
| sdk/builtins.rs | crate::tssdk | TsSdk::generate calls generate+split_bundle | ✓ WIRED | builtins.rs:711,713. |
| sdk/builtins.rs | sdk_package + ir.base_path | single source of truth | ✓ WIRED | builtins.rs:710 / 711 (no re-derivation, rule 3). |
| tssdk_compile.rs | tssdk::generate + write_to_dir | build_graph→generate→write | ✓ WIRED | materialize_sdk (lines 133–140). |
| tssdk_compile.rs | vendored tsc | discrete `node <tsc> --noEmit ...` | ✓ WIRED | `node_modules/typescript/bin/tsc`; `--lib es2022,dom` present. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| Generated `models.ts` | interfaces/enums/unions | `nestjs-bookstore` IR via `build_graph` → `tssdk::generate` | Yes — 9 real schemas emitted (BookDto, BookFormat, BookOrError, etc.) | ✓ FLOWING |
| Generated `client.ts` | operation methods | IR operations → `emit_operations` | Yes — `async listBooks(...)`, `createBook(...)` with real path/query/body | ✓ FLOWING |
| Generated SDK imports | import statements | emitter templates | Only relative `./errors` / `./models` — zero third-party | ✓ FLOWING (dependency-free) |

Independent materialization (throwaway test, since removed) wrote a real 4-file SDK; banned-import grep over the actual files confirmed zero `axios`/`node-fetch`/`@types`/`from "http"`.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Generated SDK type-checks via vendored tsc | `cargo test -p gnr8-core --test tssdk_compile` | 3 passed; 0 failed (incl. tsc --noEmit exit 0) | ✓ PASS |
| Full gnr8-core suite green | `cargo test -p gnr8-core` | all test results `0 failed`; 43 tssdk-named tests pass | ✓ PASS |
| Green gate | `make check` | exit 0 (Rust + clippy + Go fixtures + goextract) | ✓ PASS |
| No production unwrap/expect/panic; lint clean | `cargo clippy -p gnr8-core --all-targets -- -D warnings` | exit 0 | ✓ PASS |
| Real SDK generated from IR | independent dump test → /tmp | 4 files (client/errors/index/models.ts) with real content | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| TSSDK-01 | 05-01 | Dependency-free TS SDK (fetch, interface models, literal-union enums, ApiError, Client) | ✓ SATISFIED | tssdk module + real generated output (Truth 1). |
| TSSDK-02 | 05-03 | Generated TS SDK type-checks (`tsc --noEmit`) in hermetic test | ✓ SATISFIED | tssdk_compile 3/3 PASS + independent banned-import grep (Truth 2). |
| TSSDK-03 | 05-02 | `TsSdk` Target built-in; deterministic | ✓ SATISFIED | TsSdk + prelude + determinism tests (Truth 3). |

No orphaned requirements: REQUIREMENTS.md maps exactly TSSDK-01/02/03 to Phase 5; all claimed by plans and verified.

### CLAUDE.md Invariant Checks

| Invariant | Status | Evidence |
|-----------|--------|----------|
| Rule 2 — zero new gnr8-core crates | ✓ | `git diff $PHASE_START..HEAD` over all Cargo.toml/Cargo.lock: empty. |
| Rule 2 — typescript vendored, no new install | ✓ | No package.json/package-lock/node_modules diff across phase. |
| No production unwrap/expect/panic in tssdk | ✓ | All unwrap/expect/panic inside `#[cfg(test)]` (bundle 101, mod 117, emit 877). |
| Deterministic byte-identical output | ✓ | Two-run byte-identical tests pass; fixed file order. |
| Rule 3 — one source per fact | ✓ | package via `sdk_package`, base via `ir.base_path`; no fallback. |
| Acceptance snapshots untouched | ✓ | No `.snap` changes across phase; all multi-language snapshots green. |

### ROADMAP Integrity

All 6 `### Phase` headings present (1–5 complete, 6 pending). The phase-4 planner truncation bug is not present — 6 phases confirmed. (Note: the ROADMAP Progress *table* at line ~227 lists Phase 4 as "Not started 0/TBD", inconsistent with the traceability table marking TSSRC-01..04 Complete — a pre-existing bookkeeping discrepancy unrelated to Phase 5's goal, outside this verification's scope.)

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | — | No TBD/FIXME/XXX; all unwrap/expect/panic test-scoped; no banned imports in generated output | ℹ️ Info | None |

### Documented Deferrals (Not Regressions)

Two code-review warnings deferred in `05-REVIEW-FIX.md`, present in code exactly as documented — they were never claimed fixed and are NOT part of the phase success criteria:

- **WR-02** — non-scalar query params stringified via `String(value)` (emit.rs:792,796). Array → `"a,b"`, object → `"[object Object]"`. Backlog candidate (query-serialization design decision).
- **WR-04** — asymmetric success/error JSON decode: error path has `.catch(() => null)`, success path `(await res.json()) as models.X` does not (emit.rs:834,839). Backlog candidate (error-contract design decision).

Both are minor (non-goal-blocking): the generated SDK still type-checks and is dependency-free. Recommended for backlog / Phase 6 hardening, not a phase-goal blocker.

### Human Verification Required

None. The load-bearing typecheck is fully automated (node + vendored tsc present) and ran green in-process; the dependency-free claim is grep-proven over real output. No visual/UX/external-service surface.

### Gaps Summary

No gaps. All 3 ROADMAP success criteria are observably true in the codebase, verified against real generated output rather than SUMMARY claims:
- The `tssdk` module emits a dependency-free 4-file TS SDK with exhaustive type mapping.
- The hermetic `tsc --noEmit` gate passes (exit 0) and the supply-chain grep is clean — proven by running the tests in-process and by independently materializing + grepping the real SDK.
- The `TsSdk` Target is wired into the `.gnr8/` Pipeline seam (prelude-exported), deterministic, with typed errors and a path-safety guard.

CLAUDE.md invariants hold (zero new crates, vendored typescript untouched, no production panics, byte-identical output, snapshots unchanged). `make check` and `cargo clippy -D warnings` both exit 0.

---

_Verified: 2026-06-26T00:40:00Z_
_Verifier: Claude (gsd-verifier)_
