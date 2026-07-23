# gnr8 — Engineering Invariants (non-negotiable)

These are **product-strategy invariants**, not style preferences. The entire premise of gnr8 is that
it **owns its pipeline end-to-end and stands on its own legs**. Violating any rule below is a defect,
no matter how convenient. If a task seems to require breaking one of these, STOP and surface it — do
not work around it.

## 1. Never couple to another tool's conventions or output format

gnr8 derives API facts from the **source language's own constructs** (Go code, `go/ast`, `go/types`)
and from **the user's own configuration of our engine** — never from another tool's annotations,
comments, or formats.

**FORBIDDEN — do not parse, infer from, detect, or depend on, in any way:**
- any other tool's directive-style annotations embedded in code comments (e.g. `// @...`-style comment
  directives that encode API facts)
- any code generator's templates, markers, or sidecar formats
- any other tool's comment dialect or sidecar format

There must be **zero code anywhere in the repo that reads or understands another tool's convention.**
We are a *replacement* for those tools, not a consumer of them.

## 2. Own the product chain; bound commodity dependencies

gnr8 owns every product-defining stage from typed source extraction through the neutral graph to
OpenAPI and generated SDKs. Focused open-source libraries are allowed for commodity concerns such as
serialization, CLI parsing, hashing, file watching, and access to a language's reference compiler.
They must not define gnr8's API model, generation behavior, configuration surface, or public contract.

Keep dependencies narrow and reviewable:

- prefer the standard library or existing focused dependencies when either is sufficient;
- do not add overlapping frameworks or a second implementation path for the same concern;
- do not use dependencies to read another generator's annotations, configuration, or output;
- keep `gnr8-core`'s product behavior deterministic and covered by repository-owned tests; and
- keep generated Go, Python, and TypeScript SDKs standard-library-only.

Before adding a dependency, document the bounded commodity concern it serves and verify that it does
not weaken rules 1, 3, or 4.

### TypeScript toolchain (required, not shipped)

No `typescript` compiler is vendored or bundled. The TypeScript extractor borrows the target project's
own compiler as a *toolchain prerequisite*, the same class of fact as "a Go service needs `go` on
PATH."

The `tsextract` Node sidecar reads TypeScript types via the language's own reference Compiler API. It
gets that compiler the same way every sidecar gets its toolchain: it **borrows the USER's own
`typescript`, resolved from the target project being analyzed** (`tsextract/ts.js`) — exactly as
`goextract` uses the user's `go`, `pyextract` uses the user's `python3`, and every sidecar uses
`node`/`cargo`. `typescript` is therefore a **REQUIRED USER TOOLCHAIN, not a shipped/bundled/vendored
OSS dependency.** (A gitignored `devDependency`, restored via `npm ci` / `make tsextract-deps`, backs
gnr8's OWN test suite only — it is never shipped to users and never committed.)

The bright line:

- `tsextract` derives facts ONLY from the source's own TypeScript types via that toolchain — it NEVER
  reads `@nestjs/swagger`, `zod`, `class-validator`, or any third-party schema/annotation tool (rule 1
  forbids those absolutely).
- Every other sidecar uses its source language's own typed or standard-library facilities.
- Every generated SDK (GoSdk, PySdk, TsSdk) remains dependency-free.
- A future hand-rolled stdlib-pure TypeScript parser (FUT-04) could remove even the toolchain
  requirement.

## 3. No fallback logic / no dual control-flow paths

There must be **exactly one deterministic way** to derive each fact. **Forbidden patterns:**
- "if the annotation is present use it, otherwise parse the code" (the classic dual-source mistake)
- "try strategy A; on failure fall back to strategy B"
- any branch whose only purpose is to recover from a missing/secondary source

One source of truth per fact, one path, always. If the single source can't provide a fact, that fact
comes from the user's config (rule 4) — it is never "filled in" by a fallback.

## 4. What the source can't express comes from user code-as-config — never from scraping

Some facts are genuinely not present in typed source (e.g. security schemes — auth lives in middleware,
not handler signatures). Those are provided by **the user configuring our engine in code they write to
drive gnr8** (the `.gnr8/` crate, below), **not** by scraping another tool's annotations or output.
Examples that MUST come from config, not inference:
- security schemes and which operations they apply to
- any cross-cutting metadata the handler/types don't carry

The config surface is part of *our* product. Other tools' annotations are not.

**The config surface is code, never a data file.** Configuration is a Rust **binary crate** at `.gnr8/`
that depends on `gnr8-core` and composes a `Pipeline` of `Source`/`Transform`/`Target`/`PostProcess`
stages. There is **no TOML/YAML/JSON config file** — every setting is a method call, and anything the
built-ins can't express is ordinary Rust the user writes (a custom stage). `gnr8 init` **always**
scaffolds this crate; the tool does not run without it — adapting that code *is* the product. Extension
is **compile-time** (the host `cargo run`s the user's crate, which links `gnr8-core`); there is no
dynamic plugin runtime, FFI, or macro-heavy config DSL.

---

## Dependency review boundary

Existing Go and Rust dependencies serve bounded implementation concerns. They are not precedent for
outsourcing extraction semantics, the neutral graph, SDK behavior, or code-as-config. Replacing one
with owned code is worthwhile only when it measurably improves correctness, security, distribution,
or maintenance; dependency removal is not a product goal by itself.

When touching dependency integration, prefer the standard library or an existing focused dependency
over broadening the dependency surface, and keep product semantics in repository-owned code.

---

## Other standing constraints (from PROJECT.md, still in force)

- Internal API graph is the source of truth; OpenAPI/SDK are **artifacts** generated from it.
- Code-first extraction; the user's engine config — the `.gnr8/` Rust crate, never a data file — is the
  only escape hatch (see rule 4).
- No dynamic plugin runtime, no macro-heavy config API, no graph database; extension is compile-time only.
- Typed library errors; no production `unwrap`/`expect`/`panic`; deterministic, sorted output
  (identical input ⇒ byte-identical output).
