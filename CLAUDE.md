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

## 2. No third-party / OSS dependencies

We stand entirely on our own. The **only** thing we may use is the language's **standard library**
(Rust `std`; Go stdlib such as `go/ast`, `go/types`, `go/parser`, `go/token`, `net/http`,
`encoding/json`). **No external crates or modules** — none. Not for convenience, not for serialization,
not for parsing, not for hashing, not for CLI, not for file watching.

**STRONGLY PREFER hand-rolled, in-repo code over even the standard library** wherever it is reasonable
to own it. When in doubt, write it ourselves and keep it in this codebase.

Before adding ANY dependency: the answer is no. There is no approval path that adds an OSS dependency.

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

## Known debt — current violations of the above (retire these; do not add more)

This codebase was bootstrapped before these rules were locked. The following **violate rule 2** and are
**debt to be removed**, not precedent to copy:

- **Go (`goextract/`):** `golang.org/x/tools/go/packages` (+ `golang.org/x/mod`, `golang.org/x/sync`).
  Target: load/typecheck Go using **stdlib only** (`go/parser`, `go/types`, `go/token`, `go/importer`,
  `go/build`) — own the package/module resolution ourselves.
- **Rust (`crates/`):** `serde`, `serde_json`, `clap`, `thiserror`, `anyhow`, `blake3`,
  `notify-debouncer-full`, `ctrlc`, `insta` (dev). Target: hand-rolled JSON/YAML, arg parsing, error
  types, hashing, file-watching, and snapshot testing — in-repo. (`toml` is already gone — config is
  code now, not a data file; see rule 4.)

When you touch a file that uses one of these, prefer replacing the usage with owned code over extending
it. Never add a new one.

---

## Other standing constraints (from PROJECT.md, still in force)

- Internal API graph is the source of truth; OpenAPI/SDK are **artifacts** generated from it.
- Code-first extraction; the user's engine config — the `.gnr8/` Rust crate, never a data file — is the
  only escape hatch (see rule 4).
- No dynamic plugin runtime, no macro-heavy config API, no graph database; extension is compile-time only.
- Typed library errors; no production `unwrap`/`expect`/`panic`; deterministic, sorted output
  (identical input ⇒ byte-identical output).
