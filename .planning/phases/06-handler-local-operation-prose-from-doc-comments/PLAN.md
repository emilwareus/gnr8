# Phase 6 — Handler-local operation prose from doc comments

**Milestone:** v3.0+ (post-v3.0 feature phase)
**Research:** `thoughts/research/2026-08-07-handler-doc-prose.md`
**Origin:** OAIZ feature request (comment DSL asked for; plain-prose doc comments delivered instead)
**Baseline:** `make check` green at `848bdb0d`

---

## Goal

Operation `summary` and `description` come from the **routed handler's own doc comment**, read as plain
prose using each language's native synopsis convention, and reach OpenAPI *and* all three generated
SDKs. Adding an endpoint must not require a `.gnr8/` edit.

## Non-goals (permanent)

Tags, `deprecated`, security, params, responses, status codes, operationId from comments — ever. No
`gnr8:` marker, no `@Summary`, no key/value grammar inside a comment. Field-level struct-tag prose is
untouched.

---

## Requirements

| ID | Requirement |
|---|---|
| DOC-01 | A routed handler's doc comment yields `summary` (first sentence) and `description` (remainder), in Go, Python, and TypeScript, by one identical rule. |
| DOC-02 | Only **routed** handlers are read. An unrouted function's doc comment never reaches output. |
| DOC-03 | Prose is a first-class field on `graph::Operation`, not a `operation_docs` policy entry. |
| DOC-04 | Exactly one source per operation. `DocumentOperation` colliding with source-derived prose is a **hard error**, never precedence, never a fallback. |
| DOC-05 | Summary/description reach OpenAPI **and** Go method comments, Python docstrings, TypeScript JSDoc. |
| DOC-06 | `RequireOperationDocs` is an opt-in `Transform` that fails on any operation with no summary, naming operation id + method + path + handler. Default off. |
| DOC-07 | Structure (params/body/responses/status) is provably unchanged by any doc-comment edit. |

## Success criteria

- **A** Doc-commented handler → OpenAPI `summary`/`description` + same words in all three SDK method docs.
- **B** Missing summary + `RequireOperationDocs` → hard failure naming handler and method+path.
- **D** Adding/removing/editing doc comments never changes request/response schemas.
- **E** Warm cache → doc-comment-only edit → `gnr8 check` is not a no-op.
- **F** `DocumentOperation` colliding with source prose → hard error.
- `make check` green end to end, including `examples-check` byte-identical regeneration.

---

## The one split rule (mirrored in three sidecars)

```
summary     = text up to and including the first sentence terminator (. ! ?) that is
              followed by whitespace or end-of-text, where a `.` preceded by exactly
              one uppercase letter does not terminate (go/doc's initials guard).
              No terminator anywhere -> the whole text is the summary.
description = the remainder, trimmed. Empty -> None.
```

Total, lossless, deterministic, identical across languages. Blank lines are formatting, not semantics.

**Go only:** a leading `<funcName> ` is stripped and the next rune capitalized (Go's universal doc
convention). Python and TypeScript get no strip.

`go/doc.Synopsis` is deliberately **not** called: it is deprecated, it collapses whitespace (Python/TS
would not), and it returns `""` for text starting `Deprecated:`/`Copyright`. We steal its rule, not its
implementation.

---

## Plans

### 06-01 — Graph contract
`graph::Operation` += `summary`/`description: Option<String>` with `skip_serializing_if`. Update graph
JSON snapshot. **Blocks everything.**

### 06-02 — Go extraction
Atomic four-file wire edit: `analyze/facts.rs` (`#[serde(default)]` first, so un-updated sidecars still
parse) → `goextract/internal/facts/facts.go` → `facts_test.go` canonical key list + `fullyPopulatedDoc`
→ read `handlerDecl.decl.Doc` in `handlers.go`, surface on `CodeFacts`, assemble in `main.go:buildRoutes`.
New `goextract/internal/docs` package holding the split rule + unit tests.

### 06-03 — Python + TypeScript extraction
`pyextract/routes.py`: `ast.get_docstring(stmt)` in `recognize_fastapi` and `recognize_flask`.
`tsextract/routes.js`: `checker.getSymbolAtLocation(member.name).getDocumentationComment(checker)` +
`ts.displayPartsToString` — leading description only, tags excluded by construction. Shared split rule
ported to each, with the Go unit-test table mirrored so the three cannot drift.

### 06-04 — Lowering + conflict detection
`lower/mod.rs` reads `Operation.summary`/`.description` (invert the `:1342` "no summary survives" test).
`DocumentOperation::apply` errors on collision with source prose. `openapi_source.rs` moves imported
spec prose onto `Operation`.

### 06-05 — SDK doc emission
`gosdk/emit.rs` (`emit_operation` + facade), `pysdk/emit.rs` (docstring inside the def, `ruff
format`-stable at 88 cols), `tssdk/emit.rs` (`emit_operation` + `emit_operation_module`). Per-target
sanitization: `*/` in JSDoc, `"""` in docstrings, CR/LF normalization. Facade/non-facade × single/split
layouts.

### 06-06 — `RequireOperationDocs`
New `Transform` shaped like `DiagnosticPolicy`. Opt-in, placed after the user's own filters (gnr8-core
cannot know when consumer filtering finished — there is no built-in exclusion transform). Prelude
export + `docs/pipeline/transforms.md` + `docs/reference/public-api.md`.

### 06-07 — Fixtures, examples, acceptance tests, docs
Doc comments on `fixtures/goalservice` handlers; regenerate `expected/openapi.yaml` + `expected/sdk`.
Regenerate all five examples. Acceptance tests A/B/D/E/F. Cache regression test (behavior already
correct — locking it). `USAGE.md`, `AGENT-USAGE.md`, `docs/agents/`, `docs/extraction/sources.md`,
README, CHANGELOG, version bump 0.3.0 → 0.4.0.

---

## Dependency order

```
06-01 ──┬── 06-02 ──┐
        └── 06-03 ──┴── 06-04 ── 06-05 ──┐
                                06-06 ───┴── 06-07
```

06-02 and 06-03 are independent once the contract lands. 06-06 is independent of 06-05.

## Risks

| Risk | Mitigation |
|---|---|
| Wide golden/snapshot churn (4 contract snapshots, ~139 emitter tests, 5 examples) | `make examples-check` is the oracle — it regenerates and byte-diffs |
| `ruff format` instability from long docstrings | emit prose verbatim, never re-wrap |
| `tsc --strict` breakage from `*/` in prose | sanitize + adversarial test |
| Breaking `DocumentOperation` callers | CHANGELOG + 0.4.0; hard error, never silent |
| Three split implementations drifting | one shared unit-test table mirrored in all three sidecars |

## Invariant compliance

Rule 0.1 category 2 (native doc convention, prose only). Rule 1 (no foreign dialect, no invented
grammar). Rule 3 (one source per fact; collision = error, not precedence). Rule 4 (config is for
cross-cutting facts, not per-endpoint prose). `make invariants` stays green.
