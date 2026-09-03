# Research: OpenAPI tags and breaking-change gating

Date: 2026-09-03 · Branch base: `origin/main` @ `c380c2b` · Workspace version `0.10.2`
(`Cargo.toml:9`)

Question:

> `gnr8 changes --base <ref>` (issue #75) must fail CI on breaking changes to the contracts a team
> chooses to protect. The owner has fixed standard OpenAPI operation tags as the classification
> mechanism: how do tags reach the graph and artifacts today, what exact tag filter should control
> the gate, and how does the comparison prevent a revision from exempting its own break?

This document **supersedes the classification mechanism, not the verified ground truth**, in
`thoughts/research/2026-09-03-endpoint-classification-breaking-changes.md`. The owner rejected its
custom `Audience` enum and `ClassifyOperations` transform in favor of metadata already defined by
OpenAPI and already carried by gnr8. The earlier findings about projected graphs, two-revision
comparison, change categories, reporting, default artifact inclusion, and transitive schema
consumers remain evidence unless this document explicitly replaces them.

Everything under **Verified** was read in this checkout or measured on this machine. Everything under
**Recommendation** / **Open** is judgement, not measurement.

---

## 1. Verified: tags already travel through most of the pipeline

### 1.1 `gnr8 changes` is still a design, not an implementation

`crates/gnr8/src/cli.rs:47-96` enumerates the complete command set: `Init`, `Guide`, `Generate`,
`Watch`, `Check`, `Inspect`, `Doctor`. There is no `Changes` variant and no `--base` or tag-filter
argument. The user-facing command table likewise lists only those seven commands
(`docs/cli/commands.md:27-37`). Nothing in this checkout therefore settles the new command's filter
syntax or its comparison semantics.

The earlier document's graph boundary remains the right one. `ApiGraph` is the source of truth
(`crates/gnr8-sdk/src/graph.rs:1-16`); `pipeline::build_ir` produces a post-transform graph
(`crates/gnr8-core/src/pipeline/mod.rs:226-275`), and `pipeline::run` applies the generation
projection before targets (`:280-299`). A contract diff must compare those projected graphs because
they are the facts from which OpenAPI and every SDK target are generated.

One path correction matters for every citation below: there is no
`crates/gnr8-core/src/graph.rs` file in this checkout. The public node definitions live in
`crates/gnr8-sdk/src/graph.rs`; `crates/gnr8-core/src/graph/mod.rs:1-16` re-exports those definitions
and adds only host-side direction/projection algorithms. This is intentional so the host and a
user's `.gnr8/` transform speak one IR (`graph/mod.rs:4-11`).

### 1.2 The graph has two tag-shaped facts, with one exact resolution rule

`ApiGraph` has no operation-level `tags` field and no root tag-definition collection in its complete
field list (`crates/gnr8-sdk/src/graph.rs:58-102`). Instead it carries:

| Graph fact | Meaning | Evidence |
|---|---|---|
| `Operation::group: Option<String>` | One source-derived or configured router/SDK group | `crates/gnr8-sdk/src/graph.rs:481-517` |
| `OperationDocsPolicy::tags: Vec<String>` | Zero or more public OpenAPI operation tags | `crates/gnr8-sdk/src/graph.rs:391-419` |

The graph documents `group` as static router grouping used as an OpenAPI tag and SDK grouping hint
(`graph.rs:484-487`). It documents the tags side-table's empty value as “use the source-derived group
tag, if any” (`graph.rs:407-409`). Lowering makes that sentence executable in exactly one place:

```text
effective_tags(operation) =
    operation_docs.tags, if that vector is non-empty;
    otherwise [operation.group], if group exists;
    otherwise [].
```

`crates/gnr8-core/src/lower/mod.rs:760-764` implements precisely that branch. It does **not** union
the two sources. Any tag-based diff must call the same resolver—or move this resolver into shared
graph code and have both lowering and diffing call it—because reading only `operation_docs.tags`
would miss source-derived tags, while unioning tags would compare a set different from the OpenAPI
artifact.

Both containers are serialized graph data: `Operation.group` is a normal `Operation` field
(`graph.rs:515-517`), and `ApiGraph::operation_docs` is a serde-defaulted, operation-id-keyed
side-table (`graph.rs:95-97`). Unlike the rejected design, standard operation tags need no new
`OperationAudiencePolicy`, no enum, and no second classification field in the worker contract.

### 1.3 Go/Gin supplies a legitimate first tag from route structure

The Gin recognizer's `Route` records `Group` as the deepest static route-group segment
(`goextract/internal/routes/routes.go:52-67`). It first resolves static `Group(...)` assignments and
emits a diagnostic rather than guessing a dynamic prefix (`routes.go:116-169`); route construction
then calls `groupNameFromPrefix(prefix)` (`:214-224`). That helper normalizes the prefix, scans path
segments from right to left, skips empty and parameter segments, and returns the first static segment
(`:560-568`).

The result travels without another interpretation:

```text
Gin static group prefix
    → routes.Route.Group                  goextract/internal/routes/routes.go:214-224
    → facts.RouteFact.Group               goextract/main.go:161-185
    → Operation.group                     crates/gnr8-sdk/src/graph.rs:909-951
    → effective OpenAPI operation tag     crates/gnr8-core/src/lower/mod.rs:760-764
```

The route test makes the algorithm concrete: `/api/books` and `/api/books/{id}` have group `books`,
`/api/admin/stats` has `admin`, and routes directly under `/api` have `api`
(`goextract/internal/routes/routes_test.go:123-161`). The bookstore example mounts
`r.Group("/books")` (`examples/bookstore/main.go:19-29`), and the checked-in document emits
`tags: [books]` on its operations (`examples/bookstore/generated/openapi.yaml:8-31`, `:51-79`).

This is allowed by the repository invariants: the extractor reads the language/framework's actual
routing calls, not a generator annotation, a magic comment, a vendor extension, or a foreign config
file. There is no rule that a segment named `internal` means “exempt”; it is merely a standard tag
string until a `changes` invocation names it.

### 1.4 `.gnr8/` can already attach several standard tags

There is no `SetTags` or `TagOperations` built-in in the exhaustive transform enum
(`crates/gnr8-sdk/src/sdk/stage.rs:65-85`). The existing surface is
`DocumentOperation::when(OperationSelector)`, whose `.tag(...)` and `.tags(...)` builders append one
or several strings (`crates/gnr8-sdk/src/sdk/builtins.rs:1552-1625`). It is exported in the `.gnr8/`
prelude (`crates/gnr8-sdk/src/sdk/mod.rs:642-652`) and documented with
`.tag("Books")` (`docs/pipeline/transforms.md:343-364`).

The transform is a multi-operation selector, not an exactly-one override. Its implementation scans
every operation, applies to each match, and errors if the selector matched zero operations
(`crates/gnr8-core/src/sdk/builtins.rs:2150-2211`). It validates that tag values are not empty
(`:2215-2220`), extends the existing policy vector, sorts it, and deduplicates it (`:2325-2335`).
Therefore this is valid today:

```rust
.transform(
    DocumentOperation::when(OperationSelector::path_prefix("/internal"))
        .tags(["internal", "beta"]),
)
```

There is one subtle but verified effect. For a source operation whose only tag came from
`Operation.group`, adding the first configured documentation tag makes the policy vector non-empty;
the resolver then emits the configured vector **instead of** the group fallback
(`lower/mod.rs:760-764`). The example above emits `[beta, internal]`, not
`[beta, internal, <old-group>]`, although `Operation.group` itself is unchanged. Repeated
`DocumentOperation` transforms accumulate with existing *policy* tags, but they do not materialize
the fallback group into that policy.

The generic selector vocabulary is operation id, exact route, path prefix, methods, middleware,
`Any`, and `All` (`crates/gnr8-sdk/src/sdk/builtins.rs:1134-1152`), all handled by one matcher
(`crates/gnr8-core/src/sdk/builtins.rs:1915-1938`). The one practical selector gap from the earlier
research remains: it cannot say “all operations whose provenance file starts with
`internal/debug/`.” Source-prefix matching exists only inside `GroupOperations`
(`crates/gnr8-sdk/src/sdk/builtins.rs:1778-1808`; core implementation at
`crates/gnr8-core/src/sdk/builtins.rs:2498-2504`). Tagging by path, method, middleware, route, or id
needs no new transform; source-directory tagging needs either ordinary custom Rust today or an
additive `OperationSelector::SourcePrefix` variant.

### 1.5 OpenAPI imports and emits Operation Object tags, but not root Tag Objects

The `OpenApi` source reads a standard Operation Object's `tags` twice for two distinct existing
purposes. It copies the first array element to `Operation.group`
(`crates/gnr8-core/src/sdk/openapi_source.rs:709-719`, assignment at `:877-894`) and copies every
string, in source order, to `OperationDocsPolicy.tags` (`:846-865`). The round-trip test imports
`tags: [Reports, Audited]`, asserts that both survive in the policy, lowers the graph, and asserts
that both reappear on the operation (`:3080-3156`).

On output, the typed lower model has `Operation.tags: Vec<String>`
(`crates/gnr8-core/src/lower/model.rs:108-125`); `lower_operation` fills it through the effective-tag
resolver (`crates/gnr8-core/src/lower/mod.rs:499-520`); YAML emits a non-empty flow sequence
(`crates/gnr8-core/src/lower/yaml.rs:155-171`); JSON emits a string array
(`crates/gnr8-core/src/lower/json.rs:147-175`). That is a verified standard Operation Object tag path
in both directions.

Root Tag Objects are the boundary of current support. The complete `OpenApiDoc` model has
`openapi`, `info`, `servers`, `security`, `paths`, and `components`, but no root `tags` field
(`crates/gnr8-core/src/lower/model.rs:16-32`); the top-level YAML writer has the same set
(`crates/gnr8-core/src/lower/yaml.rs:28-41`). `OpenApiMetadataPolicy` has no tag definitions either
(`crates/gnr8-sdk/src/graph.rs:158-179`), and the importer contains no root-tag read. Consequently:

- operation tag strings are imported, represented, and emitted;
- name/description/`externalDocs` metadata from root Tag Objects is neither imported nor preserved;
- generated documents do not declare their operation tags at the root.

That omission does not make the current document invalid. OpenAPI 3.1 explicitly permits an
operation to use tags that are absent from the root list, although tools may then organize them
arbitrarily ([OpenAPI 3.1.2, OpenAPI Object](https://spec.openapis.org/oas/v3.1.2.html#openapi-object)).
It is an artifact-completeness gap, not a blocker for graph-level gating.

### 1.6 gnr8 SDK grouping is singular `group`, not the operation tag set

The name `operations_per_tag()` is broader than its implementation. Its own documentation defines
the “tag/group” as `Operation::group`, normally set by `GroupOperations` or the first imported
OpenAPI tag, with `default` for an ungrouped operation
(`crates/gnr8-sdk/src/sdk/layout.rs:73-80`). `SdkFileLayout::split()` selects that `PerTag` layout
(`layout.rs:32-42`, enum at `:187-195`).

The common emitter returns only `op.group` or `default`
(`crates/gnr8-core/src/sdk/emit_common.rs:526-532`). Go, Python, and TypeScript each build a sorted
map keyed by that one value and emit a group file for it
(`crates/gnr8-core/src/gosdk/mod.rs:139-150`, `:194-203`;
`crates/gnr8-core/src/pysdk/mod.rs:305-345`;
`crates/gnr8-core/src/tssdk/mod.rs:243-280`). `SdkModel` makes the distinction explicit: service
membership comes from `op.group`, while documentation tags come from a non-empty docs policy or the
same group fallback (`crates/gnr8-core/src/sdk/model.rs:306-370`).

Therefore:

- a source group or the **first** tag on an imported OpenAPI operation can determine native SDK
  service/group names and per-group file layout;
- `GroupOperations` changes that singular value, first matching rule wins
  (`crates/gnr8-core/src/sdk/builtins.rs:2485-2528`);
- `DocumentOperation::tag("internal")` changes effective OpenAPI/docs tags but does not change the
  native SDK service or group-file name, because it leaves `Operation.group` alone.

This distinction is load-bearing for the replacement design. The gate may inspect the full effective
standard tag set without making `internal`, `beta`, or `partner` into SDK grouping policy. Conversely,
a first-tag/group change imported from OpenAPI can still be an SDK-surface change independently of
its effect on the gate.

### 1.7 The verified gaps, stated narrowly

For “tag these operations `internal` from the pipeline,” the core capability already exists:
`DocumentOperation::when(selector).tag("internal")`. There is **one practical selector gap**:
`OperationSelector` lacks the source-file-prefix variant that `GroupOperations` already implements.
That can be added without adding a second classification model.

Two separate limitations must not be disguised as that gap:

1. Configured tags currently replace the source-group fallback in the emitted tag array rather than
   augment it (`crates/gnr8-core/src/lower/mod.rs:760-764`). That is current semantics which users
   can make explicit with `.tags(["books", "internal"])`; changing it is an API-semantics decision,
   not required to make gating work.
2. Root Tag Objects are not modeled or emitted (`crates/gnr8-core/src/lower/model.rs:16-32`). They are
   optional in OpenAPI and are not the classification source, so the gate must not wait on them.

### 1.8 Exit status and invariant constraints remain hard boundaries

`docs/cli/commands.md:103-111` defines `check` as a non-writing gate whose actionable result is exit
`1` and clean result is `0`. The general table reserves `0` for a passed command gate, `1` for an
actionable finding, and other nonzero values for invalid invocation or execution/configuration
failure (`docs/cli/commands.md:148-156`). `changes` should preserve a binary **domain result**—passed
or failed—even though process errors remain other nonzero statuses.

The repository invariants also rule out shortcuts. The design cannot read a comment marker, a
foreign annotation, `x-internal`, or a generator's filter config; cannot add a YAML/TOML policy file;
and cannot maintain both tags and a custom audience side-table. Standard OpenAPI tags already in the
graph plus an explicit CLI gate policy are the clean path: standard carrier, gnr8-owned comparison
semantics, and one fact per operation.

---

## 2. Verified: facts the tag gate must preserve from the earlier research

### 2.1 Each revision must contribute its own tags

The earlier research established that `--base <ref>` needs two historical projected graphs, not one
graph plus today's pipeline rules. Standard tags improve that design because both existing tag facts
are already serialized on each `ApiGraph` (§1.2). A base operation's tags come from the base graph;
the current operation's tags come from the current graph. Re-running today's tagging transform over
both revisions would erase the history the gate needs and recreate the rejected document's §2.4
trap under a different name.

The comparison therefore has three operation states on each side:

- **checked** — the operation exists and none of its effective tags is in the configured exempt set;
- **exempt** — the operation exists and at least one effective tag is in that set;
- **absent** — the operation does not exist on that graph side.

Absent is not “untagged.” That distinction is required for a removed exempt operation: only the base
graph has an operation whose tags can be evaluated.

### 2.2 Change detection and gate scope are separate questions

The earlier document's change taxonomy remains: `BREAKING`, `ADDITIVE`, and `DOC-ONLY` describe what
changed; `gating` describes whether a reported breaking change fails this invocation. The tag filter
must not delete an operation before diffing. Pre-filtering either OpenAPI document would turn a
one-sided tag change into an apparent operation addition/removal, a behavior oasdiff explicitly
warns about for its one-document-at-a-time extension filter
([oasdiff endpoint filtering](https://github.com/oasdiff/oasdiff/blob/main/docs/FILTERING-ENDPOINTS.md)).

Tags must instead be read after operation identities and structural changes have been paired. Every
change remains visible; a derived boolean controls only the exit gate.

### 2.3 Schema scope is derived from operation consumers

Schemas do not carry operation tags, and adding schema tags would create a second classification
surface. `schema_directions` already demonstrates the needed graph walk: it gathers request body,
parameter, response, imported-fragment, and non-HTTP roots (`crates/gnr8-core/src/graph/direction.rs:76-119`),
then computes transitive `$ref` reachability with ordered collections (`:120-163`). Tag-based scope
uses the same reachability relation with roots partitioned by checked/exempt operation instead of by
request/response direction.

The earlier “most public consumer wins” conclusion becomes “most checked consumer wins”: if any
non-exempt operation reaches a schema, a breaking change to that schema is in gate scope. The result
must be computed separately on the base and current graphs so a retagged consumer cannot rewrite its
past scope.

