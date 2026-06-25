# gnr8 — Reference (agent-oriented)

Dense reference for operating and editing gnr8. Terse by design. Source of truth for behavior is the
code; this matches the current build. Product invariants: [`../CLAUDE.md`](../CLAUDE.md) (one source per
fact, no third-party deps, no fallback/dual paths, config supplies what typed source can't).

## What it is / isn't
- Reads a **Go + Gin** service via `go/types`, builds a router-agnostic API graph, emits **OpenAPI 3.1**
  + a **compiling Go SDK**. Code-first: your code is the single source of truth for the API. Go + Gin is
  the first supported frontend; the model is general by design.
- **Envelope (hard limits today):** Go only; Gin only; **exactly ONE route group per service** (one
  base path). Multi-group/multi-domain services are NOT supported (paths collapse → error). Other Go
  routers extract zero routes.
- Facts not in typed source (base/mount path, OpenAPI title, security schemes) come from `.gnr8/config.toml`.

## Build
```
cargo build --release -p gnr8        # binary: target/release/gnr8
make check                           # fmt + clippy -D warnings + all tests + go builds
make gates                           # the contract suite (4 snapshots, sdk_compile, determinism, lifecycle)
```
Requires a Go toolchain on PATH (gnr8 shells a Go helper to load the target module).

## Canonical workflow
```
cd <your-go-service>      # the dir whose .gnr8/ holds config; inputs are resolved from here
gnr8 init                 # scaffold .gnr8/config.toml + .gnr8/.gitignore (idempotent)
# edit .gnr8/config.toml: inputs, base_path, title, output.*, [[security.schemes]]
gnr8 generate             # write OpenAPI + Go SDK; tracks ownership; skips unchanged files
gnr8 check                # CI gate: exit 1 if any output is stale/drifted, else 0
```

## CLI
All commands except `inspect` operate on the **current project** (cwd must hold `.gnr8/config.toml`).
Global flags: `--json` (machine output), `-v`/`-vv` (verbosity).

| Command | Args/flags | Reads | Writes | Exit |
|---|---|---|---|---|
| `gnr8 init` | — | — | `.gnr8/config.toml`, `.gnr8/.gitignore` (skips existing — idempotent) | 0; 1 on error |
| `gnr8 generate` | `--force` | `.gnr8/config.toml`, `inputs` Go src | `output.openapi`, `output.sdk_dir/*`, `.gnr8/cache/manifest.json` | 0; 1 on error |
| `gnr8 check` | — | config, src, manifest | — (dry run) | **0 up-to-date; 1 stale/drifted**; 1 on error |
| `gnr8 watch` | `--debounce-ms N` (def 200) | config, src | same as generate, on each change | 0 on Ctrl-C; 1 on error |
| `gnr8 doctor` | — | config, src, manifest | — | **0 healthy; 1 actionable problem**; never crashes |
| `gnr8 inspect routes\|schemas\|graph` | `[<dir>]` (positional, defaults to bundled fixture) | the `<dir>` Go module | — (prints) | 0; 1 on error |

Notes:
- `--force` overwrites outputs a user hand-edited (otherwise generate warns+skips them — ownership protection).
- `inspect` is the ONLY command taking a target dir; the others use `inputs` from config.
- No command panics on bad input/missing toolchain — typed error → clean stderr + non-zero.

## Config: `.gnr8/config.toml`
TOML is a PoC stand-in (not the long-term UX). `deny_unknown_fields` — unknown keys error.

| Key | Type | Req | Default | Meaning |
|---|---|---|---|---|
| `inputs` | `[string]` | yes | — | Go source dir(s), project-relative. **Single dir only** — >1 entry is rejected. Usually `["."]`. |
| `base_path` | string | no | `"/"` | API base/mount path joined to every operation path. SINGLE source for the prefix (the Gin group arg is often a runtime value, so it's declared here, not scraped). |
| `title` | string | no | `"API"` | OpenAPI `info.title`. |
| `output.openapi` | string | yes | — | OpenAPI artifact path, project-relative (e.g. `openapi.yaml`). |
| `output.sdk_dir` | string | yes | — | Generated Go SDK directory (e.g. `sdk`). |
| `output.go_module` | string | yes | — | Go module path of the generated SDK. **SDK package name = sanitized last path segment** (`.../sdk` → `package sdk`; `.../gnr8sdk` → `package gnr8sdk`). |
| `naming.operations` | map<string,string> | no | `{}` | Remap operationId → name (e.g. `listBooks = "List"`). |
| `naming.types` | map<string,string> | no | `{}` | Remap generated type name. Renaming a referenced type updates its `$ref`s; a collision/cycle is a typed error. |
| `security.schemes` | `[{id,kind,location,name}]` | no | `[]` | API security. `kind="apiKey"`, `location="header"` only (PoC). Applies to ALL operations. Unsupported kind/location or duplicate id → typed error. |

Example:
```toml
inputs    = ["."]
base_path = "/books"
title     = "Bookstore API"

[output]
openapi   = "generated/openapi.yaml"
sdk_dir   = "generated/sdk"
go_module = "example.com/bookstore/sdk"

[[security.schemes]]
id = "ApiKeyAuth"; kind = "apiKey"; location = "header"; name = "X-API-Key"
```

## Recognized Go/Gin patterns (code-first)
Resolution is via `go/types` (alias/import-robust), not string matching.

| Fact | Source pattern | Notes |
|---|---|---|
| route | `group := r.Group(...)` then `group.GET/POST/PUT/DELETE(path, handler)` | ONE group only. `path` is group-relative; final path = `base_path` + path. |
| path param | `:name` segment + `c.Param("name")` | → OpenAPI `{name}`. |
| query param | `c.Query("name")` | name only; type=`string`, `required:false` (no type/enum/required inferable from `c.Query`). |
| request body | `c.ShouldBindJSON(&x)` where `x: T` | T → request schema. |
| response | `c.JSON(http.StatusXxx, v)` where `v: T` | status→T. Unresolved/dynamic → diagnostic. |
| operationId | handler func/method name | overridable via `naming.operations`. |
| required field | struct tag `binding:"required"` | → schema `required`. |
| optional field | pointer `*T` and/or `json:",omitempty"` | not in `required`. |
| enum | named `string` type + `const` set | → OpenAPI string enum + Go typed newtype. |
| from config (not source) | security schemes, base/mount path, title | not expressible in typed source — declared in `.gnr8/` config. |

## Type mapping (Go → OpenAPI → generated SDK) — verified
| Go | OpenAPI | SDK Go | Note |
|---|---|---|---|
| `string` | `string` | `string` | |
| `bool` | `boolean` | `bool` | |
| `int`/`int64` | `integer` | `int64` | |
| `float64` | `number` | **`float32`** | ⚠ narrows precision → diagnostic emitted |
| `time.Time` | `string`/`date-time` | `time.Time` | |
| `uuid.UUID` | `string`/`uuid` | `string` | well-known |
| `*T` or `,omitempty` | optional (not required) | **value `T` + `,omitempty`** (pointer dropped) | can't distinguish null vs zero |
| `[]T` | `array` | `[]T` | |
| `map[string]T` | `object`,`additionalProperties:true` | `map[string]T` | free-form → diagnostic |
| named-string+consts | string `enum` | typed newtype | |
| nested struct | `$ref` | nested type | |
| embedded struct | flattened fields | flattened | |

## Generated SDK shape
Single package `<go_module last segment>`, files: `client.go`, `models.go`, `operations.go`, `errors.go`.
- `Client{ baseURL, httpClient, apiKey }`; `NewClient(baseURL string, ...Option)`; options `WithHTTPClient(*http.Client)`, `WithAPIKey(string)`.
- One method per operation: `func (c *Client) Op(ctx context.Context, [id string,] [params P,] [in Req]) (Resp, error)` — `context.Context` first.
- Models: structs with json tags; typed enums; nested types.
- `APIError{ ... }` implements `error` (`Error()`), plus helpers like `IsNotFound()`.
- Imports stdlib only (`net/http`, `context`, `encoding/json`, `time`, `fmt`, `net/url`) → the generated SDK `go build`s with zero third-party requires.

## Diagnostics
Each carries severity + message + `file:line` provenance. Classes: `float64->float32` narrowing,
free-form map (`map[string]any`), untyped query param, dynamic/unresolvable response, duplicate handler
name. Diagnostics are non-fatal to generation (output still produced) EXCEPT a dangling `$ref` (a route
references a type with no schema) which IS fatal — see Errors. `gnr8 doctor` aggregates them; `gnr8
inspect graph <dir>` lists them.

## Lifecycle semantics
- **Ownership:** generate records a blake3 hash per output in `.gnr8/cache/manifest.json`. If an output
  on disk differs from its recorded hash (user edited it), generate warns+skips it unless `--force`.
- **No-op:** if regenerated bytes equal the on-disk file, the write is skipped (mtime preserved).
- **Determinism:** identical input ⇒ byte-identical output (sorted everywhere). `gnr8 generate` twice → 0 written.
- **`check`:** dry-run of the write plan; non-zero if anything would be written or was user-edited.
- **`watch`:** debounced; ignores gnr8's own output paths (no regen loop); reports cold / warm-no-op /
  single-file-edit latency; Ctrl-C exits 0.
- gnr8 EXCLUDES the configured output paths from analysis (never ingests its own generated SDK).

## Known quirks / limits (do not treat as bugs unless fixing them)
- **One route group only.** Multiple groups (distinct base paths) → `duplicate <METHOD> operation on a
  single path` (lowering). This is the headline gap.
- `float64` → SDK `float32` (precision narrowing).
- Optional pointer fields → value type + `,omitempty` (pointer-ness lost).
- A handler whose success response is built dynamically may infer an odd response type (e.g. an error
  type), or emit a dynamic-response diagnostic.
- Gin-only, Go-only.

## Errors → cause → fix
| Message (substring) | Cause | Fix |
|---|---|---|
| `dangling $ref '<pkg>.<Type>'` | a route references a type not extracted (out of `inputs` scope, or only partially loaded) | widen `inputs` to a dir that includes the type's package; ensure the module type-checks |
| `duplicate <METHOD> operation on a single path` | >1 route group, paths collapse | unsupported today — scope to a single group, or wait for multi-group support |
| `unsupported security scheme` | `kind`/`location` not `apiKey`/`header` | fix `[[security.schemes]]` |
| `duplicate security scheme id` | two schemes share `id` | dedupe |
| config error pointing at `gnr8 init` | no `.gnr8/config.toml` in cwd | run `gnr8 init` (and `cd` to the project root) |
| go toolchain / module load error (reported, not crash) | `go` missing or target not buildable | install Go; make the target module `go build`-clean |

## Recipes
```
# generate + verify in CI
gnr8 generate && gnr8 check            # check exits 1 if generate left drift

# inspect what it sees (no generation, takes a dir)
gnr8 inspect routes ./internal         # human table; add --json for machine
gnr8 inspect graph  ./internal --json  # full graph incl. diagnostics

# diagnose health (exit 1 if actionable)
gnr8 doctor --json

# rename an operation / type
#   .gnr8/config.toml → [naming.operations]\n listBooks = "List"   (or [naming.types])

# live loop
gnr8 watch --debounce-ms 150
```
Runnable end-to-end example with committed input + generated output: [`../examples/bookstore/`](../examples/bookstore/).

## Repo map (for editing the engine)
| Path | Role |
|---|---|
| `goextract/internal/load` | load+typecheck target module (Go helper) |
| `goextract/internal/{routes,handlers,types}` | recognize Gin routes/handlers, extract structs/types → JSON facts |
| `crates/gnr8-core/src/analyze` | subprocess driver + serde facts DTOs |
| `crates/gnr8-core/src/graph` | the API graph (stable ids, sorted) |
| `crates/gnr8-core/src/lower` | graph → OpenAPI 3.1 (`to_openapi(graph, title, base_path, security)`) + YAML writer |
| `crates/gnr8-core/src/sdk` | graph → Go SDK (`generate(graph, package, base_path)`, emit, bundle) |
| `crates/gnr8-core/src/config` | `.gnr8/config.toml` schema |
| `crates/gnr8-core/src/lifecycle` | manifest, `plan_writes`, no-op, `regenerate`, `check`, output-path exclusion |
| `crates/gnr8-core/src/{workspace,diagnostics}` | `init`; diagnostics aggregation |
| `crates/gnr8/src/{main,cli,doctor,watch,render}` | CLI dispatch, exit codes, doctor, watch, output rendering |
| `crates/gnr8-core/tests` | contract snapshots (`snapshot_{graph,openapi,sdk,diagnostics}`), `sdk_compile`, `determinism`, `lifecycle` |

When editing: obey `../CLAUDE.md`. Changing emitted output requires regenerating snapshots
(`fixtures/goalservice/expected/*` + `crates/gnr8-core/tests/snapshots/*.snap`) and the example
(`examples/bookstore/generated/*`); keep `make check` + `make gates` green.
