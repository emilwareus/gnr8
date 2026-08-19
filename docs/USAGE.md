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

The crates.io package named `gnr8` is the public Rust API. Generated `.gnr8/Cargo.toml` files use the
exact `crates/gnr8-core` path from the selected source tree or complete release archive. `gnr8 init`
fails with an actionable error when that resource is missing; it never silently switches to a
registry version.

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
| `gnr8 generate` | `--force` | `.gnr8/` crate, the source dirs its `Source` reads | the paths the pipeline's targets declare, `.gnr8/cache/manifest.json` | 0 when fully reconciled; 1 on protected outputs or error |
| `gnr8 check` | — | `.gnr8/` crate, src, manifest | — (dry run) | **0 up-to-date; 1 stale/drifted**; 1 on error |
| `gnr8 watch` | `--debounce-ms N` (def 200) | `.gnr8/` crate (incl. `.gnr8/src/`), src | same as generate, on each change | 0 on Ctrl-C; 1 on error |
| `gnr8 doctor` | — | `.gnr8/` crate, src, manifest | — | **0 healthy; 1 actionable problem**; never crashes |
`doctor` probes the **source toolchain** for the detected source language (`go`/`python3`/`node`) — it
reports `source_toolchain` + the `language` field, not a hardcoded Go probe.
| `gnr8 inspect routes\|schemas\|graph` | `[<dir>]` (positional, defaults to bundled fixture) | the `<dir>` Go module | — (prints) | 0; 1 on error |

Notes:
- Missing local cache state is safe: generate adopts byte-identical outputs without rewriting and
  reconstructs ownership; divergent outputs are preserved and make the command fail. A **stale** cache
  is safe too: a cache may only make a run faster, so an entry that cannot prove it belongs to this
  run is discarded and recomputed rather than reported.
- `gnr8 check -v` lists the stale and drifted paths behind the summary line; `-vv` adds the bulk
  written/deleted/skipped lists to `generate`.
- `--force` overwrites protected emitted paths (otherwise generate warns and skips them). It never
  recursively deletes unrelated files that share an output directory.
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
The `gnr8` dependency is a version pin (`gnr8 = "=x.y.z"`) when you install from a release
archive. Sidecar extractors still come from the packaged CLI (`share/gnr8` next to the real
executable, or `$GNR8_RESOURCE_DIR`). In-repo development scaffolds keep a local path dependency.

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
- `Cx { project_root }` — the root relative paths resolve against. `Artifacts::create(path, text)` adds
  a generated file with explicit ownership and rejects collisions.
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
        out.create(self.path.clone(), md)
    }
    fn output_anchors(&self) -> Vec<String> { vec![self.path.clone()] } // loop-safety: don't re-ingest
}
// …then: .transform(DropInternalRoutes).target(ApiMarkdown { path: "generated/API.md".into() })
```

Public contract changes should stay at the graph layer. Common examples:
`GroupOperations::new().by_path_prefix("/billing", "Billing")` to define API service grouping,
`RenameOperation::new("listInternal", "listAccounts")` for SDK public method names, and
`StaticFiles::new().from("sdk-static").to("generated/typescript").include(["README.md", "docs/**"])`
for hand-authored docs that must stay lifecycle-owned.
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
| Gin | Go | full | static nested route groups, path/query params, `ShouldBindJSON` body, `c.JSON` responses, const enums, nested structs | dynamic route paths skipped and dynamic group prefixes omitted with diagnostics; `map[string]any` free-form (diag); untyped `c.Query` → string (diag); this frontend recognizes Gin, not arbitrary Go routers. |
| FastAPI | Python | full | `@app`/`@router` verbs, static router/include prefixes, path params (template∩args), typed query params (defaults→required/optional), Pydantic/`@dataclass` bodies, `response_model=` or typed handler returns, collection responses, `status_code=`, `Literal`/`Enum`, `Union` aliases; `Depends` injection is excluded | static `ast` only (never imports/executes the target); unresolvable/foreign type → diagnostic + omit (no guess). |
| Flask | Python | typed-envelope (honest second-class) | `@app.route`/`methods=`, `Blueprint(url_prefix=)`, `<int:id>` converter path params, OPT-IN typed DTOs/returns; method-derived status (typed `POST`→201, else 200) | untyped `request.json` / unannotated `request.args` / missing return annotation → **diagnostic, NEVER inferred**. State plainly: untyped surfaces are NOT recovered (typed-envelope only). |
| NestJS | TypeScript | class-DTO scope | `@nestjs/common` verb + `@Param`/`@Query`/`@Body` decorators, statically composed `@Controller` prefix, DTO **classes**, enums + string-literal-union, synchronous or `Promise<T>` returns, collection responses, method-derived status (`@HttpCode` override) | DTO **classes** only (bare `interface`s are erased — not extracted); never reads `@nestjs/swagger` / `zod` / `class-validator` (rule 1); unresolvable → diagnostic + omit. |

Generated SDKs keep HTTP dependency-free: GoSdk uses `net/http`, PySdk uses `urllib`, and TsSdk uses
the built-in `fetch`. PySdk emits Pydantic v2 `BaseModel` models by default, with
`.dataclasses()` available for stdlib-only model consumers. The `tsextract` sidecar resolves the
**project's own `typescript`** toolchain (required, not shipped — see CLAUDE.md); every other sidecar is
stdlib-only (Go `go/types`, Python `ast`), and `gnr8-core` itself keeps a small Rust dependency set.
The CLI's focused open-source dependencies support bounded commodity concerns; the source-to-SDK
pipeline remains gnr8-owned end to end.

TsSdk query serialization is explicit: scalar parameters use one key and one-dimensional scalar
arrays use repeated keys (`?tag=a&tag=b`). Object, map, union, nested-array, and unconstrained query
shapes fail generation because the graph does not declare a wire encoding for them.

Graph-level field presence overrides are available for explicit source corrections:

```rust
ApiOverrides::new()
    .force_optional("User", "settings")
    .force_required("Event", "id")
```

Each states presence outright — "this key may be absent" / "this key is always present" — rather than
correcting one axis, so it reads the same in every direction the schema is reached from and in every
generated SDK model. `force_optional` therefore also marks the field omittable in the SDKs
(`,omitempty`, `?`, a default); `force_required` also removes that marking.

Request-body overrides can also create or replace an operation body when the source graph lacks the
required fact. Typed helpers default to required; `.optional()` applies to the most recently configured
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

OpenAPI targets support narrow document presentation patches. `enum_values(...)` sorts values
deterministically; `enum_values_in_order(...)` preserves caller order.

```rust
OpenApi31::new()
    .to("generated/openapi.yaml")
    .schema_patch(
        OpenApiSchemaPatch::new("Book").field(
            OpenApiFieldPatch::new("status")
                .description("Public lifecycle status")
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
| summary / description | the handler's own **doc comment**, as plain prose | first sentence → `summary`, remainder → `description`. Only routed handlers are read. No marker or grammar of any kind; a comment can carry nothing else. |
| required field | struct tag `binding:"required"` or `validate:"required"` | → schema `required` **on a request**. Only the field's own scope counts — see below. |
| source-optional field | `json:",omitempty"` / `json:",omitzero"` — **not** pointer `*T` | the presence axis, which is what keeps a field **out of** schema `required` **on a response** — a field with no omission option is written on every response, so it is always present. A bare `*T` keeps its key (`encoding/json` writes `null` into it), so it is nullable, not optional. |
| enum | named `string` type + `const` set | → OpenAPI string enum + Go typed newtype. |
| from config (not source) | security schemes, base/mount path, title | not expressible in typed source — set by transforms (`ApplySecurity`/`SetBasePath`/`SetTitle`) in the `.gnr8/` crate. |

### Presence is answered from the direction the schema is reached from

"Must this key be present?" is one question, and which source code fact answers it depends on which
side of the exchange the payload is on. The OpenAPI `required` array:

| The schema is reached from | `required` is | Because |
|---|---|---|
| requests only (a request body, a parameter, or a schema one of those reaches) | the `binding:`/`validate:` `required` rules | that is what the server rejects a request for lacking |
| responses only | every field with **no** `json` omission option | that is what `encoding/json` writes unconditionally; nothing validates a response |
| both | only the fields that satisfy **both** rules | one component describes both payloads, so it can only promise what holds in each |
| no route at all | the `binding:`/`validate:` `required` rules | a DTO struct is a component schema whether or not a route uses it, and an unwired one occupies no position to be read from |

The direction is a property of your routes, not a setting. A DTO used in one direction gets an exact
answer; a struct shared between a request body and a response body gets the narrower one, because a
field the request does not demand may legitimately be absent from what a client sends, and a field
the serializer may omit is not something a client can count on receiving. **If you want the exact
answer in each direction, use a separate type for each** — that is the only thing that makes the two
questions separately answerable.

Generated SDK models — Go's `,omitempty`, TypeScript's `?:`, Python's `= None` — read the same walk:

| The schema is reached from | The model may leave the key out when | Because |
|---|---|---|
| requests only | **no** `binding:`/`validate:` `required` rule demands it | `,omitempty` governs marshalling, and your server unmarshals a request DTO — it never marshals one, so the tag says nothing about what a client may omit |
| responses only, both, or no route at all | the field carries a `json` omission option | the model is (or may be) the decode side, where the key's absence is your server's choice and not the caller's |

The first row is the one that keeps a caller from building a request the server rejects: a field
written `json:"name,omitempty" binding:"required"` is required in a request model, and the SDK will
not let it be omitted. The second row is what keeps a response model decodable, so a field the
serializer may drop is never demanded — which is also why a type used in **both** directions keeps
the response answer: over-requiring it would break decoding a payload your server is entitled to
send. On such a type a validated-and-omittable field stays omittable in the model, and the document
publishes it as not required too; splitting the type in two is what makes both directions exact.

### Validation rules apply at the scope they are written in

A `binding:`/`validate:` tag can talk about the field or about what is *inside* it. `dive` steps
into the field's elements and `keys`…`endkeys` selects a map's keys, so only the tokens **before
the first `dive`** describe the field:

```go
Headers  map[string]string `json:"headers,omitzero" binding:"omitempty,dive,keys,required,endkeys,required"`
Segments []string          `json:"segments" validate:"required,dive,min=1,max=100"`
```

`headers` is **optional** — the tag forbids empty map keys and values, it does not demand the key.
`segments` is **required** because that `required` precedes the `dive`, and its `min`/`max` bound
each string rather than the slice, so they are not published as the array's constraints.

The same rule applies to bound query, header, and form parameters: `binding:"omitempty,dive,required"`
on a repeated parameter forbids empty entries, it does not make the parameter itself required.

For a schema field, gnr8 records constraints on the field only, so a rule written past a `dive`
reaches nothing it can bind. What happens next depends on whether gnr8 knows the rule, not on where
it was written:

| The rule | Behind a `dive` | At field scope |
|---|---|---|
| one gnr8 lowers, with a value it can read (`required`, `min`, `max`, `gte`, `lte`, `gt`, `lt`, `oneof`) | dropped silently — the graph has no element schema to carry it | applied |
| one gnr8 does not lower (`email`, `uuid`, any other validator) | `schema.metadata.unresolved` diagnostic | `schema.metadata.unresolved` diagnostic |
| one gnr8 lowers, with a value it cannot read (`gte=abc`, a bare `oneof`) | `schema.metadata.unresolved` diagnostic | `schema.metadata.unresolved` diagnostic |

So `validate:"dive,min=1"` is silent, while `validate:"dive,email"` and `validate:"dive,gte=abc"` warn
exactly as `validate:"email"` and `validate:"gte=abc"` do — gnr8 drops the rule in every case, and you
hear about it whenever it could not read what you wrote. The sharp edge to know is the first row:
**`dive,min=1` does not become a per-item `minLength` in the emitted document, and says nothing when
it disappears.** If you need element constraints in the spec, state them on the element's own named
type, or add them with a `Transform` in your `.gnr8/` crate.

`oneof` on a **bound parameter** is the one rule a `dive` does not discard. A parameter carries a
schema and nothing beside it — there is no constraint object, which is why `min`/`max` never reach a
parameter at any scope — so `oneof` is stated by replacing a schema, and the scope says which one:

| Written on a bound parameter | Where the enum lands |
|---|---|
| `binding:"oneof=…"` on a scalar | the parameter's own schema |
| `binding:"oneof=…"` on an array or map | what the container holds — an array or map has no room for an enum beside it |
| `binding:"dive,oneof=…"` | the array's element, or the map's **values** |
| `binding:"dive,keys,oneof=…,endkeys"` | nowhere — see below |
| a scope the parameter has no value for (`dive` on a scalar, `keys` on an array) | nowhere — dropped in silence, as on a schema field |

So `Filters map[string]string` tagged `binding:"dive,oneof=red green"` publishes a map whose values
are an enum, with the key left as the plain string OpenAPI requires. A map *query* parameter still
has no TsSdk wire encoding, so that target rejects it with or without the enum — the enum used to
collapse the parameter to a bare string and hide that, which is the bug, not the rejection.

**A `keys`…`endkeys` enum is read and discarded.** An OpenAPI object key is always a string, and gnr8
does not emit `propertyNames` to constrain it, so there is nowhere to put the members — and lowering
*rejects* a map whose key is not a plain string, which would abort generation for the whole document
rather than drop one rule. A well-formed tag must never do that, so the rule is dropped in silence
like any other gnr8 understands but cannot carry. A `Transform` cannot put it back either: writing the
enum onto the parameter's schema reaches the same gate and fails the same way, and the graph has no
other vocabulary for a constrained key. Constrain the map's values instead, or — when the key set is
known and finite — model it as a named object type, whose keys are properties rather than map keys.

**Two rules that land on the same value are an error, not a contest.** `binding:"oneof=a b,dive,oneof=c d"`
on a `[]string` states the element's enum twice; so does `binding:"oneof=a b" validate:"oneof=a b"` on
a string, even though both spellings agree. gnr8 emits a `request.parameter.ambiguous` **ERROR**
naming each rule with the tag key that carries it, applies neither, and leaves the parameter's schema
as the source types it — picking a winner would be a precedence rule, and a fact stated twice has no
winner. State it once.

## Operation prose (all three languages)

An operation's human-readable `summary` and `description` come from the routed handler's own
documentation comment, read the way that language's own toolchain reads it:

| Language | Read from |
|---|---|
| Go | the handler's doc comment (`// listBooks returns …`). A leading `listBooks ` is stripped and the next letter capitalized, matching Go's universal convention. |
| Python | the handler's docstring (PEP 257). |
| TypeScript | the method's JSDoc **leading description**. JSDoc tags are excluded by the compiler, so an `@param` or `@openapi` block is invisible to gnr8. |

The rule is identical everywhere:

> **first sentence → `summary`; everything after it → `description`.**

A `.` preceded by a single capital does not end the sentence, so `Reviewed by A. Smith before
release.` stays one sentence. The summary is folded to a single line; the description keeps the
author's line structure verbatim and is never re-wrapped. Nothing is dropped.

There is **no directive syntax, no marker prefix, and no key/value grammar** inside the comment —
not `@Summary`, not `gnr8:summary`, not anything. A comment adds words and nothing else: method,
path, params, body, responses, status codes, tags, security, deprecation, and operationId are all
code-inferred and a comment can neither state nor override them.

Only handlers that are **actually routed** are read, so documenting an internal helper cannot
affect generation.

The prose reaches the OpenAPI document *and* the generated SDKs — Go method comments, Python
docstrings, and TypeScript JSDoc — so an IDE shows the same words as the spec.

To make documentation mandatory, add the opt-in `RequireOperationDocs` transform; see
[`docs/pipeline/transforms.md`](pipeline/transforms.md).

## Type mapping (Go → OpenAPI → generated SDK) — verified
| Go | OpenAPI | SDK Go | Note |
|---|---|---|---|
| `string` | `string` | `string` | |
| `bool` | `boolean` | `bool` | |
| `int`/`int64` | `integer` | `int64` | |
| `float32` | `number`/`float` | `float32` | width preserved |
| `float64` | `number`/`double` | `float64` | width preserved |
| `time.Time` | `string`/`date-time` | `time.Time` | |
| `uuid.UUID` | `string`/`uuid` | `string` | well-known |
| `*T` | nullable | `*T`; `,omitempty` only when optional | `encoding/json` writes `"k":null` and **keeps the key** — a bare pointer is nullable, not optional. |
| `,omitzero` | source-optional | **value `T` + `,omitempty`** | omits the zero value of **any** type; the omission signal, not nullability. |
| `,omitempty` | source-optional *for the types it omits* | **value `T` + `,omitempty`** | omits only `false`, `0`, `""`, nil pointer/interface, zero-length array/slice/map. A **no-op on a struct, a `time.Time`, or a non-zero-length array** → `schema.omit_option.ineffective`. |
| `[]T` | `array`, nullable | `[]T` | a nil slice marshals to `null`, so the value axis is nullable whatever the tag says. |
| `map[string]T` | `object`,`additionalProperties:true`, nullable | `map[string]T` | free-form → diagnostic; a nil map marshals to `null`. |
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
Each carries severity + message + `file:line` provenance. INFO classes include fully represented but
unconstrained free-form maps. WARN classes include untyped query params, dynamic responses,
unsupported static patterns, and duplicate handler names. ERROR classes include unknown handlers,
missing response facts, and package-load failures; any ERROR makes `gnr8 doctor` unhealthy. Lowering
also fails on incomplete response/request media facts and dangling refs. `gnr8 inspect graph <dir>`
lists every diagnostic.

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
- Optional and nullable are independent, and each reads exactly one thing. **Presence** (may the key be
  absent) comes from the `json` tag's omission option alone — never from the declared type. **Nullability**
  (may the value be `null`) comes from the declared type alone: a nil pointer, slice, map, or interface is
  what `encoding/json` writes as `null` — and what `json.Unmarshal` accepts a `null` into, whatever
  the tag says, which is why an `,omitempty` field is nullable even though it can never *write*
  `null`. So `[]T json:"k"` is nullable-but-always-present, and
  `*T json:"k"` is too — neither is optional until the tag says so. A `form:`-tagged field is a
  different wire with different rules: never nullable (a part has no `null`), and optional when the
  part is a pointer **or** the tag carries `,omitempty` (`,omitzero` is read on the `json` wire only).
- Field presence is answered from the direction the schema is reached from, in the document and in
  every generated SDK model, so the same struct fields can come out required in a request DTO and
  omittable in a response DTO. A struct shared between the two keeps the response answer in its model
  and publishes only what holds in both in its document — see
  [presence is answered from the direction the schema is reached from](#presence-is-answered-from-the-direction-the-schema-is-reached-from).
- A handler whose success response is built dynamically may infer an odd response type (e.g. an error
  type), or emit a dynamic-response diagnostic.
- The Go frontend recognizes Gin route registration, not arbitrary Go routers.

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
