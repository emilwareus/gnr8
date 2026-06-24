---
phase: 02-go-analysis-and-api-graph
reviewed: 2026-06-24T00:00:00Z
depth: deep
files_reviewed: 23
files_reviewed_list:
  - goextract/main.go
  - goextract/internal/load/load.go
  - goextract/internal/facts/facts.go
  - goextract/internal/diag/diag.go
  - goextract/internal/types/extract.go
  - goextract/internal/routes/routes.go
  - goextract/internal/handlers/handlers.go
  - goextract/internal/handlers/annotations.go
  - crates/gnr8-core/src/analyze/facts.rs
  - crates/gnr8-core/src/analyze/helper.rs
  - crates/gnr8-core/src/analyze/mod.rs
  - crates/gnr8-core/src/graph/mod.rs
  - crates/gnr8-core/src/diagnostics/mod.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/lib.rs
  - crates/gnr8/src/cli.rs
  - crates/gnr8/src/main.rs
  - crates/gnr8/src/render.rs
  - crates/gnr8-core/tests/snapshot_graph.rs
  - crates/gnr8-core/tests/snapshot_diagnostics.rs
  - crates/gnr8-core/tests/determinism.rs
  - .github/workflows/ci.yml
  - Makefile
findings:
  critical: 0
  warning: 4
  info: 3
  total: 7
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-06-24
**Depth:** deep
**Files Reviewed:** 23
**Status:** issues_found

## Summary

Phase 2 builds the Go sidecar (`goextract/`), the Rust subprocess driver + serde facts
contract, the router-agnostic `ApiGraph`, the diagnostics renderer, and the `inspect
routes|schemas|graph` CLI. I verified all blocking gates independently: `cargo clippy
--all-targets --all-features --locked -- -D warnings` is clean (exit 0); the lib/bin/
snapshot/determinism Rust tests pass; `go build/vet/test ./...` in `goextract` passes.

The phase-critical guarantees hold up under close inspection:

- **Determinism (GRAPH-02):** Every serialized slice is sorted before emit. On the Go
  side `facts.Marshal` sorts schemas, fields, enum values, diagnostics, routes, params,
  responses, tags, and security schemes; I grepped every `range` in the helper and found
  **no map-into-output iteration** — the only map (`groups`) is lookup-only, and
  `scope.Names()` is sorted by go/types. On the Rust side `ApiGraph::from_facts` re-sorts
  every collection. The end-to-end `determinism.rs` test proves two runs are byte-identical.
- **No-panic (GO-06):** No production `unwrap`/`expect`/`panic` in `gnr8-core` library
  code; the `#![allow(clippy::unwrap_used, clippy::expect_used)]` allows are all scoped to
  `#[cfg(test)]` modules. Every subprocess/parse failure maps to a typed `CoreError` via
  `?` (`GoToolchainMissing` / `HelperExit` / `FactsParse`). The Go helper returns errors
  and degrades unparsable positions to `("", 0)` rather than panicking.
- **go/types soundness:** Gin method identity is resolved via `Info.Selections` +
  `MethodVal` kind + resolved receiver package path (`GinMethod`), gated on the *resolved*
  package path, never the import alias. The `aliasedgin` testdata proves recognition
  survives `import grouter "...gin"`.
- **Subprocess security (ASVS L1):** `run_goextract_with` passes `["run", ".", target_dir]`
  as discrete `Command` args with no `sh -c`; `target_dir` is canonicalized, never
  interpolated. `#[serde(deny_unknown_fields)]` is on every facts DTO, so a drifted/forged
  helper payload is rejected, not trusted.
- **Snapshots are real, not bogus:** I cross-checked the `.snap` against the fixture
  source. 4 routes (POST `/`, GET `/list`, PUT/DELETE `/{uuid}`), 9 schemas (8 objects +
  `TargetDirection` enum), 7 diagnostics. Provenance line numbers match the fixture exactly
  (e.g. `deleteGoal` `uuid` param at handlers.go:119, `updateGoal` at :94; query params at
  57/58/59; float64 fields at goal.go 32/43/57). Embedded-struct flattening
  (`CommandMessageWithUUID` promoting `message`), `@ID` vs handler-symbol operation ids, and
  annotation-fill-without-clobber (404 added, 200/400 bodies kept) all render correctly.

No blockers. The four warnings are latent-correctness/robustness issues that do not affect
the goalservice fixture (whose inputs avoid the triggering conditions) but should be
hardened before the analyzer is pointed at arbitrary third-party Go modules.

## Warnings

### WR-01: HTTP status from a constant is truncated to `u16` with no range check

**File:** `goextract/internal/handlers/handlers.go:189-209`
**Issue:** `statusOf` resolves the first `c.JSON(status, ...)` argument and returns
`uint16(v)` in **both** branches (`info.Types[arg]` exact-value path and the
`*types.Const` fallback) without validating that `v` is in a sane HTTP range. A handler
written as `c.JSON(70000, x)` — or any const expression that resolves to a large int —
silently wraps (`70000 & 0xFFFF = 4464`) and is emitted as a real response status instead
of being diagnosed or rejected. This is exactly the malformed-Go-input class GO-06 asks the
analyzer to handle gracefully. Note the inconsistency: the annotation path
(`parseResponse`, annotations.go:147) *does* bound-check `status < 0 || status > 599` and
skip, so the code path is strictly less defensive than the annotation path for the same
fact.
**Fix:**
```go
v, exact := constant.Int64Val(c.Val())
if !exact || v < 100 || v > 599 {
    return 0, false // out-of-range or non-exact -> diagnose as dynamic response
}
return uint16(v), true
```
Apply the same `100..=599` guard in the `info.Types[arg]` branch, and let the existing
`analyzeJSON` dynamic-response diagnostic fire when `statusOf` returns `ok=false`.

### WR-02: Handler index keyed by bare name collides across packages (correctness + determinism)

**File:** `goextract/internal/handlers/handlers.go:56-73`; consumed at `handlers.go:90`
and `annotations.go:47-49`
**Issue:** `BuildIndex` keys every handler `FuncDecl` by `fn.Name.Name` (the bare symbol)
across **all** target packages, so `idx[fn.Name.Name] = ...` is last-write-wins. The route
recognizer also matches handlers by bare name (`handlerSymbol` returns only the trailing
selector). If a target module declares two functions/methods with the same name in
different packages (e.g. `func (a) Handle` and `func (b) Handle`, common in larger Go
services), the index silently keeps only one — chosen by `res.Packages` × `file.Decls`
iteration order — and `Analyze` / `ParseAnnotations` then attach the **wrong** body,
responses, params, and doc-comment annotations to a route. Because the survivor depends on
load order, this is also a GRAPH-02 determinism hazard, not just a correctness one. The
goalservice fixture has globally-unique handler names, so the snapshot is unaffected, but
the assumption is undocumented and unenforced.
**Fix:** Key the index by a package-qualified identity (e.g. `pkg.PkgPath + "." +
fn.Name.Name`) and carry the handler's package on `routes.Route` so lookups disambiguate;
or, minimally, detect a duplicate bare name and emit a `diag.Warn` so the collision surfaces
instead of silently resolving to one handler.

### WR-03: Process-global mutable state for module/annotation context is non-reentrant

**File:** `goextract/internal/handlers/handlers.go:252-256` (`modulePrefix` +
`SetModule`); `goextract/internal/handlers/annotations.go:280-287` (`annotationPkgPaths` +
`SetAnnotationPackages`)
**Issue:** The module prefix and the swaggo selector→package map are stored in
package-level `var`s mutated by setter functions, then read during `Analyze` /
`ParseAnnotations`. This couples correctness to call ordering (the setters MUST run before
any analysis — `main.run` does, but nothing enforces it) and is not reentrant: two
concurrent `Extract`/`Analyze` runs over different modules in one process would clobber each
other's prefix and produce wrong/cross-contaminated schema refs. It works today only because
the helper is a single-shot `os.Exit` process. It is also a hidden global the unit tests
have to set up manually (see `handlers_test.go:29-30`).
**Fix:** Thread the module prefix and annotation-package map through an analyzer struct or
explicit parameters (e.g. `Analyze(route, idx, ctx, diags)` where `ctx` carries
`modulePrefix` + `annotationPkgPaths`) rather than package globals. This removes the
ordering dependency and makes the helper safe to call in-process later (Phase 4 watch).

### WR-04: `relativize` strips the root prefix without a path-boundary check

**File:** `crates/gnr8-core/src/graph/mod.rs:371-379` and
`crates/gnr8-core/src/diagnostics/mod.rs:60-68`
**Issue:** Both `relativize` helpers do `file.strip_prefix(root)` on raw strings with no
separator boundary. A file under a **sibling** directory whose name shares the root as a
prefix is mis-relativized: with `root = "/a/svc"` a span file `"/a/svc-utils/x.go"` strips
to `"-utils/x.go"` (after `trim_start_matches('/')`), producing a corrupted, non-portable
path rather than being left absolute. This is latent because go/types spans for the analyzed
module are always *inside* the module root, but it is a fragile string operation in the
exact code whose job is portability/byte-stability (GRAPH-02).
**Fix:** Strip only on a path-separator boundary, e.g.:
```rust
fn relativize(file: &str, root: &str) -> String {
    if root.is_empty() { return file.to_string(); }
    match file.strip_prefix(root) {
        Some(rest) if rest.is_empty() => String::new(),
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/').to_string(),
        _ => file.to_string(), // not actually under root/
    }
}
```
The two copies are also duplicated logic (see IN-03).

## Info

### IN-01: `typeString` wraps `gotypes.TypeString` in a no-op `fmt.Sprintf`

**File:** `goextract/internal/types/extract.go:396-400`
**Issue:** `fmt.Sprintf("%s", gotypes.TypeString(...))` formats an already-`string` value
through `Sprintf`, which `go vet`'s S1025-style checks (and reviewers) flag as redundant —
it allocates and does nothing the bare call doesn't.
**Fix:** `return gotypes.TypeString(t, func(p *gotypes.Package) string { return p.Name() })`.

### IN-02: Two divergent `optString` helpers with the same name

**File:** `goextract/internal/types/extract.go:388-394` (no trim) vs
`goextract/internal/handlers/annotations.go:412-419` (trims whitespace)
**Issue:** Two unexported helpers share the name `optString` but differ in behavior (one
trims, one does not). Same-named helpers with subtly different semantics across sibling
packages are an easy source of "why is this field trimmed here but not there" bugs during
later edits. Not a defect today, but a maintainability trap.
**Fix:** Give them distinct names reflecting behavior (e.g. `optStringTrimmed`) or factor a
single shared helper into a small internal util package with explicit trim/no-trim variants.

### IN-03: `relativize` duplicated verbatim in graph and diagnostics modules

**File:** `crates/gnr8-core/src/graph/mod.rs:371-379` and
`crates/gnr8-core/src/diagnostics/mod.rs:60-68`
**Issue:** The prefix-stripping helper is copy-pasted in two modules. Combined with WR-04,
a fix has to be applied in two places or the two renderers drift.
**Fix:** Hoist a single `pub(crate) fn relativize(file, root)` into a shared location (e.g.
`analyze` or a small `path` util) and call it from both `graph` and `diagnostics`.

---

_Reviewed: 2026-06-24_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
