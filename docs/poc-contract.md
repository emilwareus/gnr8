# gnr8 PoC Contract

**Status:** Locked (Phase 1) · **Last updated:** 2026-06-24

This document is the proof-of-concept (PoC) contract for the gnr8 v1.0 milestone. It is committed
**before** analyzer/lowering/SDK work begins (POC-02). It records the locked scope, the locked
generation surface, and the explicit non-goals. **Any change to this contract — especially pulling a
non-goal into scope — requires an explicit roadmap update (POC-03).**

---

## 1. Scope lock (POC-01)

The first milestone is locked to a single vertical slice:

> **Go source → OpenAPI → Go SDK.**

Nothing else this milestone (D-01):

- **One source language:** Go. No TypeScript / Python / Rust source frontends.
- **One output spec:** OpenAPI (see version below). OpenAPI is an **output artifact**, not the
  internal model — the internal API graph is the source of truth.
- **One SDK target:** Go. No TypeScript / Python / Rust SDK targets.

The point of the narrow slice is to prove the graph model and the owned pipeline end-to-end before
the design is generalized to more languages or targets.

---

## 2. Locked surface (POC-02)

### 2.1 Router family — Gin only

The single supported router family for the PoC is **Gin** (`gin-gonic/gin`) (D-02). Recognized
patterns:

| Concern | Recognized Gin pattern |
|---------|------------------------|
| Route group / prefix | `router.Group(prefix)` |
| Route registration | `group.METHOD(path, handler)` (e.g. `group.POST("/", h.Create)`) |
| Request body binding | `c.ShouldBindJSON(&t)` |
| Path parameters | `c.Param("name")` |
| Query parameters | `c.Query("name")` |

Although Gin is the only **recognized** router, the internal graph stores **router-agnostic HTTP
route facts** — method, path template, params, request type, and response type + status — and never
Gin internals. This seam (D-03) lets chi / echo / net-http be added later without reshaping the
graph. No Gin-specific assumptions are baked into the graph types.

Extraction is **code-first**: API facts are derived from route registration, handler binding, and
struct tags — the code is the single source of truth. Facts the typed source genuinely cannot express
(security schemes, the mount/base path, the document title) are declared in the engine config, never
inferred. Where neither the code nor the config can supply a fact, lowering emits a diagnostic.

### 2.2 OpenAPI target version — 3.1.0

The OpenAPI output target is **3.1.0** (JSON-Schema-aligned, modern) (D-04). Lowering must emit a
**diagnostic** when a graph fact cannot be represented cleanly in OpenAPI (OAPI-03 groundwork).
OpenAPI 3.0 downstream-generator compatibility is a future concern, behind diagnostics — not PoC
scope.

### 2.3 Go SDK shape

The generated Go SDK is a single SDK package shaped as idiomatic Go (D-05):

- A **`Client`** constructed with **functional options** (base URL + a custom `*http.Client`).
- **Typed operation methods**, with `context.Context` as the first argument.
- Generated **request / response model structs**.
- JSON encode / decode of bodies.
- A **typed API error**.

### 2.4 `.gnr8/` layout (now implemented)

The `.gnr8/` project-local workspace is split into two lifecycles (D-06):

- A **checked-in code-as-config** directory — a small Rust **binary crate** (`Cargo.toml` + `src/main.rs`)
  that depends on `gnr8-core` and drives generation. **Code is the configuration; there is no TOML / YAML
  / JSON config file.** `gnr8 init` scaffolds it and `gnr8 generate` compiles + runs it.
- A **git-ignored cache / output lifecycle** subtree (`.gnr8/target/`, `.gnr8/cache/`).

The customization surface is the `gnr8::sdk` API: a `Pipeline` of `Source` / `Transform` / `Target` /
`PostProcess` stages, with built-ins for the common cases and user-implemented traits for the rest. See
[`code-as-config.md`](code-as-config.md) and [`extensibility.md`](extensibility.md) for the full design,
and `docs/USAGE.md` for the reference.

---

## 3. Non-goals (POC-03)

The following are explicitly **out of scope** for the PoC milestone (copied verbatim from
`REQUIREMENTS.md` "Out of Scope"). **Any item entering the PoC requires an explicit roadmap update.**

| Feature | Reason |
|---------|--------|
| Multi-language source implementation | Go must prove the graph model first. |
| Multi-language SDK generation | The first Go SDK must be high quality before targets multiply. |
| Dynamic plugin loading | Too much lifecycle and stability surface before repeated extension pressure exists. |
| Macro-heavy configuration API | Plain Rust code should be tested first. |
| Graph database | Stable IDs and typed structs are sufficient for the PoC. |
| Full Go framework coverage | One or two router styles are enough to validate the loop. |
| Full OpenAPI 3.2 coverage | Useful modern output matters before complete spec coverage. |
| Arbitrary handler body interpretation | Static analysis should start with explicit supported patterns and diagnostics. |
| Wrapping existing generators as the core | The product promise is an owned native pipeline. |

---

## 4. Router-agnostic seam note (D-03)

To restate the most easily-forgotten constraint: even though **Gin is the only recognized router**,
the internal graph stores **router-agnostic HTTP route facts** (method, path template, params,
request type, response type + status). This keeps chi / echo / net-http addable later without
reshaping the graph or revisiting OpenAPI lowering and SDK generation. Do not introduce
router-framework-specific fields into the graph types.

---

## 5. CI policy — blocking gates vs. the red-by-design contract suite (RUST-03 + FIX-04)

Phase 1 deliberately ships a test suite that is **red by design**: the four contract snapshot tests
(`crates/gnr8-core/tests/snapshot_{graph,openapi,sdk,diagnostics}.rs`) call gnr8-core seams that
still return `CoreError::NotYetImplemented`, so they **fail clearly today** (FIX-04 — they are never
`#[ignore]`d / silently skipped). This is in direct tension with RUST-03, which requires
`cargo fmt` / `cargo clippy -D warnings` / `cargo test` to **pass**.

These two requirements are reconciled with **Open Question 1, option (d)** from the phase research:
**split the gate from the contract suite.** CI (`.github/workflows/ci.yml`) and local `make` define
three concerns:

| Job / target | Blocking? | What it runs | Status today |
|--------------|-----------|--------------|--------------|
| `gates` (CI) / `make gates` | **BLOCKING** | `cargo fmt --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, and the **genuinely-green** tests only (`cargo test -p gnr8-core --lib && cargo test -p gnr8` — the integration `tests/` dir is excluded) | **Green.** Enforces RUST-03; a real regression (compile / clippy / unit / parse failure) still blocks merges. |
| `go-fixture` (CI) / `make fixture-build` | **BLOCKING** | `go build ./...` + `go vet ./...` in `fixtures/goalservice/` | **Green.** Keeps the standalone Go fixture (which cargo never compiles) from rotting. |
| `contract` (CI) / `make contract` | **NON-BLOCKING** (`continue-on-error: true`) | The four red-by-design snapshot tests | **Red on purpose.** Visibly fails (FIX-04) without blocking merges. |

**Why a `.expect()`-driven red (not a missing snapshot):** the primary redness mechanism is a
panicking `.expect()` on the stubbed seam, which fires *before* any `insta::assert_*`. Because CI sets
`CI=true`, insta runs in `INSTA_UPDATE=no` mode, so it never auto-accepts an empty snapshot — but the
panic happens first anyway, so a real failing assertion is what turns the suite red. No `.snap` files
are pre-authored in Phase 1.

**Local parity:** `make check` runs the *full* gate, so `make test` shows the same four red contract
failures a developer would see in the non-blocking CI job — the contract is visible locally, not just
in CI.

**Promotion to blocking:** the `contract` job is promoted to a **blocking** gate once **Phase 3**
lands the analyzer, OpenAPI lowering, and Go SDK generation and the four snapshots are reviewed and
accepted (turning the suite green). Until then, blocking enforcement lives in `gates` + `go-fixture`.
