# gnr8 — Reference (agent-oriented)

Dense reference for operating and editing gnr8. Terse by design. Source of truth for behavior is the
code; this matches the current build. Product invariants: [`../CLAUDE.md`](../CLAUDE.md) (one source per
fact, an end-to-end owned generation chain, no fallback chains, config supplies what typed source can't).

## What it is / isn't
- Reads source services or existing OpenAPI artifacts into a router-agnostic API graph, emits
  **OpenAPI 3.1**, and generates client SDKs. Supported source frontends today: **Go + Gin**,
  **Python FastAPI**, **Python Flask typed-envelope**, **TypeScript NestJS class DTOs**, and
  **Swagger 2.0 / OpenAPI 3.0 / OpenAPI 3.1 JSON or YAML artifacts**.
- **Envelope (hard limits today):** each `Source` takes exactly one input directory. Go support is Gin
  only, with static nested route groups folded into operation paths. Dynamic Gin route paths and group
  prefixes are diagnostics, not guesses. Flask intentionally extracts typed DTO/return envelopes only. NestJS
  extracts DTO classes, not erased interfaces or swagger/zod/class-validator metadata. Unsupported facts
  become diagnostics, not guesses.
- **Config is CODE, not a file.** Facts not in typed source (base/mount path, OpenAPI title, security
  schemes) — and the whole parse/generate lifecycle — are expressed as Rust in a project-local `.gnr8/`
  crate that drives the engine. There is **no TOML/YAML/JSON config** (see "Config: the `.gnr8/` crate").

## Build
```
cargo build --release -p gnr8-cli    # binary: target/release/gnr8
make check                           # fmt + clippy -D warnings + all tests + go builds
make gates                           # the contract suite (4 snapshots, sdk_compile, determinism, lifecycle)
```
Requires the **source language's toolchain** (Go/Python/TypeScript) on PATH — gnr8 shells a
per-language helper to load the target (Go module, Python `ast`, TS Compiler API). The toolchain that
matters is the one the analyzed project is written in: a Go service needs `go`, a FastAPI/Flask service
needs `python3`, a NestJS service needs `node` + the project's own `typescript` (see CLAUDE.md
"TypeScript toolchain (required, not shipped)").

## Install

The CLI install path is the GitHub release archive:

```bash
curl -fsSL https://raw.githubusercontent.com/emilwareus/gnr8/main/scripts/install.sh | bash
```

The crates.io package named `gnr8` is the Rust API used by generated `.gnr8/Cargo.toml` files. The
generated project-local crate depends on `gnr8 = "0.1"` when no release-archive resource copy is found.

## Canonical workflow
```
cd <your-go-service>      # the dir whose .gnr8/ crate drives generation; inputs resolve from here
gnr8 init --source go-gin --sdk go
# edit .gnr8/src/main.rs: the Pipeline IS the config — source, transforms, targets, post-process
gnr8 generate             # compile + run .gnr8/, write OpenAPI + Go SDK; track ownership; skip unchanged
gnr8 check                # CI gate: exit 1 if any output is stale/drifted, else 0
```

## CLI
All commands except `inspect` operate on the **current project** (cwd must hold the `.gnr8/` crate, i.e.
`.gnr8/Cargo.toml`). `generate`/`check`/`watch`/`doctor` **delegate to the `.gnr8/` crate**: the host
runs `cargo run --manifest-path .gnr8/Cargo.toml -- __emit` (cwd = project root), parses the JSON
artifact bundle the child prints, and owns the writes. Global flags: `--json` (machine output),
`-v`/`-vv` (verbosity).

| Command | Args/flags | Reads | Writes | Exit |
|---|---|---|---|---|
| `gnr8 guide` | `[go-gin-to-python-typescript\|python-apis-to-python-sdk\|nestjs-to-typescript-sdk]` | bundled docs | — (prints basic or scenario-specific agent guide) | 0 |
| `gnr8 init` | `--source go-gin\|fastapi\|flask\|nestjs`, `--sdk go\|python\|typescript` | — | `.gnr8/Cargo.toml`, `.gnr8/src/main.rs`, `.gnr8/README.md`, `.gnr8/.gitignore` (skips existing — idempotent) | 0; 1 on error |
| `gnr8 generate` | `--force` | `.gnr8/` crate, the source dirs its `Source` reads | the paths the pipeline's targets declare, `.gnr8/cache/manifest.json` | 0; 1 on error |
| `gnr8 check` | — | `.gnr8/` crate, src, manifest | — (dry run) | **0 up-to-date; 1 stale/drifted**; 1 on error |
| `gnr8 watch` | `--debounce-ms N` (def 200) | `.gnr8/` crate (incl. `.gnr8/src/`), src | same as generate, on each change | 0 on Ctrl-C; 1 on error |
| `gnr8 doctor` | — | `.gnr8/` crate, src, manifest | — | **0 healthy; 1 actionable problem**; never crashes |
`doctor` probes the **source toolchain** for the detected source language (`go`/`python3`/`node`) — it
reports `source_toolchain` + the `language` field, not a hardcoded Go probe.
| `gnr8 inspect routes\|schemas\|graph` | `[<dir>]` (positional, defaults to bundled fixture) | the `<dir>` Go module | — (prints) | 0; 1 on error |

Notes:
- `--force` overwrites outputs a user hand-edited (otherwise generate warns+skips them — ownership protection).
- `inspect` is the ONLY command taking a target dir; the others derive inputs from the pipeline's `Source`.
- `watch` re-runs on a source-language edit (`.go`/`.py`/`.ts`, picked from the detected source language)
  OR a `*.rs` edit under `.gnr8/src/` (you changed the pipeline → recompile + re-run); it ignores its own
  outputs and the `.gnr8/target`/`.gnr8/cache` dirs (no regen loop).
- No command panics on bad input/missing toolchain — typed error → clean stderr + non-zero. A `.gnr8/`
  that is missing, won't compile, or whose `cargo` is absent surfaces as an actionable error.

## Config: the `.gnr8/` crate (code, not TOML)
**There is no config file.** Configuration is a small Rust **binary crate** at `.gnr8/` that depends on
`gnr8` and drives the lifecycle. `gnr8 init` scaffolds it; `gnr8 generate` compiles + runs it. The
crate's `src/main.rs` builds a `Pipeline` and hands it to the runner — that pipeline IS the config.
Every knob that used to be TOML is now a method call; anything the knobs couldn't express, you write as
ordinary Rust (a custom `Source`/`Transform`/`Target`/`PostProcess`).

`.gnr8/` layout (scaffolded, idempotent — each file written only if absent):
```
.gnr8/
  Cargo.toml      # name "<dir>-gnr8-gen", edition 2021, publish=false, empty [workspace]; gnr8 dep
  src/main.rs     # the Pipeline — THE config; you edit this
  .gitignore      # /target/  /cache/
  cache/          # ownership manifest (git-ignored)
```
The `gnr8` dep is a `path = "…"` dep when `.gnr8/` is inside the gnr8 repo or release archive, and a
version dep (`gnr8 = "0.1"`) otherwise.

### The SDK surface (`gnr8::sdk`, re-exported as `gnr8::sdk::prelude`)
A pipeline composes four kinds of stage, decoupling **N sources** from **M targets** through one IR
(`gnr8::graph::ApiGraph`: `operations`, `schemas`, `base_path`, `title`, `security`, `diagnostics`).

| Trait | Signature | Role | Built-ins |
|---|---|---|---|
| `Source` | `load(&self, &Cx) -> Result<ApiGraph, CoreError>` | source code/artifact → IR | `GoGin`, `FastApi`, `Flask`, `NestJs`, `OpenApi` |
| `Transform` | `apply(&self, &mut ApiGraph, &Cx) -> Result<(), CoreError>` | IR → IR (where TOML knobs now live, as code) | `SetBasePath`, `SetTitle`, `ApplySecurity`, `RenameOperation`, `RenameType`, `GroupOperations`, `ApiOverrides`, `SetEnumOrder` |
| `Target` | `generate(&self, &ApiGraph, &mut Artifacts, &Cx) -> Result<(), CoreError>` (+ `output_anchors()`) | frozen IR → `Artifacts` | `OpenApi31`, `GoSdk`, `PySdk`, `TsSdk` |
| `PostProcess` | `run(&self, &mut Artifacts, &Cx) -> Result<(), CoreError>` | `Artifacts` → `Artifacts` (after all targets) | `Header` |

- `Pipeline::new().source(..).transform(..).target(..).post(..)` — builder, stages kept in call order.
- `Cx { project_root }` — the root relative paths resolve against. `Artifacts::write(path, text)` — add
  a generated file (kept sorted by path → deterministic; same path twice = last-write-wins).
- `gnr8::runner::run(pipeline) -> ExitCode` — the entry point `main()` returns. It parses argv
  (`__emit` → print the artifact bundle JSON; `__inspect` → print the frozen IR JSON) and never panics.

Built-in builder methods (each replaces a former TOML key):

| Was (TOML) | Now (code) |
|---|---|
| `inputs = ["."]` | `GoGin::new().inputs(["."])` (one dir; >1 is a typed error) |
| existing OpenAPI artifact | `OpenApi::new().input("openapi.yaml")` |
| `base_path = "/books"` | `.transform(SetBasePath::new("/books"))` |
| `title = "Bookstore API"` | `.transform(SetTitle::new("Bookstore API"))` |
| `[[security.schemes]]` (apiKey/header) | `.transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))` |
| `naming.operations` | `.transform(RenameOperation::new("listBooks", "List"))` |
| `naming.types` | `.transform(RenameType::new("Old", "New"))` ($ref-rewriting; collision/cycle → typed error) |
| `output.openapi` | `.target(OpenApi31::new().to("generated/openapi.yaml"))` |
| `output.sdk_dir` + `output.go_module` | `.target(GoSdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))` |

`GoSdk` derives the SDK package name from the module's sanitized last path segment (`.../sdk` → `package
sdk`). `Header::generated()` stamps `// Code generated by gnr8. DO NOT EDIT.` on every `.go` artifact.

Example `.gnr8/src/main.rs` (the bookstore lifecycle):
```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(GoGin::new().inputs(["."]))
            .transform(SetBasePath::new("/books"))
            .transform(SetTitle::new("Bookstore API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(GoSdk::new().module("example.com/bookstore/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

### Writing your own stage (the escape hatch is code)
Anything the built-ins don't cover, you implement as a trait and add to the pipeline — no forking a
generator, no config DSL. The IR (`gnr8::graph`) is read/write so a `Transform` edits it freely;
`ApiGraph::operations[]` are `Operation { id, method, path, handler, params, request_body, responses }`,
`schemas[]` are `Schema { id, name, kind, fields, enum_values }`.

```rust
use gnr8::graph::ApiGraph;
use gnr8::sdk::prelude::*;
use gnr8::CoreError;

// A custom Transform: edit the IR before generation (e.g. drop internal routes
// that existed in an old generator input but should not ship in public SDKs).
struct DropInternalRoutes;
impl Transform for DropInternalRoutes {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.operations.retain(|op| !op.path.starts_with("/internal/"));
        Ok(())
    }
}

// A custom Target: write your own generator (e.g. an API.md summary).
struct ApiMarkdown { path: String }
impl Target for ApiMarkdown {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        let mut md = format!("# {}\n\n", ir.title);
        for op in &ir.operations { md.push_str(&format!("- {} {} ({})\n", op.method, op.path, op.id)); }
        out.write(self.path.clone(), md);
        Ok(())
    }
    fn output_anchors(&self) -> Vec<String> { vec![self.path.clone()] } // loop-safety: don't re-ingest
}
// …then: .transform(DropInternalRoutes).target(ApiMarkdown { path: "generated/API.md".into() })
```

Production migration patches should stay at the graph/profile layer. Common examples:
`GroupOperations::new().by_path_prefix("/billing", "Billing")` to preserve API service grouping,
`RenameOperation::new("legacyOp", "LegacyApi_legacyOp")` for SDK public method names, and
`StaticFiles::new().from("sdk-static").to("generated/typescript").include(["README.md", "docs/**"])`
for hand-authored compatibility docs that must stay lifecycle-owned.
A `Source` shells out / parses to produce an `ApiGraph`; a `PostProcess` rewrites the in-memory
`Artifacts` (license header, import rewrite). The full runnable Go example: `examples/taskflow/`. The
cross-language example lifecycles (real committed output) live at `examples/fastapi-bookstore/` (Python →
OpenApi31 + PySdk), `examples/flask-bookstore/` (Python, the honest typed-envelope — untyped surfaces
become diagnostics → OpenApi31 + PySdk), and `examples/nestjs-bookstore/` (TypeScript → OpenApi31 + TsSdk).
All five examples (plus `examples/bookstore/` Go/Gin) are byte-identical-regen-gated by `make examples-check`.

### Host ↔ child boundary
`gnr8 generate` runs `cargo run --manifest-path .gnr8/Cargo.toml -- __emit` with `cwd = project root`.
The child runs the pipeline (source → transforms → freeze → targets → post) and prints a versioned JSON
bundle (`{ version, artifacts: [{path, text}], diagnostics }`) on stdout. The **host** then owns the
writes: the ownership manifest, no-op skip (byte-identical), edit-protection (warn+skip user-edited
unless `--force`), and excluding the pipeline's own output paths from analysis. The child is a pure,
side-effect-free function; the host is the single trusted writer — so `check`/`watch`/`doctor` reuse it.

## Supported source frontends (the honest envelope)
gnr8 supports four source frontends across three languages. Each row states what is actually recognized
and where the limits are — there is no overclaiming; an unrecognized/untyped surface becomes a diagnostic
and the fact is omitted (never guessed). The per-language behavior below is the verified extractor
contract (the committed graph/OpenAPI snapshots are the spec).

| Frontend | Lang | Status | Recognized | Limits / diagnostics |
|---|---|---|---|---|
| Gin | Go | full | static nested route groups, path/query params, `ShouldBindJSON` body, `c.JSON` responses, const enums, nested structs | dynamic route paths skipped and dynamic group prefixes omitted with diagnostics; `float64`→`float32` narrowing (diag); `map[string]any` free-form (diag); untyped `c.Query` → string (diag); Gin-only. |
| FastAPI | Python | full | `@app`/`@router` verbs, `APIRouter`/literal prefixes, path params (template∩args), typed query params (defaults→required/optional), Pydantic/`@dataclass` bodies, `response_model=`, `status_code=`, `Literal`/`Enum`, `Union` aliases | static `ast` only (never imports/executes the target); unresolvable/foreign type → diagnostic + omit (no guess). |
| Flask | Python | typed-envelope (honest second-class) | `@app.route`/`methods=`, `Blueprint(url_prefix=)`, `<int:id>` converter path params, OPT-IN typed DTOs/returns; method-derived status (typed `POST`→201, else 200) | untyped `request.json` / unannotated `request.args` / missing return annotation → **diagnostic, NEVER inferred**. State plainly: untyped surfaces are NOT recovered (typed-envelope only). |
| NestJS | TypeScript | class-DTO scope | `@nestjs/common` verb + `@Param`/`@Query`/`@Body` decorators, `@Controller` prefix (provenance, never folded), DTO **classes**, enums + string-literal-union, method-derived status (`@HttpCode` override) | DTO **classes** only (bare `interface`s are erased — not extracted); never reads `@nestjs/swagger` / `zod` / `class-validator` (rule 1); unresolvable → diagnostic + omit. |

Generated SDKs keep HTTP dependency-free: GoSdk uses `net/http`, PySdk uses `urllib`, and TsSdk uses
the built-in `fetch`. PySdk emits Pydantic v2 `BaseModel` models by default, with
`.dataclasses()` available for stdlib-only model consumers. The `tsextract` sidecar resolves the
**project's own `typescript`** toolchain (required, not shipped — see CLAUDE.md); every other sidecar is
stdlib-only (Go `go/types`, Python `ast`), and `gnr8-core` itself keeps a small Rust dependency set.
The CLI's focused open-source dependencies support bounded commodity concerns; the source-to-SDK
pipeline remains gnr8-owned end to end.

Graph-level field requiredness overrides are available for source quirks and legacy migration patches:

```rust
ApiOverrides::new()
    .force_optional("User", "settings")
    .force_required("Event", "id")
```

Request-body overrides can also create or replace an operation body when the source graph lacks the
legacy shape. Typed helpers default to required; `.optional()` applies to the most recently configured
body. Plain `.request_body(method, path).optional()` keeps its existing meaning: requiredness-only, and
it errors if no body already exists.

```rust
ApiOverrides::new()
    .json_request_body("POST", "/books", "CreateBookRequest")
    .optional()
    .form_request_body("POST", "/oauth/token", "OAuthTokenRequest")
    .multipart_request_body("POST", "/files/upload", "UploadFileRequest");
```

These overrides mutate the graph before OpenAPI or SDK targets render, so all generated surfaces agree.

OpenAPI targets support narrow document patches for migration-only polish. `alias` emits a `$ref`
component alias; `clone_alias` duplicates the canonical schema body for generators that require a
distinct schema. `enum_values(...)` sorts values deterministically; `enum_values_in_order(...)`
preserves caller order.

```rust
OpenApi31::new()
    .to("generated/openapi.yaml")
    .schema_aliases(
        OpenApiSchemaAliases::new()
            .alias("CreateBookRequest", "BookCreateRequest")
            .clone_alias("Book", "LegacyBook"),
    )
    .schema_patch(
        OpenApiSchemaPatch::new("Book").field(
            OpenApiFieldPatch::new("status")
                .description("Lifecycle status shown in the legacy SDK")
                .enum_values_in_order(["beta", "alpha"])
                .example_string("beta")
                .extension_string("x-gnr8-render", "input")
                .extension_number("x-rank", 2)
                .extension_bool("x-visible", true)
                .extension_null("x-empty"),
        ),
    );
```

## Recognized Go/Gin patterns (code-first)
Resolution is via `go/types` (alias/import-robust), not string matching.

| Fact | Source pattern | Notes |
|---|---|---|
| route | `group := r.Group(...)` then `group.GET/POST/PUT/DELETE(path, handler)` | Static nested groups compose into the route path. `path` is group-relative; final path = `base_path` + grouped path. |
| path param | `:name` segment + `c.Param("name")` | → OpenAPI `{name}`. |
| query param | `c.Query("name")` | name only; type=`string`, `required:false` (no type/enum/required inferable from `c.Query`). |
| request body | `c.ShouldBindJSON(&x)` where `x: T` | T → request schema. |
| response | `c.JSON(http.StatusXxx, v)` where `v: T` | status→T. Unresolved/dynamic → diagnostic. |
| operationId | handler func/method name | overridable via a `RenameOperation` transform. |
| required field | struct tag `binding:"required"` or `validate:"required"` | → schema `required`. |
| source-optional field | pointer `*T` and/or `json:",omitempty"` | source optionality signal; schema `required` still comes from required tags. |
| enum | named `string` type + `const` set | → OpenAPI string enum + Go typed newtype. |
| from config (not source) | security schemes, base/mount path, title | not expressible in typed source — set by transforms (`ApplySecurity`/`SetBasePath`/`SetTitle`) in the `.gnr8/` crate. |

## Type mapping (Go → OpenAPI → generated SDK) — verified
| Go | OpenAPI | SDK Go | Note |
|---|---|---|---|
| `string` | `string` | `string` | |
| `bool` | `boolean` | `bool` | |
| `int`/`int64` | `integer` | `int64` | |
| `float64` | `number` | **`float32`** | ⚠ narrows precision → diagnostic emitted |
| `time.Time` | `string`/`date-time` | `time.Time` | |
| `uuid.UUID` | `string`/`uuid` | `string` | well-known |
| `*T` | nullable, source-optional | **value `T` + `,omitempty`** (pointer dropped) | schema `required` is still tag-driven. |
| `,omitempty` | source-optional | **value `T` + `,omitempty`** | omission signal, not nullability. |
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
free-form map (`map[string]any`), untyped query param, dynamic/unresolvable response, unsupported Gin
route pattern, duplicate handler name. Diagnostics are non-fatal to generation (output still produced)
EXCEPT a dangling `$ref` (a route
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
- Static Gin group prefixes are folded only when they are literal strings. Dynamic route paths are
  skipped with diagnostics; dynamic group prefixes are omitted with diagnostics.
- `float64` → SDK `float32` (precision narrowing).
- Optional pointer fields → value type + `,omitempty` in the Go SDK (pointer-ness lost there), while
  the graph still carries nullability for OpenAPI and TypeScript targets.
- A handler whose success response is built dynamically may infer an odd response type (e.g. an error
  type), or emit a dynamic-response diagnostic.
- Gin-only, Go-only.

## Errors → cause → fix
| Message (substring) | Cause | Fix |
|---|---|---|
| `dangling $ref '<pkg>.<Type>'` | a route references a type not extracted (out of the `Source`'s input scope, or only partially loaded) | widen `GoGin::new().inputs([..])` to a dir that includes the type's package; ensure the module type-checks |
| `unsupported Gin route pattern` | dynamic path/group prefix or unnamed route handler | make the route path/group literal, use a named handler, or add an explicit custom source/transform patch |
| `duplicate <METHOD> operation on a single path` | two routes normalize to the same method/path | rename/scope one route, or add `SetBasePath`/group prefixes so public paths are distinct |
| `unsupported security scheme` | `kind`/`location` not `apiKey`/`header` | use `ApplySecurity::api_key(..)` (apiKey/header is the supported scheme) |
| `duplicate security scheme id` | two `ApplySecurity` transforms share an `id` | dedupe |
| `no .gnr8/ workspace … run `gnr8 init`` | no `.gnr8/Cargo.toml` in cwd | run `gnr8 init` (and `cd` to the project root) |
| child won't compile / `cargo` not found | `.gnr8/src/main.rs` has a Rust error, or no cargo on PATH | fix the reported compile error; install a Rust toolchain |
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

# rename an operation / type (edit .gnr8/src/main.rs)
#   .transform(RenameOperation::new("listBooks", "List"))   // or RenameType::new("Old", "New")

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
| `crates/gnr8-core/src/gosdk` | graph → Go SDK (`generate(graph, package, base_path)`, emit, split bundle) |
| `crates/gnr8-core/src/sdk` | the code-as-config SDK: `Pipeline`, the 4 traits, built-in stages, `Artifacts`/`Cx`, `prelude` |
| `crates/gnr8-core/src/runner` | the `.gnr8/` child entry (`run`): `__emit`/`__inspect`, the `ArtifactBundle` wire schema |
| `crates/gnr8-core/src/lifecycle` | manifest, `plan_writes`, no-op, `regenerate`, `check`, output-path exclusion |
| `crates/gnr8-core/src/{workspace,diagnostics}` | `init` (scaffolds the `.gnr8/` crate); diagnostics aggregation |
| `crates/gnr8/src/{main,cli,child,doctor,watch,render}` | CLI dispatch, the host→child driver, exit codes, doctor, watch, rendering |
| `crates/gnr8-core/tests` | contract snapshots (`snapshot_{graph,openapi,sdk,diagnostics}`), `sdk_compile`, `determinism`, `lifecycle` |

When editing: obey `../CLAUDE.md`. Changing emitted output requires regenerating snapshots
(`fixtures/goalservice/expected/*` + `crates/gnr8-core/tests/snapshots/*.snap`) and the examples
(`examples/bookstore/generated/*`, `examples/taskflow/generated/*`); keep `make check` + `make gates`
green.
