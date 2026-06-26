---
phase: 06-cross-language-hardening-examples-docs
plan: 03
subsystem: docs
tags: [docs, usage, invariants, claude-md, project-md, roadmap, backlog, typescript-toolchain, no-new-dep, honest-envelope, capstone]

# Dependency graph
requires:
  - phase: 02-python-source-pyextract (Plan 03)
    provides: "verified FastAPI full-envelope behavior (routes/params/bodies/response_model/status_code, Union aliases, static ast, unresolvable -> diagnostic+omit) — source of the USAGE.md FastAPI row"
  - phase: 02-python-source-pyextract (Plan 04)
    provides: "verified Flask typed-envelope gaps (untyped request.json/args/return -> diagnostic, NEVER inferred; method-derived status; blueprint prefix never folded) — source of the USAGE.md Flask row"
  - phase: 04-typescript-source-tsextract (Plan 03)
    provides: "verified NestJS class-DTO scope + bright-line exclusions (@nestjs/common decorators only; never @nestjs/swagger/zod/class-validator; bare interfaces erased) — source of the USAGE.md NestJS row"
  - phase: 06-cross-language-hardening-examples-docs (Plan 01)
    provides: "source_toolchain + language field (doctor/watch parity) — the wording USAGE.md generalizes to"
  - phase: 06-cross-language-hardening-examples-docs (Plan 02)
    provides: "examples/fastapi-bookstore + examples/nestjs-bookstore + make examples-check — the examples USAGE.md points to and the green gate this plan asserts"
provides:
  - "docs/USAGE.md '## Supported source frontends (the honest envelope)' table (Gin/FastAPI/Flask/NestJS) with stated limits; Source/Target built-in tables list FastApi/Flask/NestJs + PySdk/TsSdk; generalized build/watch/doctor wording; new-examples pointers"
  - "CLAUDE.md '### TypeScript toolchain (required, not shipped)' subsection: typescript is a REQUIRED USER TOOLCHAIN (tsextract borrows the user's own, ts.js), NOT shipped/bundled/vendored; gnr8 ships ZERO OSS; rule 2 holds literally; bright line excludes @nestjs/swagger/zod/class-validator (rule 1)"
  - "PROJECT.md typescript decision reworded from 'carve-out / OSS-in-toolchain exception' to 'required user toolchain, gnr8 ships none' (agrees with CLAUDE.md + ts.js)"
  - "ROADMAP.md '## Backlog (deferred)': 999.1 WR-02 (TsSdk non-scalar query String() wire rule), 999.2 WR-04 (TsSdk asymmetric success/error decode) — append-only, all 6 phases + Progress table preserved"
  - "no-NEW-OSS-dep assertion: gnr8-core [dependencies] unchanged vs v1 baseline (thiserror/serde/serde_json/blake3); cargo tree adds zero crates; make check (incl examples-check) green end-to-end"
affects: [phase-06-complete, milestone-v2.0-ready-for-verification]

# Tech tracking
tech-stack:
  added: []  # ZERO OSS added — the whole point of XLANG-05. gnr8-core deps byte-unchanged vs the v1 baseline.
  patterns:
    - "Docs as derived artifact: every USAGE.md envelope limit is traced to a verified Phase 2/4 SUMMARY (no overclaiming); the committed snapshots are the spec"
    - "Invariant record (corrected framing): typescript is a REQUIRED USER TOOLCHAIN resolved from the target project (ts.js), the same class as go/python3 on PATH — gnr8 ships ZERO OSS, so rule 2 holds LITERALLY (not a loosening/exception)"
    - "Append-only governance edits: ROADMAP backlog appended after the Progress table; all 6 phase sections + the table preserved verbatim (a prior planner truncated ROADMAP via full-file Write — not repeated)"
    - "No-new-dep as a verifiable assertion: gnr8-core Cargo.toml unchanged vs baseline + cargo tree adds nothing + make check green — XLANG-05's 'zero OSS' read as 'add no NEW dep', NOT a serde/blake3 rewrite (the four debt crates stay tracked, not retired)"

key-files:
  created:
    - .planning/phases/06-cross-language-hardening-examples-docs/06-03-SUMMARY.md
  modified:
    - docs/USAGE.md
    - CLAUDE.md
    - .planning/PROJECT.md
    - .planning/ROADMAP.md

key-decisions:
  - "CORRECTED FRAMING applied (overriding the older RESEARCH 'Documented exception' wording): the JUST-un-vendored typescript is recorded as a REQUIRED USER TOOLCHAIN that tsextract resolves from the target project (ts.js), exactly as goextract uses go and pyextract uses python3. gnr8 ships ZERO OSS (gnr8-core takes no crates; nothing is vendored; the gitignored devDependency backs only gnr8's own tests). Rule 2 holds LITERALLY — this is a toolchain prerequisite, NOT a carve-out/loosening. CLAUDE.md subsection titled '### TypeScript toolchain (required, not shipped)'; PROJECT.md reworded to agree."
  - "XLANG-05 'zero OSS' read as 'add no NEW dep this phase' (locked Q1): the four v1 debt crates (thiserror/serde/serde_json/blake3) stay tracked and UNCHANGED, NOT retired (that is FUT/cleanup, deferred). Asserted via Cargo.toml-unchanged-vs-baseline + cargo tree adds nothing."
  - "WR-02/WR-04 BACKLOGGED (not folded): folding the TsSdk query String() wire rule + the asymmetric success/error decode would change the byte-committed TsSdk snapshots + the NestJS example output (the Phase-5 REVIEW-FIX skipped them for that reason). Recorded as ROADMAP backlog 999.1/999.2."
  - "USAGE.md EXTENDED in place (never rewritten): the accurate Go/Gin patterns + Type-mapping sections are preserved; the per-language envelope section is added BESIDE them; the Go-specific build/watch/doctor wording is generalized to the source language."

patterns-established:
  - "Capstone docs/invariant edit: derive every documented limit from a verified upstream SUMMARY; keep governance edits append-only; assert (not just claim) the no-new-dep invariant with a baseline diff + cargo tree"

requirements-completed: [XLANG-03, XLANG-05]

# Metrics
duration: ~9min
completed: 2026-06-26
---

# Phase 6 Plan 3: Honest Envelope Docs + TypeScript-Toolchain Record + No-New-Dep Gate Summary

**`docs/USAGE.md` now documents the honest per-language source envelope — Gin (Go, full), FastAPI (Python, full), Flask (Python, typed-envelope second-class with its untyped-surface gaps stated plainly: `request.json`/`request.args`/missing-return → diagnostic, NEVER inferred), and NestJS (TypeScript, class-DTO scope; bare interfaces erased; never reads `@nestjs/swagger`/`zod`/`class-validator`) — with the Source/Target built-in tables extended (`FastApi`/`Flask`/`NestJs`, `PySdk`/`TsSdk`), the build/watch/doctor wording generalized to the source language, and the two new examples pointed to; `CLAUDE.md` records the JUST-un-vendored `typescript` as a REQUIRED USER TOOLCHAIN (`tsextract` borrows the user's own from the target project via `ts.js`, exactly as `goextract` uses `go` / `pyextract` uses `python3`) — gnr8 ships ZERO OSS so rule 2 holds LITERALLY (not a loosening), with `PROJECT.md` reworded to agree; gnr8-core is asserted to add zero NEW OSS deps (Cargo.toml byte-unchanged vs the v1 baseline; `cargo tree` adds nothing; the four debt crates stay tracked, not retired); WR-02/WR-04 are recorded as ROADMAP backlog 999.x; and `make check` (incl. the cross-language `examples-check`) is GREEN end-to-end — closing Phase 6 and v2.0.**

## Performance

- **Duration:** ~9 min
- **Completed:** 2026-06-26
- **Tasks:** 4 (3 `type=auto` + 1 `checkpoint:human-verify` auto-approved in autonomous mode)
- **Files modified:** 4 (docs/USAGE.md, CLAUDE.md, .planning/PROJECT.md, .planning/ROADMAP.md); **created:** 1 (this SUMMARY)

## Accomplishments

- **Task 1 — docs/USAGE.md honest envelope (XLANG-03).** Added `## Supported source frontends (the honest envelope)` with a per-frontend table derived from the verified Phase 2/4 SUMMARYs: Gin (Go, full), FastAPI (Python, full), Flask (Python, typed-envelope second-class — untyped `request.json`/unannotated `request.args`/missing return annotation → diagnostic, NEVER inferred; untyped surfaces NOT recovered), NestJS (TypeScript, class-DTO scope — DTO **classes** only, bare interfaces erased; never reads `@nestjs/swagger`/`zod`/`class-validator`, rule 1). Added the "dependency-free in every language" SDK line (GoSdk net/http, PySdk urllib+@dataclass, TsSdk fetch+interfaces). Extended the Source built-ins table with `FastApi`/`Flask`/`NestJs` and the Target table with `PySdk`/`TsSdk`. Generalized the Go-specific wording: build toolchain → source language toolchain (Go/Python/TypeScript); `watch` trigger → a source-language edit (`.go`/`.py`/`.ts`); the `doctor` row → source toolchain + `language`/`source_toolchain` (matching Plan 01). Pointed to `examples/fastapi-bookstore/` + `examples/nestjs-bookstore/`. The accurate Go/Gin patterns + Type-mapping sections are preserved untouched (extended, not rewritten).
- **Task 2 — TypeScript toolchain record (corrected framing) + backlog (XLANG-05).** Added `### TypeScript toolchain (required, not shipped)` under CLAUDE.md rule 2: `tsextract` borrows the USER's own `typescript` resolved from the target project (`tsextract/ts.js`), exactly as `goextract` uses `go` and `pyextract` uses `python3`; `typescript` is a REQUIRED USER TOOLCHAIN, NOT shipped/bundled/vendored; gnr8 ships ZERO OSS (gnr8-core takes no crates; nothing vendored; the gitignored devDependency restored via `make tsextract-deps`/`npm ci` backs gnr8's OWN tests only); rule 2 holds LITERALLY. Bright line stated: facts ONLY from the source's own TS types, NEVER `@nestjs/swagger`/`zod`/`class-validator` (rule 1); every other sidecar stdlib-only; gnr8-core + every generated SDK dependency-free; FUT-04 could retire even the prerequisite. Reworded PROJECT.md l.119 from "carve-out to rule 2 / OSS-in-toolchain exception" → "required user toolchain, gnr8 ships none." Appended ROADMAP `## Backlog (deferred)` with 999.1 (WR-02: TsSdk non-scalar query `String()` wire-encoding rule) and 999.2 (WR-04: TsSdk asymmetric success/error JSON decode), both noting that folding them changes the green TsSdk snapshots. All 6 ROADMAP phase sections + the Progress table preserved verbatim (append-only). The rule 2 heading + the Known-debt retire-later schedule (four crates) are intact.
- **Task 3 — no-NEW-OSS-dep assertion + full green gate (XLANG-05).** Asserted `crates/gnr8-core/Cargo.toml` `[dependencies]` is byte-unchanged vs the v1 baseline — exactly the four tracked debt crates (thiserror, serde, serde_json, blake3), last touched by the initial commit `8331359`, untouched by the entire v2.0 milestone. `cargo tree -p gnr8-core` lists exactly those four + `insta` (dev), zero new crates. The four debt crates are NOT retired (Pitfall 5 — deferred FUT/cleanup). Ran the full gate: `make check` (fmt-check + clippy `-D warnings` + full test incl. `tssdk_compile` + fixture/goextract builds + `examples-check` regen-and-diff across Go/Python/TS) exits **0** with `0 written, 5 unchanged` for all three example languages — the committed example output is the determinism proof.
- **Task 4 — human-verify (auto-approved, autonomous mode).** ⚡ Auto-approved (autonomous mode): envelope honest + toolchain note bounded. Verified PROGRAMMATICALLY: (1) the USAGE.md Flask gaps + NestJS class-DTO scope match the verified Phase 2/4 SUMMARYs with no overclaiming; (2) the CLAUDE.md note scopes to the TS sidecar toolchain only, bright-line excludes `@nestjs/swagger`/`zod`/`class-validator`, states gnr8-core + SDKs stay dependency-free, and does NOT loosen rule 2 (heading + Known-debt schedule intact); (3) the CLAUDE.md wording agrees with the PROJECT.md decision (both say "required user toolchain").

## Task Commits

1. **Task 1: docs/USAGE.md honest per-language envelope** — `b827923` (docs)
2. **Task 2: CLAUDE.md required-user-toolchain record + PROJECT.md reword + ROADMAP backlog** — `bf2d012` (docs)
3. **Task 3: no-NEW-OSS-dep assertion + full green gate** — no commit (assertion/gate task; gnr8-core Cargo.toml intentionally unchanged — the assertion itself; `make check` exit 0 recorded here)
4. **Task 4: human-verify** — auto-approved (autonomous mode); no commit (verification)

**Plan metadata:** (final commit) (docs: complete plan)

## Decisions Made

- **Corrected framing over the older RESEARCH wording.** The 06-RESEARCH "Documented exception / the ONE sanctioned carve-out" snippet predates the un-vendoring; the plan Task 2 action, 06-CONTEXT, and the executor framing OVERRIDE it. `ts.js` already implements the corrected behavior (resolve the user's own `typescript`). Recorded as "required user toolchain, gnr8 ships none" — rule 2 literal, not a loosening.
- **XLANG-05 = no NEW dep, not a debt rewrite.** The four v1 debt crates stay tracked and unchanged; retiring them is FUT/cleanup (deferred). Asserted with a baseline diff + `cargo tree`.
- **WR-02/WR-04 backlogged, not folded.** Folding changes byte-committed TsSdk snapshots + the NestJS example output; recorded as ROADMAP 999.1/999.2.
- **USAGE.md extended, not rewritten.** The accurate Go/Gin content is preserved; the envelope section is added beside it.

## Deviations from Plan

None — plan executed as written. The only judgment call (the RESEARCH's older "Documented exception" wording vs. the plan/CONTEXT's "required user toolchain" framing) was resolved in favor of the corrected framing, exactly as the plan Task 2 action + the executor framing directed.

## Known Stubs

None — all edits are final prose/invariant/governance records derived from verified behavior; no placeholder or unwired surface.

## Threat Flags

None — no new security surface. The plan's threat register (T-06-07 docs overclaiming, T-06-08 carve-out scope creep, T-06-09 ROADMAP truncation, T-06-SC supply chain) is mitigated: the envelope is SUMMARY-derived (no overclaiming, verified programmatically); the toolchain note is bounded (TS sidecar only, bright-line excludes swagger/zod/class-validator, rule 2 + Known-debt intact); ROADMAP edits are append-only (all 6 phases + Progress table preserved); zero OSS added (Cargo.toml unchanged + cargo tree adds nothing — `typescript` is a required user toolchain, not vendored/shipped, so no install checkpoint applies).

## CLAUDE.md Compliance

- **Rule 1 (no coupling to another tool's conventions):** the docs + the toolchain note both state the bright line explicitly — `tsextract` reads facts ONLY from the source's own TS types, NEVER `@nestjs/swagger`/`zod`/`class-validator`; the Flask/FastAPI rows likewise document facts derived from the language's own constructs.
- **Rule 2 (no third-party deps):** ZERO OSS added — gnr8-core Cargo.toml byte-unchanged vs baseline; `cargo tree` adds nothing; `typescript` is recorded as a REQUIRED USER TOOLCHAIN (resolved from the target project), not a shipped/vendored dep — rule 2 holds literally. The Known-debt retire-later schedule is untouched.
- **Rule 3 (no fallback / one path):** the docs describe the single-source recognizers (untyped surface → diagnostic + omit, never a guess); `ts.js` resolution is a single deterministic search, not a guessed fallback.
- **Rule 4 (config is code):** unchanged; the examples USAGE.md points to are `.gnr8/` code-as-config Pipelines.
- **Determinism:** `make check` `examples-check` regen-and-diff is `0 written, 5 unchanged` for Go/Python/TS — byte-identical committed output.

## Next Phase Readiness

- **Phase 06 is COMPLETE and v2.0 is ready for verification.** All three plans done (06-01 CLI parity, 06-02 examples + determinism gate, 06-03 docs + invariant record + no-new-dep gate). The honest per-language envelope is documented; `typescript` is recorded as a required user toolchain (gnr8 ships zero OSS); the no-new-dep invariant is asserted; WR-02/WR-04 are backlogged; `make check` (incl. `examples-check`) is green end-to-end.
- No blockers. This is the FINAL plan of the milestone — the phase is ready for the verifier.

## Self-Check: PASSED

- Modified files verified present on disk: `docs/USAGE.md`, `CLAUDE.md`, `.planning/PROJECT.md`, `.planning/ROADMAP.md`.
- Task commits verified in git history: `b827923` (USAGE.md), `bf2d012` (CLAUDE.md + PROJECT.md + ROADMAP).
- USAGE.md acceptance: FastAPI/Flask/NestJS named (6 hits ≥3); FastApi/Flask/NestJs + PySdk/TsSdk in tables; Flask gaps + dependency-free line + source-toolchain wording + both new examples present; Go/Gin content preserved (2 hits).
- CLAUDE.md acceptance: "TypeScript toolchain (required, not shipped)" + "required user toolchain" present; bright-line `@nestjs/swagger`/`zod`/`class-validator` present; rule 2 heading + Known-debt section both present (2); four debt crates NOT removed; no claim that gnr8 ships typescript.
- PROJECT.md reworded to "required user toolchain" (agrees with CLAUDE.md).
- ROADMAP integrity: 6 phase sections (==6) + Progress table (==1) preserved; WR-02/WR-04 under `## Backlog (deferred)`.
- No-new-dep: gnr8-core Cargo.toml unchanged vs baseline (last touched `8331359`, initial commit); `cargo tree -p gnr8-core` = four debt crates + insta, zero new; `make check` exit 0; working tree clean.

---
*Phase: 06-cross-language-hardening-examples-docs*
*Completed: 2026-06-26*
