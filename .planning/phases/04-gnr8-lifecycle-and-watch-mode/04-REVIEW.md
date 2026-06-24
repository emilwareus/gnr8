---
phase: 04-gnr8-lifecycle-and-watch-mode
reviewed: 2026-06-24T00:00:00Z
depth: deep
files_reviewed: 13
files_reviewed_list:
  - crates/gnr8-core/src/lifecycle/mod.rs
  - crates/gnr8-core/src/manifest/mod.rs
  - crates/gnr8-core/src/config/mod.rs
  - crates/gnr8-core/src/workspace/mod.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/lib.rs
  - crates/gnr8-core/src/sdk/mod.rs
  - crates/gnr8/src/watch.rs
  - crates/gnr8/src/main.rs
  - crates/gnr8/src/cli.rs
  - Cargo.toml
  - Makefile
  - .github/workflows/ci.yml
findings:
  critical: 0
  warning: 4
  info: 4
  total: 8
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-06-24
**Depth:** deep
**Files Reviewed:** 13
**Status:** issues_found

## Summary

Reviewed the Phase 4 `.gnr8` lifecycle and watch-mode diff (`2c68411..HEAD`, excluding the
unrelated `style(04)`/`docs(04)` commits). This is the subtlest state machine in the project and the
implementation is, on the whole, careful and correct: the three headline guarantees hold in the
actual code, the pure/impure split is real and exhaustively unit-tested, hashing uses blake3 (not
`DefaultHasher`), dependency pins are all stable (blake3 1.8.5, toml 1.1.2, notify-debouncer-full
0.7.0, ctrlc 3.5.2, notify 8.2.0 — no rc), path traversal is rejected, and there are no production
`unwrap`/`expect`/`panic` paths. WS-03 honesty is intact (no faked plugin field; the v2 seam lives in
prose only). The `ctrlc` deviation is idiomatic and confined to the binary boundary — not re-flagged.

I traced all three guarantees in code:

1. **No silent clobber** — `plan_writes` arm 4 (`blake3(disk) != recorded ⇒ UserEdited`) and arm 5
   (`present, not in manifest ⇒ UserEdited`); `apply_writes` writes `UserEdited` only under `force`,
   else routes to `skipped`; `run_generate`/`watch` warn per skipped path. Correct.
2. **No-op = no write** — `plan_writes` arm 2 (`disk == new_bytes ⇒ Unchanged`); `apply_writes`
   short-circuits before the `fs::write`. Holds because Phase 2-3 generation is byte-deterministic.
3. **Watch doesn't loop** — two layers: watch source dirs only, plus the pure filter dropping
   anything under the canonicalized output set (config anchors + manifest). Both verified.

The `plan_writes` truth table is sound: the five match arms are mutually exclusive and ordered so the
`UserEdited` guard (arm 4) precedes the `Unchanged` (arm 2) / `Write` (arm 3) arms, which is the
correct precedence — a hash mismatch must win over a content comparison.

No BLOCKERs. Findings below are robustness/maintainability gaps, the most material being a macOS
watch edge case (WR-01) and a same-batch chained-rename footgun (WR-02).

## Warnings

### WR-01: Deleted-output watch event can slip the loop filter on macOS and trigger one extra regen

**File:** `crates/gnr8/src/watch.rs:109-125`, `207-223`
**Issue:** `build_output_set` canonicalizes each output path *once at startup* (e.g. the SDK dir
becomes `/private/var/.../sdk` on macOS). Per-event paths are canonicalized via `canonicalize_or_keep`,
which **falls back to the raw path when canonicalization fails** — and canonicalization fails for a
path that no longer exists, which is exactly the case for a *delete/rename* event. So if a generated
SDK `*.go` file is deleted, the event path resolves to the non-canonical `/var/.../sdk/client.go`
while the output set holds the canonical `/private/.../sdk`. `is_under_any_output`'s `starts_with`
then fails, the path is `.go`, and `is_trigger_path` returns `true` → a regeneration fires for what is
in fact gnr8's own (or a transient) output churn.

This is self-limiting (a delete is a one-shot event; regenerating recreates the file rather than
re-deleting it, so it does not sustain an infinite loop), which is why it is a WARNING and not a
BLOCKER. But it is a real hole in the "output writes never re-trigger" guarantee on the platform the
canonicalization machinery was specifically added to defend (the doc comment at L116-122 calls out the
`/private/...` vs `/...` mismatch as the whole reason canonicalization exists).

**Fix:** Compare on a path that does not depend on the file still existing. Either (a) also fold the
*non-canonical* `project_root.join(...)` forms into the output set alongside the canonical ones, so a
raw-fallback event path still matches; or (b) canonicalize each event path's *existing parent* and
re-append the final component, so a just-deleted leaf still resolves under the canonical output dir:
```rust
fn canonicalize_or_keep(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) { return p; }
    // Fall back to canonicalizing the nearest existing ancestor (handles delete/rename events
    // whose leaf no longer exists), so the comparison still matches the canonical output set.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map(|c| c.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}
```

### WR-02: `apply_naming` type rename can chain/collapse two distinct types in one config pass

**File:** `crates/gnr8-core/src/lifecycle/mod.rs:283-325`
**Issue:** The type-rename loop is `for (key, new_name) in &naming.types`, and each iteration renames
*both* the schema's `id` **and** `name` to `new_name`, then rewrites every ref. Because the match key
test (L287-288) is `s.id == key || s.name == key`, a later iteration can match a schema that an
*earlier* iteration already renamed. Two failure modes:

- **Chained rename:** config `A = "B"` then `B = "C"`. Iteration 1 renames schema `A`→`B`. Iteration 2
  (key `B`) now matches *both* the original `B` and the just-renamed `A`, renaming both to `C` and
  collapsing two distinct types into one.
- **Collision:** renaming `Foo`→`Bar` when a `Bar` schema already exists yields two schemas sharing
  `id == name == "Bar"`. `to_openapi` builds `ref_to_name` as a `BTreeMap` keyed by `schema.id`
  (`lower/mod.rs:65`), so the duplicate key silently collapses and `build_component_schemas` emits two
  `Bar:` entries — invalid/ambiguous OpenAPI rather than a clean error.

`naming.types` is a `BTreeMap`, so iteration order is the *sorted key* order, not config order, making
the chained-rename outcome order-dependent and surprising. This requires conflicting user config to
trigger (hence WARNING, not BLOCKER), but it silently mis-generates instead of erroring (counter to
the `deny_unknown_fields`/V5 "fail loud, never silently mis-generate" stance elsewhere in this phase).

**Fix:** Snapshot the original ids before applying any rename, match only against the *original* id/name
set, and detect target-name collisions explicitly, e.g.:
```rust
// Resolve all (old_id, new_name) pairs against the ORIGINAL graph first…
let original: Vec<(String, String)> = graph.schemas.iter()
    .map(|s| (s.id.clone(), s.name.clone())).collect();
// …then reject a rename whose target collides with an existing/!renamed schema id:
// return Err(CoreError::Config { message: format!("naming.types {key:?} → {new_name:?} collides …") })
```
At minimum, document that `naming.types` keys must reference original type names and that targets must
be unique, and add a collision guard so the failure is a typed `CoreError`, not a malformed artifact.

### WR-03: `regenerate_and_report` hard-codes the `"single-file-edit"` scenario for every batch

**File:** `crates/gnr8/src/watch.rs:253`, `274`
**Issue:** Every debounced batch — including multi-file edits and coalesced bursts drained at
`watch.rs:252` — is reported with `scenario: "single-file-edit"`. The `LatencyReport.scenario` field
is part of the documented WATCH-03 `--json` contract (the test at L408-435 asserts the exact field
set, and the SUMMARY states Phase 5 benchmark tooling keys off `scenario`). Labeling a multi-file or
coalesced regeneration as `single-file-edit` makes the machine-readable latency record inaccurate for
any non-trivial change, which will skew the Phase 5 latency buckets it feeds.

**Fix:** Either derive the scenario from the batch (e.g. `single-file-edit` only when exactly one
trigger path was seen, else a `batch`/`multi-file` label), or rename the constant to a neutral
`"watch-regen"` so the field does not over-claim a precision it cannot guarantee. If `single-file-edit`
must remain the canonical warm-edit label, document that watch always reports it regardless of batch
size so downstream tooling does not over-interpret it.

### WR-04: `build_outputs` silently analyzes only the first input when several are configured

**File:** `crates/gnr8-core/src/lifecycle/mod.rs:350-356`; cf. `crates/gnr8/src/watch.rs:237-242`
**Issue:** `config.inputs` is a `Vec<String>` and the config/CLI accept multiple entries, but
`build_outputs` uses only `config.inputs.first()` and ignores the rest with no diagnostic. Meanwhile
`watch::run` (L237-242) *does* iterate and watch **every** input dir. The result is an inconsistency:
a user who lists two source dirs will see watch fire on edits in the second dir, but regeneration only
ever reflects the first — edits in the additional watched dirs trigger a regen that cannot observe
them, which looks like a no-op/non-deterministic watch and is hard to diagnose. The multi-input
fan-in being out of scope (D-02) is fine, but the *silent* drop is the problem.

**Fix:** Either reject `inputs.len() > 1` with a clear `CoreError::Config` ("multi-input is not yet
supported; configure a single source dir") so the limitation is loud, or have `watch::run` watch only
`inputs.first()` to match what `build_outputs` actually analyzes, keeping the watched set and the
analyzed set in agreement.

## Info

### IN-01: `safe_output_path` accepts symlink-based escapes (lexical-only traversal check)

**File:** `crates/gnr8-core/src/lifecycle/mod.rs:234-260`
**Issue:** `safe_output_path` rejects `..`, root, and prefix components *lexically* but does not resolve
symlinks, so an output path like `sdk/link/client.go` where `sdk/link` is a pre-existing symlink to an
outside directory would pass the check and write outside the project root. Output paths come from the
checked-in `config.toml` (the project owner's own file, not untrusted input), so the practical risk is
low and this is consistent with the PoC threat model (T-04-02-01 targets accidental `..` config typos,
not a malicious local config). Noting it so the lexical-only nature of the guard is on record.
**Fix:** If output paths are ever sourced from less-trusted input, canonicalize the resolved parent and
assert it `starts_with(project_root.canonicalize())` before writing.

### IN-02: Manifest `recorded_hash`/`record`/`prune_to` are O(n) linear scans over a Vec

**File:** `crates/gnr8-core/src/manifest/mod.rs:78-108`
**Issue:** Lookups and inserts walk the `files` Vec linearly, and `prune_to` is O(n·m). For the PoC's
handful of generated files this is irrelevant and the sorted-Vec choice is the right call for
deterministic diffs (the stated GRAPH-02 rationale). Flagged only as a forward note — not a v1 concern
(performance is explicitly out of review scope) and not worth changing now.
**Fix:** None needed for v1; revisit only if output counts grow large.

### IN-03: `relativize` fallback in `workspace::relative` constructs `PathBuf::from(path)` redundantly

**File:** `crates/gnr8-core/src/workspace/mod.rs:137-142`
**Issue:** In the `strip_prefix` failure arm, `path.to_path_buf()` is mapped through
`PathBuf::from(path)` (the closure arg is the already-owned `PathBuf`), an unnecessary round-trip. Pure
style; behavior is correct. The fallback is defensive and unreachable in practice (init only passes
paths under `root`).
**Fix:** `.map_or_else(|_| path.to_path_buf(), Path::to_path_buf)` or simply return the stripped
relative path directly; cosmetic only.

### IN-04: `--debounce-ms 0` is accepted and passed straight to `new_debouncer`

**File:** `crates/gnr8/src/cli.rs:48-51`; `crates/gnr8/src/main.rs:263-268`
**Issue:** `debounce_ms` is a `u64` with no lower bound, so `gnr8 watch --debounce-ms 0` creates a
zero-window debouncer. notify-debouncer-full tolerates a zero duration (it degrades to effectively no
coalescing rather than erroring), so this does not crash — but a 0ms window defeats the burst-coalescing
the flag exists to provide and could amplify the WR-01 edge case. Low severity; documenting the lack of
a floor.
**Fix:** Optionally clamp to a small minimum (e.g. `debounce_ms.max(10)`) or document that 0 disables
coalescing intentionally.

---

_Reviewed: 2026-06-24_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
