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
