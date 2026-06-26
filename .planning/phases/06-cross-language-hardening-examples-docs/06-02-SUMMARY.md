---
phase: 06-cross-language-hardening-examples-docs
plan: 02
subsystem: examples
tags: [fastapi, nestjs, pysdk, tssdk, openapi, examples, determinism, makefile]

# Dependency graph
requires:
  - phase: 02-cross-language-frontends-python
    provides: FastAPI route + DTO extraction (pyextract) and the PySdk target
  - phase: 04-cross-language-frontends-typescript
    provides: NestJS route + DTO extraction (tsextract) and the TsSdk target
  - phase: 06-cross-language-hardening-examples-docs
    provides: 06-01 source-language toolchain dispatch (doctor/check/watch)
provides:
  - examples/fastapi-bookstore — self-contained FastAPI example with a .gnr8/ Pipeline crate and REAL committed OpenAPI 3.1 + Python SDK
  - examples/nestjs-bookstore — self-contained NestJS example (no node_modules) with a .gnr8/ Pipeline crate and REAL committed OpenAPI 3.1 + TS SDK
  - make examples-check — cross-language regen-and-diff determinism gate wired into make check
affects: [docs, usage, onboarding, cross-language]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "End-to-end example = copied static source + a .gnr8/ Rust Pipeline crate (config is code, rule 4) + committed REAL gnr8 generate output"
    - "FastApi source must use inputs([\".\"]) (the project root), not inputs([\"app\"]), so the source's absolute `from app.models import …` imports resolve; .gnr8/ is already excluded from language detection"
    - "make examples-check uses gnr8's own `gnr8 check` (exits non-zero on drift) as the regen-and-diff — no bespoke compare script (rule 2)"

key-files:
  created:
    - examples/fastapi-bookstore/.gnr8/src/main.rs
    - examples/fastapi-bookstore/app/{__init__,main,models}.py
    - examples/fastapi-bookstore/generated/openapi.yaml
    - examples/fastapi-bookstore/generated/sdk/*.py
    - examples/fastapi-bookstore/README.md
    - examples/nestjs-bookstore/.gnr8/src/main.rs
    - examples/nestjs-bookstore/src/{books.controller,books.dto}.ts
    - examples/nestjs-bookstore/generated/openapi.yaml
    - examples/nestjs-bookstore/generated/sdk/*.ts
    - examples/nestjs-bookstore/README.md
  modified:
    - Makefile
    - examples/bookstore/generated/openapi.yaml

key-decisions:
  - "FastApi example scopes inputs to [\".\"] not [\"app\"]: the fixture uses absolute `from app.models import …` imports that only resolve when the project root (the parent of app/) is the analysis root; .gnr8/ is already excluded from detection, so the tree still reads as Python (Rule 1 fix — inputs([\"app\"]) produced incomplete output with unresolvable-type WARNs)."
  - "NestJs example scopes inputs to [\"src\"]: the fixture uses relative `from './books.dto'` imports that resolve within src/, so no project-root widening is needed."
  - "examples-check uses `gnr8 check` (existing drift detector, exits 1 on drift) as the regen-and-diff — no bespoke compare script (rule 2 / Don't-Hand-Roll)."
  - "Regenerated the stale committed examples/bookstore/generated/openapi.yaml: the v1 bytes predated gnr8's nullable lowering (type: [T, null] / oneOf+null), so a fresh generate drifted; refreshed to REAL current output so the new gate is honest and green."
  - "No ApplySecurity in either example: the fixtures carry no auth in source; the fixture snapshot tests inject a config-supplied ApiKeyAuth scheme, but the examples intentionally omit it (security is config, not scraped — rule 4)."

patterns-established:
  - "Cross-language example layout: copied static source beside a .gnr8/{Cargo.toml,Cargo.lock,.gitignore,src/main.rs} crate + committed generated/ output; cache/ and target/ gitignored."
  - "Determinism gate: build the release gnr8 once, then per-example cd + generate + check; non-zero check exit fails make check."

requirements-completed: [XLANG-01, XLANG-02, XLANG-05]

# Metrics
duration: 18min
completed: 2026-06-26
---

# Phase 6 Plan 02: Cross-Language Examples + Determinism Gate Summary

**Two self-contained end-to-end examples (FastAPI→OpenAPI 3.1+Python SDK, NestJS→OpenAPI 3.1+TS SDK, both driven by a `.gnr8/` Rust Pipeline crate with REAL committed output) plus a `make examples-check` gate that proves byte-identical cross-language determinism across all three examples.**

## Performance

- **Duration:** ~18 min
- **Started:** 2026-06-26
- **Completed:** 2026-06-26
- **Tasks:** 3
- **Files modified:** 26 (24 created, 2 modified)

## Accomplishments
- `examples/fastapi-bookstore/`: copied static FastAPI source + a `.gnr8/` Pipeline crate (FastApi → OpenApi31 + PySdk) + REAL committed OpenAPI 3.1 and a dependency-free `urllib`/`@dataclass` Python SDK; regenerates byte-identically (XLANG-01).
- `examples/nestjs-bookstore/`: copied static NestJS source (NO `node_modules`) + a `.gnr8/` Pipeline crate (NestJs → OpenApi31 + TsSdk) + REAL committed OpenAPI 3.1 and a dependency-free `fetch` TS SDK; regenerates byte-identically (XLANG-02).
- `make examples-check`: builds the release `gnr8` once, then `generate` + `check` each of the three examples (Go/Python/TS); wired into `.PHONY` and as a `check:` prerequisite. `make check` is green end-to-end (XLANG-05).

## Task Commits

Each task was committed atomically:

1. **Task 1: Author the FastAPI example** - `4e235fd` (feat)
2. **Task 2: Author the NestJS example** - `dc306b3` (feat)
3. **Task 3: Wire the examples-check determinism gate into make check** - `a9ab8d8` (feat)

## Files Created/Modified
- `examples/fastapi-bookstore/.gnr8/src/main.rs` - FastAPI Pipeline (FastApi → OpenApi31 + PySdk); config is code
- `examples/fastapi-bookstore/app/{__init__,main,models}.py` - copied static FastAPI source (never executed)
- `examples/fastapi-bookstore/generated/{openapi.yaml, sdk/*.py}` - REAL `gnr8 generate` output
- `examples/fastapi-bookstore/README.md` - example walkthrough (no pip install)
- `examples/nestjs-bookstore/.gnr8/src/main.rs` - NestJS Pipeline (NestJs → OpenApi31 + TsSdk); config is code
- `examples/nestjs-bookstore/src/{books.controller,books.dto}.ts` - copied static NestJS source (never executed)
- `examples/nestjs-bookstore/generated/{openapi.yaml, sdk/*.ts}` - REAL `gnr8 generate` output
- `examples/nestjs-bookstore/README.md` - example walkthrough (NO node_modules needed)
- `Makefile` - added `examples-check` target + `.PHONY`/`check:` wiring
- `examples/bookstore/generated/openapi.yaml` - regenerated to current REAL output (nullable lowering)

## Decisions Made
- **FastApi `inputs(["."])` not `inputs(["app"])`:** the FastAPI fixture imports `from app.models import …` (absolute), which only resolves when the analysis root is the project root (the parent of `app/`). `inputs(["app"])` made `app/` the root, so the absolute import failed and the body/response/`fmt` facts were silently omitted (WARN: unresolvable type name). `.gnr8/` is already excluded from language detection, so pointing at `.` still classifies the tree as Python. The NestJS fixture uses a relative `./books.dto` import, so `inputs(["src"])` is fine there.
- **`gnr8 check` is the diff:** the gate reuses gnr8's own drift detector (exits 1 on drift) rather than a hand-rolled byte comparison (rule 2).
- **No `ApplySecurity`:** the source carries no auth; security is config-supplied (rule 4), and the examples intentionally omit it, so they structurally match the fixture snapshots minus the test-only `ApiKeyAuth` scheme.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] FastApi example produced incomplete OpenAPI with `inputs(["app"])`**
- **Found during:** Task 1 (FastAPI example)
- **Issue:** The plan/PATTERNS specified `FastApi::new().inputs(["app"])`. Running that emitted `WARN: unresolvable type name 'Book'/'BookFormat'/'BookFilters'` and dropped the request bodies, response `$ref`s, and the `fmt` query param — the generated OpenAPI did not match the fixture snapshot structure (Pitfall 4). Cause: the source uses absolute `from app.models import …` imports, which require the parent of `app/` to be the analysis root; scoping to `app/` makes those imports unresolvable.
- **Fix:** Changed to `FastApi::new().inputs(["."])` (the example project root). `.gnr8/` is already excluded from `scan_markers`/`detect_language`, so the tree still classifies as Python and the absolute imports resolve. Generated output then structurally matches the fixture snapshot (4 routes, 9 schemas, all `$ref`s) with zero diagnostics.
- **Files modified:** examples/fastapi-bookstore/.gnr8/src/main.rs (+ README + doc-comment header)
- **Verification:** `gnr8 generate` emits no WARNs; `grep` confirms response content/`$ref`s, request bodies, and the `fmt` param are present; `gnr8 check` exits 0; second pass byte-identical.
- **Committed in:** `4e235fd` (Task 1 commit)

**2. [Rule 1 - Bug] Stale committed examples/bookstore/generated/openapi.yaml failed the new gate**
- **Found during:** Task 3 (wiring examples-check)
- **Issue:** The committed Go bookstore output predated gnr8's nullable lowering. A fresh `gnr8 generate` drifted (9 insertions / 7 deletions: `type: string` → `type: [string, null]`, `$ref` → `oneOf` + `type: null`), so the new `examples-check` gate failed on the Go example with "drifted".
- **Fix:** Regenerated `examples/bookstore/generated/openapi.yaml` via `gnr8 generate --force` (REAL output, never hand-written). The Go SDK files were already current (only the OpenAPI nullable types changed).
- **Files modified:** examples/bookstore/generated/openapi.yaml
- **Verification:** `cd examples/bookstore && gnr8 generate && gnr8 check` exits 0; `make examples-check` green for all three examples.
- **Committed in:** `a9ab8d8` (Task 3 commit)

---

**Total deviations:** 2 auto-fixed (2 Rule 1 bugs)
**Impact on plan:** Both fixes were required for correctness — fix 1 made the FastAPI example's committed output honest (matching the proven fixture extraction); fix 2 refreshed stale Go output so the cross-language determinism gate is true and green. No scope creep; no OSS deps added.

## Issues Encountered
- `gnr8 generate` shells out to `cargo run` (the `.gnr8/` child) and the per-language sidecars (`go`/`python3`/`node`), so the `examples-check` recipe must have those on PATH. `go` is not on the sandbox default PATH (relocatable install), so the recipe prepends `/home/vercel-sandbox/.local/go-install/go/bin`; cargo/node/python3 are already on the inherited PATH.

## Threat Surface
- T-06-04 (committed output ↔ fresh regen): mitigated — output is produced ONLY by `gnr8 generate` and asserted byte-identical by `gnr8 check` in `make examples-check`.
- T-06-05 (executing the example app): mitigated — pyextract is static `ast`, tsextract reads types via the Compiler API; neither runs the app. No `pip install`, no `node_modules`.
- T-06-SC (package installs): mitigated — zero packages installed; `git diff --exit-code crates/gnr8-core/Cargo.toml` is clean; the vendored `typescript` is the pre-existing Phase-4 carve-out.

No new threat surface introduced beyond the registered items.

## User Setup Required
None - no external service configuration required. (`make tsextract-deps` restores the dev `typescript` for the NestJS example's generate + the existing TS test suite; it is the existing on-demand dev install.)

## Next Phase Readiness
- Two runnable cross-language examples now exist for docs (06-03 USAGE.md updates can point at them alongside the Go bookstore).
- `make examples-check` is the enforced determinism proof across Go/Python/TS.
- No blockers.

## Self-Check: PASSED

All claimed files exist (FastAPI/NestJS `.gnr8/src/main.rs`, both `generated/openapi.yaml`, Makefile) and all three task commits are in the git log (`4e235fd`, `dc306b3`, `a9ab8d8`).

---
*Phase: 06-cross-language-hardening-examples-docs*
*Completed: 2026-06-26*
