# Phase 2: Go Analysis And API Graph - Research

**Researched:** 2026-06-24
**Domain:** Go static analysis (`go/packages` + `go/ast` + `go/types`), Rust↔Go subprocess JSON contract, deterministic API-graph modeling + inspect reports
**Confidence:** HIGH (stack, type-mapping, contract) / MEDIUM (some `go/types` selector edge cases)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Parse Go via a small **Go sidecar helper** using the official `golang.org/x/tools/go/packages` loader (with `go/ast` + `go/types`), NOT a pure-Rust parser. Accurate type resolution (GO-03) and request/response schema inference (GO-05) require `go/types`. Target users have the Go toolchain installed.
- **D-02:** The helper is a Go program in its own module in this repo (`goextract/`), invoked by the Rust `gnr8-core` analyzer as a subprocess. It emits a single **JSON facts document** (stable, sorted) on stdout that the Rust side deserializes via `serde`. The JSON is the Rust↔Go contract boundary.
- **D-03:** Helper invocation: prefer running a prebuilt/located helper; for the PoC, invoking via the Go toolchain (`go run ./goextract <target>`) is acceptable. Surface a clear typed diagnostic (not a panic) when the Go toolchain or target module is missing/unbuildable (GO-06).
- **D-04:** Recognize the **Gin** call patterns from TARGET-API.md: `router.Group(prefix)` + `group.METHOD(path, handler)` (METHOD ∈ GET/POST/PUT/DELETE), middleware on a group → security/marker fact, `c.ShouldBindJSON(&t)` → request body type, `c.Param("x")` → path param, `c.Query("x")` → query param. Extraction produces router-agnostic facts; Gin specifics stay in the recognizer, NOT the graph (honors Phase-1 D-03).
- **D-05:** Response inference: from supported typed handler patterns (`c.JSON(http.StatusXxx, dtoValue)`), infer status→response-type. Where the handler builds responses dynamically or the type can't be resolved, emit a diagnostic rather than guessing.
- **D-06:** Map Go types to graph schema types per TARGET-API.md §4: primitives, `bool`, ints, `float64`; pointers/`omitempty` → optional; `[]T` → array; `map[string]T` → object/additionalProperties; named structs → schema ref; embedded structs → field flattening; type aliases → underlying; `uuid.UUID` → string(uuid), `time.Time` → string(date-time); named-string-with-consts → enum. `json` tags drive field names; `binding:"required"` drives required. Unsupported/uncertain types → diagnostic.
- **D-07:** The internal `ApiGraph` models: routes, operations, parameters (path/query), request bodies, responses (by status), schemas (with fields), generated-file placeholders (filled later phases), and **source provenance** (file + line span) on every node.
- **D-08:** Node IDs are **deterministic and stable across unchanged runs** (GRAPH-02): operation IDs from method + normalized path; schema IDs from package-qualified type name. All report/serialized output sorted by a stable key so unchanged source ⇒ byte-identical output.
- **D-09:** `inspect routes`, `inspect schemas`, `inspect graph` render human-readable tables by default and machine JSON under the global `--json` flag (reuse Phase-1 CLI surface). Reports explain inferred facts and list diagnostics.
- **D-10:** Diagnostics carry a severity, a message, and a **source location** (file:line) for unsupported patterns. Unsupported/uncertain inference NEVER panics and NEVER silently drops — it produces a diagnostic (GO-06). `diagnostics::collect` output must match the Phase-1 `expected/diagnostics.txt` acceptance target (the `snapshot_diagnostics` test locks the exact text).

### Claude's Discretion
- Exact `goextract` JSON schema shape, internal Rust graph struct layout, the deterministic ID hashing/format, table column choices, and whether the helper is `go run` vs a built binary cached under `target/` — left to research/planning. Snapshot contents (graph/diagnostics) authored to match the Phase-1 `expected/` targets.

### Deferred Ideas (OUT OF SCOPE)
- OpenAPI lowering + Go SDK generation — Phase 3 (this phase stops at graph + diagnostics + inspect).
- Additional routers (chi/echo/net-http) — post-PoC; only the router-agnostic graph seam is reserved.
- Incremental/partial graph invalidation + watch — Phase 4.
- Deep handler-body interpretation beyond supported typed patterns — out of scope (diagnose instead).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| GO-01 | Analyzer discovers Go packages and source files for configured inputs | `packages.Load` with `Dir` set to the target module + pattern `./...`; LoadMode `NeedName\|NeedFiles\|NeedSyntax\|NeedTypes\|NeedTypesInfo\|NeedDeps\|NeedImports\|NeedTypesSizes` (= `LoadAllSyntax`). See §1. |
| GO-02 | Extracts structs, fields, JSON tags, source spans, basic schema facts | Walk `*types.Struct` via `Field(i)`/`Tag(i)`/`Embedded()`; parse tags with `reflect.StructTag`; `token.Position` for spans. See §3. |
| GO-03 | Maps common Go types: primitives, pointers, slices, maps, named structs, aliases, `time.Time` | `go/types` kind switch (`*types.Basic/Pointer/Slice/Map/Named`) + `types.Unalias`; well-known pkg-path match for `uuid.UUID`/`time.Time`. See §3 + type-mapping table. |
| GO-04 | Recognizes router call patterns; extracts method, path, router family, handler symbol, source span | `types.Info.Selections` to resolve `*gin.RouterGroup` method identity (version-robust); group-prefix chain resolution; handler func from call arg. See §2. |
| GO-05 | Infers request/response schemas for supported typed handler patterns | `c.ShouldBindJSON(&x)` → `TypeOf(x)`; `c.JSON(http.StatusXxx, y)` → status const + `TypeOf(y)`; fall back to swaggo annotations; diagnose when neither resolves. See §2.3–2.5. |
| GO-06 | Unsupported/uncertain inference produces diagnostics, not panics or silent omissions | Helper emits a `diagnostics[]` array (severity+message+file:line) for every lossy/unknown case; Rust never `unwrap`s subprocess output; typed `CoreError` variants for toolchain/build/exit failures. See §5 + §8. |
| GRAPH-01 | Internal graph models routes, operations, params, request bodies, responses, schemas, generated files, provenance | Rust `ApiGraph` struct layout in §4.2; provenance `SourceSpan` on every node (D-07). |
| GRAPH-02 | Graph node IDs and outputs stable across unchanged runs | Sort everything by stable key in the Go helper before emit; derive IDs from source identity (method+normalized-path, pkg-qualified type name); byte-identical reports. See §6. |
| GRAPH-03 | `inspect routes\|schemas\|graph` explain inferred facts and diagnostics | Wire the three `InspectAction` arms to render tables (human) / JSON (`--json`); each surfaces diagnostics. See §7. |
</phase_requirements>

## Summary

This phase builds a Go **sidecar helper** (`goextract/`, its own `go.mod`) that loads the fixture module with the official `golang.org/x/tools/go/packages` loader in full-type mode, recognizes the Gin route/handler patterns via `go/ast` walked against `go/types` (so method identity is resolved semantically, not by string-matching package aliases), extracts DTO structs/fields/tags, and emits one **deterministic, sorted JSON facts document** on stdout. The Rust `gnr8-core::analyze::build_graph` runs the helper as a subprocess, deserializes the JSON with serde into an `ApiGraph`, and `diagnostics::collect` renders the diagnostics list. The two red-by-design contract tests (`snapshot_graph`, `snapshot_diagnostics`) flip to real `insta` snapshots, and the three `inspect` CLI arms render tables/JSON.

The single most important discovery from the fixture is that **the expected acceptance outputs depend on swaggo `// @...` annotations, not just code inference**. The expected `openapi.yaml` shows `operationId: goalUuidPut` (from `@ID goalUuidPut`), query-param types/required-ness and the `aggregation` enum (from `@Param ... Enums(...)`), operation summaries (from `@Summary`), and the `X-API-Key` security scheme (from `@Security ApiKeyAuth`). The expected `diagnostics.txt` explicitly says `aggregation`'s value set is "recovered only from the swaggo `Enums(...)` annotation, not from code." Therefore the Phase-2 graph must capture **both** code-inferred facts **and** parsed swaggo doc-comment facts, with code as primary and annotations as the escape hatch (exactly the TARGET-API.md thesis). `go/packages` with `NeedSyntax` gives the doc comments (`ast.FuncDecl.Doc`), so no extra tooling is needed — just a small comment parser in the helper.

The second critical finding: **`diagnostics.txt` is an exact-text snapshot** (7 specific WARN lines in a specific order). `diagnostics::collect` must reproduce those lines byte-for-byte. The 7 diagnostics decompose into three rules (float64-narrowing ×3, free-form-map ×1, untyped-query-param ×3) — every one is derivable from the fixture, so the helper must emit exactly these and nothing more.

**Primary recommendation:** Build `goextract/` as a single-purpose facts extractor (Go → JSON), keep ALL Gin/OpenAPI knowledge out of the JSON shape (router-agnostic HTTP facts only), make the helper sort and stably-ID everything before emit, and have Rust treat the helper purely as `Command` → stdout JSON → serde → `ApiGraph`, mapping every failure mode to a typed `CoreError` + diagnostic rather than a panic.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Go package discovery + type resolution | Go helper (`goextract`) | — | Only `go/packages`+`go/types` give semantic truth (D-01); Rust cannot type-check Go. |
| Gin route/handler recognition | Go helper (recognizer) | — | D-04: Gin specifics confined to the helper; graph stays router-agnostic. |
| Struct/field/tag/type-mapping extraction | Go helper | — | Needs `go/types` resolved types + struct tags. |
| Swaggo annotation parsing (escape hatch) | Go helper | — | Doc comments available via `ast.FuncDecl.Doc` from same load; keep parse in helper. |
| Diagnostics generation (file:line) | Go helper (emit) | Rust (render/serialize) | Helper has the positions; Rust formats the final text (D-10). |
| Subprocess invocation + failure→typed error | Rust `analyze` | — | D-03/GO-06: typed `CoreError`, no panic. |
| `ApiGraph` modeling + stable IDs + sorted serialization | Rust `gnr8-core::graph` | Go helper (pre-sort) | D-07/D-08: graph is the Rust-owned source of truth (GRAPH-02). |
| `inspect routes\|schemas\|graph` rendering | Rust `gnr8` binary | Rust `gnr8-core` (graph access) | D-09: CLI surface already exists in Phase 1. |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `golang.org/x/tools/go/packages` | v0.46.0 | Load Go module: packages, files, AST, full type info, positions | [CITED: pkg.go.dev/golang.org/x/tools/go/packages] The official, supported loader; the only API that gives `*types.Info` + ASTs + `*token.FileSet` together. Replaces the deprecated `go/loader`. |
| `go/ast` (stdlib) | go 1.26 | Walk syntax trees; doc comments; selector/call expressions | [CITED: pkg.go.dev/go/ast] Stdlib; paired with `go/types`. |
| `go/types` (stdlib) | go 1.26 | Resolve method/identifier identities, struct fields, type kinds | [CITED: pkg.go.dev/go/types] Stdlib; the semantic-truth layer (D-01). |
| `go/token` (stdlib) | go 1.26 | `token.FileSet.Position(pos)` → `{Filename, Line, Column}` for provenance/diagnostics | [CITED: pkg.go.dev/go/token] Stdlib. |
| `encoding/json` (stdlib) | go 1.26 | Marshal the facts document on stdout | Stdlib; deterministic with sorted slices + map-key sorting. |
| `reflect.StructTag` (stdlib) | go 1.26 | Parse `json:"..."` / `binding:"..."` / `description:"..."` / `example:"..."` tags | [CITED: pkg.go.dev/reflect#StructTag] Stdlib; `tag.Get("json")`/`tag.Lookup`. |
| `serde` / `serde_json` | 1.0 (pinned) | Rust-side deserialize the facts JSON; serialize `ApiGraph` / reports | Already in `[workspace.dependencies]`. |
| `thiserror` | 2.0 (pinned) | Extend `CoreError` with analysis/subprocess variants (D-09/RUST-04) | Already pinned; lib-level typed errors. |
| `insta` | 1.48 (pinned, `yaml`) | `snapshot_graph` (YAML) + `snapshot_diagnostics` (text) | Already pinned + harness wired in Phase 1. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `std::process::Command` (Rust stdlib) | 1.96 | Spawn the Go helper, capture stdout/stderr, read exit status | Always — the subprocess boundary (D-02/D-03). |
| `cargo-insta` (CLI) | latest | `cargo insta review`/`accept` to author the two `.snap` files | Dev convenience only; CI runs `INSTA_UPDATE=no` (hard-fail on mismatch). Not installed locally — install with `cargo install cargo-insta`. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Go sidecar (`go/packages`) | Pure-Rust tree-sitter-go / regex | REJECTED by D-01: no type resolution → cannot resolve `dto.CreateGoalInput` across files, cannot map `uuid.UUID`/`time.Time`, cannot follow embedded structs. TARGET thesis needs semantic truth. |
| `go/packages` | `go/loader` (`golang.org/x/tools/go/loader`) | DEPRECATED upstream; `go/packages` is the modules-aware successor. [CITED: pkg.go.dev/golang.org/x/tools/go/packages] |
| `go run ./goextract` per call | Prebuilt binary cached under `target/` | `go run` recompiles each invocation (slower) but zero build-step plumbing — fine for PoC (D-03). A prebuilt binary is the Phase-4 watch-mode optimization (deferred). Recommend **`go run` for this phase**, leave a seam to swap. |
| Helper emits JSON | Helper emits the graph directly | REJECTED: graph modeling + stable IDs + sorting is Rust-owned (D-07/D-08); keep the helper to *facts only* (router-agnostic), so the JSON boundary stays small and the graph can evolve without touching Go. |

**Installation (Go helper module `goextract/`):**
```bash
# goextract/go.mod — its own module, separate from fixtures/goalservice
cd goextract && go mod init github.com/gnr8/goextract
go get golang.org/x/tools/go/packages@v0.46.0   # pulls go/packages; go/ast,go/types,go/token,reflect,encoding/json are stdlib
go mod tidy
```
No new **Rust** crates are required — `serde`, `serde_json`, `thiserror`, `insta` are already pinned in `[workspace.dependencies]`. `std::process::Command` is stdlib.

**Version verification:**
- `golang.org/x/tools` **v0.46.0** — VERIFIED latest via `go list -m -versions golang.org/x/tools` (output ended `... v0.44.0 v0.45.0 v0.46.0`), published 2026-06-11 per pkg.go.dev. v0.45.0 already present in the local module cache.
- Go toolchain **go1.26.2** present (`go version go1.26.2 darwin/arm64`); fixture `go.mod` declares `go 1.26.2`. Use `go 1.26` in `goextract/go.mod`.
- Rust **1.96.0** / Cargo **1.96.0** present; workspace MSRV floor 1.85.

## Package Legitimacy Audit

> slopcheck could not be installed in this environment (`pip install slopcheck` failed — package not available). Per protocol, packages are tagged `[ASSUMED]` UNLESS confirmed via an authoritative source. `golang.org/x/tools` is published by the Go team under the official `golang.org/x` namespace and was confirmed via the Go module proxy + pkg.go.dev official docs, so it is `[CITED]`, not a slopsquat risk. No Rust packages are added this phase.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `golang.org/x/tools` | Go module proxy (proxy.golang.org) | ~9 yrs (official x/ repo) | n/a (Go has no download counter; ubiquitous) | github.com/golang/tools (official Go team) | unavailable | Approved — official Go team module, confirmed via `go list -m -versions` + pkg.go.dev |
| `serde` / `serde_json` / `thiserror` / `insta` | crates.io | already pinned in Phase 1 | — | — | unavailable | Already approved (Phase 1 RESEARCH) — no change |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

The fixture's own `go.sum` already pins Gin + transitive deps; `goextract/` is a *separate* module and only needs `golang.org/x/tools` (which transitively pulls `golang.org/x/mod`, `golang.org/x/sync` — all official Go team). When `goextract/` is created, commit its `go.mod` + `go.sum` so the CI build is reproducible (mirrors the `go-fixture` CI job pattern).

## Architecture Patterns

### System Architecture Diagram

```
  gnr8 CLI (gnr8/src/main.rs, dispatch)
    │  inspect routes|schemas|graph   (--json toggles render)
    ▼
  gnr8-core::analyze::build_graph(fixture_dir)            [Rust]
    │
    │ 1. locate Go toolchain (which/go), else CoreError::GoToolchainMissing + diagnostic
    │ 2. std::process::Command:  go run ./goextract <fixture_dir>
    │        (cwd = goextract module dir; arg = absolute fixture path)
    ▼
  goextract (Go helper)                                   [Go subprocess]
    │
    │  packages.Load(cfg{Dir:fixture_dir, Mode:LoadAllSyntax}, "./...")
    │       ├─► []*packages.Package  (Syntax ASTs, TypesInfo, Fset)
    │       │
    │       ├─ RouteRecognizer  ── walk ast for *gin.RouterGroup method calls
    │       │     • Group(prefix)  → prefix chain → full path template
    │       │     • GET/POST/PUT/DELETE(path, handler) → route + handler symbol
    │       │     • Use(mw)        → group security marker
    │       │     (identity resolved via types.Info.Selections, NOT alias strings)
    │       │
    │       ├─ HandlerAnalyzer ── for each handler FuncDecl:
    │       │     • ShouldBindJSON(&x)      → request type = TypeOf(x)
    │       │     • JSON(http.StatusX, y)   → response[status] = TypeOf(y)
    │       │     • Param("uuid")/Query("k")→ path/query params
    │       │     • Doc comments (@Param/@Success/@ID/@Security/Enums) → annotation facts
    │       │
    │       └─ TypeExtractor   ── reachable named structs:
    │             • Field(i)/Tag(i)/Embedded() → fields, json/binding/description tags
    │             • type kind switch → schema-type + well-known (uuid/time)
    │             • named-string + const set → enum
    │             • float64 / map[string]any → diagnostics
    │
    │  ►  SORT everything by stable key  ►  json.Marshal  ►  stdout
    ▼
  Facts JSON  ──(serde_json::from_slice)──►  GoFacts (Rust)
    │
    ▼
  graph::ApiGraph::from_facts(GoFacts)                    [Rust]
    • derive stable operation IDs (method + normalized path)
    • derive stable schema IDs (pkg-qualified type name)
    • attach SourceSpan provenance to every node
    • collect diagnostics
    ▼
  ApiGraph  +  Vec<Diagnostic>
    ├─► snapshot_graph        (insta::assert_yaml_snapshot)
    ├─► diagnostics::collect → String (insta::assert_snapshot, == expected/diagnostics.txt)
    └─► inspect renderers (tables / --json)
```

File-to-implementation mapping (Component Responsibilities):

| Component | File(s) | Owns |
|-----------|---------|------|
| Subprocess driver + JSON deserialize | `crates/gnr8-core/src/analyze/mod.rs` (+ submodules e.g. `analyze/helper.rs`, `analyze/facts.rs`) | Spawn `goextract`, capture stdout/stderr/exit, serde-parse `GoFacts`, map failures → `CoreError` |
| Graph model + stable IDs + provenance | `crates/gnr8-core/src/graph/mod.rs` | `ApiGraph` + child structs, `from_facts`, ID derivation, sorted serialization |
| Diagnostics collection/rendering | `crates/gnr8-core/src/diagnostics/mod.rs` | `collect(dir) -> String` matching `expected/diagnostics.txt` |
| Error variants | `crates/gnr8-core/src/error.rs` | `CoreError::{GoToolchainMissing, HelperBuild, HelperExit, FactsParse, ...}` |
| CLI render | `crates/gnr8/src/main.rs` + `crates/gnr8/src/cli.rs` | Wire 3 `InspectAction` arms to table/JSON renderers |
| Go facts extractor | `goextract/main.go` + `goextract/internal/...` | Load, recognize, extract, sort, emit JSON |

### Recommended Project Structure
```
goextract/                      # NEW Go module (its own go.mod) — D-02
├── go.mod                      # module github.com/gnr8/goextract ; go 1.26 ; require x/tools v0.46.0
├── go.sum                      # committed for reproducible CI build
├── main.go                     # CLI: goextract <target-dir> -> JSON on stdout, errors on stderr+exit
└── internal/
    ├── load/load.go            # packages.Load wrapper (LoadAllSyntax)
    ├── routes/routes.go        # Gin recognizer (RouterGroup method calls, prefix chain)
    ├── handlers/handlers.go    # ShouldBindJSON / JSON / Param / Query + swaggo doc-comment parse
    ├── types/extract.go        # struct/field/tag/type-mapping + enum detection
    ├── diag/diag.go            # diagnostic accumulation (severity,msg,file:line)
    └── facts/facts.go          # the JSON DTOs + stable sort + marshal

crates/gnr8-core/src/
├── analyze/
│   ├── mod.rs                  # build_graph(): orchestrate subprocess → facts → graph
│   ├── helper.rs               # locate+run goextract, capture, typed errors
│   └── facts.rs                # #[derive(Deserialize)] mirror of the Go JSON
├── graph/mod.rs                # ApiGraph + from_facts + stable IDs
├── diagnostics/mod.rs          # collect() -> String
└── error.rs                    # extended CoreError
```

### Pattern 1: Full-type package load (GO-01)
**What:** Load the target module with ASTs + complete type info + positions in one pass.
**When to use:** Once, at the top of the helper.
```go
// Source: pkg.go.dev/golang.org/x/tools/go/packages (LoadMode docs)
cfg := &packages.Config{
    // Run the loader inside the target module so go.mod resolves.
    Dir:  targetDir,                 // absolute path to fixtures/goalservice
    Mode: packages.NeedName | packages.NeedFiles | packages.NeedImports |
          packages.NeedDeps | packages.NeedTypes | packages.NeedTypesSizes |
          packages.NeedSyntax | packages.NeedTypesInfo,   // == packages.LoadAllSyntax
    Tests: false,
}
pkgs, err := packages.Load(cfg, "./...")  // all packages in the module
if err != nil { /* hard error -> stderr + exit 1 */ }
if packages.PrintErrors(pkgs) > 0 {
    // type/parse errors inside the target -> emit diagnostics, do not silently drop (GO-06)
}
// Each pkg has: pkg.Syntax ([]*ast.File), pkg.TypesInfo (*types.Info), pkg.Fset (*token.FileSet), pkg.Types (*types.Package)
```
`LoadAllSyntax = LoadSyntax | NeedDeps`; `LoadSyntax = LoadTypes | NeedSyntax | NeedTypesInfo`. `NeedDeps` is what makes imported packages (`gin`, `uuid`, `time`) fully-typed rather than placeholder stubs — required to resolve `gin.RouterGroup` method identities and `uuid.UUID`. [CITED: pkg.go.dev/golang.org/x/tools/go/packages]

### Pattern 2: Resolve a method selector by identity, not alias string (GO-04)
**What:** Decide `c.JSON(...)` / `group.GET(...)` is *the Gin method*, robust to import aliasing and Gin version.
**When to use:** In the route recognizer and handler analyzer, for every `*ast.CallExpr` whose `Fun` is an `*ast.SelectorExpr`.
```go
// Source: pkg.go.dev/go/types (Info.Selections / Selection.Obj)
func ginMethod(info *types.Info, call *ast.CallExpr) (name, recvPkgPath string, ok bool) {
    sel, isSel := call.Fun.(*ast.SelectorExpr)
    if !isSel { return "", "", false }
    s := info.Selections[sel]                 // resolved selection (method value)
    if s == nil || s.Kind() != types.MethodVal { return "", "", false }
    fn, _ := s.Obj().(*types.Func)
    if fn == nil { return "", "", false }
    if p := fn.Pkg(); p != nil { recvPkgPath = p.Path() }   // e.g. "github.com/gin-gonic/gin"
    return fn.Name(), recvPkgPath, true                      // e.g. ("GET","github.com/gin-gonic/gin")
}
// Gate on recvPkgPath == "github.com/gin-gonic/gin" + name in {Group,GET,POST,PUT,DELETE,Use}.
// For c.JSON: receiver pkg is gin, method "JSON"; for c.ShouldBindJSON: method "ShouldBindJSON".
```
This is the key correctness lever vs string-matching `api.GET` — it survives `import grouter "github.com/gin-gonic/gin"`. [CITED: pkg.go.dev/go/types]

### Pattern 3: Group-prefix chain → full path template (GO-04)
**What:** `h.Router.Group("/" + basePath)` then `api.POST("/", ...)` → `POST /goal/`. Resolve the prefix even when it's `"/" + basePath` (a non-constant) by treating the route table as relative to a single group and recording the *literal* segments; the base path is a string concat the helper cannot constant-fold.
**When to use:** Building the route table.
```text
Observed in fixture: api := Router.Group("/" + basePath)   // basePath is a param -> dynamic
  api.POST("/", h.createGoal)        -> path segment "/"      under group
  api.GET("/list", h.listGoals)      -> "/list"
  api.PUT("/:uuid", h.updateGoal)    -> "/:uuid"
  api.DELETE("/:uuid", h.deleteGoal) -> "/:uuid"
```
The fixture's group prefix is dynamic (`"/" + basePath`), but the **expected openapi.yaml uses `/goal/...`**. Resolution: the `@Router /list [get]` / `@Router /{uuid} [put]` annotations + the const string passed at registration give the concrete base. For the graph, record the **group-relative path template** from code (`/`, `/list`, `/:uuid`) plus the literal prefix where constant, and normalize Gin `:param` → `{param}`. The concrete `/goal` prefix is supplied where `RegisterGoalRoutes("goal")` is called or via the `@Router` annotation — capture the annotation's router path as an authoritative override (escape hatch). **Plan note:** the snapshot author decides whether the graph stores `/goal/...` (annotation-resolved) or group-relative + a `basePath` field; either is acceptable as long as it is deterministic and the eventual OpenAPI yields `/goal/...`.

### Pattern 4: Request/response inference inside a handler (GO-05)
```go
// request body: find ShouldBindJSON(&x) -> the type of x
// c.ShouldBindJSON(&input) : arg[0] is &ast.UnaryExpr{Op: token.AND, X: input-ident}
reqType := info.TypeOf(unary.X)        // dto.CreateGoalInput
// response: find JSON(http.StatusXxx, y) -> (status, type of y)
// arg[0] resolves to a const from net/http; map the const VALUE to the numeric status.
statusConst := info.ObjectOf(statusSelIdent)        // *types.Const named "StatusCreated"
statusVal := constant.Int64Val(statusConst.Val())   // 201  (go/constant)
respType  := info.TypeOf(call.Args[1])              // dto.CommandMessageWithUUID
```
`http.StatusCreated`→201 etc. resolve via `go/constant` from the `*types.Const` value — robust and self-documenting. When `args[1]` is a composite literal (`dto.HttpError{...}`), `TypeOf` still yields the named type. When the response is built dynamically (no resolvable named type), emit a diagnostic (D-05) and fall back to the `@Success/@Failure` annotation.

### Pattern 5: Struct → schema facts incl. embedded + tags (GO-02/GO-03)
```go
// Source: pkg.go.dev/go/types (Struct.Field/Tag/Embedded) + pkg.go.dev/reflect#StructTag
st := named.Underlying().(*types.Struct)
for i := 0; i < st.NumFields(); i++ {
    f := st.Field(i)
    tag := reflect.StructTag(st.Tag(i))
    if f.Embedded() {           // e.g. CommandMessage in CommandMessageWithUUID
        // flatten: recurse into the embedded named struct, promote its fields
        continue
    }
    jsonName, opts := parseJSONTag(tag.Get("json"))      // "message", ["omitempty"]
    required := strings.Contains(tag.Get("binding"), "required")
    optional := isPointer(f.Type()) || hasOmitempty(opts)
    desc := tag.Get("description"); example := tag.Get("example")
    schemaType := mapType(f.Type())                      // see type table
}
```
Embedded detection is `Field(i).Embedded()` (a.k.a. `Anonymous()`); when embedded, recurse and promote (matches `CommandMessageWithUUID` → flattened `message` + `uuid` in expected outputs).

### Pattern 6: Well-known type + enum detection (GO-03)
```go
func mapType(t types.Type) SchemaType {
    switch u := t.(type) {
    case *types.Pointer: return optional(mapType(u.Elem()))
    case *types.Slice:   return array(mapType(u.Elem()))
    case *types.Map:     // map[string]any -> object+additionalProperties + DIAGNOSTIC
        return freeFormObject(diagnose(u))
    case *types.Named:
        if path := pkgPath(u); path == "github.com/google/uuid" && u.Obj().Name() == "UUID" {
            return SchemaType{Kind:"string", Format:"uuid"}
        }
        if path == "time" && u.Obj().Name() == "Time" {
            return SchemaType{Kind:"string", Format:"date-time"}
        }
        if isStringEnum(u) { return enumOf(u) }          // named string + const set
        return ref(qualifiedName(u))                      // $ref to schema
    case *types.Basic:
        return mapBasic(u)   // float64 -> {number} + float64-narrowing DIAGNOSTIC
    }
}
```
Enum detection: `u.Underlying()` is `*types.Basic` of kind string AND there exist `*types.Const` declarations of that named type in the package (scan `pkg.Types.Scope()` for consts whose type == the named type) → collect their string values, sorted. Matches `TargetDirection{gte,lte}`.

### Anti-Patterns to Avoid
- **String-matching package aliases** (`if pkgIdent.Name == "gin"`): breaks under import aliasing. Use `types.Info.Selections` identity (Pattern 2).
- **Putting Gin/OpenAPI concepts in the JSON facts** (e.g. a field named `ginGroup`): violates router-agnostic D-03/D-04. The JSON describes HTTP route facts only.
- **Iterating Go maps for output** without sorting: Go randomizes map iteration → non-deterministic JSON → GRAPH-02 failure. Sort every slice before marshal; never range a map into output order.
- **`unwrap()`/`expect()` on subprocess output in Rust** (`String::from_utf8(out.stdout).unwrap()`): violates RUST-04. Use typed errors + `?` (Pattern in §8).
- **Pre-authoring `.snap` files by hand** then asserting: let `cargo insta` capture the real output, then review/accept (§7) so the snapshot reflects actual serialization.
- **Constant-folding dynamic prefixes incorrectly**: don't guess `/goal` from `"/" + basePath`; treat it as group-relative + annotation override (Pattern 3).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parse Go source | A Go parser / tree-sitter grammar | `go/packages` + `go/ast` | Type resolution across files/packages is the whole point (D-01). |
| Resolve which type `c.JSON`'s arg is | AST shape heuristics | `types.Info.TypeOf` / `ObjectOf` / `Selections` | Handles aliases, embedded, cross-package refs correctly. |
| file:line for a node | Manual byte counting | `pkg.Fset.Position(node.Pos())` | `token.FileSet` already tracks it; handles `//line` directives. |
| Map HTTP status const → number | Hardcode `"StatusCreated":201` table | `go/constant` on the `*types.Const` value | Self-maintaining; covers any `http.Status*`. |
| Parse struct tags | Manual split on backtick/quotes | `reflect.StructTag.Get/Lookup` | Stdlib handles the quoting/escaping rules. |
| Spawn + capture subprocess | Threads + pipes by hand | `std::process::Command::output()` | Captures stdout+stderr+status atomically. |
| Deterministic graph serialization | Custom ordering logic everywhere | Sort-before-marshal in Go + `BTreeMap`/sorted `Vec` in Rust | One discipline (sorted keys) gives GRAPH-02 for free. |

**Key insight:** Every "hard" part of this phase (cross-file type resolution, status-code mapping, positions, tags) already has a precise stdlib/`x/tools` answer. The novel work is purely the *facts schema*, the *recognizer rules*, and the *deterministic ordering* — keep custom code confined to those.

## Runtime State Inventory

> This is a greenfield extraction phase (new `goextract/` module, new Rust analyzer code, new snapshots). It renames nothing and migrates no stored data. The Runtime State Inventory below is completed for completeness; all categories are "none."

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — no datastores; the analyzer reads source files only. | none |
| Live service config | None — no external services configured for this phase. | none |
| OS-registered state | None — no daemons/tasks/services registered. | none |
| Secrets/env vars | None — analyzer reads no secrets. (`GOFLAGS`/`GOPROXY` are toolchain env, not project state — see Environment Availability.) | none |
| Build artifacts | New: `goextract/` module produces a build cache under `$GOMODCACHE`; first build of `golang.org/x/tools` may hit the network (v0.45.0 already cached locally). The fixture's `go.sum` is unaffected (separate module). | Commit `goextract/go.mod` + `go.sum`; add a `goextract` build/vet CI gate (mirror `go-fixture`). |

**Nothing found in categories 1–4:** verified by directory inspection — no `.env`, no service config, no datastore, no scheduler entries in the repo touched by this phase.

## Common Pitfalls

### Pitfall 1: Non-deterministic JSON from Go map iteration (GRAPH-02 killer)
**What goes wrong:** Ranging a `map` to build the routes/schemas list yields a different order each run → the `snapshot_graph` `.snap` flaps → "byte-identical on unchanged source" fails.
**Why it happens:** Go intentionally randomizes map iteration order.
**How to avoid:** Accumulate into slices, then `sort.Slice` by a stable key (operationId, schemaId, param name, status code) immediately before `json.Marshal`. On the Rust side, deserialize into `Vec` and re-sort or use `BTreeMap`. `encoding/json` already sorts `map[string]T` keys, so prefer maps only where JSON-object semantics are wanted and slices everywhere ordering matters.
**Warning signs:** `cargo insta test` passes once, fails on re-run with only reordering in the diff.

### Pitfall 2: `go/packages` needs a *buildable* module
**What goes wrong:** If the target module doesn't compile (missing dep, type error), `Load` returns packages with `.Errors` populated and incomplete `TypesInfo` → inference silently degrades.
**Why it happens:** Type checking requires resolvable imports; `go/packages` reports per-package errors instead of failing hard.
**How to avoid:** After `Load`, call `packages.PrintErrors`/inspect `pkg.Errors`; for any package with errors, emit a `WARN`/`ERROR` diagnostic with file:line (GO-06) rather than producing a partial graph silently. The fixture *does* build (verified: `go build ./...` OK), so the happy path is green; the failure path must still diagnose.
**Warning signs:** Empty/partial route table with no error surfaced.

### Pitfall 3: First `goextract` build needs network (CI + cold dev)
**What goes wrong:** `go run ./goextract` on a clean machine downloads `golang.org/x/tools` from `proxy.golang.org`; an offline/locked-down CI step fails.
**Why it happens:** Module not yet in `$GOMODCACHE`.
**How to avoid:** Commit `goextract/go.sum`; CI is allowed network for module download (the existing `go-fixture` job already runs `go build` with network). Document that `golang.org/x/tools` v0.45.0 is *already cached locally* here, so local runs are offline-OK; CI uses `actions/setup-go` which fetches. No CGO is needed (pure-Go analysis), so `CGO_ENABLED=0` is safe and avoids a C toolchain dependency.
**Warning signs:** `go run` errors `dial tcp ... connect: ...` in a sandboxed step.

### Pitfall 4: Diagnostics text must match `expected/diagnostics.txt` exactly
**What goes wrong:** `snapshot_diagnostics` compares `collect()` output to a snapshot that must reconcile with the 7 specific WARN lines (float64 ×3, free-form-map ×1, untyped-query ×3) in `expected/diagnostics.txt`. Wrong wording/order/count fails.
**Why it happens:** It's an exact-text contract (D-10), not a structural one.
**How to avoid:** Make the helper emit diagnostics in a **stable sorted order** and have `collect()` render them to the canonical phrasing. Author the `.snap` from real output, then verify it lines up with `expected/diagnostics.txt` (the file is the acceptance target the snapshot must honor "in spirit," per D-10; the snapshot locks the exact text). Decide explicitly whether the rendered text reproduces the file verbatim or a normalized form — recommend reproducing the **field-identity + rule** of each line (e.g. `CreateGoalInput.TargetValue (*float64)`), which the fixture data fully determines. Note the 3 `float64` lines have slightly different trailing phrasing in the fixture file (line 8 has extra "; map to float64 or surface a compatibility diagnostic") — the snapshot author must pick one canonical phrasing and apply it uniformly OR reproduce the file's exact lines; flag for the snapshot-authoring task.
**Warning signs:** Snapshot diff shows only whitespace/wording deltas.

### Pitfall 5: Path normalization `:param` → `{param}` and dynamic base path
**What goes wrong:** Operation IDs / paths come out as `/:uuid` (Gin) instead of `/{uuid}` (OpenAPI), or the `/goal` prefix is wrong/missing because it's `"/" + basePath` (non-constant).
**Why it happens:** Gin uses `:param`; the base path is a runtime concat.
**How to avoid:** Normalize `:x`→`{x}` deterministically. Resolve the concrete prefix from the `@Router` annotation (authoritative escape hatch) and/or the const passed to `RegisterGoalRoutes`; where unresolved, store group-relative + flag. Operation ID derivation (D-08) uses the *normalized* path so it's stable.
**Warning signs:** `inspect routes` shows `/:uuid`; OpenAPI later mismatches expected `/goal/{uuid}`.

### Pitfall 6: Subprocess error surfaced as a panic instead of a diagnostic (GO-06 violation)
**What goes wrong:** Go missing / helper build fails / non-zero exit / non-UTF8 or non-JSON stdout → a Rust `unwrap` panics with a backtrace.
**Why it happens:** Easy to `.unwrap()` `Command::output()`/`from_utf8`/`from_slice`.
**How to avoid:** Typed `CoreError` variants + `?`; never `unwrap` in `gnr8-core` (clippy `unwrap_used = "deny"` will catch it). See §8.
**Warning signs:** `cargo clippy` flags `unwrap_used`; a missing-Go run prints a panic instead of `gnr8: ...`.

## Code Examples

### Rust↔Go facts DTOs (serde mirror)
```rust
// crates/gnr8-core/src/analyze/facts.rs
// Source: serde docs; mirrors the goextract JSON. Deny unknown fields to catch drift.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoFacts {
    pub module: String,                 // module path of the target
    pub routes: Vec<RouteFact>,         // sorted by (method, path)
    pub schemas: Vec<SchemaFact>,       // sorted by id (pkg-qualified name)
    pub diagnostics: Vec<DiagnosticFact>, // sorted by (file, line, message)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteFact {
    pub method: String,                 // "POST"
    pub path: String,                   // normalized "/goal/{uuid}" or group-relative
    pub handler: String,                // "createGoal"
    pub operation_id: Option<String>,   // from @ID, else derived
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub secured: bool,                  // group had Use(authMiddleware) (D-04/D-14)
    pub params: Vec<ParamFact>,         // path + query
    pub request_body: Option<TypeRef>,  // pkg-qualified schema id
    pub responses: Vec<ResponseFact>,   // by status
    pub span: SourceSpan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamFact {
    pub name: String, pub location: String,  // "path" | "query"
    pub required: bool, pub schema: SchemaType,
    pub description: Option<String>, pub enum_values: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseFact { pub status: u16, pub body: Option<TypeRef>, pub description: Option<String> }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaFact {
    pub id: String,                     // "internal/common/dto.CreateGoalInput"
    pub name: String,                   // "CreateGoalInput"
    pub kind: String,                   // "object" | "enum"
    pub fields: Vec<FieldFact>,         // sorted by json name; empty for enums
    pub enum_values: Vec<String>,       // for enums
    pub span: SourceSpan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldFact {
    pub json_name: String, pub required: bool, pub optional: bool,
    pub schema: SchemaType, pub description: Option<String>, pub example: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaType {       // router-/OpenAPI-agnostic primitive description
    pub kind: String,         // "string"|"integer"|"number"|"boolean"|"array"|"object"|"ref"
    pub format: Option<String>,   // "uuid"|"date-time"|"int64"
    pub items: Option<Box<SchemaType>>,        // for arrays
    pub ref_id: Option<String>,                // for "ref"
    pub additional_properties: Option<bool>,   // for free-form maps
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeRef { pub ref_id: String }

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticFact {
    pub severity: String,     // "WARN" | "ERROR"
    pub message: String,
    pub file: String, pub line: u32,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan { pub file: String, pub start_line: u32, pub end_line: u32 }
```

### Subprocess driver with typed failures (GO-06, §8)
```rust
// crates/gnr8-core/src/analyze/helper.rs
use std::process::Command;
use crate::CoreError;

/// Run `go run ./goextract <target_dir>` from the goextract module dir, return parsed facts.
pub fn run_goextract(target_dir: &str, goextract_dir: &str) -> Result<super::facts::GoFacts, CoreError> {
    let output = Command::new("go")
        .args(["run", ".", target_dir])
        .current_dir(goextract_dir)
        .output()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;   // `go` not found / not executable

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CoreError::HelperExit { code: output.status.code(), stderr });
    }
    let facts: super::facts::GoFacts = serde_json::from_slice(&output.stdout)
        .map_err(|source| CoreError::FactsParse { source })?;
    Ok(facts)
}
```

### Extended CoreError (error.rs)
```rust
// crates/gnr8-core/src/error.rs — add variants (keep NotYetImplemented).
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("'{command}' is not yet implemented (arrives in phase {phase})")]
    NotYetImplemented { command: String, phase: u8 },

    #[error("Go toolchain not available (is `go` installed and on PATH?): {source}")]
    GoToolchainMissing { #[source] source: std::io::Error },

    #[error("goextract helper exited with status {code:?}:\n{stderr}")]
    HelperExit { code: Option<i32>, stderr: String },

    #[error("failed to parse goextract JSON facts: {source}")]
    FactsParse { #[source] source: serde_json::Error },
}
```

### Stable operation/schema IDs (GRAPH-02, D-08)
```rust
// operation id: prefer @ID annotation, else derive from method + normalized path
fn operation_id(r: &RouteFact) -> String {
    r.operation_id.clone().unwrap_or_else(|| {
        // e.g. POST /goal/  -> "post_goal_root" ; deterministic, lowercase, {param} kept
        format!("{}_{}", r.method.to_lowercase(),
                r.path.trim_matches('/').replace('/', "_").replace(['{','}'], ""))
    })
}
// schema id: package-qualified type name, already stable from the helper ("dto.CreateGoalInput")
```
**Plan note:** the *exact* derived-ID format is Claude's discretion (D-08) — choose one, snapshot it, keep it stable. The fixture's expected `operationId`s come from `@ID` for `updateGoal` (`goalUuidPut`) and from the Go func name for the rest (`createGoal`, `listGoals`, `deleteGoal`) per `openapi.yaml`; the graph can store the annotation ID when present and the handler name otherwise (both deterministic).

### Go: deterministic emit
```go
// goextract/internal/facts/facts.go
sort.Slice(doc.Routes, func(i, j int) bool {
    if doc.Routes[i].Path != doc.Routes[j].Path { return doc.Routes[i].Path < doc.Routes[j].Path }
    return doc.Routes[i].Method < doc.Routes[j].Method
})
sort.Slice(doc.Schemas, func(i, j int) bool { return doc.Schemas[i].ID < doc.Schemas[j].ID })
sort.Slice(doc.Diagnostics, func(i, j int) bool {
    if doc.Diagnostics[i].File != doc.Diagnostics[j].File { return doc.Diagnostics[i].File < doc.Diagnostics[j].File }
    return doc.Diagnostics[i].Line < doc.Diagnostics[j].Line
})
enc := json.NewEncoder(os.Stdout)
enc.SetIndent("", "  ")            // stable indentation; encoding/json sorts map keys
if err := enc.Encode(doc); err != nil { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `golang.org/x/tools/go/loader` | `golang.org/x/tools/go/packages` | Go modules era (2018+) | Modules-aware; `go/loader` deprecated. Use `go/packages`. |
| swaggo `swag init` over comment annotations → swagger 2.0 → openapi-generator | Code-first native extraction (this project) | gnr8's thesis | Annotations become an *escape hatch*, not the source of truth (TARGET-API.md). |
| Hand-written `JSONSchema()` methods per enum | `const`-set inference from named string types | gnr8 default path | Inference is default; hand-written methods are a later escape hatch (deferred). |

**Deprecated/outdated:**
- `go/loader`: superseded by `go/packages` for module-aware loading.
- String-alias matching for framework detection: superseded by `types.Info` identity resolution.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The graph/diagnostics snapshots must capture swaggo `// @...` annotation facts (operationId, query types/enum, summary, security), not just code inference. | Summary, §2.3 | If the snapshot author models code-only, the produced graph cannot yield the expected `openapi.yaml` in Phase 3 (operationId `goalUuidPut`, `aggregation` enum, query required-ness). HIGH impact — but strongly evidenced by `expected/openapi.yaml` + `expected/diagnostics.txt` lines 12–14. |
| A2 | `go run ./goextract` (recompile-per-call) is acceptable performance for this phase; prebuilt binary deferred to watch-mode. | Standard Stack, §5 | If `build_graph` is called in a hot loop this phase, latency suffers. Low risk — only contract tests + 3 inspect calls invoke it here; D-03 explicitly permits `go run`. |
| A3 | The exact rendered text of `diagnostics::collect` should reproduce the *rule + field identity* of each `expected/diagnostics.txt` line; the snapshot locks final wording. | §6, Pitfall 4 | If wording diverges from what the snapshot-author/discuss expects, `snapshot_diagnostics` churns. Mitigated by authoring the snapshot from real output and reconciling against the file. |
| A4 | `goextract/` is a *separate* Go module from `fixtures/goalservice` (own go.mod), so the helper's deps don't perturb the fixture's `go.sum`. | Structure, Pitfall 3 | If accidentally made one module, the fixture build gate could pull `x/tools`. Low — D-02 says "its own module." |
| A5 | The concrete `/goal` base-path prefix is resolvable from the `@Router` annotation and/or the const passed to `RegisterGoalRoutes`, since `Group("/" + basePath)` is non-constant. | §2.2, Pitfall 5 | If neither is treated as authoritative, paths come out group-relative and Phase-3 OpenAPI mismatches. Medium — evidenced by `@Router /list [get]` / `@Router /{uuid} [put]` in handlers.go. |
| A6 | No new Rust crates are needed (serde/serde_json/thiserror/insta already pinned; `std::process::Command` is stdlib). | Standard Stack | If a helper-locating crate (e.g. `which`) is later wanted, it's an easy add; not required now. Very low. |

**If this table looks long:** A1 and A5 are the load-bearing ones — confirm the graph captures annotation facts and resolves the base path, or Phase 3 cannot reproduce the expected OpenAPI.

## Open Questions (RESOLVED)

> All three resolved by the plans: path storage (02-02/02-03), diagnostics phrasing (02-03), goextract-dir resolution (02-01).

1. **Does the Phase-2 `ApiGraph` store the resolved absolute path (`/goal/{uuid}`) or group-relative + basePath?**
   - What we know: expected OpenAPI uses `/goal/...`; the code's group prefix is dynamic (`"/" + basePath`); `@Router` annotations give `/list`, `/{uuid}`.
   - What's unclear: whether the snapshot models the absolute path now or defers prefix-joining to Phase-3 lowering.
   - Recommendation: store **both** the group-relative template (from code, always available) and the annotation `@Router` path (when present) + a `base_path` field; let the snapshot author pick the rendered form. Either is deterministic. Capture it as a planning decision in 02-03.

2. **Exact canonical phrasing of the 7 diagnostic lines.**
   - What we know: `expected/diagnostics.txt` has 7 WARN lines; the 3 float64 lines have slightly inconsistent trailing clauses.
   - What's unclear: reproduce verbatim vs normalize to one template.
   - Recommendation: normalize to one template per rule (`float64->float32 narrowing: field <Struct>.<Field> (*float64) ...`), author the `.snap` from real output, and reconcile against the file in the snapshot-authoring task. D-10 says the file is the target "in spirit" and the snapshot locks exact text.

3. **`go run` vs cached binary path discovery for `goextract_dir`.**
   - What we know: the helper lives at `<repo>/goextract`; tests run from `CARGO_MANIFEST_DIR`.
   - What's unclear: how Rust locates the `goextract` dir at runtime (relative to manifest? env var? `CARGO_MANIFEST_DIR/../../goextract`).
   - Recommendation: resolve `goextract` dir as `concat!(env!("CARGO_MANIFEST_DIR"), "/../../goextract")` (mirrors how `FIXTURE_DIR` is resolved in the contract tests). Document and keep a single helper-path function.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Go toolchain (`go`) | goextract build/run (D-01/D-02) | ✓ | go1.26.2 darwin/arm64 | None — emit `CoreError::GoToolchainMissing` + diagnostic (GO-06). CI uses `actions/setup-go@v5` go 1.26. |
| `golang.org/x/tools` | go/packages loader | ✓ (v0.45.0 cached; v0.46.0 latest, fetch on demand) | v0.46.0 | Module proxy (network) on cold cache; commit `go.sum`. |
| Rust / Cargo | gnr8-core + gnr8 build/test | ✓ | 1.96.0 (MSRV 1.85) | None. |
| `cargo-insta` (CLI) | authoring/reviewing the two `.snap` | ✗ | — | `cargo install cargo-insta` for review; not required to *run* tests (`cargo test` works; CI runs `INSTA_UPDATE=no`). |
| Network (proxy.golang.org) | first `goextract` build only | ✓ (CI allows; locally cached) | — | If sandboxed offline and uncached: pre-warm `$GOMODCACHE` or vendor. v0.45.0 already cached here. |
| CGO / C toolchain | NOT required (pure-Go analysis) | n/a | — | Set `CGO_ENABLED=0` to avoid any C dependency. |

**Missing dependencies with no fallback:** none (Go + Rust both present).
**Missing dependencies with fallback:** `cargo-insta` CLI (install for review; tests run without it).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `insta` 1.48 (yaml feature) |
| Config file | none dedicated; `insta` config via attributes; `.snap` files under `crates/gnr8-core/tests/snapshots/` |
| Quick run command | `cargo test -p gnr8-core --test snapshot_graph --test snapshot_diagnostics` |
| Full suite command | `make check` (fmt-check + clippy + full test + go fixture build/vet) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| GO-01..05, GRAPH-01/02 | Graph extracted from fixture matches golden | snapshot | `cargo test -p gnr8-core --test snapshot_graph` | ✅ (red-by-design; flip to real `.snap` this phase) |
| GO-06, D-10 | Diagnostics text matches expected (7 WARN lines) | snapshot | `cargo test -p gnr8-core --test snapshot_diagnostics` | ✅ (red-by-design; flip to real `.snap`) |
| GO-06 (toolchain missing) | `build_graph` returns typed error, no panic | unit | `cargo test -p gnr8-core --lib analyze::` | ❌ Wave 0 (add `analyze` unit tests) |
| GO-04 (method identity) | Gin recognizer resolves selectors via types | go test | `cd goextract && go test ./...` | ❌ Wave 0 (add Go unit tests in goextract) |
| GRAPH-02 (determinism) | Two runs → byte-identical facts JSON | go test / unit | `cd goextract && go test ./internal/facts -run Determinism` | ❌ Wave 0 |
| GRAPH-03 | `inspect routes\|schemas\|graph` render | integration/unit | `cargo test -p gnr8 inspect` (or snapshot of render) | ❌ Wave 0 (add render tests) |

### Sampling Rate
- **Per task commit:** `cargo test -p gnr8-core --lib && cd goextract && go test ./...`
- **Per wave merge:** `make gates && make fixture-build` plus `cargo test -p gnr8-core --test snapshot_graph --test snapshot_diagnostics`
- **Phase gate:** full `make check` green; the `contract` CI job (the two snapshots) GREEN — promote `snapshot_graph`/`snapshot_diagnostics` from the non-blocking `contract` job toward blocking as they turn green (full promotion is Phase-3 per CI policy; this phase makes graph+diagnostics pass).

### Wave 0 Gaps
- [ ] `goextract/` module skeleton + `go.mod`/`go.sum` + `make`/CI `goextract` build/vet gate (mirror `go-fixture`).
- [ ] `goextract/internal/*/_test.go` — Go unit tests for route recognition, type mapping, determinism.
- [ ] `crates/gnr8-core/src/analyze/` unit tests — toolchain-missing → typed error; facts→graph mapping.
- [ ] `crates/gnr8-core/tests/snapshots/` — the two real `.snap` files authored from actual output (reconciled with `expected/diagnostics.txt`).
- [ ] `inspect` render tests in `crates/gnr8` (table + `--json`).
- [ ] Framework install (review only): `cargo install cargo-insta`.

## Security Domain

> `security_enforcement: true`, `security_asvs_level: 1`, `security_block_on: high`. This phase reads untrusted *source code* and spawns a subprocess; it serves no network traffic and stores no secrets.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Analyzer has no auth surface (the fixture's auth middleware is *analyzed data*, not a live guard — see fixture comment "threat T-02-01: fixture is analyzer INPUT only"). |
| V3 Session Management | no | No sessions. |
| V4 Access Control | no | CLI tool, local FS only. |
| V5 Input Validation | yes | The fixture Go source + the helper's JSON are untrusted-ish input. Validate: (a) `serde(deny_unknown_fields)` on `GoFacts` catches malformed/forward-incompatible JSON; (b) never `eval`/execute extracted strings; (c) the path passed to `go run` is a directory arg, not a shell string (use `Command` args, never `sh -c`). |
| V6 Cryptography | no | No crypto in this phase. |

### Known Threat Patterns for {Rust CLI + Go subprocess}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Command injection via crafted target path | Tampering/Elevation | Pass the target dir as a discrete `Command` arg (`args(["run",".",target])`), never interpolate into a shell. No `sh -c`. |
| `go run` executing arbitrary code from the *target* module's `go:generate`/build | Elevation of Privilege | `go run ./goextract` builds/runs ONLY the helper (trusted, in-repo); it does NOT `go run` the *target* — it *parses* the target with `go/packages` (which type-checks but does not execute target code). `go/packages` may run the target's `go list`/build for type info, so treat untrusted target modules as a known boundary; for the PoC the only target is the in-repo fixture. Document this boundary; broader untrusted-input hardening is a later concern. |
| Subprocess stderr/stdout DoS (huge output) | DoS | `Command::output()` buffers fully; acceptable for the small fixture. If targets grow, stream/limit later (not this phase). |
| Panic on malformed helper output → crash | DoS | Typed `CoreError` + `?` (no `unwrap`); clippy `unwrap_used = "deny"` enforces. |
| Supply-chain (helper deps) | Tampering | Commit `goextract/go.sum`; only dep is official `golang.org/x/tools`. |

**Block-on-high check:** No high-severity findings. The notable boundary is "`go/packages` invokes the Go build system on the target module" — for the PoC the target is the trusted in-repo fixture, so this is acceptable; flag it as a documented limitation for when arbitrary user modules are analyzed (post-PoC).

## Recommended 3-Plan Split

Aligned to the phase intent (02-01 discovery/type-mapping, 02-02 router/handler, 02-03 graph+inspect+diagnostics):

- **02-01 — Discovery, struct/type extraction, type mapping, helper skeleton + JSON contract (GO-01, GO-02, GO-03):**
  Create `goextract/` module; `packages.Load` (LoadAllSyntax); walk reachable structs → fields/tags/embedded/enums; type-mapping (well-known uuid/time, slices, maps→diagnostic, float64→diagnostic); define the **facts JSON schema** + Rust serde mirror; subprocess driver + extended `CoreError` (toolchain/exit/parse). Determinism scaffolding (sort+emit). Go unit tests for type mapping.

- **02-02 — Router + handler extraction (GO-04, GO-05):**
  Gin recognizer via `types.Info.Selections` (Group/METHOD/Use); group-prefix chain + `:param`→`{param}` normalization; handler analysis (`ShouldBindJSON`→request, `JSON(status,…)`→response via `go/constant`, `Param`/`Query`→params); swaggo doc-comment parse (escape hatch: `@Param`/`@Success`/`@Failure`/`@ID`/`@Summary`/`@Security`/`Enums`); diagnostics for dynamic responses + untyped query params. Go unit tests for recognition.

- **02-03 — Graph model, stable IDs, inspect reports, diagnostics snapshot (GRAPH-01, GRAPH-02, GRAPH-03, GO-06):**
  Rust `ApiGraph` + `from_facts` (provenance on every node, stable operation/schema IDs, sorted serialization); `diagnostics::collect` → text reconciled with `expected/diagnostics.txt`; wire `inspect routes|schemas|graph` (table + `--json`); author the two real `.snap` files (turn `snapshot_graph` + `snapshot_diagnostics` GREEN); end-to-end determinism test (two runs byte-identical). Update Makefile/CI with a `goextract` gate.

## Sources

### Primary (HIGH confidence)
- pkg.go.dev/golang.org/x/tools/go/packages — `Config`, `LoadMode` (`LoadAllSyntax = LoadSyntax | NeedDeps`; `LoadSyntax = LoadTypes | NeedSyntax | NeedTypesInfo`), `Dir`, `Syntax`/`TypesInfo`/`Fset` fields, `PrintErrors`. Version v0.46.0.
- pkg.go.dev/go/types — `Info.Selections`/`Selection.Obj()`, `Info.TypeOf`/`ObjectOf`, `Struct.Field/Tag`, `Field.Embedded()`, `*Named/*Pointer/*Slice/*Map/*Basic`, `Named.Obj().Pkg().Path()`, `types.Unalias`.
- pkg.go.dev/go/token — `FileSet.Position(pos)` → `{Filename, Line, Column}`; `Position` struct.
- `go list -m -versions golang.org/x/tools` → confirmed v0.46.0 latest (local run).
- Repo: `fixtures/goalservice/**` (real analyzer input), `expected/{openapi.yaml,sdk/models.go,diagnostics.txt}` (acceptance targets), `crates/gnr8-core/**` (seams, error, graph placeholder, the two contract tests), `Cargo.toml`/manifests (pinned deps), `Makefile` + `.github/workflows/ci.yml` (gates), `thoughts/skills/rust-best-practices/**` (typed errors, snapshot guidance).

### Secondary (MEDIUM confidence)
- pkg.go.dev/reflect#StructTag — `Get`/`Lookup` for parsing struct tags.
- WebSearch (DeepWiki "Package Loading (go/packages)", golang/tools issue #65965) — `Config.Dir` semantics, `./...` pattern, LoadSyntax loads initial packages from source with full ASTs.

### Tertiary (LOW confidence)
- None relied upon for load-bearing claims.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — official Go team APIs, version confirmed via `go list`; no third-party Rust crates added.
- Architecture (helper + JSON contract + graph): HIGH — directly grounded in the fixture, expected outputs, and existing seams.
- Type mapping: HIGH — fixture + `expected/openapi.yaml` + `expected/sdk/models.go` enumerate every case explicitly.
- Router/handler recognition: MEDIUM-HIGH — `types.Info` selector resolution is well-documented; the dynamic base-path prefix (A5/OpenQ1) is the one genuine modeling decision left to planning.
- Diagnostics exact text: MEDIUM — the 7 lines are known; canonical phrasing is a snapshot-authoring decision (OpenQ2).
- Pitfalls: HIGH — determinism + subprocess + buildable-module are well-understood and fixture-verified.

**Research date:** 2026-06-24
**Valid until:** ~2026-07-24 (stable; `golang.org/x/tools` moves but the loader API is long-stable; re-check the latest patch before pinning).

## RESEARCH COMPLETE

**Phase:** 02 - go-analysis-and-api-graph
**Confidence:** HIGH

### Key Findings
- The graph MUST capture swaggo `// @...` annotation facts (operationId `goalUuidPut`, query types/required, `aggregation` enum, summaries, `X-API-Key` security) in addition to code inference — `expected/openapi.yaml` + `expected/diagnostics.txt` lines 12–14 prove it. Doc comments come free from `go/packages` `NeedSyntax` (`ast.FuncDecl.Doc`); no extra tool.
- Stack is locked and confirmed: `golang.org/x/tools` **v0.46.0** + stdlib `go/ast`/`go/types`/`go/token`/`reflect`/`encoding/json`; LoadMode = `LoadAllSyntax`. Resolve Gin method identity via `types.Info.Selections` (version/alias-robust), not string matching. No new Rust crates (serde/serde_json/thiserror/insta already pinned).
- `diagnostics.txt` is an exact 7-line WARN contract decomposing into 3 rules (float64 ×3, free-form-map ×1, untyped-query ×3) — all fixture-derivable; the snapshot author must pick one canonical phrasing.
- Determinism (GRAPH-02) hinges on one discipline: sort every slice before `json.Marshal` in Go (never range a map into output order); `serde(deny_unknown_fields)` + sorted Rust collections on the other side.
- Every subprocess failure mode (Go missing / build fail / non-zero exit / bad JSON) maps to a typed `CoreError` variant + `?`; clippy `unwrap_used = "deny"` enforces no-panic (GO-06).

### File Created
`/Users/emilwareus/conductor/workspaces/gnr8/tripoli-v7/.planning/phases/02-go-analysis-and-api-graph/02-RESEARCH.md`

### Confidence Assessment
| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | Official Go APIs; v0.46.0 confirmed via `go list`; no new Rust deps |
| Architecture | HIGH | Grounded in fixture, expected outputs, existing seams |
| Pitfalls | HIGH | Determinism/subprocess/buildable-module fixture-verified |

### Open Questions
- Whether the Phase-2 graph stores absolute (`/goal/{uuid}`) or group-relative + basePath (dynamic `"/" + basePath` prefix) — recommend storing both + annotation `@Router` override.
- Exact canonical phrasing of the 7 diagnostic lines (verbatim vs normalized template).
- How Rust locates the `goextract` dir at runtime — recommend `CARGO_MANIFEST_DIR/../../goextract`.

### Ready for Planning
Research complete. Planner can now create the three PLAN.md files (02-01 discovery/type-mapping, 02-02 router/handler, 02-03 graph+inspect+diagnostics).
