# Phase 1: Foundation And Fixtures - Context

**Gathered:** 2026-06-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss --auto — Claude selected recommended defaults)

<domain>
## Phase Boundary

Establish the smallest Rust workspace and fixture harness that can drive all later
implementation. This phase delivers: a thin CLI skeleton with the planned command surface, a
realistic Go (Gin) fixture service that encodes the PoC acceptance cases, snapshot/test scaffolding
that defines expected graph/OpenAPI/SDK/diagnostic behavior *before* the analyzer exists, and wired
Rust quality gates. No real Go analysis, OpenAPI lowering, or SDK generation logic is implemented
here — those are Phases 2–3. This phase only locks the contract and builds the harness that fails
loudly until later phases fill it in.

</domain>

<decisions>
## Implementation Decisions

### PoC Contract Lock (POC-01, POC-02, POC-03)
- **D-01:** Scope is locked to **Go source → OpenAPI → Go SDK**. No other source languages or SDK
  targets in this milestone.
- **D-02:** First supported router family is **Gin** (`gin-gonic/gin`): `router.Group(prefix)` +
  `group.METHOD(path, handler)` registration, `c.ShouldBindJSON(&t)` request binding, `c.Param`/
  `c.Query` param access. This is the single PoC router. Rationale: it is the real-world target
  (see `.planning/research/TARGET-API.md`) and the user's stated primary framework.
- **D-03:** The extraction model is **router-agnostic**: the graph stores *HTTP route facts* (method,
  path template, params, request type, response type+status), not Gin internals, so chi/echo/
  net-http can be added later without reshaping the graph. Do NOT bake Gin-only assumptions into the
  graph types. (This supersedes the earlier `go-chi-basic` placeholder in `thoughts/`.)
- **D-04:** OpenAPI target version is **3.1.0** (JSON-Schema-aligned, modern). Lowering must emit a
  diagnostic when a graph fact cannot be represented cleanly (OAPI-03 groundwork). 3.0 downstream-
  generator compatibility is a future concern, not PoC scope.
- **D-05:** Go SDK shape: a single SDK package exposing a **`Client`** constructed with functional
  options (base URL + custom `*http.Client`), **tag-grouped typed operation methods**, generated
  **request/response model structs**, JSON encode/decode, and a **typed API error**. Idiomatic Go
  (`context.Context` first arg) — NOT the verbose openapi-generator builder pattern.
- **D-06:** `.gnr8/` layout (documented now, implemented in Phase 4): checked-in **code-as-config**
  customization directory + git-ignored **cache/output lifecycle** directory. Detailed customization
  surface is deferred to Phase 4; Phase 1 only records the intended split so the contract is stable.

### Rust Workspace Shape (RUST-01, RUST-04)
- **D-07:** Cargo **workspace** with a library crate `gnr8-core` (extraction, graph, lowering, SDK gen,
  diagnostics live here as modules) and a thin binary crate `gnr8` (CLI only). Keeps later modules
  testable and the binary thin.
- **D-08:** Edition **2021**, latest stable toolchain. Pin shared lints via `[workspace.lints]`.
- **D-09:** Errors: **`thiserror`** typed errors in library code; **`anyhow`** only at the binary
  boundary (CLI `main`). No production `unwrap`/`expect` in library paths (RUST-04). Test helpers may
  use `anyhow`.

### CLI Surface (RUST-02)
- **D-10:** **`clap`** (derive API) for argument parsing.
- **D-11:** Command surface present (skeletal where logic is not yet built): `init`, `generate`,
  `watch`, `check`, `inspect`, `doctor`. `inspect` has subcommands `routes | schemas | graph`.
- **D-12:** Output: human-readable tables by default, machine output behind a global `--json` flag;
  `-v/--verbose` for detail. Skeletal commands parse args and exit with a clear "not yet implemented
  in phase N" message rather than panicking.

### Fixture & Snapshot Harness (FIX-01, FIX-02, FIX-03, FIX-04)
- **D-13:** Snapshot testing via the **`insta`** crate. Snapshots kept small and reviewable.
- **D-14:** Go fixture is a **real Gin service module** under `fixtures/` mirroring
  `.planning/research/TARGET-API.md` §6: a CRUD resource (POST/GET-list/PUT/DELETE) plus a
  list-with-query-filters read resource. Covers path params, request bodies, response bodies, JSON
  tags, optional (pointer/omitempty) fields, nested structs, enum newtypes, well-known types
  (`uuid.UUID`, `time.Time`), error responses, an auth-middleware group, and **at least one
  unsupported pattern** (e.g. `map[string]any` field / dynamically-built response) for diagnostics.
- **D-15:** Expected `graph`, `openapi`, `sdk`, and `diagnostics` snapshots are defined as the
  acceptance target. Because the analyzer does not exist yet, these tests must **fail clearly**
  (FIX-04) — e.g. asserting on not-yet-produced output — not be silently skipped. Mark them so the
  suite is red-by-design until Phases 2–3 land.

### Quality Gates (RUST-03)
- **D-16:** Local + CI gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features --locked
  -- -D warnings`, `cargo test`. Wrapped in a **`Makefile`** (familiar entrypoint) and enforced by a
  **GitHub Actions** CI workflow.

### Claude's Discretion
- Exact crate module layout inside `gnr8-core`, snapshot file naming, Makefile target names, CI matrix
  details, and the precise wording of skeletal-command messages are left to planning/execution.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### PoC contract & target shape
- `.planning/PROJECT.md` — Vision, constraints (Rust, code-as-config, code-first, owned pipeline).
- `.planning/REQUIREMENTS.md` — POC-01..03, RUST-01..04, FIX-01..04 acceptance criteria (Phase 1).
- `.planning/ROADMAP.md` §"Phase 1" — Goal, success criteria, the 3 planned plans.
- `.planning/research/TARGET-API.md` — **Primary target spec.** Gin route/handler/DTO patterns,
  Go→OpenAPI→Go-SDK type-mapping table, pitfalls→diagnostics list, and the fixture plan (§6) that
  D-14/D-15 implement.

### Architecture & Rust guardrails
- `.planning/research/ARCHITECTURE.md` — Owned pipeline stages; internal API graph is source of truth.
- `.planning/research/STACK.md`, `.planning/research/PITFALLS.md` — Stack choices and known traps.
- `thoughts/ARCHITECTURE.md`, `thoughts/research/` — Original architecture/SDK/lifecycle research.
- `thoughts/skills/rust-best-practices/` (vendored skill) — `thiserror`/`anyhow` boundaries, clippy
  invocation, fixture-test and snapshot guidance, benchmark-before-optimize. MUST follow.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- No Rust code exists yet — greenfield. Repo currently contains only `.planning/`, `thoughts/`,
  `.context/`, `LICENSE`, `.gitignore`. This phase creates the first code.

### Established Patterns
- Project conventions are doc-defined (PROJECT.md constraints): typed errors, no prod `unwrap`,
  clippy-clean, realistic fixtures, small snapshots, code-as-config under `.gnr8/`.

### Integration Points
- `gnr8-core` modules will be consumed by the `gnr8` binary and by snapshot tests.
- Go fixture module under `fixtures/` is the input that Phase 2's analyzer will read.
- Expected snapshots authored here become the contracts Phases 2–3 must satisfy.

</code_context>

<specifics>
## Specific Ideas

- Fixture should mirror a real production Gin service, not a toy — derived from the
  `.planning/research/TARGET-API.md` reduction (CRUD + list-with-filters, nested/enum/array DTOs,
  well-known types, validation tags, error responses, auth group).
- Snapshots should be reviewable in PRs (small, scoped per concern: routes / schemas / openapi / sdk /
  diagnostics) rather than one giant golden file.

</specifics>

<deferred>
## Deferred Ideas

- Detailed `.gnr8/` customization surface and code-as-config language — Phase 4.
- Additional router families (chi, echo, net/http) — post-PoC (v2); only the generic extraction seam
  is reserved now.
- OpenAPI 3.0 downstream-generator compatibility mode — future, behind diagnostics.
- TypeScript/Python SDK targets — v2 (out of scope this milestone).

</deferred>

---

*Phase: 01-foundation-and-fixtures*
*Context gathered: 2026-06-24 (auto mode)*
