---
phase: 2
slug: python-source-pyextract
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-25
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `make check`; Python sidecar self-tests via `python3 -m unittest` (stdlib only) |
| **Config file** | `Cargo.toml` workspace + `Makefile` (existing); `pyextract/` stdlib package |
| **Quick run command** | `cargo test -p gnr8-core` (after `source ~/.bashrc` for cargo+go+python) |
| **Full suite command** | `make check` |
| **Estimated runtime** | ~60-150 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test -p gnr8-core` (+ `python3 -m unittest discover pyextract` for sidecar tasks)
- **After every plan wave:** `make check`
- **Before verify:** `make check` green AND the FastAPI fixture snapshots flipped green (no longer `#[ignore]` red)
- **Max feedback latency:** 150 seconds

> The acceptance signal for this phase is **snapshot tests flipping from red-by-design to GREEN**: the
> FastAPI graph+OpenAPI snapshots MUST pass through real extraction; Flask snapshots pass to the honest
> typed-envelope limit (any intentionally-still-red Flask gap must be documented + kept out of the green gate).

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-W0 | 0x | 0 | PYSRC-05 | T-static-exec | Language-aware build_graph dispatch + run_pyextract driver + PythonToolchainMissing; subprocess spawns discrete args (no shell) | unit | `cargo test -p gnr8-core` | ❌ W0 | ⬜ pending |
| 02-W1 | 1x | 1 | PYSRC-03 | T-static-exec | stdlib `ast` load + owned cross-module symbol table; NEVER imports/executes target (static-only) | unit | `python3 -m unittest discover pyextract` | ❌ W0 | ⬜ pending |
| 02-W2 | 2x | 2 | PYSRC-01 | — | FastAPI routes/params/bodies/responses/status → neutral facts; FastAPI snapshots GREEN | snapshot | `cargo test -p gnr8-core --test snapshot_fastapi_graph --test snapshot_fastapi_openapi` | ✅ | ⬜ pending |
| 02-W3 | 3x | 3 | PYSRC-02, PYSRC-04 | — | Flask routes/blueprint prefixes/typed DTOs; untyped/dynamic → diagnostics (no fallback); Flask snapshots green-to-limit | snapshot | `cargo test -p gnr8-core --test snapshot_flask_graph --test snapshot_flask_openapi` | ✅ | ⬜ pending |
| 02-Wn | nx | n | PYSRC-05 | — | `.gnr8/` FastApi/Flask Source built-ins drive the Pipeline end-to-end | unit | `cargo test -p gnr8-core` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `CoreError::PythonToolchainMissing` variant + `run_pyextract`/`pyextract_dir` driver (mirror goextract)
- [ ] Language-aware `build_graph` + `diagnostics::collect` dispatch (the load-bearing host seam)
- [ ] Sidecar test-harness wiring so the FastAPI/Flask snapshot tests drive `pyextract` (replace the `.expect()`-panic-with-no-extractor)
- [ ] `pyextract/` stdlib package skeleton + `python3 -m unittest` entry

*Reuses existing Rust snapshot/determinism harness; no new framework. No new OSS (rule 2).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Sidecar never imports/executes target code | PYSRC-03 | Static-only is a security boundary; assert by construction (no `import`/`exec`/`eval`/`compile` of target) + code read | grep the sidecar for `importlib`/`exec(`/`eval(`/`__import__`/`compile(` on target paths — must be absent; only `ast.parse` of file TEXT is allowed |
| Flask honest-limit gaps are intentional, not bugs | PYSRC-02, PYSRC-04 | Distinguishing documented typed-envelope limit from a real miss needs human read | Review any still-red Flask assertion against the documented envelope in the SUMMARY/USAGE |

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 150s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
