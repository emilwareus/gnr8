# Research — handler-local operation prose (summary / description)

**Date:** 2026-08-07
**Origin:** feature request from OAIZ (consumer of gnr8 for Core OpenAPI + Go/TS/Python SDKs)
**Status:** research complete; design decided; ready to plan

---

## 1. What was asked for, and what we are building instead

OAIZ asked for a `gnr8:`-prefixed comment DSL (`// gnr8:summary …`, `// gnr8:description …`) read from
Go handler doc comments, with unknown `gnr8:*` directives a hard error.

**We are not building that.** A comment with grammar is a dialect regardless of who owns it, and a
two-directive allow-list is a social commitment, not a technical constraint — the request itself
pre-announces wanting `tags` and `deprecated` later. It also only makes sense in Go; in Python and
TypeScript the same marker becomes a docstring/JSDoc dialect, which is the shape CLAUDE.md rule 0.1
names by name.

**We are building the idea one level up, which is what every language already does:** read the
declaration's own doc comment as **plain prose**, using each language's native synopsis convention.

- Go doc comment (`go/doc`'s first-sentence synopsis)
- Python docstring (PEP 257)
- TypeScript JSDoc (leading description, tags excluded)

No marker, no prefix, no grammar. `CLAUDE.md` was amended in this branch to permit exactly this and to
forbid `gnr8:`-style prefixes explicitly (rule 0.1 category 2, rule 1, rule 3, rule 4).

---

## 2. Verified facts about the current codebase

Every claim below was checked against the tree at `848bdb0d`.

### 2.1 Prose is already modelled — just not sourced from code

| Stage | State |
|---|---|
| `graph::ApiGraph.operation_docs` | `Vec<OperationDocsPolicy>` — a **policy** side-channel, `graph/mod.rs:81` |
| `DocumentOperation::summary()/.description()` | exists today, `sdk/builtins.rs:3088` |
| OpenAPI lowering | reads the policy, `lower/mod.rs:491` — **already works** |
| Reference markdown | reads the policy, `sdk/docs.rs:226` — **already works** |
| `SdkOperationDocs{summary, description}` | built at `sdk/model.rs:360` and **read by nobody** |
| Go/Python/TS emitters | grep for `summary` across all three `emit.rs` → **zero hits** |

So the OAIZ claim "prose lands in OpenAPI / reference markdown but not SDK method docs" is exactly
correct. Generated operation methods carry no doc comments at all in any of the three languages.

### 2.2 Summary was deliberately removed once before

`goextract/internal/facts/facts.go:38-42` states plainly that `Security`, `summary`, router-path
overrides and param enum/required are *deliberately absent* because they were doc-comment-annotation
facts. `lower/mod.rs:393` and its test at `:1342` assert `summary:` never appears in output. Those
assertions invert in this work.

### 2.3 Cache invalidation (request item 6) is **already correct**

`go_gin_cache_key` (`sdk/builtins.rs:175`) blake3-hashes the *contents* of every file under the input
dir via `hash_files` (`sdk/mod.rs:911`). `FileHashCacheState::hash_path` (`sdk/mod.rs:1017`) memoizes on
`(len, mtime_ns)` and reads bytes on miss. A comment-only edit changes bytes and mtime → new key → cache
miss. **No fix needed; needs a regression test only.**

### 2.4 There is no built-in operation-exclusion transform

The prelude (`sdk/mod.rs:1131`) has no `ExcludeOperations`. OAIZ's `NON_PUBLIC_ROUTES` is a custom
`Transform` in their own crate. **Consequence:** gnr8-core cannot know when the consumer's
public-surface filtering has finished, so a completeness gate *must* be an explicit pipeline stage the
user places after their filters — not a hardcoded check inside `GoGin::load`.

### 2.5 ERROR-severity diagnostics do not fail a run

`crates/gnr8/src/main.rs:2161` counts them; only `DiagnosticPolicy` (`sdk/builtins.rs:693`) converts
diagnostics into `CoreError::DiagnosticsDenied`. So "missing summary" cannot be a diagnostic if it must
be blocking — it has to be a transform that returns `Err`.

### 2.6 Pre-existing inconsistency, deliberately not expanded

`goextract/internal/types/extract.go:174-179` reads `description:"…"` / `example:"…"` struct tags and
falls back to `schema:"description=…"`. That is gnr8-invented tag grammar *and* a rule-3 fallback
chain. It is flagged in `CLAUDE.md` as under review. **Out of scope here** — field prose is not touched.

---

## 3. Insertion points — all three sidecars confirmed reachable

This was the one genuine unknown flagged during evaluation. **All three are clean.**

### Go — `goextract`

- `handlerDecl` already bundles `decl *ast.FuncDecl` (`internal/handlers/handlers.go:59`).
- `Analyzer.handlerForRoute(route)` (`:1256`) resolves it per route.
- `internal/load/load.go:20` uses `packages.NeedSyntax` with **no custom `ParseFile`**, so `go/packages`
  parses with `parser.ParseComments` by default → `FuncDecl.Doc` is populated.
- `ast.CommentGroup.Text()` strips comment markers **and drops `//go:`-style directive lines**, so
  `//go:generate` never pollutes prose. Free correctness.
- Assembly site: `goextract/main.go:154 buildRoutes` via `CodeFacts`.

### Python — `pyextract`

- `recognize_fastapi` (`routes.py:610`) iterates `stmt` = `ast.FunctionDef`/`ast.AsyncFunctionDef` and
  builds the route dict at `:701`. `ast.get_docstring(stmt)` works directly on that node.
- `recognize_flask` (`routes.py:1033`) has the same shape.
- `ast.get_docstring(..., clean=True)` applies `inspect.cleandoc`, i.e. PEP 257 indentation handling.

### TypeScript — `tsextract`

- `routes.js:558` iterates class members; `member` is a `ts.MethodDeclaration`, route dict built at
  `:608`.
- `loaded.checker` is available (`load.js:26-28,78`).
- `checker.getSymbolAtLocation(member.name).getDocumentationComment(checker)` +
  `ts.displayPartsToString(...)` returns the **leading description only** — JSDoc tags are excluded by
  construction, so `@openapi`/`@nestjs/swagger` blocks are invisible to us without any tag matching.
  This is a rule-0.1 win, not just convenience.

---

## 4. The split rule — one rule, mirrored three times

`go/doc.Synopsis` is **not** used directly, for three reasons: it is deprecated in favour of
`(*Package).Synopsis`; it collapses whitespace (Python/TS would not, so outputs would diverge); and it
returns `""` for text beginning `Deprecated:` / `Copyright` / `All rights reserved`, which would make Go
behave differently from the other two languages on ordinary input.

Instead: **steal the rule, implement it identically in all three sidecars.**

```
summary     = text up to and including the first sentence terminator
              (`.` `!` `?`) that is followed by whitespace-or-end-of-text,
              where a `.` preceded by exactly one uppercase letter does not
              terminate (the go/doc initials guard: "A. Smith" is not two sentences).
              If no terminator exists, the whole text is the summary.
description = the remainder, trimmed. Empty -> None.
```

Properties: total (defined for all input), lossless (no text is silently dropped), deterministic, and
identical across languages. Blank lines become formatting, not semantics.

> **Deviation from the evaluation message:** an earlier framing said "everything after the first blank
> line becomes the description." That silently drops sentences 2..n of an opening paragraph that has no
> blank line after it. The rule above supersedes it.

**Go only:** if the text begins with the function's own name followed by a space, strip it and
capitalize the next rune (`listWidgets returns widgets.` → `Returns widgets.`). This is Go's universal
doc convention; Python and TypeScript have no such convention and get no strip.

**Scope guard:** only handlers that are actually *routed* are read. An unrouted helper's doc comment is
never consulted, so prose cannot leak from internal code.

---

## 5. Wire contract — atomic four-file edit

`analyze/facts.rs` deserializes under `deny_unknown_fields`, so a sidecar emitting an unknown key is a
hard rejection. Order matters:

1. `crates/gnr8-core/src/analyze/facts.rs` — `RouteFact` += `summary`/`description`, `#[serde(default)]`
   so a sidecar that has not been updated yet still parses.
2. `goextract/internal/facts/facts.go` — mirror tags, `omitempty`.
3. `goextract/internal/facts/facts_test.go:125` — add `"summary"` to `canonicalFieldNames`
   (`"description"` is already present via `FieldFact`) and populate the new fields in
   `fullyPopulatedDoc()`, or the drift guard fails.
4. `pyextract/facts.py` / `tsextract/facts.js` — no key whitelist in either; they emit whatever the
   route dict/object contains, sorted. No validator change needed.

---

## 6. Dual-source resolution (rule 3)

`DocumentOperation::summary()/.description()` already set these fields. Two sources for one fact is the
defect rule 3 exists to prevent.

**Resolution — conflict is a hard error, never precedence:**

- The fact lives on `graph::Operation`, **not** in `operation_docs`. If it lived in the policy bucket
  there would be no way to distinguish source-set from config-set, and the conflict would be
  undetectable.
- `DocumentOperation` targeting an operation that already carries source-derived prose →
  `CoreError::Config` naming the operation.
- `DocumentOperation` remains valid for operations with no doc-comment source (notably `OpenApi`-imported
  ones, and handlers whose prose genuinely lives elsewhere).
- `openapi_source.rs:832` moves imported spec prose onto `Operation` too, so the "one source per
  operation, determined by which `Source` produced it" invariant holds uniformly.

**This is a breaking change** for anyone calling `DocumentOperation::summary()` on a Go-extracted
operation. Version bump 0.3.0 → 0.4.0 + CHANGELOG.

---

## 7. Emission targets

All three `emit_operation` functions already receive `op: &Operation` and `graph: &ApiGraph`, so once
the fields are on `Operation` the insertion is local:

| Target | Site | Shape |
|---|---|---|
| Go | `gosdk/emit.rs:1440` (+ facade `:1411`) | `// MethodName <summary>`, `//`, description lines |
| Python | `pysdk/emit.rs:2000` (header built by `method_def` `:1919`) | docstring **inside** the def |
| TypeScript | `tssdk/emit.rs:1801` (+ `emit_operation_module` `:1214`) | `/** … */` above the method |

Sanitization required per target: `*/` in JSDoc, `"""` in docstrings, CR/LF normalization, and
determinism. Python output must stay `ruff format`-stable (88 cols); Go output passes through `gofmt`.

Covers facade/non-facade × single/split-file layouts in each language.

---

## 8. Risk register

| Risk | Severity | Mitigation |
|---|---|---|
| Golden/snapshot churn across 4 contract snapshots, ~139 emitter tests, 5 examples, 32 generated files | **High effort, low danger** | `make examples-check` regenerates and diffs; it is the oracle |
| Python `ruff format` instability from long docstring lines | Medium | never re-wrap prose; emit verbatim, one line per source line |
| TS `tsc --strict` breakage from `*/` in prose | Medium | sanitize; add an adversarial test |
| Flask/FastAPI docstring reachability | **Resolved** — confirmed reachable | — |
| NestJS JSDoc reachability | **Resolved** — confirmed via checker | — |
| Breaking `DocumentOperation` users | Medium | CHANGELOG + minor version bump; no silent behavior change |
| `gofmt` reflowing emitted comments | Low | `gofmt` does not re-wrap comment text lines |

---

## 9. Scope

**In:** operation `summary` + `description` from routed-handler doc comments in Go/Python/TypeScript;
`Operation` graph fields; OpenAPI + all three SDKs; `RequireOperationDocs` opt-in gate; conflict
detection; cache regression test; fixtures, examples, docs, CHANGELOG.

**Out:** tags, `deprecated`, security, params, responses, operationId from comments — ever. Field-level
prose (the struct-tag inconsistency) stays as-is. No `gnr8:` markers. No swaggo tag reading.

---

## 10. Acceptance criteria (adapted from the request)

- **A.** Doc-commented handler → OpenAPI `summary`/`description` + the same words in Go/Python/TS client
  method docs. ✅
- **B.** Missing summary on a published route + `RequireOperationDocs` → hard failure naming handler and
  method+path. ✅
- **C.** *Dropped.* There are no directives, so there is no unknown-directive case.
- **D.** Params/body/responses unchanged by any doc-comment edit. ✅ (explicit test)
- **E.** Warm cache → doc-comment-only edit → `gnr8 check` is not a no-op. ✅ (regression test)
- **F.** *(new)* `DocumentOperation` colliding with source prose is a hard error. ✅
