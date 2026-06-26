---
phase: 05-typescript-target-tssdk
plan: 01
subsystem: api
tags: [typescript, sdk, codegen, fetch, ir, dependency-free]

# Dependency graph
requires:
  - phase: 01-language-neutral-ir
    provides: the neutral ApiGraph IR + Type/Prim/WellKnown enum (the source ts_type maps exhaustively)
  - phase: 03-python-target-pysdk
    provides: the pysdk module (bundle/emit/mod) cloned structurally as the tssdk twin
provides:
  - "crate::tssdk module (bundle.rs, emit.rs, mod.rs) registered via pub mod tssdk"
  - "tssdk::generate(graph, package, base_path) -> deterministic 4-file TS bundle String"
  - "tssdk::write_to_dir + split_bundle (path-safety guard) for the Plan-02 Target + Plan-03 test"
  - "ts_type: exhaustive IR Type -> TypeScript mapping (no _ => catch-all)"
affects: [05-02 TsSdk Target wiring, 05-03 hermetic tsc typecheck test]

# Tech tracking
tech-stack:
  added: []  # zero new crates (CLAUDE.md rule 2); generated SDK is dependency-free (fetch global)
  patterns:
    - "IR->string SDK emitter as a structural clone of the pysdk twin, MINUS Python-only workarounds"
    - "Exhaustive per-target Type match (no _ =>) so a future IR variant fails to compile (rule 3)"
    - "Two independent field axes: optional -> ?: at the field site, nullable -> | null in ts_type"

key-files:
  created:
    - crates/gnr8-core/src/tssdk/bundle.rs
    - crates/gnr8-core/src/tssdk/emit.rs
    - crates/gnr8-core/src/tssdk/mod.rs
  modified:
    - crates/gnr8-core/src/lib.rs

key-decisions:
  - "[05-01] tssdk is a structural clone of pysdk MINUS the four Python-only workarounds (required-first field partition, from __future__ header, PEP-484 forward-ref aliases, f-string safe='' trick) — TS ?: is order-free, type aliases are order-independent, template literals have no backslash restriction"
  - "[05-01] ts_type maps Map{value} to the stricter typed Record<string, ts_type(value)> (Open Q1) rather than widening to Record<string, unknown>; Any -> unknown; WellKnown -> string"
  - "[05-01] named enum body -> export type X = \"a\" | \"b\" (string-literal union, no member identifiers to sanitize); named union/alias -> plain order-free export type X = A | B; inline Object -> typed CoreError::SdkGen (parity with Go/Python targets)"
  - "[05-01] generated Client uses the platform fetch global + an injectable fetch? transport (typeof fetch); typed body/response referenced through a models namespace import so client.ts has no per-name import to compute (determinism)"
  - "[05-01] FIXED alpha frame order client.ts/errors.ts/index.ts/models.ts; path params via encodeURIComponent(String(x)), query via URLSearchParams (required always set, optional guarded by !== undefined)"

patterns-established:
  - "Pattern 1: exhaustive ts_type/ts_primitive match with scoped #[allow(clippy::match_same_arms)] to preserve one-arm-per-variant exhaustiveness even when bodies coincide (Int/Float->number)"
  - "Pattern 2: emit_operation split into emit_op_path/emit_op_query/emit_op_dispatch helpers to keep each function under the 100-line clippy threshold"

requirements-completed: [TSSDK-01]

# Metrics
duration: 18min
completed: 2026-06-25
---

# Phase 05 Plan 01: TypeScript SDK Emitter (tssdk module) Summary

**Dependency-free IR->TypeScript SDK emitter: interface models, string-literal-union enums, a fetch-based Client with an injectable transport, and a typed ApiError extends Error — a structural clone of the pysdk twin with an exhaustive ts_type mapping and zero new crates.**

## Performance

- **Duration:** ~18 min
- **Started:** 2026-06-25T23:50:00Z (approx)
- **Completed:** 2026-06-25
- **Tasks:** 3
- **Files modified:** 4 (3 created, 1 modified)

## Accomplishments
- `crate::tssdk` module created and registered (`pub mod tssdk;` in lib.rs) — bundle framing, the five emitters, and the generate/split_bundle/write_to_dir orchestration.
- `ts_type` maps every IR `Type` variant exhaustively (no `_ =>`): Primitive->string/number/boolean, WellKnown->string, Array->T[], Map->Record<string,V>, Named->resolved name (dangling->SdkGen), Enum->literal union, Union->A|B, Any->unknown, inline Object->typed SdkGen error.
- Generated TS is dependency-free: `interface` models (graph order, no required-first partition), `export type` literal-union enums, order-free `export type` aliases (no forward-ref hack), a `fetch`-global Client with an injectable `fetch?` transport, and a typed `ApiError extends Error` with `isNotFound()`.
- Deterministic byte-identical 4-file emission in FIXED alpha order (client.ts, errors.ts, index.ts, models.ts); path-safety guard on write_to_dir; path params percent-escaped via encodeURIComponent, query via URLSearchParams.

## Task Commits

Each task was committed atomically:

1. **Task 1: Clone bundle.rs + register the module** - `43d98e8` (feat)
2. **Task 2: tssdk/emit.rs — exhaustive ts_type + the five emitters** - `451904b` (feat)
3. **Task 3: tssdk/mod.rs — generate + split_bundle + write_to_dir** - `5c13ee9` (feat)

_Tasks 2 and 3 were marked tdd="true"; the analog tests were ported alongside the production code (clone-and-adapt) and verified green at each step, with clippy `-D warnings` reconciled in the Task 3 commit (where the module first compiles fully wired)._

## Files Created/Modified
- `crates/gnr8-core/src/tssdk/bundle.rs` - SdkFile/SdkBundle framing + parse (TS-relabeled verbatim clone of pysdk/bundle.rs; the `//` marker is valid TS).
- `crates/gnr8-core/src/tssdk/emit.rs` - ts_type/ts_primitive + emit_models/emit_errors/emit_client/emit_operations/emit_index + camel casing + the join_path/success_of/body_model_of/path_tokens helpers + the path/query/dispatch sub-emitters.
- `crates/gnr8-core/src/tssdk/mod.rs` - generate (FIXED 4-file order), split_bundle (pub(crate)), write_to_dir (path-safety guard), and the mod tests.
- `crates/gnr8-core/src/lib.rs` - `pub mod tssdk;` registration (alpha order, after `pub mod sdk;`).

## Decisions Made
See key-decisions frontmatter. Headline: tssdk is a structural clone of pysdk that deliberately DROPS the four Python-only workarounds (RESEARCH Pitfall 6), uses the typed `Record<string, V>` map mapping (Open Q1), and references models through a namespace import in client.ts for determinism.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Scoped clippy allows + a refactor to satisfy `-D warnings`**
- **Found during:** Task 3 (full module wiring)
- **Issue:** `cargo clippy -p gnr8-core -- -D warnings` flagged five lib-level issues that block the verification gate: `match_same_arms` on `ts_primitive` (Int/Float both `number`, String/Bytes both `string`), `string_extend_chars` in `camel`, `dead_code` on `split_bundle` (its in-crate caller — the TsSdk Target — lands in Plan 02), and `too_many_lines` on `emit_operation` (110/100).
- **Fix:** (a) added a scoped `#[allow(clippy::match_same_arms)]` on `ts_primitive` with a comment justifying the deliberate one-arm-per-`Prim` exhaustiveness (mirrors the ts_type rule-3 discipline); (b) replaced `.extend(chars)` with `.push(char)` in `camel`; (c) added a scoped `#[allow(dead_code)]` on `split_bundle` documenting the Plan-02 caller; (d) extracted `emit_op_path`/`emit_op_query`/`emit_op_dispatch` helpers out of `emit_operation`.
- **Files modified:** crates/gnr8-core/src/tssdk/emit.rs, crates/gnr8-core/src/tssdk/mod.rs
- **Verification:** `cargo clippy -p gnr8-core --all-targets -- -D warnings` exits 0.
- **Committed in:** `5c13ee9` (Task 3 commit)

---

**Total deviations:** 1 auto-fixed (Rule 3 - blocking)
**Impact on plan:** The refactor and scoped allows are required to pass the plan's own clippy `-D warnings` verification gate; no behavior change, no scope creep. The exhaustive-match property and the `split_bundle` visibility are preserved (allows are scoped + documented, not blanket).

## Issues Encountered
- The `cargo test ... tssdk::bundle` filter initially appeared to match 0 tests because the filter also ran the integration-test binaries (which have no matching tests); `cargo test -p gnr8-core --lib tssdk` confirmed all unit tests run and pass. No code issue.

## User Setup Required
None - no external service configuration required. (The hermetic `tsc` typecheck arrives in Plan 03; this plan is pure string emission with no toolchain.)

## Next Phase Readiness
- `tssdk::generate` / `write_to_dir` / `split_bundle` are ready for Plan 02 to wire a `TsSdk` Target into `sdk::builtins` (reusing `sdk_package` + `ir.base_path` verbatim) and the `prelude` re-export.
- Plan 03 can drive the hermetic `tsc --noEmit --strict --lib es2022,dom` typecheck over the emitted SDK (the `models` namespace import + `typeof fetch` are typecheck-ready; `--lib dom` is load-bearing for the `fetch` global).
- No blockers.

## Self-Check: PASSED
- FOUND: crates/gnr8-core/src/tssdk/bundle.rs
- FOUND: crates/gnr8-core/src/tssdk/emit.rs
- FOUND: crates/gnr8-core/src/tssdk/mod.rs
- FOUND: `pub mod tssdk;` in crates/gnr8-core/src/lib.rs
- FOUND commit 43d98e8 (Task 1)
- FOUND commit 451904b (Task 2)
- FOUND commit 5c13ee9 (Task 3)

---
*Phase: 05-typescript-target-tssdk*
*Completed: 2026-06-25*
