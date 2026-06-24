---
phase: 02-go-analysis-and-api-graph
fixed_at: 2026-06-24T00:00:00Z
review_path: .planning/phases/02-go-analysis-and-api-graph/02-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 2: Code Review Fix Report

**Fixed at:** 2026-06-24
**Source review:** `.planning/phases/02-go-analysis-and-api-graph/02-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope: 4 (all WARNING — WR-01..WR-04)
- Fixed: 4
- Skipped/deferred: 0
- Info findings (IN-01..IN-03): out of scope for this `--fix` run; not addressed.

All four warnings were latent robustness gaps that the goalservice fixture does not
trigger (its inputs avoid every triggering condition). Each fix therefore hardens the
analyzer for arbitrary third-party Go input **without changing fixture output**: the
`snapshot_graph`, `snapshot_diagnostics`, and `determinism` gates remained GREEN and
byte-identical throughout, verified after every commit. The Phase-3 contract tests
(`snapshot_openapi`, `snapshot_sdk`) were left red-by-design (`NotYetImplemented`).

## Fixed Issues

### WR-01: HTTP status from a constant truncated to `u16` with no range check

**Files modified:** `goextract/internal/handlers/handlers.go`
**Commit:** `f2e6c24`
**Applied fix:** Added `httpStatusInRange(v int64) (uint16, bool)`, which narrows to
`uint16` only when the resolved constant is a valid HTTP status (`100..=599`). Both
branches of `statusOf` (the `info.Types[arg]` exact-value path and the `*types.Const`
fallback) now route through it, so an out-of-range or non-exact constant (e.g.
`c.JSON(70000, x)`, which used to wrap silently to `4464`) returns `ok=false` and the
existing `analyzeJSON` dynamic-response diagnostic fires instead of emitting a corrupted
status. This brings the code path to parity with the annotation path's existing
`status < 0 || status > 599` guard (GO-06). Fixture statuses are all standard
`http.StatusXxx` in range, so the snapshot is unaffected.

### WR-04: `relativize` strips the root prefix without a path-boundary check

**Files modified:** `crates/gnr8-core/src/graph/mod.rs`,
`crates/gnr8-core/src/diagnostics/mod.rs`
**Commit:** `2592f71`
**Applied fix:** Replaced the raw `strip_prefix(root)` + `trim_start_matches('/')` in both
`relativize` copies with a separator-boundary match: an exact root match maps to the empty
path; `"<root>/rest"` maps to `"rest"`; anything not actually under `"<root>/"` is left
absolute. This prevents a sibling directory sharing the root as a string prefix (e.g.
`root = "/a/svc"`, file `"/a/svc-utils/x.go"`) from being mis-stripped to `"-utils/x.go"`
(GRAPH-02 portability/byte-stability). The fixture's spans are all inside the module root,
so the graph + diagnostics snapshots stay byte-identical. (The IN-03 duplication of this
helper across the two modules was left in place — out of scope for the WARNING set.)

### WR-03: Process-global mutable module/annotation state is non-reentrant

**Files modified:** `goextract/internal/handlers/handlers.go`,
`goextract/internal/handlers/annotations.go`, `goextract/main.go`,
`goextract/internal/handlers/handlers_test.go`,
`goextract/internal/handlers/annotations_test.go`
**Commit:** `41200a2`
**Applied fix:** Replaced the package-level `modulePrefix`/`SetModule` and
`annotationPkgPaths`/`SetAnnotationPackages` globals with an `Analyzer` struct that carries
the module prefix and the swaggo selector->package map as per-invocation context, built
once via `NewAnalyzer(res, module, diags)`. `Analyze`, `ParseAnnotations`, and the
schema-id/annotation-ref helpers became methods reading the struct, so setup ordering is
enforced by construction (not call discipline) and the helper is reentrant — two in-process
analyses over different modules can no longer clobber each other's prefix/selector map
(unblocks the Phase-4 in-process watch). `main.go` and both handler/annotation tests were
updated to the struct flow. Fixture output is unchanged.

### WR-02: Handler index keyed by bare name collides across packages

**Files modified:** `goextract/internal/handlers/handlers.go`,
`goextract/internal/handlers/index_collision_test.go` (new)
**Commit:** `bf17267`
**Applied fix:** `handlerDecl` now carries its owning `pkgPath`. On a bare-name collision in
`BuildIndex`, the surviving decl is chosen deterministically by a fully-qualified identity
key (`pkgPath + receiver + name + file:line`) instead of go/packages load order, and a
`diag.Warn` naming the dropped declaration is emitted so the collision is surfaced rather
than silently last-write-wins. This fixes both the correctness bug (wrong body/responses/
annotations attached to a route) and the GRAPH-02 determinism hazard the review flagged. A
new unit test synthesizes a two-package `Handle` collision and asserts (a) exactly one WR-02
diagnostic fires and (b) the dropped decl is load-order-independent. The goalservice fixture
has globally-unique handler names, so no collision diagnostic appears and the snapshots stay
byte-identical.

## Skipped Issues

None — all four in-scope WARNING findings were fixed.

## Gate Status (post-fix)

- `cargo fmt --all -- --check`: GREEN
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: GREEN
- `make gates` (lib 22, bin 9, `determinism`, `snapshot_graph`, `snapshot_diagnostics`):
  GREEN — snapshots byte-identical for `fixtures/goalservice` (GRAPH-02 preserved).
- `cd goextract && go build ./... && go vet ./... && go test ./...`: GREEN.
- `snapshot_openapi` + `snapshot_sdk`: still RED-by-design (`NotYetImplemented`,
  `lower::to_openapi` / `sdk::generate` are Phase 3) — intentionally untouched.
- No production `unwrap`/`expect`/`panic` introduced.

## Note on isolation

The agent's file-edit tools resolve writes to the session working tree
(`/Users/.../tripoli-v7`), not to a `/tmp` git worktree, so the planned worktree isolation
could not hold the edits; the temporary worktree + recovery sentinel were created and then
cleanly removed (worktree pruned, temp branch deleted, sentinel dropped). All four fixes
were committed directly on the active branch with narrow, file-scoped `git commit` calls
(the `gsd-sdk query commit` helper operates against the main repo root and was not used).
Pre-existing unrelated changes (`.planning/HANDOFF.json`, untracked `02-REVIEW.md` /
`02-VERIFICATION.md`) were left untouched and excluded from every commit.

---

_Fixed: 2026-06-24_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
