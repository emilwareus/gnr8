# Research: endpoint classification and breaking-change sensitivity

Date: 2026-09-03 · Branch base: `origin/main` @ `c380c2b` · Workspace version `0.10.2`
(`Cargo.toml:9`)

Question:

> `gnr8 changes --base <ref>` (issue #75) must fail CI on a breaking change. But not every reachable
> endpoint is a customer contract — some exist to seed test data or to debug. Where does the
> customer-facing/internal distinction live, what is its type, how do `gnr8 changes` and the PR
> report (issue #76) consume it, and what should the OpenAPI artifact say about it?

Everything under **Verified** was read in this checkout or measured on this machine. Everything under
**Recommendation** / **Open** is judgement, not measurement.

---

## 1. Verified: what exists today, and what does not

### 1.1 `gnr8 changes` does not exist

`crates/gnr8/src/cli.rs:49-96` enumerates the complete command set: `Init`, `Guide`, `Generate`,
`Watch`, `Check`, `Inspect`, `Doctor`. There is no `Changes` variant, no `--base` flag, and
`grep -rn 'breaking' docs/ thoughts/ARCHITECTURE.md README.md` returns only release-process prose
about *gnr8's own* semver (`docs/RELEASE.md:91`, `:143`, `:169`) and the worker protocol version
(`docs/extensibility.md:334`). Issue #75 is a specification, not a description.

Two consequences that shape everything below:

- **There is no git integration anywhere in the Rust workspace.** `grep -rn 'Command::new("git")'`
  over `crates/` returns nothing; the single hit for the string `git` is a cargo-manifest key check
  in `crates/gnr8-core/src/worker/build.rs:367`. Resolving `--base <ref>` to a second `ApiGraph` is
  net-new machinery, not a re-use of something present.
- **Classification is greenfield too**, so it can be designed to fit the diff rather than retrofitted.

### 1.2 The two graphs, and which one a diff should see

`ApiGraph` (`crates/gnr8-sdk/src/graph.rs:58`) is the source of truth. `pipeline::build_ir`
(`crates/gnr8-core/src/pipeline/mod.rs:226`) produces the post-transform graph; `pipeline::run`
(`:280`) then calls `graph::projection::into_generation` (`:295`) before any target runs.

That projection is not cosmetic. `crates/gnr8-core/src/graph/projection.rs:50` splits any schema
reached from both request and response positions whose contract differs, minting `::input` /
`::output` ids and `Input` / `Output` name suffixes (`projection.rs:21-24`) and rewriting every
`$ref` that pointed at it. A schema that gains one write-only field can therefore split into two
components — and a naive diff of pre-projection graphs would call that "one field added" while the
emitted OpenAPI and every generated SDK show a renamed type.

**The projected graph is the one a public-contract diff must compare.** It is what the OpenAPI
document and the SDKs are lowered from, and issue #75's classification list ("schemas … SDK group
names") is a list of artifact-visible facts.

### 1.3 The graph's existing per-operation metadata pattern

`ApiGraph` carries four side-tables keyed by operation id, each set by a transform and read by
targets:

| Field | Type | `graph.rs` |
|---|---|---|
| `operation_security` | `Vec<OperationSecurityPolicy>` | `:85`, type at `:280` |
| `operation_runtime` | `Vec<OperationRuntimePolicy>` | `:91`, type at `:327` |
| `pagination` | `Vec<PaginationPolicy>` | `:94`, type at `:340` |
| `operation_docs` | `Vec<OperationDocsPolicy>` | `:97`, type at `:393` |

Every one is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, sorted, and documented as
"facts the typed source cannot express … CLAUDE.md rule 4" (`graph.rs:48-56`). This is the
established shape for a new per-operation fact, and it is deliberately a *side-table*, not a field on
`Operation`: `Operation`'s doc comment (`graph.rs:484-492`) states that every structural field on it
is derived purely from source.

`OperationDocsPolicy` carries the sharpest precedent for what NOT to do
(`graph.rs:399-402`):

```rust
// NOTE: `summary`/`description` are deliberately NOT here. Prose lives on
// [`Operation`] itself, so an operation has exactly one place its words are written
// down and a second source is structurally impossible rather than merely discouraged
// (CLAUDE.md rule 3).
```

### 1.4 `OperationSelector` — the selector vocabulary already exists

`crates/gnr8-sdk/src/sdk/builtins.rs:1137`:

```rust
pub enum OperationSelector {
    OperationId(String),
    Route { method: String, path: String },
    PathPrefix(String),
    Methods(Vec<String>),
    Middleware(String),
    Any(Vec<OperationSelector>),
    All(Vec<OperationSelector>),
}
```

It is consumed by `ApplySecurity` (`:1129`, field `selectors: Vec<OperationSelector>`),
`DocumentOperation` (`:1554`), and the `ApiOverrides` parameter/security/response tables (`:488-491`).
The matcher is one function, `operation_selector_matches`
(`crates/gnr8-core/src/sdk/builtins.rs:1915`), and `PathPrefix` deliberately matches either the
group-relative path or the base-path-joined path (`:1923-1926`).

**Gap:** there is no source-file selector on `OperationSelector`. Provenance-based selection exists,
but only inside `GroupOperations`, as `GroupRule::SourcePrefix` (`builtins.rs:1780`), matching
`op.provenance.file.starts_with(prefix)` (core `builtins.rs:2504`). `Operation::provenance` is a
`SourceSpan` with a module-relative `file` (`graph.rs:540`, `:804`).

### 1.5 Two incompatible precedents for "several rules matched"

The repo has **both** resolution styles already, and they are used for different jobs:

- **First-match-wins, silent.** `GroupOperations` (`crates/gnr8-core/src/sdk/builtins.rs:2485-2528`)
  iterates rules in configuration order per operation and `break`s on the first match. Its SDK-side
  doc says so plainly: "Rules run in the order they are configured; the first match for an operation
  wins" (`crates/gnr8-sdk/src/sdk/builtins.rs:1771`).
- **Exactly-one-match-or-hard-error.** `find_selected_operation_index`
  (`crates/gnr8-core/src/sdk/builtins.rs:1941`) returns `CoreError::Config` for zero matches
  (`"… did not match any operation"`) and for many (`"… must match exactly one operation but matched
  N"`).

`DocumentOperation` is stricter still: it pre-scans every matched operation for a prose collision
*before mutating any of them* — "so a transform that is going to fail leaves the graph exactly as it
found it. A half-applied transform would make the error depend on operation order"
(`crates/gnr8-core/src/sdk/builtins.rs:2171-2174`) — and errors if the operation already has prose
from source, with the message "would be a second source for one fact" (`:2296-2303`). It also errors
when the selector matches nothing (`:2207`).

That pre-scan-then-mutate discipline is the pattern any new classifying transform must copy.

### 1.6 Transform ordering is composition order, and nothing re-sorts it

`crates/gnr8-core/src/pipeline/mod.rs:9`: "Stage order is composition order. A pipeline with no
custom stages never sends a work frame." `build_ir` walks `plan.transforms` in order (`:265-270`),
grouping consecutive custom stages into one round-trip but never reordering. So a classification
transform's position is user-visible and load-bearing — exactly like `DiagnosticPolicy`, whose doc
says "Place this transform after explicit correction transforms … and before targets"
(`crates/gnr8-sdk/src/sdk/builtins.rs:333-335`).

### 1.7 The status quo for internal endpoints: delete them

Today the only way to keep an internal route out of the public surface is to remove it from the
graph. `examples/taskflow/.gnr8/src/main.rs:48-54`:

```rust
struct DropDebugRoutes;
impl Transform for DropDebugRoutes {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), Error> {
        ir.operations.retain(|op| !op.path.contains("_debug"));
        Ok(())
    }
}
```

The repo already knows this is the only option and says so in `RequireOperationDocs`'s doc comment
(`crates/gnr8-sdk/src/sdk/builtins.rs:365-368`):

> This is OPT-IN and a PIPELINE STAGE rather than a check inside a `Source`, because only the user's
> own pipeline knows when their public-surface filtering has finished: **gnr8 has no built-in
> operation-exclusion transform**, so an internal route a consumer strips later must not fail the
> gate before it is stripped.

The route is real: `examples/taskflow/main.go:36` registers `tasks.GET("/_debug", debugTasks)`
inside `r.Group("/tasks")`, and its own comment calls it "a real internal endpoint"
(`main.go:22-24`). And the deletion is total — `grep -rc 'debugTasks\|_debug'
examples/taskflow/generated/` matches **zero** lines across `openapi.yaml`, `API.md`, and all of
`generated/sdk/`.

Deletion is exactly wrong for Emil's case. An endpoint that seeds test data needs to be **in the
generated SDK** — that is what the test harness calls — while being **out of the merge-blocking
contract**. A `retain` that drops it makes the SDK unable to reach it; keeping it makes it block
merges. Those are the only two states available today, and neither is the wanted one.

### 1.8 Gates and vocabulary constraints on any design

- **Determinism.** `graph.rs:9-14`: every collection is a sorted `Vec`, "the graph never serializes an
  unordered hash map". `make examples-check` regenerates all five examples and `diff -ru`s them.
  A classification that depends on rule iteration order over a hash map would break this.
- **`make invariants`** (`scripts/check-invariants.sh`) is a hard gate over `crates`, `docs`,
  `examples`, `fixtures`, `scripts`, `.github`, `action.yml`, `Makefile`, `Cargo.toml`, `Cargo.lock`,
  `README.md`, `PLAN.md`, `llms*.txt`. `thoughts/` and `.planning/` are explicitly out of scope
  (`check-invariants.sh:7-8`).
- **Specific naming landmine:** rule at `check-invariants.sh:108` forbids CLI flags matching
  `--(compat|legacy|migration|baseline)\b` and declarations matching
  `(fn|mod|struct|enum|trait|const) [a-zA-Z_]*(compat|legacy|brownfield|migration|baseline)`.
  Issue #75's spelling — `gnr8 changes --base origin/main` — **passes**. A `--baseline` flag, a
  `BaselineGraph` struct, or a `mod compat` would fail `make check` and CI. This is worth stating
  loudly because "baseline" is the word every other diff tool in §4 uses.
- **`unsafe_code = "forbid"`, `unwrap_used`/`expect_used`/`panic` denied** in production
  (`Cargo.toml:27`, `:33-37`).
- **`--json` is a global flag** (`crates/gnr8/src/cli.rs:22`), and the established CI-gate exit
  convention is `std::process::exit(1)` after printing (`crates/gnr8/src/main.rs:584` for `check`,
  `:1487` for `doctor`), with the comment "Deliberate non-zero exit so `gnr8 check` is a usable CI
  gate".

### 1.9 What gnr8 can emit into OpenAPI today

Vendor extensions exist in exactly one place: **object schema fields**.
`OpenApiFieldPatch::extension_string` / `_number` / `_bool` / `_null`
(`crates/gnr8-sdk/src/sdk/builtins.rs:1992-2031`) push `Extension { name, value }`
(`crates/gnr8-sdk/src/facts.rs:303`) onto `SchemaObject::extensions`
(`crates/gnr8-core/src/lower/model.rs:271-272`).

The lowered `Operation` (`crates/gnr8-core/src/lower/model.rs:115-137`) has `operation_id`,
`summary`, `description`, `deprecated`, `tags`, `security`, `parameters`, `request_body`,
`responses` — **and no extensions field**. So emitting an operation-level `x-…` today requires new
plumbing in the lowering model, not just a transform.

On the input side, `OpenApi` source *does* preserve `x-` keys, but only inside parameter fragments:
`crates/gnr8-core/src/sdk/openapi_source.rs:1023-1027` copies every `name.starts_with("x-")` entry
into the parameter's preserved fields, which the graph carries verbatim as
`Param::openapi_fields` (`graph.rs:580`). No `x-` key anywhere in the repo is *branched on*.

---

## 2. Verified: the three candidate homes, tested against this checkout

### 2.1 (a) "In-source" — the tension dissolves once you look at what the graph already carries

CLAUDE.md 0.1 closes the obvious door: "There is **no directive syntax, no marker prefix, and no
key/value grammar inside the comment** — not `@Summary`, not `gnr8:summary`, not anything." 0.5 closes
the subtle one: "If a change ever introduces matching, tokenizing, or branching on *content* inside a
comment, that is a dialect and it must be rejected in review." So a `// internal` comment, an
`x-internal`-in-a-docstring, and a magic first word are all out — not on a technicality, but because
each would make gnr8's behaviour depend on a grammar someone has to learn and maintain.

But "in-source" was never really about comments. The facts that make an endpoint internal in a real Go
or Nest codebase are *structural*, and the graph already carries three of them, each derived purely
from the source language's own routing constructs:

| Graph fact | `graph.rs` | What it is in source |
|---|---|---|
| `Operation::group` | `:517` | "Optional static route-group/tag metadata" — a Gin `r.Group("/internal")`, a FastAPI `APIRouter(prefix=…)`, a Nest `@Controller(…)` |
| `Operation::middleware` | `:520` | "Source middleware symbols applied before the handler" — e.g. an internal-auth guard |
| `Operation::provenance.file` | `:540`, `:804` | module-relative source file of the route registration |

Router prefixes and controller prefixes are *statically composed into the path* as of P0.4
(`PLAN.md`, "Preserve static router/controller prefixes") — that item's validation note records that
`pyextract/routes.py` and `tsextract/routes.js` used to discard them, and that this was fixed. So the
group structure a developer sees in the code is already faithfully in the graph.

**This is the resolution of the brief's tension.** Source-structure-based classification does not need
a new reading mechanism, because gnr8 already reads route groups, middleware, and file provenance as
first-class typed facts. What is missing is not a way to *read* structure, it is a way to *say what
structure means* — and saying what a fact means is, by rule 4, the config's job. "Close to the code"
and "in the pipeline" are not competing homes; the evidence lives in the code and the interpretation
lives in the pipeline.

The refactor-survival argument follows from the same table. A rule keyed on `group` or `middleware`
survives file moves and renames, because moving a handler out of `r.Group("/internal")` is exactly
the act of making it public — the rule tracks the thing a reviewer would look at anyway. A rule keyed
on `provenance.file` does not survive a file move, and that is a real, if acceptable, weakness
(`GroupRule::SourcePrefix` has it today).

### 2.2 (b) Pipeline config — fits, with one honest cost

Rule 4 is direct: "What the source can't express comes from user code-as-config." Which endpoints are
customer contracts is a *product* fact — the same category as security schemes. Two teams can ship
byte-identical Go and disagree about which routes they support.

It also lands on the existing machinery: a `Transform` writing a sorted side-table keyed by operation
id (§1.3), selected by the existing `OperationSelector` (§1.4), ordered by composition order (§1.6).
Nothing new is invented.

The honest cost is the one CLAUDE.md itself names in rule 4: "A rule that forces a `.gnr8/` edit every
time someone adds an endpoint is a bad rule … it lets new routes ship undocumented, and it scales as a
central table nobody maintains."

A per-operation-id table would be exactly that bad rule. A rule keyed on *structure* — `PathPrefix`,
`Middleware`, group, source prefix — is not: adding a route under `/internal` classifies it with no
`.gnr8/` edit at all. Rule 4's own next sentence draws that line: "Reach for config when a fact spans
operations or lives outside the handler entirely." A classification policy spans operations by
construction.

### 2.3 (c) Purely derived on the graph — cannot be the whole answer, and must not be the default

For gnr8 to derive classification with no user input, it would have to *guess* — path prefixes like
`/internal` or `/_debug`, or a handler-name convention. Three problems, in increasing severity:

1. It is gnr8 inventing a convention users must then comply with. That is rule 0 pointed inward.
2. It cannot express the exception ("this one endpoint under `/v1` is internal"), so it would need a
   config override alongside it — and "derive it, unless config overrides" is the dual-source pattern
   rule 3 forbids by name.
3. It is a *silent* semantic. A team that names a customer route `/internal-tools/export` would have
   gnr8 quietly stop blocking merges on it. The failure mode of a wrong guess here is a shipped
   breaking change, not a cosmetic defect.

What *is* legitimately derived is the resolution: given the user's declared rules and the graph, the
classification of each operation is a pure, deterministic function of the two. That is derivation in
the same sense `graph::direction` derives which side a schema is reached from — computed, not
guessed.

### 2.4 The trap that makes or breaks the feature: reclassification is itself a change

Whatever the home, one property is non-negotiable, and it is easy to miss.

If `gnr8 changes` reads only the **current** classification, then the cheapest way to make a breaking
change pass CI is to add one line to `.gnr8/src/main.rs` marking the broken endpoint internal. The
gate would then be advisory in the precise situation it exists for.

So the diff must read classification from **both** graphs and treat the transition as a first-class
change:

- customer-facing → internal is a **removal from the public contract**. Every consumer SDK loses a
  supported operation. It is BREAKING, and it is breaking *for the same reason deleting the route is*.
- internal → customer-facing is new public surface: ADDITIVE.

This is the only mechanism that keeps an override path honest, and it costs nothing extra — the base
graph is already being materialised to diff against.

---

## 3. Verified: what `--base <ref>` costs, and why it constrains the classification design

Issue #75 says "a graph from a Git revision". Getting one is harder than it reads, and the
constraints land directly on where classification may live.

**The host cannot run an old `.gnr8` worker.** `crates/gnr8-core/src/worker/mod.rs:298-303` compares
the worker's capability digest to its own by strict equality and errors with "gnr8 capability
mismatch: host {}, worker {}. Rebuild both …". `capability_digest`
(`crates/gnr8-sdk/src/protocol/mod.rs:74-79`) hashes
`"gnr8-sdk:{sdk_version};protocol:{PROTOCOL_VERSION};frames:2;plan:1;artifacts:1;patched:1"`, and
`sdk_version()` is `env!("CARGO_PKG_VERSION")` (`:82-85`). A base ref whose `.gnr8/Cargo.toml` pins a
different `gnr8` version produces a worker that **cannot handshake with the installed host at all**.
`validate_workspace` additionally rejects a pin below `FIRST_SDK_VERSION` outright
(`crates/gnr8-core/src/worker/build.rs:292-307`).

So "check out the base ref and run its pipeline" is not generally available. Three viable shapes,
each with a different meaning:

| Shape | Base graph = | Cost | Meaning |
|---|---|---|---|
| Re-run **base config + base source** | what the API actually was | needs a matching host per ref; blocked by the digest above whenever the pin moved | the true published contract |
| Re-run **current config + base source** | what today's pipeline says about yesterday's code | one worker build, current host | isolates *source* changes; a config change is invisible |
| Read a **committed graph artifact** at the base ref (`git show <ref>:<path>`) | whatever was committed | no build at all, and it is already deterministic and sorted | the true published contract, if the artifact was committed |

The third is the only one that is both cheap and correct, and gnr8 is unusually well set up for it:
the graph is already fully `Serialize`/`Deserialize` (`graph.rs:57`), already deterministic and sorted
by construction (`graph.rs:9-14`), and `gnr8 inspect graph --json` already serialises it straight from
those impls (`crates/gnr8/src/render.rs:9-10`). Committing the projected graph as a generated artifact
would make `--base <ref>` a `git show` plus two `serde_json::from_str` calls.

It also has a hard cost: it only works if the artifact was committed at the base ref, and "committed,
otherwise re-run the pipeline" is precisely the dual-path shape rule 3 forbids. Whichever shape is
chosen has to be the *only* shape. This is left as an Open decision (§8.1) because it is a `changes`
design question, not a classification one — but it bears on classification in one specific way:

**If the base graph comes from a committed artifact, the classification must be IN the graph**, not
recomputed from the current `.gnr8` rules against old operations. Recomputing would apply today's
policy to yesterday's API, which silently defeats §2.4: relabel an endpoint internal today and the
recomputation says it was always internal. A classification stored on the graph, serialised with it,
and read back is the only form that makes the transition detectable.

That single requirement rules option (b)-as-pure-config out as the *storage* location and settles the
shape: rules in config, resolved value on the graph.

---

## 4. Verified: issue #75's change categories, mapped to graph facts

Every category in the issue has a home in the graph. This matters for classification because it
determines what "this change belongs to an internal endpoint" can even mean.

| Issue #75 category | Graph fact | `graph.rs` |
|---|---|---|
| operations | `ApiGraph::operations`, sorted by `(path, method)` | `:62` |
| paths | `Operation::path` + `ApiGraph::base_path` | `:502`, `:69` |
| methods | `Operation::method` | `:497` |
| parameters | `Operation::params` → `Param::{name, location, required, schema, default, style, explode}` | `:522`, `:547-567` |
| request bodies | `request_body`, `request_body_required`, `request_body_content_type` | `:524-530` |
| responses | `Operation::responses` → `Response::{status, body, body_kind, content_type, content_types}` | `:532`, `:587-604` |
| schemas | `ApiGraph::schemas` → `Schema::{id, name, body, enum_source_order}` | `:64`, `:661-671` |
| required fields | derived per direction: `SchemaDirections::field_is_required` | `graph/direction.rs:48-56` |
| nullability | derived per direction: `SchemaDirections::field_is_nullable` | `graph/direction.rs:65-72` |
| enums | `Type::Enum(Vec<String>)` + `Schema::enum_source_order` | `facts.rs:341`, `graph.rs:671` |
| security | `security`, `security_requirements`, `operation_security`, `Operation::security` | `:79-85`, `:535` |
| operation names | `Operation::id`, plus `OperationDocsPolicy::openapi_operation_id` | `:495`, `:398` |
| SDK group names | `Operation::group` | `:517` |

Two of these deserve emphasis.

**"Required" and "nullable" are not stored fields — they are direction-dependent derivations.**
`field_is_required` is a three-arm match over `(request, response)`
(`crates/gnr8-core/src/graph/direction.rs:48-56`), built from four independent extraction facts on
`FieldFact` (`deserializer_accepts_absent`, `validator_requires_presence`, `serializer_may_omit`,
and the null trio). A diff must therefore compare the *derived* value in the *same position*, not the
raw field, or it will report phantom changes on a schema whose direction set changed.

**`Operation::group` is not cosmetic.** `SdkFileLayout::split()` sets
`OperationFileSplit::PerTag` (`crates/gnr8-sdk/src/sdk/layout.rs:34-43`), and per-tag emission puts
operations in a file named for the group, falling back to the synthetic `"default"` group
(`layout.rs:73-77`, `:114`, `:192`). Renaming a group renames generated SDK files and, in Go and
TypeScript, the symbols users import. That is why the issue lists it as a breaking-change category
alongside HTTP facts.

**And the SDK is why `Operation::id` matters at all.** For a pure OpenAPI producer, renaming
`operationId` is cosmetic. gnr8 generates SDK method names from the graph id, so an id rename is a
compile error in every consumer — as breaking as deleting the route.

---

## 5. Prior art

### 5.1 Marking an operation internal: four incompatible models, no standard

**There is no spec-native concept.** OpenAPI 3.1.1's Operation Object has `tags`, described as "A list
of tags for API documentation control … logical grouping of operations by resources or any other
qualifier", and the Tag Object has exactly three fields — `name`, `description`, `externalDocs`
([spec.openapis.org/oas/v3.1.1.html](https://spec.openapis.org/oas/v3.1.1.html)). No audience,
visibility, or access field exists anywhere in the document model.

`deprecated` is a *lifecycle* signal, not an *audience* one, and it is advisory: on the Operation
Object, "Declares this operation to be deprecated. Consumers SHOULD refrain from usage of the declared
operation. Default value is `false`." The Schema Object adds no `deprecated` of its own — it inherits
JSON Schema 2020-12's, whose wording is likewise "applications SHOULD refrain from usage"
([json-schema.org §9.3](https://json-schema.org/draft/2020-12/json-schema-validation.html)). Nothing
is removed, and nothing is guaranteed.

Specification Extensions carry an explicit non-promise (3.1.1 §4.9): "The field name MUST begin with
`x-`, for example, `x-internal-id`. Field names beginning `x-oai-` and `x-oas-` are reserved for uses
defined by the OpenAPI Initiative", and — the load-bearing sentence — **"Support for any one extension
is OPTIONAL, and support for one extension does not imply support for others."**

**`x-internal` is not registered.** The OAI extension registry
([spec.openapis.org/registry/extension](https://spec.openapis.org/registry/extension/)) lists
`x-codeSamples`, `x-data-classification`, `x-sensitive-data`, the `x-jsonschema-*` and `x-oai-*`
families, and a handful more. `x-internal`, `x-hidden`, `x-private`, and `x-visibility` are all
absent.

What the spec *does* bless is omission. §4.10 "Security Filtering": "Some objects in the OpenAPI
Specification MAY be declared and remain empty, or be completely removed … The reasoning is to allow
an additional layer of access control over the documentation", and it states this behaviour is "not
part of the specification itself."

The ecosystem then splits four ways:

| Model | Who | Mechanism | What ships to the client |
|---|---|---|---|
| **Omit at generation** | FastAPI, NestJS, ASP.NET Core | `include_in_schema=False`; `@ApiExcludeEndpoint()`; `[ApiExplorerSettings(IgnoreApi = true)]` / `ExcludeFromDescription()` | nothing — the operation is absent, no key emitted |
| **Mark, delete at build** | Redocly CLI, openapi-filter | `x-internal: true` + a bundling decorator | nothing, *after* the extra build step |
| **Mark, filter at render** | Stoplight Elements, Mintlify | `x-internal` + `hideInternal` prop; `x-hidden` / `x-excluded` | everything — hiding is the consumer's opt-in |
| **Platform ACL** | Gravitee (root-level `visibility` inside `x-graviteeio-definition`), Azure APIM products/groups | not an operation property at all | n/a |

Details worth carrying:

- **FastAPI** ([docs](https://fastapi.tiangolo.com/advanced/path-operation-advanced-configuration/)):
  "To exclude a *path operation* from the generated OpenAPI schema (and thus, from the automatic
  documentation systems), use the parameter `include_in_schema` and set it to `False`." The route
  stays live and routable; only the document omits it. Router and route compose with `and`, so a
  `False` upstream cannot be re-enabled by a child.
- **NestJS** ([docs.nestjs.com/openapi/decorators](https://docs.nestjs.com/openapi/decorators)):
  `@ApiExcludeController()` on a controller, `@ApiExcludeEndpoint()` on a method. Same shape — route
  live, document silent.
- **ASP.NET Core**: `[ApiExplorerSettings(IgnoreApi = true)]` suppresses the `ApiDescription` itself
  ("If `true` then no `ApiDescription` objects will be created for the associated controller or
  action" —
  [learn.microsoft.com](https://learn.microsoft.com/en-us/dotnet/api/microsoft.aspnetcore.mvc.apiexplorersettingsattribute.ignoreapi)),
  so *every* .NET OpenAPI generator loses sight of it at once. Minimal APIs use
  [`ExcludeFromDescription()`](https://learn.microsoft.com/en-us/aspnet/core/fundamentals/openapi/include-metadata).
- **Redocly** ([remove-x-internal decorator](https://redocly.com/docs/cli/decorators/remove-x-internal)):
  "Removes nodes that have a specific flag property. Nodes that don't have the flag property defined
  are not impacted." The property name is *configurable* — `internalFlagProperty`, default
  `x-internal`. Their own guide adds the caveat: "**Security through obscurity** — … Removing APIs
  from documentation is not a security mechanism."
- **Redoc, the open-source renderer, does not implement `x-internal` at all** — the flag only takes
  effect if the document is first run through Redocly CLI's bundler.
- **Stoplight Elements** ([elements-options](https://github.com/stoplightio/elements/blob/main/docs/getting-started/elements/elements-options.md)):
  "`hideInternal` - Pass `\"true\"` to filter out any content which has been marked as internal with
  `x-internal`." It defaults to **off**, deliberately —
  [issue #1266](https://github.com/stoplightio/elements/issues/1266) records the reasoning: "Elements
  OSS will not know anything about a user being logged in or not, so it cannot know if it should
  display internal or not, so we can leave that up to the caller to decide."
- **The spelling is not even stable across the "mark" camp.** Redocly `x-internal` (configurable),
  [openapi-filter](https://github.com/Mermade/openapi-filter) `--flags x-internal` (its own README
  demos `x-private x-hidden`), Mintlify `x-hidden` *and* `x-excluded` with different meanings, and
  [LoopBack 4](https://github.com/loopbackio/loopback-next) `x-visibility: documented|undocumented`.
  LoopBack's [PR #1896](https://github.com/loopbackio/loopback-next/pull/1896) explicitly rejected a
  boolean `x-internal` in review in favour of the open-valued enum.

**The finding that matters for gnr8.** "Internal" is at least two requirements wearing one name — the
spec's own §4.10 distinguishes an empty Path Item ("the viewer will be aware that the path exists")
from removing the path outright, and Mintlify needed two keys for exactly that. And every "mark"
mechanism has the same default failure mode: an unmodified renderer shows the operation. There is
nothing here to *comply* with, because there is no single thing to comply with — which, conveniently,
is also what makes borrowing safe.

---

### 5.2 Governance standards: audience and stability are two different dials

**A correction to the brief's premise first: AIP-183 does not exist.** `https://google.aip.dev/183`
returns 404, as does the raw `aip/general/0183.md`, and the AIP repository's general directory goes
`0180.md, 0181.md, 0182.md, 0184.md, 0185.md`
([aip-dev/google.aip.dev](https://github.com/aip-dev/google.aip.dev/tree/master/aip/general)). The
relevant AIPs are 180, 181, and 185.

**[AIP-180 "Backwards compatibility"](https://google.aip.dev/180)** states the obligation — "Existing
client code **must not** be broken by a service updating to a new minor or patch release" — across
three axes it names *source*, *wire*, and *semantic* compatibility. It contains no occurrence of
"internal", "alpha", "beta", or "visibility". What it does contain is a scope note that is, almost
word for word, Emil's problem:

> This guidance assumes that APIs are intended to be called from a range of consumers … Any API which
> has a more limited scope (for example, an API which is only called by client code written by the
> same team as the API producer, or deployed in a way which can enforce updates) **should carefully
> consider its own compatibility requirements.**

**[AIP-181 "Stability levels"](https://google.aip.dev/181)** carries the per-level guarantees: alpha
"undergoes rapid iteration with a known set of users who **must** be tolerant of change … Breaking
changes **must** be both allowed and expected"; beta "**may** include backwards-incompatible changes …
made only after a reasonable deprecation period"; stable "**must** be fully-supported over the
lifetime of the major API version … there **must** be no breaking changes."

**[AIP-185](https://google.aip.dev/185) §"Visibility-based versioning"** is where Google's actual
visibility mechanism lives, and its framing is the sharpest sentence in this whole survey:

> A visibility label is a case-sensitive string that can be used to tag any API element … An implicit
> `PUBLIC` label is applied to all API elements unless an explicit visibility label is applied … Each
> visibility label is an allow-list … **In other words, an API visibility label is like an ACL'ed API
> version.**

The wire form is
[`google/api/visibility.proto`](https://github.com/googleapis/googleapis/blob/master/google/api/visibility.proto),
six extensions sharing field number `72295727` (`api_visibility`, `method_visibility`,
`message_visibility`, `field_visibility`, `enum_visibility`, `value_visibility`), carrying
`VisibilityRule { selector, restriction }`. Two of its own doc comments matter here:

> If an element and all its parents have no visibility label, its visibility is unconditionally
> granted.

> If a rule has multiple labels, removing one of the labels but not all of them **can break clients.**

That second line is Google independently arriving at §2.4: narrowing a visibility label is a breaking
change.

**[Zalando's rule #219](https://opensource.zalando.com/restful-api-guidelines/#219)** is the closest
thing to a standard `audience` field, and its shape is instructive precisely because it is *not* what
gnr8 needs. `MUST provide API audience`, at `/info/x-audience`, `x-extensible-enum` over
`component-internal`, `business-unit-internal`, `company-internal`, `external-partner`,
`external-public` — five levels with "clear organisational and legal boundaries". But:

> **Note:** Exactly *one audience* per API specification is allowed … **If parts of your API have a
> different target audience, we recommend to split API specifications along the target audience** —
> even if this creates redundancies.

So Zalando's audience is **document-level and governance-facing**: it drives review process,
publication obligation, naming rules, hostname conventions, and partner deprecation consent. It is
explicitly *not* a compatibility dial — the word `audience` never appears in Zalando's
`compatibility.adoc`. Its compatibility rule
[#106](https://opensource.zalando.com/restful-api-guidelines/#106) is flat across audiences, and
carries one hint gnr8 should read carefully:

> Please note that the compatibility guarantees are for the 'on the wire' format. **Binary or source
> compatibility of code generated from an API specification is not covered by these rules.**

gnr8 *is* the code generator, so gnr8's diff must cover exactly what Zalando disclaims: operation-id
renames, group renames, model-field optionality. That is a real difference in remit, not a detail.

**[Microsoft's REST API Guidelines](https://github.com/microsoft/api-guidelines/blob/vNext/graph/Guidelines-deprecated.md)**
take the opposite position and refuse to tier at all:

> These guidelines are applicable to any REST API exposed publicly … **Private or internal APIs SHOULD
> also try to follow these guidelines because internal services tend to eventually be exposed
> publicly.**

They also delegate the definition of breaking: "**Teams MAY define backwards compatibility as their
business needs require** … Services MUST explicitly define their definition of a breaking change."
(The top-level `Guidelines.md` on `vNext` is now a deprecation stub pointing at the Azure and Graph
documents; [`azure/Guidelines.md`](https://github.com/microsoft/api-guidelines/blob/vNext/azure/Guidelines.md)
is absolute — "**DO NOT** introduce any breaking changes into the service" — with preview status
encoded in the `api-version` string, not in a label.)

**[Kubernetes](https://kubernetes.io/docs/reference/using-api/#api-versioning)** ties the guarantee to
the version identifier: alpha "may be dropped at any time without notice … may change in incompatible
ways in a later software release without notice"; beta "maximum lifetime of 9 months or 3 minor
releases … schema and/or semantics may change in incompatible ways in a subsequent beta or stable API
version"; stable "remain available for all future releases within a Kubernetes major version". Its
[deprecation policy](https://kubernetes.io/docs/reference/using-api/deprecation-policy/) Rule #1 is
track-*independent*: "Once an API element has been added to an API group at a particular version, it
can not be removed from that version or have its behavior significantly changed, **regardless of
track**." Alpha is not mutable — an alpha *version* is immutable too; you are merely allowed to delete
the whole version quickly.
[`api_changes.md`](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api_changes.md#alpha-beta-and-stable-versions)
does use the word **Audience** as one attribute of each maturity stage — "developers and expert users
interested in giving early feedback" (alpha) up to "**all users**" (stable).

**The finding that matters most for gnr8:** across all four, **nobody makes breaking-change policy a
function of an audience label.** The differentiator is always a *stability level*, and in both Google
and Kubernetes that level is carried by the version identifier itself (`v1alpha1`,
`2021-06-04-preview`). Google's visibility labels are the single exception, and AIP-185 frames them as
versioning by other means — an "ACL'ed API version" whose labels are removed entirely at GA.

So among the *governance standards*, the differentiator is never an audience label. The one place a
per-operation label really does gate breaking-change detection is a diff tool rather than a standard —
oasdiff's `x-stability-level`, covered in §5.4 — and it is the single closest prior art to issue #75's
ask. Emil's version differs from it in one way that matters: oasdiff's axis is *stability* (how
finished is this?), Emil's is *audience* (who is promised this?). They are genuinely different
questions — an endpoint can be rock-stable and still not a customer contract — and conflating them is
the mistake §6.2 avoids by keeping `Audience` a separate, closed vocabulary from `deprecated`.

### 5.3 SDK generators: exclusion is a boolean everywhere but Fern

| Tool | Mechanism | Shape | Effect |
|---|---|---|---|
| [Speakeasy](https://www.speakeasy.com/docs/speakeasy-reference/extensions) | `x-speakeasy-ignore` (operation *and* parameter) | boolean | "Exclude certain methods from your SDK with this extension." |
| [Fern](https://buildwithfern.com/learn/api-definitions/openapi/extensions/audiences) | `x-fern-audiences` (server, operation, schema, property) + `x-fern-ignore` | **list of user-defined labels** | selected per generator group via `audiences:` in `generators.yml` / `docs.yml` |
| [Stainless](https://www.stainless.com/docs/reference/config/) | `unspecified_endpoints: ["post /internal_endpoint", …]`, `skip`, `only` | list / boolean | skip list in config, no visibility concept |
| [OpenAPI Generator](https://openapi-generator.tech/docs/customization/) | `x-internal: true` | boolean | **skips generation, on by default, no flag needed** |

Three of these bear directly on decisions below.

**OpenAPI Generator honours `x-internal` unconditionally.** `DefaultGenerator.java:1606` skips any
operation whose extensions carry `x-internal: true`, logging "Operation ({} {} - {}) not generated
since x-internal is set to true"; `:501` does the same for schemas. The `REMOVE_X_INTERNAL`
normalizer rule *strips* the extension so the element is generated again — it is an opt-out of the
hiding. Its `--openapi-normalizer FILTER=…` works by *writing* `x-internal: true` onto every operation
that does not match. So if gnr8 emitted `x-internal` into its OpenAPI document, any downstream
consumer running OpenAPI Generator would **silently lose those operations from their SDK**, with no
flag involved.

**Fern's audience model is element-level, list-valued, and fail-open**, and its own docs concede the
consequence:

> When you specify audiences, elements tagged with a matching audience are included, and elements
> tagged only with other audiences are dropped. **Untagged elements aren't scoped to any audience, so
> they're always included.**

> **To keep internal endpoints out of partner-facing output entirely, split them into a separate API
> definition or exclude their paths with `settings.filter` rather than relying on tags alone.**

Google reaches the same default from the other direction ("If an element and all its parents have no
visibility label, its visibility is unconditionally granted"). **A fail-closed visibility default is
unattested in every source surveyed.** That is worth knowing because gnr8's safe default points the
same way: unclassified ⇒ treated as a customer contract ⇒ visible *and* gating. The two polarities
agree.

**Speakeasy is the cautionary tale.**
[`x-speakeasy-extension-rewrite`](https://www.speakeasy.com/docs/speakeasy-reference/extensions) is,
verbatim, a compliance surface: "You can use `x-speakeasy-extension-rewrite` to map any extension from
the wider OpenAPI ecosystem or another vendor to the equivalent Speakeasy extension. This allows you
to use your existing OpenAPI spec without needing to make changes to it." Their own documented example
maps `x-enum-varnames` — an OpenAPI Generator convention — onto a Speakeasy key. That is precisely the
feature CLAUDE.md rule 0.1 forbids, in a shipped commercial product, so it is a useful concrete image
of what the rule is protecting against rather than an abstraction.

Also worth recording, because it corrects a common assumption: **Speakeasy does not derive SDK versions
from a semantic spec diff.**
[Their versioning docs](https://www.speakeasy.com/docs/sdks/manage/versioning) state "Speakeasy does
not currently analyze the actual content of the OpenAPI document (such as added or removed
operations). Only the `info.version` field and the overall document checksum are evaluated." Their
[breaking-change tooling](https://www.speakeasy.com/docs/sdks/manage/breaking-changes) is separate and
advisory. `gnr8 changes` would not be catching up to them; it would be doing something they don't.

---

### 5.4 Diff tooling: severity vocabularies, exit codes, and — critically — how they exempt things

Six tools, read from source rather than docs where the two disagree.

**Severity vocabularies:**

| Tool | Levels (verbatim) |
|---|---|
| [oasdiff](https://github.com/oasdiff/oasdiff) | `ERR` / `WARN` / `INFO` as flag values; `3` / `2` / `1` as the JSON `level` int; `error` / `warning` / `info` from `Level.String()` |
| [Azure `oad`](https://github.com/Azure/openapi-diff) | `Info` / `Warning` / `Error` (`Category`), orthogonal to `Addition` / `Update` / `Removal` (`MessageType`) |
| [Criteo openapi-comparator](https://github.com/criteo/openapi-comparator) | `Info` / `Warning` / `Error` (a C# re-implementation of Azure's, same 1000-series rule numbering) |
| Atlassian `openapi-diff` (Bitbucket-only; `github.com/atlassian/openapi-diff` 404s) | `Breaking` / `Non-breaking` / **`Unclassified`** — "changes that have been detected by the tool but can't be classified" |
| [OpenAPITools/openapi-diff](https://github.com/OpenAPITools/openapi-diff) | weighted enum `NO_CHANGES(0)`, **`METADATA(1)`**, `COMPATIBLE(2)`, `UNKNOWN(3)`, `INCOMPATIBLE(4)`; `isIncompatible() { weight > 2 }` |
| [buf](https://buf.build/docs/breaking/) | **no severity at all** — strictness is category membership (`FILE`, `PACKAGE`, `WIRE`, `WIRE_JSON`, `CSR`) |

Two structural observations. First, **four of the six carry an explicit "detected but undecidable"
tier** — oasdiff's `WARN`, Atlassian's `Unclassified`, OpenAPITools' `UNKNOWN`. Issue #75's
vocabulary has no such tier. Second, OpenAPITools' `METADATA` is exactly issue #75's DOC-ONLY, given
first-class status *between* "nothing changed" and "changed compatibly".

oasdiff's severity is **derived, not hand-assigned**, from a taxonomy of `effect` × `direction`
(`checker/rules/derive.go`): a narrowing on the response side is `INFO` but on the request side is
`ERR`; a widening is the mirror image. That asymmetry — request narrows break clients, response
widens break clients — is a real insight for gnr8, whose graph already models direction explicitly
(`graph/direction.rs`).

**Exit codes diverge sharply**, and this is a genuine decision point:

- **oasdiff**: `--fail-on ERR|WARN|INFO` → `1`; plus a dedicated error band `100`–`123` for distinct
  failure modes, with a source comment noting `123` is "kept under 125 to stay clear of the shell's
  reserved 126/127/128+ range".
- **buf**: `0` clean, **`100`** violations, `1` tool/system error — "We use a different exit code to be
  able to distinguish user-parsable errors from system errors."
- **Atlassian**: fails automatically — "The command will exit with an exit code 1 if any breaking
  changes were found, so that you can fail builds in CI."
- **OpenAPITools**: default is *always* `0`; you must pass `--fail-on-incompatible`.
- **Azure `oad` and Criteo**: never fail on findings at all. Gating is the caller's job.

**Suppression, in increasing quality — this is the part gnr8 should learn from:**

1. **Rendered-text matching.** oasdiff's `--err-ignore` / `--warn-ignore` files match on
   `(path, operation, rendered message)` via `strings.Contains`, with no rule ID involved. Brittle by
   construction: reword a message and every ignore line silently stops matching. (Their own
   `examples/ignore-err-example.txt` calls the lines regexes; `checker/ignore.go` imports no `regexp`.)
2. **Rule-ID × path map.** buf's `ignore_only: {FILE_SAME_TYPE: [foo/foo.proto, bar]}` is the only
   mechanism among the six that gives per-check *and* per-location granularity. oasdiff cannot do
   this: `--severity-levels` is global-per-ID, ignore files are per-path-with-no-ID, and the two axes
   never meet.
3. **Content-derived stable fingerprint.** oasdiff's `fingerprint` is
   `SHA256("{id}:{operation}:{path}:{text}")[:12]`, documented as "stable across commits — the same
   breaking change in a PR gets the same fingerprint regardless of which commit introduced it", built
   so "if a reviewer approves a breaking change on commit A, the approval can be carried forward"
   ([docs/FINGERPRINT.md](https://github.com/oasdiff/oasdiff/blob/main/docs/FINGERPRINT.md)). Its one
   flaw is that `text` is in the hash, so a wording change invalidates every approval.

**buf's hardest-won lesson, and it is exactly §2.4.** Path exclusions are tested against **both** sides
of the diff — the current `FileLocation` *and* the `AgainstFileLocation` — because otherwise an
exclusion cannot suppress a *deletion* at all: a deleted file has no current-side path. For the same
reason buf **hardcodes comment-ignores off** for breaking checks (`AllowCommentIgnores: false`,
`CommentIgnorePrefix: ""`), and there is no `buf:breaking:ignore` anywhere in the repo — a breaking
violation may exist only on the against side, where there is no comment to carry a directive.

(Noting the obvious for gnr8: buf's `// buf:lint:ignore RULE_ID` is precisely the in-comment key/value
grammar CLAUDE.md rule 0.1 forbids gnr8 from reading *or* inventing. The transferable content is the
shape of `use` / `except` / `ignore_only` and the both-sides rule, not the directive syntax.)

**And the closest prior art to Emil's actual ask, which does exist:** oasdiff's
[`x-stability-level`](https://github.com/oasdiff/oasdiff/blob/main/docs/STABILITY.md) — a per-endpoint
in-spec label over `draft` → `alpha` → `beta` → `stable`, with `--stability-level` as the threshold.
Verbatim from the docs, and verified in `checker/stability_level.go`:

> By default, oasdiff uses a **beta** threshold: endpoints marked `draft` or `alpha` are excluded from
> breaking-change detection, while `beta` and `stable` endpoints are checked.

> Endpoints with **no** `x-stability-level` are treated as `stable` and are always included regardless
> of the threshold.

That is a per-operation exemption from CI gating, with a fail-closed default for unlabelled
endpoints — the same polarity §6.5 recommends. And oasdiff closes the relabelling loophole the same
way §2.4 does:

> oasdiff detects changes to an endpoint's `x-stability-level` in **both** directions: **Decreased**
> (`stable`→`beta`, `beta`→`alpha`, etc.) — reported as `api-stability-decreased` …
> These changes are only reported when **the base stability (the level being left)** meets the
> configured threshold.

Two verified negatives worth not designing around: oasdiff has **no `--exclude-endpoints` flag**, and
`--filter-extension` compiles its argument as a regex over extension **names**, never values
(`diff/operations_diff.go`) — so `x-internal: false` is indistinguishable from `x-internal: true`.
oasdiff does have `--attributes x-audience`, which copies an extension's value into the JSON
`attributes` field for a downstream consumer to filter on; the tool itself never acts on it.

---

## 6. Recommendation

### 6.1 The one-line answer to each of Emil's questions

1. **Where does classification live?** Rules in the `.gnr8/` pipeline; the **resolved value on the
   `ApiGraph`**, serialised with it. The evidence the rules read is source structure the graph already
   carries (route group, middleware, path, provenance file), so "close to the code" is satisfied
   without a comment dialect. `.gnr8/` is not the storage location — the graph is.
2. **What is the type?** A closed two-variant `Audience` enum on a sorted, operation-id-keyed
   side-table, written by one `ClassifyOperations` transform, selected with the existing
   `OperationSelector`, first-match-wins within a transform and hard-error on disagreement between
   transforms. Unclassified ⇒ `Public` ⇒ gates.
3. **How does `gnr8 changes` consume it?** Exit `1` iff some BREAKING change's subject is `Public` in
   *either* graph. Internal breakage is always reported, never gating. Narrowing an operation's
   audience is itself a BREAKING change, so relabelling cannot silence anything.
4. **What goes in the OpenAPI artifact?** By default, everything, unmarked — classification changes no
   generated byte. Publishing a reduced document is an explicit filter on the `OpenApi31` target.
   Never emit `x-internal`; never read it.
5. **Prior art?** §5. The shape closest to right is Fern's element-level audience list and Google's
   `visibility.proto`; the gating semantics are gnr8's own.

### 6.2 The graph field

```rust
// crates/gnr8-sdk/src/graph.rs — on ApiGraph, alongside operation_docs (:97)
/// Per-operation audience, keyed by operation id. Absent means [`Audience::Public`].
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub operation_audience: Vec<OperationAudiencePolicy>,

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OperationAudiencePolicy {
    pub operation_id: String,
    pub audience: Audience,
}

/// Who an operation is promised to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// A supported contract. Breaking it blocks a merge. The default for every operation.
    Public,
    /// Reachable, generated, and callable, but not a supported contract.
    Internal,
}
```

Every detail here is copied from an existing decision, not invented:

- **A side-table, not a field on `Operation`.** `Operation`'s doc comment says every structural field
  on it is derived purely from source (`graph.rs:484-492`); audience is a rule-4 config fact. This is
  exactly `operation_security` / `operation_runtime` / `pagination` / `operation_docs` (§1.3).
- **`#[serde(default, skip_serializing_if = "Vec::is_empty")]`** matches all four. And it does real
  work here: `ApiGraph` does **not** use `deny_unknown_fields` (verified: zero occurrences in
  `graph.rs`, versus 20 in `facts.rs`), so a graph serialised before this field existed deserialises
  with an empty table — every operation unclassified — which resolves to `Public`, the safe answer.
  Adding the field cannot retroactively exempt anything.
- **Sorted by `operation_id`** on every write, as `upsert_operation_docs` does
  (`crates/gnr8-core/src/sdk/builtins.rs:2354-2364`), so determinism holds.
- **An enum, not a `bool`.** LoopBack rejected a boolean `x-internal` in review for the open-valued
  `x-visibility` ([PR #1896](https://github.com/loopbackio/loopback-next/pull/1896)); Zalando needed
  five values; Google's labels are arbitrary strings. Two variants is what gnr8 needs *today*, and an
  enum lets a third arrive without a type change at every call site. It stays **closed** — an open
  string set would be a policy vocabulary users invent, which is a dialect by another route.

### 6.3 The transform

Modelled on `GroupOperations` (`crates/gnr8-sdk/src/sdk/builtins.rs:1773`), which is the existing
rules-list transform:

```rust
.transform(
    ClassifyOperations::new()
        .internal(OperationSelector::path_prefix("/internal"))
        .internal(OperationSelector::middleware("RequireInternalToken"))
        .internal(OperationSelector::operation("debugTasks")),
)
```

`ClassifyOperations` holds `rules: Vec<AudienceRule>` where
`AudienceRule { selector: OperationSelector, audience: Audience }`. Reusing `OperationSelector`
(§1.4) means the selector vocabulary, its matcher, and its documented semantics
(`docs/pipeline/transforms.md:9-33`) are already written and already tested.

**One gap to close first.** `OperationSelector` has no source-file selector; only `GroupOperations`
has one, as `GroupRule::SourcePrefix` (`builtins.rs:1780`). A file-prefix rule is the most natural way
to say "everything in `internal/debug/` is internal", so `OperationSelector::SourcePrefix(String)`
matching `op.provenance.file.starts_with(prefix)` should be added to the shared enum. That is a
strictly additive selector variant and it benefits `ApplySecurity` and `DocumentOperation` too. Note
its weakness honestly in the docs: a file-prefix rule does not survive a file move, where a
`group`/`middleware` rule does.

### 6.4 Precedence — exactly one deterministic resolution

Three rules, in this order, and nothing else:

1. **Within one `ClassifyOperations`, the first matching rule wins**, in declaration order. This is
   verbatim the `GroupOperations` semantic — "Rules run in the order they are configured; the first
   match for an operation wins" (`crates/gnr8-sdk/src/sdk/builtins.rs:1771`), implemented as an
   ordered scan with `break` (`crates/gnr8-core/src/sdk/builtins.rs:2487-2528`) — and it is already
   documented for users (`docs/pipeline/transforms.md:268-269`). Users put the exception before the
   general rule, exactly as they already do for grouping.
2. **A rule that matches no operation is a hard error.** `CoreError::Config`, naming the selector.
   This is `DocumentOperation`'s behaviour (`crates/gnr8-core/src/sdk/builtins.rs:2205-2209`) and
   `find_selected_operation_index`'s (`:1956-1965`). A typo'd selector that silently classifies
   nothing would leave an endpoint gating when the author believed it exempt, or — worse in the other
   direction — a stale rule would keep exempting an endpoint that moved.
3. **A second transform assigning a *different* audience to an already-classified operation is a hard
   error**; assigning the same one is a no-op. The error message follows
   `check_operation_prose_conflict`'s wording (`:2284-2308`) — two rules stating one fact is the
   defect, and picking a winner between them is the same defect with extra steps (CLAUDE.md rule 3).
   Like that check, the scan runs over every matched operation **before** any mutation, so a failing
   transform leaves the graph exactly as it found it and the error never depends on operation order
   (`:2171-2174`).

**No "most-specific-wins".** Specificity across `PathPrefix`, `Middleware`, `Methods`, and
`OperationId` has no natural total order — ranking them is an invented metric, and any metric needs a
tiebreak, which is a second invented rule. Declaration order is already the repo's answer to this
exact question, is trivially explainable, and is under the user's direct control. Adding a second
resolution model for the same shape of problem would be the dual-path defect rule 3 forbids.

### 6.5 The default, and why it is the safe one

**An operation with no matching rule is `Public`.** Absent from the side-table ⇒ `Public`. There is no
"unclassified" third state to reason about, and no `Option`.

Failing this direction is the only safe choice: a breaking change to an operation nobody has thought
about is *more* likely to be a real customer break than less. Silence must not buy an exemption.

This also lands on the right side of the prior art (§5.3): Google's implicit `PUBLIC` ("If an element
and all its parents have no visibility label, its visibility is unconditionally granted") and Fern's
"an element with no tags is included for every audience" both default to maximal exposure. gnr8's
default agrees with them on visibility *and* is conservative on gating, because for gnr8 those two
point the same way.

### 6.6 Where in the pipeline, and what it must not touch

`ClassifyOperations` is a `Transform`, so its position is composition order
(`crates/gnr8-core/src/pipeline/mod.rs:9`) and it must be documented like `DiagnosticPolicy` is:
place it **after** any transform that adds, removes, renames, or regroups operations, and before the
targets. Concretely, after `RenameOperation` (a rule keyed on an id must see the final id) and after
any custom filter, and before `RequireOperationDocs`.

The point of ordering it late is one specific interaction. `RequireOperationDocs`'s own doc comment
(`crates/gnr8-sdk/src/sdk/builtins.rs:365-368`) explains that it must run after public-surface
filtering because "gnr8 has no built-in operation-exclusion transform". Once audience exists, the
obvious follow-on is `RequireOperationDocs::public_only()` — require prose on customer contracts, not
on debug endpoints. That is a natural second-step feature, and it is listed in Open (§8.5) rather than
recommended now.

**What classification must NOT do: change a single generated byte.** Not the OpenAPI document, not any
SDK, not `API.md`. Three reasons, and they compound:

- `make examples-check` (`Makefile:134-146`) regenerates all five examples and diffs them. A
  classification that filtered output would make adopting it a visible regeneration, which is exactly
  the friction that stops people adopting it.
- Emil's internal endpoints must stay in the SDK — that is what the test harness calls. A
  classification that removed them would reproduce `DropDebugRoutes`'s defect (§1.7) with a nicer
  name.
- One concept, one job. Audience answers "does breaking this block a merge?". Filtering an artifact is
  a different question with a different answer per artifact.

### 6.7 What `gnr8 changes` does with it

**Command surface.** `gnr8 changes --base <ref>`, exactly as issue #75 spells it. Note that
`--baseline` would fail `make invariants` (`scripts/check-invariants.sh:108` forbids
`--(compat|legacy|migration|baseline)\b`); `--base` passes. The global `--json` and `-v` already exist
(`crates/gnr8/src/cli.rs:22`, `:26`).

**Exit codes stay binary**, because `docs/cli/commands.md:148-155` documents exactly two meanings —
`0` "command completed and its gate passed", `1` "generated drift or an actionable doctor finding",
"other nonzero" for invalid invocation or execution failure. A third code for "breaking but internal"
would break a documented contract that agents are told to rely on. (buf's `0` / `100` / `1` split —
"a different exit code to be able to distinguish user-parsable errors from system errors" — and
oasdiff's `100`–`123` error band are the better idea in the abstract, but gnr8's existing "other
nonzero" already occupies that slot across four shipped commands, and one command inventing a second
convention is worse than either convention alone.) So:

> **Exit `1` if and only if at least one BREAKING change has a subject whose audience is `Public` in
> the base graph or in the current graph. Otherwise exit `0`.**

"…in *either* graph" is the whole trick, and it is what makes the classification honest:

| Base → current | Change | Gates? |
|---|---|---|
| `Public` → `Public` | any break | **yes** |
| `Public` → `Internal` | the audience narrowing itself is BREAKING | **yes** |
| `Internal` → `Internal` | any break | no — reported, not gating |
| `Internal` → `Public` | ADDITIVE (new public surface) | no |
| absent → `Internal` | ADDITIVE | no |
| `Public` → absent (operation removed) | BREAKING | **yes** |

Without the base-graph half, the fix for a red build would be one line in `.gnr8/src/main.rs`, and the
gate would be advisory precisely when it matters. Three independent sources reached the same
conclusion (§5.4): oasdiff reports `api-stability-decreased` and evaluates it against "**the base
stability (the level being left)**"; buf's `ignoreAnnotation` tests path exclusions against both the
current and the against side, because otherwise a deletion can never be suppressed *or* detected;
Google's `visibility.proto` warns that "removing one of the labels but not all of them **can break
clients**". This is the one design element where converging prior art is strong enough to treat as
settled.

**Internal breakage is always reported.** Visibility costs nothing; only gating is policy. A team that
wants internal changes to gate too gets `--strict`, which treats every audience as `Public`. A team
that wants a specific approved break to pass gets `--allow <id>` (below). There is no flag that hides
a change from the report.

**The override path.** Emil asked for "an explicit verification/override path". Recommendation:
`--allow <id>`, repeatable, where `<id>` is the change's stable short identity — a BLAKE3 prefix over
`(code, subject, base_value, current_value)`. gnr8 already depends on `blake3`
(`crates/gnr8-sdk/src/protocol/mod.rs:78` uses it for the capability digest), so this adds nothing.
Two properties make it an override rather than a hole:

- **It is content-addressed.** If the change changes, the id changes, and the allowance stops
  matching. An allowance cannot silently widen to cover a different break later.
- **An `--allow` that matches no reported change is an error**, mirroring rule 2 of §6.4. Stale
  suppressions are the failure mode of every ignore-file design; making them loud is cheap.

This is oasdiff's `fingerprint` idea (§5.4) with one deliberate change. oasdiff hashes
`"{id}:{operation}:{path}:{text}"` — including the **rendered human text**, so rewording a message
invalidates every stored approval. gnr8's should hash only structural facts (`code`, operation
identity, subject, before/after values) and never the prose, so the report's wording stays free to
improve. Both designs beat oasdiff's ignore files, which match on rendered text via `strings.Contains`
with no rule id at all, and buf's `ignore_only: {RULE_ID: [paths]}` — the only per-check × per-location
mechanism in the survey — is the fallback shape if content-addressing proves unergonomic in practice.

There is deliberately no blanket `--allow-all`. A deliberate breaking release is a real scenario and it
is listed in Open (§8.4).

**JSON shape**, mirroring the diagnostics contract (`docs/diagnostics/reference.md:16-35`) so agents
already parsing gnr8 output see a familiar object — stable dotted `code`, module-relative `file`/`line`
plus full `span`, `operation`/`subject` present only when known, everything deterministically sorted:

```json
{
  "base": { "ref": "origin/main", "resolved": "c380c2b78fca9123281f2bdaee196d51b509157c" },
  "summary": { "breaking": 2, "additive": 1, "doc_only": 0, "gating": 1, "allowed": 0 },
  "changes": [
    {
      "id": "9f31c0a4",
      "kind": "breaking",
      "code": "request.property.required.added",
      "operation": "POST /books",
      "operation_id": "createBook",
      "subject": "title",
      "audience": { "base": "public", "current": "public" },
      "gating": true,
      "allowed": false,
      "message": "request field `title` changed from optional to required",
      "file": "internal/books/handlers.go",
      "line": 42,
      "span": { "file": "internal/books/handlers.go", "start_line": 42, "end_line": 47 }
    }
  ]
}
```

`kind` is one of `breaking` / `additive` / `doc_only` — the change's classification, distinct from
`severity`, which in gnr8 already means a diagnostic's `INFO`/`WARN`/`ERROR`. `gating` is a derived
boolean so a consumer never has to re-implement the table above, and `summary.gating` is the number
the exit code is computed from. Source locations come from the **current** graph's `provenance`
(`graph.rs:540`), which is what issue #75 asks for; a pure removal has no current location, so `file`
and `line` are absent and the base `span` is not substituted — one source per fact, absent rather than
guessed.

**Human output** keeps issue #75's three columns verbatim and appends a suffix only where the audience
makes a difference, so nothing that parses the issue's format breaks:

```text
BREAKING  DELETE /books/{id}   operation removed
BREAKING  POST /books          request field `title` changed from optional to required
ADDITIVE  GET /books           optional response field `nextCursor` added
BREAKING  GET /tasks/_debug    response field `count` removed  (internal — not gating)
```

### 6.8 The GitHub Action (issue #76)

Three concrete points, two of which are traps.

1. **`fetch-depth: 0` is required.** `actions/checkout` defaults to `fetch-depth: 1` — "Number of
   commits to fetch. 0 indicates all history for all branches and tags. Default: 1"
   ([actions/checkout](https://github.com/actions/checkout)) — so `--base origin/main` cannot resolve
   in a default checkout. This repo's own workflows set it in exactly one place
   (`.github/workflows/release.yml:45`); everything else uses the default. The action must either
   document the requirement or fail with an error that names it, never with a git error.
2. **The five-minute budget applies.** `scripts/check-ci-budget.py:13` sets `MAX_MINUTES = 5` and the
   check "Enforce the repository's hard five-minute GitHub Actions budget" fails any job without a
   `timeout-minutes` at or under it. This is the strongest argument for materialising the base graph
   cheaply (§3) rather than building a second worker at the base ref.
3. **The report communicates classification, and communicates the trap.** The PR comment issue #76
   sketches becomes:

   ```text
   API changes: 1 breaking (gating), 1 breaking (internal), 1 additive

   Blocking
   - BREAKING [9f31c0a4] POST /books — request field `title` is now required
     internal/books/handlers.go:42

   Not blocking (internal)
   - BREAKING GET /tasks/_debug — response field `count` removed
     main.go:92
   ```

   `report-api-changes: "true"` and `base-ref:` are the inputs issue #76 names; add `allow:`
   (newline-separated ids) and `strict:`. The existing action is a composite that loops
   `working-directories` and runs `gnr8 check` (`action.yml:282-309`); the changes step is the same
   loop with a different verb, plus a `$GITHUB_STEP_SUMMARY` write and the Markdown/JSON artifacts
   issue #76 asks for when commenting is not permitted.

### 6.9 Worked example — Emil's case, on a route that exists in this repo

`examples/taskflow` already ships the exact shape: `tasks.GET("/_debug", debugTasks)` inside
`r.Group("/tasks")` (`examples/taskflow/main.go:36`), described in its own source comment as "a real
internal endpoint" (`main.go:22-24`). Today it is deleted by `DropDebugRoutes`
(`examples/taskflow/.gnr8/src/main.rs:48-54`) and appears **nowhere** in `generated/` — so a test
harness cannot call it through the SDK.

**Step 1 — replace the deletion with a classification.** `.gnr8/src/main.rs` loses the custom
transform and gains a built-in:

```rust
Pipeline::new()
    .source(GoGin::new().inputs(["."]))
    .transform(SetBasePath::new("/"))
    .transform(SetTitle::new("Taskflow API"))
    .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
    .transform(ClassifyOperations::new().internal(OperationSelector::path_prefix("/tasks/_debug")))
    .target(OpenApi31::new().to("generated/openapi.yaml"))
    .target(GoSdk::new().module("example.com/taskflow/sdk").to("generated/sdk"))
    .post(Header::generated())
```

`debugTasks` is now in the OpenAPI document and in the Go SDK — the harness can call
`client.DebugTasks(ctx)` to seed and inspect fixtures — and it is `Internal`.

**Step 2 — someone changes the debug endpoint's response.** `TaskList` gains a required `count`
field only on the debug path. `gnr8 changes --base origin/main`:

```text
BREAKING  GET /tasks/_debug    response field `count` added as required  (internal — not gating)
```

Exit `0`. CI is green. The change is in the PR report, so a reviewer sees it, and no consumer contract
claimed to cover it.

**Step 3 — someone breaks a real endpoint.** `POST /tasks` starts requiring `priority`:

```text
BREAKING  POST /tasks   request field `priority` changed from optional to required
```

Exit `1`. Merge blocked. To land it deliberately they add `--allow 4c7e10bb` to the workflow, which is
a reviewable diff naming exactly one change.

**Step 4 — the trap, closed.** Someone tries to unblock step 3 by adding
`.internal(OperationSelector::post("/tasks"))`:

```text
BREAKING  POST /tasks   audience narrowed from public to internal
BREAKING  POST /tasks   request field `priority` changed from optional to required
```

Still exit `1`, and now with an extra line saying precisely what was attempted. The base graph says
`POST /tasks` was `Public`, and §6.7's "either graph" rule keeps it gating.

### 6.10 The OpenAPI artifact: include, do not mark, and filter only on request

**Recommendation: the OpenAPI document contains every operation, unmarked, by default. Publishing a
reduced document is an explicit per-target filter.**

```rust
.target(OpenApi31::new().to("generated/openapi.yaml").audience(Audience::Public))
.target(OpenApi31::new().to("generated/openapi.internal.yaml"))
```

`OpenApi31` today carries only `path` and `schema_patches`
(`crates/gnr8-sdk/src/sdk/builtins.rs:2041-2044`), and a pipeline may already declare several targets
writing distinct paths, so this is one builder method, not a new mechanism. It is also the shape
every serious prior-art source converges on: Zalando — "**split API specifications along the target
audience** — even if this creates redundancies"; Redocly's own guide configures two `apis` entries
from one root; Fern selects `audiences:` per generator group. And it matches what the spec itself
blesses in §4.10 Security Filtering — produce a reduced document — while noting that behaviour is "not
part of the specification itself".

**Do not emit `x-internal`.** Two independent reasons:

- **It is compliance, not borrowing** (CLAUDE.md rule 0). Apply the rule's own test — *if that tool
  changed tomorrow, would we have to change?* Redocly's flag name is a configurable option
  (`internalFlagProperty`, default `x-internal`); Redoc OSS ignores the key entirely; Stoplight
  Elements' `hideInternal` defaults to **off**; Mintlify uses `x-hidden` and `x-excluded` with
  different meanings; LoopBack uses `x-visibility`. Emitting `x-internal` means promising a behaviour
  that four tools spell four ways and that the spec explicitly declines to underwrite — "Support for
  any one extension is OPTIONAL, and support for one extension does not imply support for others"
  (OAS 3.1.1 §4.9). Any of them changing would change what our output means. That is the definition of
  the wrong side of the line.
- **It would silently delete operations from downstream SDKs.** OpenAPI Generator honours
  `x-internal: true` **by default, with no flag** — `DefaultGenerator.java:1606` skips the operation
  and logs "Operation ({} {} - {}) not generated since x-internal is set to true", and `:501` does the
  same for schemas. A gnr8 user who publishes a document containing internal operations, and whose
  consumer runs OpenAPI Generator, would ship an SDK missing them without either party choosing that.

**Do not read `x-internal` on input either.** It is a third-party generator's convention and rule 0.1
forbids reading it outright. Note that `OpenApi` source already *preserves* unknown `x-` keys inside
parameter fragments (`crates/gnr8-core/src/sdk/openapi_source.rs:1023-1027`) — that is carrying bytes
through, not branching on them, and the distinction must stay that sharp.

**Do not emit a gnr8-owned audience extension either — at least not now.** `x-gnr8-audience` would be
clean under rule 0 (our name, our semantics, nobody else's behaviour to track), but nothing consumes
it, and the moment it exists someone will ask to map it onto `x-internal` — which is precisely
Speakeasy's `x-speakeasy-extension-rewrite` ("map any extension from the wider OpenAPI ecosystem or
another vendor to the equivalent Speakeasy extension"), i.e. the feature rule 0.1 forbids, arrived at
one reasonable step at a time. A user who wants `x-internal` in *their* published document can write a
custom `PostProcess` — rule 0.4's "an artifact gnr8 does not emit → a custom `Target`" row, unchanged.
Revisit if a real consumer appears (§8.6).

**Downstream SDK effects, stated plainly.** With the default, none — every SDK contains every
operation, which is what makes the test-data endpoints usable. With the filter, the reduced document
is a smaller *document*; the SDKs are generated from the graph, not from the document
(`GoSdk`/`PySdk`/`TsSdk` are `Target`s over the frozen `ApiGraph`), so filtering the OpenAPI target
does not touch them. If a user feeds the reduced document back in as an `OpenApi` source for a second
pipeline, they get the reduced surface — explicitly, because they wired it.

### 6.11 Schemas and global facts: derived, never stored

Issue #75 lists schemas, required fields, nullability, enums, and security as change categories. None
of those belongs to one operation, so each needs an audience derived rather than declared. Storing a
second audience table for schemas would be two sources for one fact.

**A schema's audience is the most public audience among the operations that transitively reach it.**
`Public` wins over `Internal`: a schema reachable from even one customer-facing operation is part of
the customer contract, whatever else also uses it. Removing a field from it breaks public consumers
regardless of the debug endpoint that happens to share it.

This is not new machinery. `graph::direction::schema_directions`
(`crates/gnr8-core/src/graph/direction.rs:83-134`) already performs exactly this walk for a different
question: it collects request roots and response roots from every operation's body, params, preserved
`OpenAPI` fragments, and `graph.schema_uses`, then calls `reachable_schemas` over each root set and
returns a `BTreeMap`. Audience is the same walk with the roots partitioned by audience instead of by
direction. Three consequences follow directly:

- **Split ids inherit.** `graph::projection` mints `::input` / `::output` variants
  (`projection.rs:21-24`); each inherits the audience of the schema it was split from, because the walk
  runs on the projected graph (§1.2).
- **`schema_uses` roots are `Public`.** A non-HTTP root (`graph.rs:151`) has no operation and therefore
  no audience of its own; the safe default applies, consistent with §6.5.
- **Graph-level facts are `Public` if any public operation exists.** `base_path`, `title`, `security`,
  and `security_requirements` (`graph.rs:69-82`) are document-wide. `operation_security`
  (`graph.rs:85`) is per-operation and takes that operation's audience.

The derivation is a pure function of `(graph, operation_audience)` and uses `BTreeMap`/`BTreeSet`
throughout, exactly as `direction.rs` does, so it is deterministic.

---

## 7. Alternatives considered and rejected

1. **A marker in the handler's doc comment** — `// internal`, a magic first word, an `x-internal:`
   line. Rejected outright by CLAUDE.md 0.1 ("no directive syntax, no marker prefix, and no key/value
   grammar inside the comment — not `@Summary`, not `gnr8:summary`, not anything") and by 0.5, which
   says such a change "must be rejected in review". It would also break rule 3: `summary`/
   `description` already have exactly one source per operation, and content-branching inside the same
   comment would make the comment mean two things.
2. **A gnr8-invented struct tag or decorator** (`gnr8:"internal"`). Same rule. CLAUDE.md's own
   "Known inconsistency" note about the existing `description:` / `example:` tag grammar states
   explicitly that it "is not a licence to add more".
3. **A magic route-group or tag name** — treat `group == "internal"` as the classification. This is
   the most tempting rejected option, because route grouping *is* legitimate source structure. Two
   problems. First, it makes gnr8 invent a convention users must comply with, which is rule 0 pointed
   inward. Second, `Operation::group` drives SDK file layout (`layout.rs:73-77`, `:192`, §4), so a
   team that wanted an "internal" *file* in their SDK would silently change their CI gating, and vice
   versa. Route groups stay **evidence a rule may match on**, never the classification itself.
4. **Built-in path-prefix inference** (`/internal`, `/_debug` are internal by default). Rejected: a
   silent semantic whose wrong-guess failure mode is a shipped breaking change, and it needs a config
   override to express exceptions, which is the "derive it, unless config overrides" dual-source
   pattern rule 3 forbids by name.
5. **A `bool` instead of an enum.** Rejected on the evidence in §5.1/§5.2: LoopBack rejected exactly
   this in code review for `x-visibility`, Zalando needed five values, and Google's labels are open
   strings. Two variants today, room for a third, no call-site churn when it arrives.
6. **A field on `Operation` rather than a side-table.** Rejected: `Operation`'s doc comment reserves
   it for facts "derived PURELY from source code" (`graph.rs:484-492`). Audience is a rule-4 config
   fact and belongs where the other four config side-tables live.
7. **Rules in `.gnr8/` only, recomputed against the base graph.** Rejected in §3 — recomputation
   applies today's policy to yesterday's API, so relabelling an endpoint would retroactively claim it
   was always internal, defeating §2.4 completely.
8. **Most-specific-wins precedence.** Rejected in §6.4 — no natural total order across
   `PathPrefix` / `Middleware` / `Methods` / `OperationId`, and any invented ranking needs an invented
   tiebreak. Declaration order is the repo's existing answer for the identically-shaped
   `GroupOperations`.
9. **Emitting `x-internal`.** Rejected in §6.10 — compliance under rule 0, and OpenAPI Generator
   silently drops such operations from downstream SDKs by default.
10. **Filtering internal operations out of the OpenAPI document by default.** Rejected: adopting
    classification would then change generated bytes, which `make examples-check` treats as a
    regression to review and which users would reasonably fear. Filtering stays an explicit,
    per-target opt-in.
11. **Keeping `DropDebugRoutes`-style deletion as the answer.** Rejected in §1.7 — it removes the
    operation from the SDK, which is precisely the capability Emil needs to keep.
12. **A third audience value now** (`Partner`, `Beta`, …). Not rejected on principle — the enum exists
    so it can arrive — but out of scope. Note that stability (`draft`/`alpha`/`beta`/`stable`, §5.4)
    is a *different axis* from audience and should not be folded into this enum if it is ever wanted.

---

## 8. Open decisions

1. **How `--base <ref>` materialises the base graph.** §3 lays out three shapes with different
   meanings and costs, and the capability-digest constraint
   (`crates/gnr8-core/src/worker/mod.rs:298-303`) rules out the obvious one whenever the base ref pins
   a different `gnr8` version. The committed-graph-artifact shape is the only one that is both cheap
   and correct, and it is the only one that fits the five-minute CI budget
   (`scripts/check-ci-budget.py:13`) comfortably. It also needs the classification to live on the
   graph, which §3 shows is required anyway. **Whichever shape is chosen must be the only shape** —
   "read the committed artifact, otherwise re-run the pipeline" is a fallback chain.
2. **Whether a graph artifact target should exist, and where it writes.** If (1) resolves to the
   committed artifact, gnr8 needs a target that emits the projected graph as JSON. `ApiGraph` is
   already `Serialize`/`Deserialize` with a byte-identical round-trip proven by
   `crates/gnr8-core/tests/determinism.rs:91` (`a_graph_survives_the_worker_frame_without_losing_a_field`),
   so the emitter is trivial; the questions are the file's path, whether it is on by default, and
   whether committing it is a requirement or a recommendation. All three are `changes` decisions, not
   classification decisions.
3. **Whether `changes` needs an "undecidable" tier.** Four of the six tools surveyed have one —
   oasdiff `WARN` ("changes where the definition genuinely does not contain enough information to
   decide"), Atlassian `Unclassified`, OpenAPITools `UNKNOWN`. Issue #75's vocabulary has three
   values and no such slot. gnr8 has less undecidability than a general OpenAPI differ because it
   diffs its own typed graph, but not none: `Type::Any {}` (`facts.rs:350`) is explicitly lossy, and
   `Response::body_kind` (`graph.rs:598`) can record a dynamic body. Adding a fourth kind is a
   vocabulary change to issue #75 and should be decided there, not here.
4. **A blanket allowance for a deliberate breaking release.** `--allow <id>` is right for one approved
   break; a major-version release that breaks twenty things is a different scenario, and enumerating
   twenty ids is busywork that will get scripted into a blanket anyway. No recommendation — the
   options are a `--strict=false`-style global off switch (bad: invisible), tying the allowance to an
   `info.version` major bump (interesting: Zalando and Microsoft both make versioning the sanctioned
   escape, and `OpenApiMetadataPolicy::version` already exists at `graph.rs:163`), or nothing.
5. **`RequireOperationDocs` gaining an audience filter.** Its doc comment already explains that it
   must run after public-surface filtering because "gnr8 has no built-in operation-exclusion
   transform" (`crates/gnr8-sdk/src/sdk/builtins.rs:365-368`). Once audience exists, "require prose on
   customer contracts only" is the obvious next step. Deliberately not recommended here — one feature
   at a time.
6. **Whether an audience extension is ever emitted, and whether `OpenApi` source reads it back.**
   §6.10 recommends emitting nothing now. If that changes, note the rule-3 shape carefully: gnr8 could
   legitimately read its own emitted key for `OpenApi`-imported operations while using pipeline rules
   for source-extracted ones — that is exactly the one-source-per-operation structure `Operation`'s
   `summary` already uses (`graph.rs:505-511`). It is defensible, but it is a second code path for one
   fact and should not be built speculatively. Also note oasdiff's `--attributes x-audience` copies an
   extension's value into its JSON output without acting on it (§5.4), so even a consumer that wanted
   this would get annotation, not behaviour.
7. **Whether `Audience` should be `PartialOrd`/`Ord` by publicness.** §6.11's "most public wins" and
   §6.7's "public in either graph" both read naturally as a max over an ordered enum, and deriving
   `Ord` on `Public < Internal` (declaration order) would give the *wrong* direction. Either declare
   `Public` last, or write the join explicitly. A small decision, but exactly the kind that silently
   inverts a safety property.
8. **Windows and non-git checkouts.** Everything in §3 and §6.8 assumes `git` on `PATH` and a
   repository with history. gnr8 has no git dependency today (§1.1); `gnr8 changes` would add the
   first. What `changes` does outside a git repository, in a shallow clone, or with a `<ref>` that
   does not resolve should be a typed error naming the cause — never a silent empty diff, which would
   report "no changes" and exit `0`.
