---
phase: 4
slug: typescript-source-tsextract
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-06-25
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `make check`; tsextract sidecar driven via `node` + the `typescript` Compiler API |
| **Config file** | `Cargo.toml` workspace + `Makefile`; `tsextract/package.json` (sole dep: `typescript`) |
| **Quick run command** | `cargo test -p gnr8-core` (after `source ~/.bashrc`; node v24 + npm on PATH) |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60-180 seconds (+ one-time `npm install`/`npm ci` for typescript) |

---

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core`
- **After every plan wave:** `make check`
- **Before verify:** `make check` green AND the 2 NestJS snapshots flipped green (no longer `#[ignore]`)
- **Max feedback latency:** 180 seconds

> Acceptance signal: the `snapshot_nestjs_graph` + `snapshot_nestjs_openapi` tests flip from red-by-design to
> GREEN through real Compiler-API extraction — ZERO snapshot edits (committed snapshots are the byte spec).

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 04-01 | 01 | 1 | TSSRC-04 | T-static-exec | Lang::TypeScript 3-arm dispatch in build_graph + diagnostics::collect; run_tsextract driver; TS-toolchain-missing typed error; NestJs Source built-in | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 04-02 | 02 | 2 | TSSRC-02, TSSRC-03 | T-static-exec, T-brightline | tsextract on the `typescript` Compiler API; facts from source's own TS types ONLY (never swagger/zod/class-validator); never executes target; unresolved→diagnostic | unit | `node tsextract/<entry> fixtures/nestjs-bookstore` (+ rust facts round-trip) | ❌ W0 | ⬜ pending |
| 04-03 | 03 | 3 | TSSRC-01 | — | NestJS routes/params/request+response DTOs → neutral facts; method-derived status; 2 NestJS snapshots GREEN | snapshot | `cargo test -p gnr8-core --test snapshot_nestjs_graph --test snapshot_nestjs_openapi` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `Lang::TypeScript` + 3-language `detect_language`/`scan_markers` (TS marker `*.ts`/`tsconfig.json`); 3-arm dispatch in BOTH `build_graph` and `diagnostics::collect`
- [ ] `run_tsextract`/`tsextract_dir` driver + TS-toolchain-missing `CoreError` variant (mirror Python)
- [ ] `tsextract/` Node package skeleton (`package.json` sole dep `typescript`); vendoring strategy decided (commit `node_modules/typescript` for hermetic test OR `npm ci` step) + `.gitignore` updated accordingly
- [ ] Snapshot-test harness wired so the NestJS tests drive `tsextract`; skip-if-(node|typescript)-absent guard (mirror the Go-toolchain skip)

*Reuses the existing Rust snapshot/determinism harness; gnr8-core adds NO crate; tsextract's sole dep is `typescript` (the documented rule-2 carve-out).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Bright-line: schema facts never come from swagger/zod/class-validator | TSSRC-02 | Rule-1 boundary — assert by construction + code read | grep `tsextract/` for `swagger`/`zod`/`class-validator` reads — must be absent; schema facts come from the TypeChecker only |
| Sidecar never executes target TS | TSSRC-03 | Static-only security boundary | grep `tsextract/` for `require(<target>)`/`import(<target>)`/`eval`/`vm` of target modules — must be absent; only `ts.createProgram` over source text |

---

## Validation Sign-Off

- [x] All tasks have automated verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 180s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-06-25
