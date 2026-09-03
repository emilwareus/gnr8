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
`Operation`: `Operation`'s doc comment (`graph.rs:485-492`) states that every structural field on it
is derived purely from source.

`OperationDocsPolicy` carries the sharpest precedent for what NOT to do
(`graph.rs:397-402`):

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
  wins" (`crates/gnr8-sdk/src/sdk/builtins.rs:1770-1771`).
- **Exactly-one-match-or-hard-error.** `find_selected_operation_index`
  (`crates/gnr8-core/src/sdk/builtins.rs:1941`) returns `CoreError::Config` for zero matches
  (`"… did not match any operation"`) and for many (`"… must match exactly one operation but matched
  N"`).

`DocumentOperation` is stricter still: it pre-scans every matched operation for a prose collision
*before mutating any of them* — "so a transform that is going to fail leaves the graph exactly as it
found it. A half-applied transform would make the error depend on operation order"
(`crates/gnr8-core/src/sdk/builtins.rs:2176-2179`) — and errors if the operation already has prose
from source, with the message "would be a second source for one fact" (`:2297-2302`). It also errors
when the selector matches nothing (`:2237`).

That pre-scan-then-mutate discipline is the pattern any new classifying transform must copy.

### 1.6 Transform ordering is composition order, and nothing re-sorts it

`crates/gnr8-core/src/pipeline/mod.rs:9`: "Stage order is composition order. A pipeline with no
custom stages never sends a work frame." `build_ir` walks `plan.transforms` in order (`:263-268`),
grouping consecutive custom stages into one round-trip but never reordering. So a classification
transform's position is user-visible and load-bearing — exactly like `DiagnosticPolicy`, whose doc
says "Place this transform after explicit correction transforms … and before targets"
(`crates/gnr8-sdk/src/sdk/builtins.rs:332-334`).

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
(`crates/gnr8-sdk/src/sdk/builtins.rs:377-381`):

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
- **`--json` is a global flag** (`crates/gnr8/src/cli.rs:23`), and the established CI-gate exit
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
`OperationFileSplit::PerTag` (`crates/gnr8-sdk/src/sdk/layout.rs:33-42`), and per-tag emission puts
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

Issue #75's ask is therefore genuinely novel in one respect and conventional in another: gating CI on
a *per-operation* classification is not attested anywhere, but the underlying idea — different parts
of one surface carry different compatibility promises — is the consensus position everywhere.

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
