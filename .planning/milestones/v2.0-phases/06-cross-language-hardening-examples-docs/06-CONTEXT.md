# Phase 6: Cross-Language Hardening + Examples + Docs - Context

**Gathered:** 2026-06-26
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous) — decisions grounded in locked PROJECT/REQUIREMENTS; recommended defaults auto-accepted

<domain>
## Phase Boundary

The milestone capstone: prove the whole multi-language pipeline works end-to-end from real `.gnr8/`
lifecycles with committed output, document the honest per-language envelope, make `doctor`/`check`/`watch`
work across all language sidecars, guarantee cross-language determinism, and RECORD the `typescript`
rule-2 carve-out in the project's own invariant docs. No new extraction or SDK logic — this phase
integrates, hardens, documents, and proves what Phases 1–5 built.

**In scope (XLANG-01..05):**
- **XLANG-01/02 — end-to-end examples with committed output:** a FastAPI example project with a `.gnr8/`
  lifecycle (`FastApi` Source → `OpenApi31` + `PySdk` targets) and a NestJS example project with a `.gnr8/`
  lifecycle (`NestJs` Source → `OpenApi31` + `TsSdk` targets). Each driven by `gnr8 generate` producing
  real, committed `generated/` output (OpenAPI 3.1 + the SDK), mirroring the v1 `examples/bookstore`
  Go example pattern (a `.gnr8/` Rust crate composing a `Pipeline`).
- **XLANG-03 — `docs/USAGE.md` honest envelope:** per-language supported-envelope tables stating limits
  (FastAPI full; Flask typed-only with its documented gaps; NestJS class DTOs), so users know exactly
  what each frontend covers and what produces diagnostics.
- **XLANG-04 — `doctor`/`check`/`watch` cross-language parity:** generalize toolchain detection so
  `gnr8 doctor` probes the RIGHT toolchain per the source language (Go→go, Python→python3, TS→node + the
  user's own typescript resolved from the project), and `check` (drift) + `watch` (loop-safety) work for python/ts sidecars. Today
  `doctor`/`check` are Go-toolchain-specific (`crates/gnr8/src/doctor.rs`, `main::run_doctor`).
- **XLANG-05 — invariants + determinism:** confirm every sidecar is stdlib-only in its language (Python
  `ast`; TS = `typescript` only) and `gnr8-core` takes ZERO OSS crates; cross-language output is
  deterministic/byte-identical; and **record the `typescript` carve-out** explicitly in CLAUDE.md (as the
  single documented exception to rule 2) and PROJECT.md.

**Out of scope:** new source frontends or SDK targets; changing the IR/lowering/extraction behavior;
new languages beyond Go/Python/TS.
</domain>

<decisions>
## Implementation Decisions

### Locked (from PROJECT.md / REQUIREMENTS / STATE — non-negotiable)
- **gnr8 ships ZERO OSS** (rule 2). gnr8-core takes zero OSS crates AND gnr8 vendors/ships no
  `typescript`. UPDATED FRAMING (post-vendoring-removal): `tsextract` borrows the USER's own
  `typescript`, resolved from the target project (`tsextract/ts.js`) — exactly as `goextract` uses
  the user's `go` and `pyextract` uses the user's `python3`. `typescript` is a **required user
  toolchain**, NOT a bundled/shipped dependency (the old 23MB `tsextract/node_modules` vendoring was
  removed; it's a gitignored devDependency restored via `npm ci` for gnr8's OWN test suite only).
  Generated SDKs stay dependency-free.
- **Every sidecar stdlib-only in its language:** Go (stdlib go/* — modulo the v1 known-debt
  golang.org/x/tools to retire later), Python (`ast`), TS (`typescript` only). Document, don't expand.
- **Deterministic, byte-identical output** across languages and runs (the committed example output is the
  determinism proof; CI/`make check` must regenerate byte-identically).
- **Config is code:** examples are driven by `.gnr8/` Rust Pipeline crates, NEVER data files (rule 4).
- **Honest limits (XLANG-03):** the Flask typed-envelope gaps and the NestJS class-DTO scope are stated
  plainly in docs/USAGE.md — no overclaiming.
- **The `typescript` toolchain dependency must be RECORDED** in CLAUDE.md + PROJECT.md (XLANG-05).
  CORRECTED FRAMING: `typescript` is a **required user toolchain** that `tsextract` resolves from the
  target project (the language's own reference compiler) — gnr8 ships/vendors NONE of it, the same way
  it ships no `go`/`python3`/`node`. So this is NOT a "bundled OSS dependency carve-out" — rule 2 holds
  literally (gnr8-core zero crates; nothing OSS shipped). What IS recorded is the bright line:
  `tsextract` reads facts ONLY from the source's own TS types via that toolchain, NEVER from
  @nestjs/swagger/zod/class-validator; generated SDKs stay dependency-free; FUT-04 (a hand-rolled
  stdlib-pure TS parser) could remove even the toolchain requirement. Frame it as a toolchain
  prerequisite + bright-line, not a loosening of rule 2.

### Recommended defaults (auto-accepted; Claude's discretion at plan/exec, guided by RESEARCH)
- **Example projects:** add `examples/fastapi-bookstore/` and `examples/nestjs-bookstore/` (or reuse the
  `fixtures/` services as the example source, with a `.gnr8/` crate + committed `generated/`). Mirror the
  v1 `examples/bookstore/.gnr8/src/main.rs` pattern (Pipeline → Source/Transform/Target/PostProcess).
  Commit the real `generated/openapi.yaml` + the SDK. A `make check`-wired regen-and-diff proves
  determinism (the committed output must equal a fresh `gnr8 generate`).
- **Toolchain detection generalization:** extend `doctor`/`check` so the probed toolchain follows the
  detected source language (reuse `analyze::detect_language` + the existing `*ToolchainMissing` error
  variants). Keep it a single deterministic decision (no fallback). `watch` loop-safety must hold for
  python/ts sidecars (the blake3-ownership-manifest no-op detection already exists; verify it covers the
  new output paths).
- **docs/USAGE.md:** a per-language table (frontend → supported constructs → diagnostics/limits) plus a
  short "how to drive each language from `.gnr8/`" section. Update, don't duplicate, the existing USAGE.md.
- **CLAUDE.md wording:** add a concise, bounded note under rule 2 recording that `tsextract` requires
  the user's own `typescript` toolchain (resolved from the target project — like `go`/`python3`/`node`),
  ships none, and reads facts only from the source's own TS types (bright-line: never
  @nestjs/swagger/zod/class-validator). Frame as a **toolchain prerequisite + bright line**, NOT a rule-2
  OSS-dependency exception — rule 2 ("zero OSS in gnr8-core, stdlib-only sidecars, ship nothing OSS")
  holds literally. Also fix `docs/USAGE.md` line ~109 / any stale "vendored typescript" references.

### Optional hardening (deferred Phase-5 code-review findings — fix here if cheap, else backlog)
- **WR-02 (TsSdk):** non-scalar query param `String()` coercion needs a defined wire-encoding rule.
- **WR-04 (TsSdk):** asymmetric success/error JSON decode — align the contract.
  These are SDK-hardening candidates surfaced by Phase-5 review; address in this hardening phase if they
  fit cleanly without changing committed snapshots, otherwise record as backlog (999.x).

### Claude's Discretion
Example project layout, the exact USAGE.md table shape, the toolchain-detection refactor design, and
which hardening items to fold in vs. backlog — all at Claude's discretion, guided by the v1 example +
doctor/check/watch code and the locked invariants.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets / Analogs
- `examples/bookstore/.gnr8/src/main.rs` — the v1 `.gnr8/` Pipeline example to mirror for FastAPI/NestJS
  (GoGin→OpenApi31+GoSdk becomes FastApi→OpenApi31+PySdk and NestJs→OpenApi31+TsSdk).
- `examples/bookstore/generated/` — the committed-output pattern (openapi.yaml + sdk) to replicate.
- `crates/gnr8/src/doctor.rs` + `crates/gnr8/src/main.rs` (`run_doctor`/`run_check`/`run_watch`) — the
  health/drift/watch surface to generalize for python/ts (today Go-toolchain-specific).
- `crates/gnr8-core/src/analyze/mod.rs` (`detect_language` + the `*ToolchainMissing` errors) — the single
  source of truth for which sidecar/toolchain a target needs.
- `crates/gnr8-core/src/sdk/prelude` (`FastApi`/`Flask`/`NestJs` Sources; `OpenApi31`/`GoSdk`/`PySdk`/
  `TsSdk` targets) — the building blocks the example `.gnr8/` crates compose.
- `crates/gnr8-core/src/lifecycle/` + `manifest/` (blake3 ownership, no-op detection, loop-safe watch) —
  reused for the new languages' output.
- `docs/USAGE.md`, `docs/demo.md`, `docs/evidence.md` — the docs to extend with the multi-language envelope.
- `fixtures/fastapi-bookstore/`, `fixtures/nestjs-bookstore/` — real services usable as example sources.

### Established Patterns
- `.gnr8/` Pipeline crate → `gnr8 generate` → committed `generated/`; `gnr8 check` = drift gate;
  `gnr8 watch` = loop-safe regen; `gnr8 doctor` = health aggregate. Determinism via byte-identical regen.
- `make check` is the green gate; toolchains: Go (at /home/vercel-sandbox/.local/go-install/go/bin),
  Python3 3.9.25, Node v24 + typescript (resolved from the target project; gnr8 ships none — a gitignored dev install backs gnr8's own tests via `make tsextract-deps`). PATH persisted in ~/.bashrc.

### Integration Points
- New `examples/*/` projects + their `.gnr8/` crates + committed `generated/`; doctor/check/watch
  toolchain-detection generalization; docs/USAGE.md envelope; CLAUDE.md + PROJECT.md carve-out record;
  a cross-language determinism gate in `make check`.

</code_context>

<specifics>
## Specific Ideas

- Prove each language end-to-end from a real `.gnr8/` lifecycle with committed output (the v1 bookstore pattern).
- doctor/check/watch must follow the source language's toolchain (single deterministic detection, reuse detect_language).
- Record that `typescript` is a REQUIRED USER TOOLCHAIN (resolved from the target project, gnr8 ships none) + the bright line in CLAUDE.md + PROJECT.md — NOT a rule-2 OSS-dependency exception.
- The committed example output IS the cross-language determinism proof (regen-and-diff in make check).

</specifics>

<deferred>
## Deferred Ideas

- FUT-01..03 (Hono/Express/Fastify/Rust sources); FUT-04 (stdlib-pure TS path retiring the carve-out) — future milestones.
- Retiring the v1 known-debt (goextract golang.org/x/tools; compile-time goextract path) — separate cleanup.
- Any Phase-5 hardening item (WR-02/WR-04) not folded in here → backlog 999.x.

</deferred>
