# Research / design: extensibility — many sources, many targets, user-owned parsers & generators

Companion to [`code-as-config.md`](code-as-config.md). That doc establishes *how* config is code (the
`.gnr8/` Rust lifecycle crate + host/child model). This doc designs *what that code can compose*: the
multi-source / multi-target architecture, the extension interfaces a user implements to add their own
parsers and generators, and the pre/post-process hooks — so it feels powerful and flexible.

---

## 1. The core idea: the IR is the narrow waist

Decouple **N sources** from **M targets** through one stable IR. A source never knows about a target; a
target never knows about a source. Add a source → every target gets it for free. Add a target → it works
for every source.

```
 sources (frontends)          IR (the API model)            targets (generators)
 ─────────────────────        ──────────────────            ───────────────────────
 Go + Gin ─┐                                          ┌─▶ OpenAPI 3.1
 Go + chi ─┤                                          ├─▶ Go SDK
 TS + Express ─┼─▶  load → merge → │ Ir │ → transform ─┼─▶ TypeScript SDK
 Python + FastAPI ─┤             (frozen)              ├─▶ Python SDK
 an OpenAPI doc ───┘                                   ├─▶ GraphQL SDL / Postman / MCP tools / docs …
   (reverse direction)                                 └─▶ your custom generator
```

Every stage is a trait with a built-in implementation and a user-implementable interface. The user's
`.gnr8/src/main.rs` composes them. The "both directions" goal falls out for free: **an OpenAPI document
is just another source** that loads into the IR, so OpenAPI→SDK is `OpenApiSource → GoSdk`.

Today's `ApiGraph` (`crates/gnr8-core/src/graph/mod.rs`: `operations[]`, `schemas[]`, `diagnostics[]`,
`SourceSpan` provenance) is already this hub — it's router-agnostic by rule. This design hardens it into
the extensibility contract.

---

## 2. Evolving the IR so it can carry many sources → many targets

The IR must be (a) source-agnostic, (b) rich enough that no target needs to re-parse, (c) extensible so
a source can carry a fact the core doesn't model and a target can read it — without forking the core.
Three changes:

### 2a. A real type system (replace stringly-typed `SchemaType.kind`)
The single most important thing for multi-target. Today `SchemaType { kind: String, format, items, ref_id,
additional_properties }`. Replace with a closed, expressive enum every target maps into its own language:

```rust
pub enum Type {
    Primitive(Prim),                 // String | Bool | Int{bits,signed} | Float{bits} | Bytes
    WellKnown(WellKnown),            // Uuid | DateTime | Date | Duration | Decimal | Email | Uri …
    Optional(Box<Type>),             // nullable / not-required
    Array(Box<Type>),
    Map { key: Box<Type>, value: Box<Type> },
    Named(SchemaId),                 // $ref to a named schema
    Object(Vec<Field>),              // inline object
    Enum(Vec<EnumMember>),
    Union(Vec<Type>),                // oneOf / sum types (sources that have them; targets that can)
    Any,                             // free-form (map[string]any) — explicitly lossy
    Ext(ExtTypeId),                  // a type only an extension understands (see 2c)
}
```
A target owns a `TypeMap: &Type -> TargetType`. The IR stays neutral; Go maps `WellKnown::DateTime →
time.Time`, TS maps it `→ string`/`Date`, Python `→ datetime`. `Union`/`Any` are where capability gaps
surface (§6).

### 2b. The IR is an API model, not a router graph
Generalize the entity names so non-Gin / non-HTTP sources fit. Keep REST as the first shape, but model
it as: `Service { ops: Vec<Operation>, schemas, servers, security_schemes, info, ext }`, where an
`Operation` carries `transport: Transport` (`Http{method,path}` today; room for `Rpc`, `GraphQlField`,
`Event` later). Targets that only understand `Http` diagnose anything else (§6). This keeps the door open
for gRPC/GraphQL/event sources without reshaping for the REST case.

### 2c. Typed extension attributes — the flexibility primitive
Every node (`Service`, `Operation`, `Field`, `Schema`, `Type`, `Param`) carries an **extension bag**: a
typed, namespaced side-channel so a source attaches facts the core IR doesn't model and a target reads
them, with everyone else ignoring them.

```rust
pub struct Extensions(/* TypeId-keyed map of Arc<dyn Any> + a stable string id per ext */);
impl Extensions {
    pub fn set<E: Extension>(&mut self, e: E);
    pub fn get<E: Extension>(&self) -> Option<&E>;
}
// e.g. a gRPC source: op.ext.set(Streaming::ServerSide);
//      a target that supports it: if let Some(s) = op.ext.get::<Streaming>() { … }
//      every other target: ignores it. No core change, no fork.
```
This is the typed analogue of OpenAPI `x-` extensions, and it's what makes the system open-ended: custom
source ↔ custom target can collaborate on facts gnr8 never anticipated. Extensions serialize by stable
string id so they survive the host↔child JSON boundary (§5 of code-as-config.md).

### 2d. Provenance + diagnostics stay on everything
`SourceSpan` on every node (already there) powers diagnostics, ownership, and "why did this generate
this." Diagnostics accumulate across all stages and feed `doctor`.

---

## 3. Sources (frontends / parsers) — `trait Source`

```rust
pub trait Source {
    fn id(&self) -> &str;                                   // "go-gin", "openapi", "ts-express" …
    fn load(&self, cx: &SourceCx) -> Result<Service, Error>; // produce IR; diagnostics via cx.diag()
}
```

- **`SourceCx`** hands the source: the project root, its configured inputs, a `diag()` sink, a
  subprocess runner (for parser sidecars), and the facts cache. How a source gets its facts is its own
  business:
  - **pure Rust** (parse with our own code),
  - **a sidecar** (today's `goextract` Go helper is exactly this — a `Source` that shells out and
    deserializes JSON; every new language is "a parser sidecar + a thin Rust `Source`"),
  - **reading an artifact** (`OpenApiSource` parses an OpenAPI doc → IR; the reverse direction).
- **Built-ins:** `GoGin` (wraps goextract). **Near-term:** `OpenApiSource`, more Go routers (`GoChi`,
  `GoEcho`, `GoNetHttp`). **User-defined:** implement `Source` for anything — a house router, a proto
  set, a different language.
- **Composition / merge.** A pipeline can take **several sources**; the IR merges them:
  `Service::merge(a, b, policy)` with a conflict/namespacing policy (prefix schemas by source, error on
  collision, or last-wins). Enables: unify a Go service + a TS service into one SDK; or split a large API
  across source modules.
- **Diagnostics, not guesses.** Unsupported source patterns produce a `Diagnostic` with provenance — the
  source never silently drops or fabricates (a standing invariant).

How a user adds a source (sketch, in `.gnr8/src/main.rs` or a shared crate):
```rust
struct MyRouter { dirs: Vec<String> }
impl Source for MyRouter {
    fn id(&self) -> &str { "my-router" }
    fn load(&self, cx: &SourceCx) -> Result<Service, Error> {
        let facts = cx.run_sidecar("myrouter-extract", &self.dirs)?;  // or pure-Rust parse
        let mut svc = Service::new();
        // … translate facts → operations/schemas, attach ext attributes, push diagnostics …
        Ok(svc)
    }
}
// pipeline.source(MyRouter { dirs: vec!["./api".into()] })
```

---

## 4. Targets (generators / emitters) — `trait Target`

```rust
pub trait Target {
    fn id(&self) -> &str;                                       // "openapi-3.1", "go-sdk", "ts-sdk" …
    fn capabilities(&self) -> Capabilities;                     // which IR features it can represent (§6)
    fn generate(&self, ir: &Service, out: &mut Artifacts, cx: &TargetCx) -> Result<(), Error>;
}
```

- A target reads the **frozen** IR (+ extensions it understands), maps types via its `TypeMap`, and
  writes files into `Artifacts` (the in-memory `path → bytes + provenance` set the host later writes).
- **Built-ins:** `OpenApi31`, `GoSdk`. **User/community:** `TypeScriptSdk`, `PythonSdk`, `RustSdk`,
  `GraphqlSdl`, `PostmanCollection`, `McpTools`, `MarkdownDocs`, `JsonSchema` — each just a `Target`.
- **Sub-configuration via builder** (customize without forking):
  ```rust
  GoSdk::new()
      .module("example.com/acme/sdk")
      .client(ClientStyle::FunctionalOptions)   // vs builder, vs context-first
      .errors(ErrorStyle::Typed)
      .map_type(WellKnown::Decimal, "decimal.Decimal")  // surgical TypeMap override
      .naming(Naming::default().operations(|op| pascal(op.id)))
      .layout(Layout::OneFilePerTag)            // file layout strategy
      .header("// Code generated by gnr8. DO NOT EDIT.")
  ```
- **Many targets, one run.** `pipeline.target(OpenApi31::new()).target(GoSdk::new()).target(TypeScriptSdk::new())`
  → one source pass, three outputs, all from the same frozen IR.
- A target emits **its own diagnostics** for facts it can't represent (in addition to the automatic
  capability check in §6).

How a user adds a target (sketch):
```rust
struct PostmanCollection { out: String }
impl Target for PostmanCollection {
    fn id(&self) -> &str { "postman" }
    fn capabilities(&self) -> Capabilities { Capabilities::http_only() }
    fn generate(&self, ir: &Service, out: &mut Artifacts, _cx: &TargetCx) -> Result<(), Error> {
        let json = build_postman(ir);                 // your logic
        out.write(&self.out, json.into_bytes(), Provenance::whole(ir));
        Ok(())
    }
}
// pipeline.target(PostmanCollection { out: "postman.json".into() })
```

### Generation mechanics inside a target
Given the no-dependency ethos, generators emit code by **owned, deterministic string building** (the
`GoSdk` already does this). For richer targets we provide an in-repo, tiny, deterministic templating
helper (ordered substitution, no external engine) so target authors aren't hand-concatenating — but it's
optional; a target can format however it likes as long as output is deterministic.

---

## 5. Pre-process — `trait Transform` (IR → IR)

Runs after sources merge, before the IR is frozen for targets. **This is where most policy lives**, as
code, and it is the thing that replaces every TOML knob.

```rust
pub trait Transform { fn apply(&self, ir: &mut Service, cx: &Cx) -> Result<(), Error>; }
```

Built-in library (each a small composable `Transform`): `SetBasePath`, `SetTitle`, `SetVersion`,
`AddServer`, `ApplySecurity`, `RenameOperation`/`RenameType` (the old `naming.*`), `Retag`,
`FilterOperations` (drop `/internal/*`, drop deprecated), `RedactFields`, `FlattenEmbedded`,
`MergeSchemas`, `InjectPagination`, `NormalizeErrors`. Users add arbitrary ones:

```rust
struct DropInternal;
impl Transform for DropInternal {
    fn apply(&self, ir: &mut Service, _: &Cx) -> Result<(), Error> {
        ir.operations.retain(|op| !op.path().starts_with("/internal"));
        Ok(())
    }
}
// pipeline.transform(SetBasePath::new("/v2")).transform(DropInternal).transform(ApplySecurity::api_key(…))
```
Ordered and composable; transforms can read/attach extension attributes; they run once on the merged IR
so every target sees the same post-transform model. (Targets never mutate the IR — they get `&Service`.)

---

## 6. Capability negotiation — making lossiness explicit

A first-class part of "many sources × many targets": when the IR carries a feature a target can't
represent, say so with provenance — never silently drop.

- Each `Target::capabilities()` declares supported features (`Http`, `Union`, `AdditionalProperties`,
  `Streaming`, specific `WellKnown` formats, specific `Ext` ids…).
- Before/with generation, the pipeline walks the IR and, for each target, emits a `Diagnostic` for every
  used feature the target doesn't support (e.g. `Union` → a target without sum types; `Transport::Rpc` →
  an HTTP-only target; `Type::Any` → "free-form map becomes `additionalProperties: true`"). Provenance
  points at the source node.
- `doctor` aggregates these per target; `check` can fail on them if configured in code.

This is the generalization of today's OAPI-03 "report compatibility gaps as diagnostics," made N×M.

---

## 7. Post-process — `trait PostProcess` (artifacts → artifacts)

Runs after all targets, before the host writes. Operates on the in-memory `Artifacts` so the host's
ownership/no-op/edit-protection still apply to the final bytes.

```rust
pub trait PostProcess { fn run(&self, artifacts: &mut Artifacts, cx: &Cx) -> Result<(), Error>; }
```

Built-ins: `Header` (license / "generated by gnr8 — do not edit" banner), `Format` (run the **target
language's own toolchain** — `gofmt` for Go, etc.; that's the target's toolchain, not a gnr8 dependency),
`CompileCheck` (e.g. `go build` the generated SDK and fail on error — already proven in the SDK compile
test), `Relayout`/`Rename`, `Split`/`Merge`, `StripEmptyFiles`. Users add their own (inject a custom
license, rewrite imports, vendor a runtime file, post-validate).

Two hook points total: **pre** = `Transform` on the IR (semantic), **post** = `PostProcess` on artifacts
(textual/structural). Together they cover "adapt before generation" and "adapt after generation."

---

## 8. How it all composes — the pipeline & lifecycle

The user's `.gnr8/src/main.rs` builds one pipeline; the host runs it (the child process from
`code-as-config.md`), receives the `Artifacts` bundle, and owns writing.

```rust
fn main() -> ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            // sources (one or many → merged)
            .source(GoGin::new().inputs(["./core"]))
            // .source(OpenApiSource::file("legacy/openapi.yaml"))   // reverse direction / merge
            // pre-process (ordered)
            .transform(SetBasePath::new("/v2"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", Header("X-API-Key")))
            .transform(DropInternal)
            // targets (one or many from the frozen IR)
            .target(OpenApi31::new().to("openapi.yaml"))
            .target(GoSdk::new().module("example.com/acme/sdk").to("sdk/go"))
            .target(TypeScriptSdk::new().to("sdk/ts"))
            // post-process (ordered)
            .post(Header::generated())
            .post(Format::per_target())
            .post(CompileCheck::go()),
    )
}
```

Data flow & ordering (deterministic): `sources.load()` → `Service::merge` → `transforms` in order → IR
**frozen** → each `target.generate()` from the frozen IR (parallelizable; results sorted) → `Artifacts`
union → `post` in order → bundle to host → host applies the **manifest / no-op / edit-protection /
exclude-own-output** and writes. `watch` re-runs on source **or** `.gnr8/src` change; `doctor`/`inspect`
render diagnostics + the IR. Determinism is preserved end to end (sorted IR, sorted artifacts).

---

## 9. Sharing & ecosystem — why it feels powerful

Because every extension is an ordinary Rust type, they're **shareable as crates**. A user's `.gnr8`
`Cargo.toml` can pull `gnr8-target-typescript`, `gnr8-source-fastapi`, `gnr8-transform-pagination` and
compose them. gnr8 ships a curated set of built-ins; the community/agents grow the rest — the
"shadcn-but-extensible" model: great defaults, surgical overrides (TypeMap, Naming, Layout, PostProcess)
*without forking a generator*, and full custom power (your own `Source`/`Target`) when you need it. And
since it's all typed Rust, an **AI agent can author or extend a source/target with compiler feedback**,
which is the product thesis.

| You want to… | You write… | Without… |
|---|---|---|
| add a 2nd SDK language | `.target(TypeScriptSdk::new())` | touching the source or IR |
| support a new router | `impl Source for MyRouter` (+ maybe a sidecar) | touching any target |
| change a type mapping | `.map_type(WellKnown::Decimal, "…")` | forking the generator |
| drop internal routes | a 4-line `Transform` | a config DSL |
| add a license header / gofmt | a built-in `PostProcess` | post-build scripts |
| go OpenAPI → SDK | `OpenApiSource` as the source | a second tool |

---

## 10. Open questions / risks

- **IR versioning.** The IR is the contract between independently-authored sources and targets (and it
  crosses the host↔child JSON boundary). It needs a **stable, versioned schema** + a compatibility policy;
  bumping it is a breaking change for third-party sources/targets. Extensions (2c) absorb most additive
  needs without a bump.
- **Merge semantics** for multiple sources (schema-id collisions, namespacing, server/security union) —
  needs a clear, configurable policy, not magic.
- **Transport generality.** REST-first is right; modeling `Rpc`/`GraphQl`/`Event` via `Transport` + `Ext`
  keeps the door open, but don't build them until a real source needs them (honest-capabilities rule).
- **Capability declaration mechanics** — keep `Capabilities` coarse and honest; a target must not claim a
  feature it silently mangles.
- **Subprocess sources** — each non-Rust language is a parser sidecar (like goextract). That's a lot of
  surface; the `Source` trait keeps it uniform, but each sidecar is its own build/distribution problem.
- **Templating vs string-building** for target authors — provide a tiny owned helper, but keep it
  optional and deterministic; never pull a template engine (invariant).
- **Performance** with many targets — generate in parallel, but keep output sorted/deterministic.
- **`Any`/`Union` lossiness** — make the capability diagnostics genuinely useful, not noise.

---

## 11. Phased build (on top of `code-as-config.md`'s plan)

1. **Freeze the IR contract** — promote `SchemaType` → the `Type` enum (2a), generalize `ApiGraph` →
   `Service` (2b), add `Extensions` (2c), version it. Port `GoGin`/`OpenApi31`/`GoSdk` onto it.
2. **The four traits** — `Source`, `Transform`, `Target`, `PostProcess` + `Cx`/`Artifacts`/`Capabilities`,
   with today's behavior re-expressed as built-in impls. This is the SDK surface from `code-as-config.md`.
3. **Prove N×M with one new edge each** — add `OpenApiSource` (reverse direction) and `TypeScriptSdk`
   (second target) against the bookstore example; this validates the IR is actually neutral.
4. **Capability diagnostics (§6)** and the built-in `Transform`/`PostProcess` libraries.
5. **Custom-extension docs + example** — a user-authored `Source` and `Target` in the example's `.gnr8`,
   and the `gnr8 add-skill` payload teaching an agent to write them.

End state: the IR is a versioned, extensible hub; sources and targets are independent, shareable Rust
types; transforms and post-process give clean before/after hooks; and a user (or an agent) can add a
parser or a generator by implementing one trait — composing it all in the `.gnr8/` lifecycle, no TOML
anywhere.
