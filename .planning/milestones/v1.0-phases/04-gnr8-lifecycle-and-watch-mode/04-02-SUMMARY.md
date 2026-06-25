---
phase: 04-gnr8-lifecycle-and-watch-mode
plan: 02
subsystem: core
tags: [manifest, blake3, ownership, no-op, plan_writes, naming-overrides, cli, generate, check]

# Dependency graph
requires:
  - phase: 04-gnr8-lifecycle-and-watch-mode
    plan: 01
    provides: "typed Config (inputs/output/naming), CoreError::Manifest/Io variants, blake3 pinned in workspace deps, tests/lifecycle.rs to extend"
  - phase: 03-openapi-and-go-sdk-generation
    provides: "deterministic pipeline (build_graph -> to_openapi + sdk::generate/write_to_dir) the lifecycle wraps; sdk bundle framing"
provides:
  - "gnr8_core::manifest::{Manifest, ManifestEntry, blake3_hex, load} — blake3 path->hash ownership record, graceful absent/corrupt degradation"
  - "gnr8_core::lifecycle::{plan_writes, WriteAction, WritePlan, PlannedFile, apply_writes, GenerateOutcome, apply_naming, regenerate, plan_only} — the pure write-decision truth table + impure shell + orchestrators"
  - "gnr8 generate (+ --force) and gnr8 check wired to the real deterministic pipeline (no silent clobber + no-op=no write + dry-run drift gate)"
  - "gnr8_core::sdk::split_bundle — pub(crate) bundle-split accessor for per-file output enumeration"
affects: [04-03-watch-latency]

# Tech tracking
tech-stack:
  added: [blake3 1.8 (now consumed in gnr8-core)]
  patterns:
    - "pure decision / thin shell split: plan_writes is I/O-free (on-disk bytes injected via &dyn Fn closure) so the full truth table is filesystem-free unit-testable (RESEARCH Pattern 2/Pitfall 3)"
    - "--force policy lives ONLY in apply_writes, not plan_writes — classification stays pure, override decision stays in one place"
    - "path-traversal guard on user-config output paths via std::path::Component inspection (reject ParentDir/RootDir/Prefix) -> CoreError::Io (T-04-02-01)"
    - "manifest degrades safely: absent -> empty default, corrupt -> empty default (regenerate-from-scratch), only real read I/O error -> typed CoreError::Manifest (T-04-02-03)"
    - "naming type-rename rewrites schema id+name AND every matching SchemaRef.ref_id so no $ref dangles (PLAN-CHECK W2)"

key-files:
  created:
    - crates/gnr8-core/src/manifest/mod.rs
    - crates/gnr8-core/src/lifecycle/mod.rs
  modified:
    - crates/gnr8-core/Cargo.toml
    - crates/gnr8-core/src/lib.rs
    - crates/gnr8-core/src/sdk/mod.rs
    - crates/gnr8-core/tests/lifecycle.rs
    - crates/gnr8/src/cli.rs
    - crates/gnr8/src/main.rs
    - Cargo.lock

key-decisions:
  - "plan_writes is PURE (on_disk injected via &dyn Fn closure, --force NOT applied here); the full five-arm truth table — including (present-on-disk, absent-from-manifest) => UserEdited — is unit-tested without a filesystem via a mock closure."
  - "apply_writes takes project_root (extends the plan's 3-arg signature) so it can resolve + path-validate user-config output paths against the root; a deviation justified by the T-04-02-01 traversal mitigation (Rule 2)."
  - "naming.types matches a schema by id OR bare name; on a match BOTH id and name become the new value and EVERY SchemaRef.ref_id / field SchemaType.ref_id pointing at the old id is rewritten — keeping id==name==new value so the component key, resolved $ref name, and references all stay consistent (no dangling $ref, PLAN-CHECK W2)."
  - "lifecycle::plan_only added in this plan (not deferred to Task 3) as the shared graph->outputs->plan_writes seam; gnr8 check reuses it for a clean dry-run with zero new policy."

requirements-completed: [WS-03, WS-04, WATCH-01]

# Metrics
duration: 10 min
completed: 2026-06-24
---

# Phase 4 Plan 02: Ownership Manifest, No-Op Generation & Pipeline Wiring Summary

**A blake3-hashed ownership manifest plus a PURE `plan_writes` truth table now drive `gnr8 generate`/`gnr8 check` through the real Phase-3 pipeline, delivering the two headline guarantees — no silent clobbering (a hand-edited generated file is warned + skipped unless `--force`) and no-op = no write (a byte-identical second generate touches zero files and zero mtimes) — with naming overrides that rename referenced types without dangling any `$ref`.**

## Performance

- **Duration:** 10 min
- **Started:** 2026-06-24T20:49:57Z
- **Completed:** 2026-06-24T21:00:35Z
- **Tasks:** 3 (Task 1 + Task 2 are TDD: RED -> GREEN; Task 3 auto)
- **Files:** 9 (2 created, 7 modified)

## Accomplishments

- **Ownership manifest (WS-04):** `manifest::{Manifest, ManifestEntry}` maps each generated output path -> its blake3 content hash + provenance (`"openapi"`/`"sdk"`), persisted to `.gnr8/cache/manifest.json` sorted by path (deterministic diff). `blake3_hex` is a stable 64-char lowercase fingerprint (NOT std `DefaultHasher`). `load` degrades safely: absent -> empty default, corrupt -> empty default (regenerate-from-scratch, never panics), only a real read I/O error -> `CoreError::Manifest`. `record`/`recorded_hash`/`prune_to` round-trip and prune dropped paths (D-04). Five named tests green, fully hermetic.
- **Pure `plan_writes` truth table (WS-04 + WATCH-01):** the I/O-free decision function classifies all five arms — absent->Write, owned+identical->Unchanged, owned+changed->Write, owned+hash-mismatch->UserEdited, present-but-untracked->UserEdited (Pitfall 5). `plan_writes_truth_table` exercises every arm with a mock `on_disk` closure and a hand-built manifest — **no filesystem**.
- **No silent clobbering + no-op (the two headlines):** `apply_writes` writes only `Write` (and `UserEdited` under `force`), skips `Unchanged` (no write, no mtime churn) and `UserEdited` (warn+skip), records hashes, prunes dropped paths. `user_edit_is_protected`, `force_overwrites_user_edit`, `untracked_output_protected`, `noop_second_run_writes_nothing`, and `noop_preserves_mtime` each pass.
- **Naming overrides (WS-03) incl. the W2 $ref fix:** `apply_naming` remaps operation ids and renames referenced types (id + name + every `$ref`). `naming_type_rename_updates_refs_no_dangling` renames a REFERENCED schema (`CreateGoalInput`) and asserts `to_openapi` SUCCEEDS with the new name in `components.schemas` AND the operation's `$ref` updated — no dangling `$ref`, no `CoreError::Lowering`.
- **Path-traversal hardening (T-04-02-01):** `apply_writes` resolves each user-config output path against the project root and rejects `..`/absolute/root-prefixed paths -> `CoreError::Io`. Two unit tests cover the guard.
- **CLI wired to the real pipeline:** `gnr8 generate [--force]` regenerates, writes only changed files, warns on stderr per protected file (T-04-02-04 visibility), and reports `<n> written, <m> unchanged, <k> skipped`; `--json` serializes the counts. `gnr8 check` dry-runs `plan_only` and exits non-zero (code 1) on stale (`Write`) or drifted (`UserEdited`) outputs, 0 when all `Unchanged`. Manually verified end-to-end on the goalservice fixture.

## Task Commits

Each task committed atomically (tagged `04-02`):

1. **Task 1 (TDD): blake3-hashed ownership manifest** — `bebd53a` (test, RED) -> `75acae2` (feat, GREEN)
2. **Task 2 (TDD): pure plan_writes + apply_writes + naming + regenerate** — `904a686` (test, RED) -> `740b509` (feat, GREEN)
3. **Task 3 (auto): wire `gnr8 generate`/`check` CLI arms** — `54e0e5c` (feat)

**Plan metadata:** committed separately (docs: complete plan) after this SUMMARY.

## Seam names for 04-03 (watch loop)

- **Regeneration entry point:** `gnr8_core::lifecycle::regenerate(project_root: &Path, config: &Config, force: bool) -> Result<GenerateOutcome, CoreError>` — the watch loop times this call (WATCH-03 latency).
- **Dry-run seam:** `gnr8_core::lifecycle::plan_only(project_root: &Path, config: &Config) -> Result<WritePlan, CoreError>` — builds the plan without writing (`gnr8 check` uses it; the watch loop can use it to detect "nothing to do").
- **Pure decision:** `plan_writes(new_outputs: &[(String, Vec<u8>)], manifest: &Manifest, on_disk: &dyn Fn(&str) -> Option<Vec<u8>>) -> WritePlan` — re-used by every regeneration.
- **GenerateOutcome shape (serde::Serialize):** `{ written: Vec<String>, unchanged: Vec<String>, skipped: Vec<String> }` — 04-03 serializes it for the `--json` latency report.
- **Schema-id renaming:** YES, a referenced-type rename DOES require `$ref` updates — `apply_naming` rewrites `SchemaRef.ref_id` (operation request_body/response.body) and field-level `SchemaType.ref_id` (recursing into array items) so `to_openapi` never dangles (PLAN-CHECK W2 handled).
- **Output-path filter source:** the manifest's `files[].path` set + `config.output.{openapi,sdk_dir}` are the output paths the watch loop must filter out of the event stream to stay loop-safe (WATCH-02).

## Files Created/Modified

- `crates/gnr8-core/src/manifest/mod.rs` (created, 210 lines) — `Manifest`/`ManifestEntry`, `blake3_hex`, `record`/`recorded_hash`/`prune_to`/`save`/`load`, graceful degradation.
- `crates/gnr8-core/src/lifecycle/mod.rs` (created, 483 lines) — `WriteAction`/`PlannedFile`/`WritePlan` (+ `has_drift`), pure `plan_writes`, `apply_writes` (+ `safe_output_path` traversal guard), `apply_naming` (+ `rewrite_schema_type_ref`), `build_outputs`, `regenerate`, `plan_only`, `GenerateOutcome`.
- `crates/gnr8-core/Cargo.toml` (modified) — added `blake3 = { workspace = true }` (first consumer, PLAN-CHECK W1).
- `crates/gnr8-core/src/lib.rs` (modified) — registered `pub mod lifecycle; pub mod manifest;`.
- `crates/gnr8-core/src/sdk/mod.rs` (modified) — added `pub(crate) fn split_bundle` (bundle-split accessor for lifecycle).
- `crates/gnr8-core/tests/lifecycle.rs` (modified) — appended 12 named tests (5 manifest + 7 lifecycle incl. the W2 type-rename test) + a `scaffold_project` helper.
- `crates/gnr8/src/cli.rs` (modified) — `Generate { force: bool }` + parse test update + `--force` assertion.
- `crates/gnr8/src/main.rs` (modified) — `run_generate`/`run_check`, `project_paths`, `LifecycleReport`; `dispatch` no longer routes generate/check.
- `Cargo.lock` (modified) — locked `blake3` + its small dep tree (`arrayref`, `arrayvec`, `constant_time_eq`).

## Decisions Made

- **`apply_writes` signature widened to take `project_root`.** The plan's `<action>` sketched `apply_writes(plan, manifest, force)`, but the T-04-02-01 path-traversal mitigation requires resolving each user-config output path against the project root. Added `project_root: &Path` as the first parameter (a Rule-2 deviation; documented below).
- **`plan_only` added in Task 2, not deferred to Task 3.** Factoring the shared `build_outputs` seam into core made `gnr8 check` a thin CLI wrapper with zero duplicated pipeline logic (RESEARCH Open Q 3 — reuse the exact pure decision).
- **`naming.types` matches by id OR bare name.** The fixture's schema `id` is package-qualified (`internal/.../dto.CreateGoalInput`) while the documented config example uses the bare type name (`CreateGoalInput = "NewGoal"`); matching either makes the documented knob actually usable. On a match the rename keeps `id == name == new value` and rewrites all refs.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] `apply_writes` widened to accept `project_root` for path-traversal validation**
- **Found during:** Task 2
- **Issue:** The plan's example signature `apply_writes(plan, manifest, force)` has no project root, but the threat register's `mitigate` disposition for T-04-02-01 requires resolving user-config output paths against the project root and rejecting escapes — impossible without the root.
- **Fix:** Added `project_root: &Path` as the first parameter and a `safe_output_path` helper that inspects `std::path::Component`s, rejecting `ParentDir`/`RootDir`/`Prefix` (and empty paths) with `CoreError::Io`. `regenerate`/`plan_only` pass the root through.
- **Files modified:** crates/gnr8-core/src/lifecycle/mod.rs
- **Verification:** `safe_output_path_rejects_traversal_and_absolute` + `apply_writes_rejects_a_traversal_output_path` unit tests pass; clippy clean.
- **Commit:** `740b509`

**2. [Rule 3 - Blocking] clippy pedantic `match_same_arms`, `assigning_clones`, `uninlined_format_args` denied under `-D warnings`**
- **Found during:** Task 2 (clippy gate)
- **Issue:** `cargo clippy --all-targets --locked -- -D warnings` flagged: the `plan_writes` truth-table match (arms 1 and 3 share the `Write` body), four `x = y.clone()` assignments in `apply_naming`, and a non-inlined `format!` arg in a test helper.
- **Fix:** Scoped `#[allow(clippy::match_same_arms)]` on the truth-table match with a comment (the arms are deliberately distinct, separately-documented cases); switched the four assignments to `clone_from`; inlined the `format!` arg. No behavior change.
- **Files modified:** crates/gnr8-core/src/lifecycle/mod.rs, crates/gnr8-core/tests/lifecycle.rs
- **Verification:** `cargo clippy --all-targets --locked -- -D warnings` clean.
- **Commit:** `740b509`

---

**Total deviations:** 2 auto-fixed (1 Rule 2 security-mitigation, 1 Rule 3 lint gate).
**Impact:** Both strengthen the implementation (the path-traversal guard is a planned threat mitigation; the lint fixes are no-behavior-change quality-gate satisfactions). All success criteria met as written.

## Authentication Gates

None — no external service, auth, or secrets involved.

## Issues Encountered

None. All gates green: `cargo test --workspace` (every suite 0 failed), `cargo clippy --all-targets --locked -- -D warnings` clean, `cargo build --locked` succeeds, `cargo fmt --all --check` clean.

## Next Phase Readiness

- **04-03 (watch + latency):** `regenerate`/`plan_only`/`GenerateOutcome` seams are stable and documented above; `notify-debouncer-full` is pinned (04-01); `CoreError::Io` is available for the watch shell; the manifest `files[].path` set + `config.output` paths are the loop-safety filter (WATCH-02). No blockers.

---

## Self-Check: PASSED

- Created files exist on disk: `crates/gnr8-core/src/manifest/mod.rs`, `crates/gnr8-core/src/lifecycle/mod.rs`, `04-02-SUMMARY.md` — all FOUND.
- Task commits exist: `bebd53a`, `75acae2`, `904a686`, `740b509`, `54e0e5c` — all FOUND.
- Gates green: `cargo test -p gnr8-core --test lifecycle` (19 passed), `cargo test -p gnr8` (9 passed), `cargo test --workspace` (all suites 0 failed), `cargo clippy --all-targets --locked -- -D warnings` clean, `cargo build --locked` succeeds, `cargo fmt --all --check` clean.
- Manual e2e verified on the goalservice fixture: cold=5 written, warm=0 written/5 unchanged (mtime preserved), hand-edit -> warn+skip (edit survives), `gnr8 check` exit 1 on drift, `--force` overwrites, `gnr8 check` exit 0 when clean.

---
*Phase: 04-gnr8-lifecycle-and-watch-mode*
*Completed: 2026-06-24*
