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

Extraction is **code-first, not comment-first**: API facts are inferred primarily from route
registration, handler binding, and struct tags. Swaggo-style comment annotations are read only as an
**escape hatch** to fill gaps the code cannot express, and to drive diagnostics when both code and
annotations are silent.

### 2.2 OpenAPI target version — 3.1.0

The OpenAPI output target is **3.1.0** (JSON-Schema-aligned, modern) (D-04). Lowering must emit a
**diagnostic** when a graph fact cannot be represented cleanly in OpenAPI (OAPI-03 groundwork).
OpenAPI 3.0 downstream-generator compatibility is a future concern, behind diagnostics — not PoC
scope.

### 2.3 Go SDK shape

The generated Go SDK is a single SDK package shaped as idiomatic Go (D-05) — **not** the verbose
openapi-generator builder pattern:

- A **`Client`** constructed with **functional options** (base URL + a custom `*http.Client`).
- **Tag-grouped typed operation methods**, with `context.Context` as the first argument.
- Generated **request / response model structs**.
- JSON encode / decode of bodies.
- A **typed API error**.

### 2.4 `.gnr8/` layout (documented now, implemented Phase 4)

The `.gnr8/` project-local workspace is split into two lifecycles (D-06):

- A **checked-in code-as-config** customization directory (code is the configuration; YAML / TOML /
  JSON is not the main customization surface).
- A **git-ignored cache / output lifecycle** directory.

The detailed customization surface and the code-as-config language are **deferred to Phase 4**. Phase
1 records only the intended split so the contract is stable.

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
