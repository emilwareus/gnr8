---
phase: 05-typescript-target-tssdk
fixed_at: 2026-06-26T00:00:00Z
review_path: .planning/phases/05-typescript-target-tssdk/05-REVIEW.md
iteration: 1
findings_in_scope: 7
fixed: 5
skipped: 2
status: partial
---

# Phase 5: Code Review Fix Report

**Fixed at:** 2026-06-26
**Source review:** .planning/phases/05-typescript-target-tssdk/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope (critical + warning): 7
- Fixed: 5
- Skipped: 2 (both documented as scope-expanding / behavior-changing)

**Acceptance gate (all green):**
- `cargo test -p gnr8-core --test tssdk_compile` — PASS (generated SDK still typechecks under `tsc --noEmit --strict --lib es2022,dom`).
- `make check` — exit 0 (GREEN). All 6 multi-language acceptance snapshots green with **zero `.snap` edits** (verified `git diff --name-only HEAD~5` lists no `.snap`).
- Determinism two-run byte-identical test — PASS.
- New regression unit tests added for CR-01 (enum member with `"`/`\`/newline), CR-02 (kebab-case / leading-digit / spaced `json_name`), and WR-01 (empty camel identifier) — all pass.
- `cargo clippy -p gnr8-core --all-targets` — clean (RUST-04 no-unwrap/expect/panic in production preserved).

## Fixed Issues

### CR-01: Enum / string-literal-union members emitted unescaped

**Files modified:** `crates/gnr8-core/src/tssdk/emit.rs`
**Commit:** c15bed7 (bundled with CR-02 — both depend on the shared `ts_string_literal` helper)
**Status:** fixed
**Applied fix:** Added a single deterministic `ts_string_literal(&str) -> String` helper that escapes `\`, `"`, `\n`, `\r`, `\t`, and any remaining C0 control char (`\uXXXX`, lower-hex) and wraps the value in double quotes. Wired it into BOTH string-literal sites: the inline-enum arm of `ts_type` and the named-enum alias in `emit_enum_alias` (replacing the raw `format!("\"{m}\"")`). The wire value is now preserved exactly — an embedded `"` no longer breaks `tsc`, and an embedded `\b` no longer silently corrupts the literal type. Updated the `emit_enum_alias` doc comment that previously conflated identifier escaping with string-literal escaping.

### CR-02: Object field property names emitted unquoted

**Files modified:** `crates/gnr8-core/src/tssdk/emit.rs`
**Commit:** c15bed7 (bundled with CR-01 — shares the `ts_string_literal` helper)
**Status:** fixed
**Applied fix:** Added `is_ident(&str) -> bool` (non-empty; first char `A-Za-z_$`; rest `A-Za-z0-9_$`). In `emit_interface`, a `json_name` that is a bare identifier stays unquoted (happy path byte-identical → no snapshot change); any other wire key is emitted as a quoted + escaped string-literal member via `ts_string_literal`, with the `?`/`:` kept OUTSIDE the quotes. One deterministic rule, no fallback (rule 3). Reserved words are intentionally left bare (TS permits them as member names).

### WR-01: Empty camelCase param identifier produced invalid TS

**Files modified:** `crates/gnr8-core/src/tssdk/emit.rs`
**Commit:** 361bfc9
**Status:** fixed
**Applied fix:** In `resolve_op_args`'s `reserve` closure, reject a param whose `camel(name)` is empty (e.g. a name of `"_"`/`"-"`) with a typed `CoreError::SdkGen` instead of emitting `: T` with no binding name. One deterministic check; the collision pass already handled the clash case. Added a regression test.

### WR-03: `success_of` comment ("lowest 2xx") vs code ("first in order") discrepancy

**Files modified:** `crates/gnr8-core/src/tssdk/emit.rs`, `crates/gnr8-core/src/pysdk/emit.rs`
**Commit:** 3cbde10
**Status:** fixed
**Applied fix:** Verified the upstream invariant: `graph/mod.rs` runs `responses.sort_by_key(|r| r.status)` at build time, locked by the `responses_sorted_by_status` test. Therefore "first 2xx in iteration order" IS "lowest 2xx" — the code is correct. Resolved the discrepancy by clarifying the doc comment to state the relied-upon single ordering rather than introducing a second `min_by_key` sort (rule 3: one source of truth, no redundant sort path). Applied the identical doc clarification to the `pysdk::emit::success_of` twin per the review's cross-check note. No behavioral/snapshot change.

### WR-05: `emit_index` re-exports without sharing the name-uniqueness check

**Files modified:** `crates/gnr8-core/src/tssdk/emit.rs`, `crates/gnr8-core/src/tssdk/mod.rs`
**Commit:** cabae09
**Status:** fixed
**Applied fix:** Extracted the duplicate-schema-name guard out of `emit_models` into a shared `check_unique_schema_names(graph) -> Result<(), CoreError>` and called it from BOTH `emit_models` and `emit_index`, so the two passes can never drift on which names are legal regardless of `generate`'s call order (`emit_index` runs first). `emit_index` now returns `Result` and is `?`-propagated at the `mod.rs` call site; the `index_reexports_...` test was updated to `.unwrap()`. The identifier-VALIDATION portion of the WR-05 suggestion (rejecting a non-identifier *schema name*) was deliberately scoped out — see Skipped Issues. No snapshot change (valid graphs have unique, identifier-safe schema names).

## Skipped Issues

### WR-02: Non-string query params stringified with `String(value)`

**File:** `crates/gnr8-core/src/tssdk/emit.rs:705,709` (`emit_op_query`)
**Reason:** skipped — would change runtime query-serialization behavior and expand scope. The fix requires a design decision (constrain query params to scalars with a typed schema-level error, OR special-case arrays to repeated-key `append` with a chosen encoding). Both alter emitted client behavior and risk touching the green acceptance snapshots; choosing a wire encoding is a product decision beyond a review-fix pass. The prompt explicitly directs skipping warnings that expand scope or change green snapshots.
**Original issue:** Every query param is serialized via `query.set("k", String(ident))` regardless of declared schema type; an array param yields `"a,b"` and an object/map yields `"[object Object]"`, while the signature advertises the richer `ts_type`.

### WR-05 (identifier-validation sub-part only): non-identifier-safe *schema names*

**File:** `crates/gnr8-core/src/tssdk/emit.rs` (`emit_models` / `emit_index` symbol emission)
**Reason:** skipped — the uniqueness/drift half of WR-05 is fixed (see above); the remaining suggestion to validate/sanitize a schema *name* to a legal TS identifier is a distinct, larger design question (where the single validation point should live — the graph or a shared emitter helper — and whether to reject vs. sanitize), affecting Go/Python twins for consistency. That is out of scope for this fix pass and is not exercised by any fixture (no snapshot impact either way). Documented for a follow-up.
**Original issue:** A schema whose `name` is not identifier-safe (e.g. contains a hyphen) would emit invalid `export interface Foo-Bar` and an invalid bare re-export; neither pass validates schema names as legal TS identifiers.

### WR-04: Asymmetric error-vs-success JSON decode

**File:** `crates/gnr8-core/src/tssdk/emit.rs:745-749,752` (`emit_op_dispatch`)
**Reason:** skipped — the review's stated minimum is "note the asymmetry"; any code change (wrapping the success `res.json()` parse) alters the runtime error contract of every typed-return method and would change the acceptance snapshots. Deferred as a deliberate behavior decision (is the success body trusted, or should a malformed/empty body surface as a typed `ApiError`?). Noted here for the developer.
**Original issue:** The error path uses `await res.json().catch(() => null)` but the success path `(await res.json()) as models.X` has no `.catch`, so a malformed/empty success body throws a raw `SyntaxError` instead of an `ApiError` — an inconsistent error contract.

_Note: IN-01..IN-04 are Info-tier and out of the `critical_warning` scope; not attempted._

---

_Fixed: 2026-06-26_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
