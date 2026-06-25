# gnr8

**One tool for the whole loop between your API code and OpenAPI + client SDKs — end to end, as a single native binary.**

`gnr8` reads your service code, builds a language- and router-agnostic model of its API, and generates
an **OpenAPI 3.1** document and a client **SDK** from it. The generation lifecycle is **configured in
code**, so you — or an AI agent — can adapt exactly how it parses and generates for your project.

> Status: **early.** The first supported frontend is **Go + Gin**, working end to end. The internal
> model is general by design, so more source frameworks and SDK targets are additive, not rewrites.

> **Full reference (CLI, config, type mapping, recipes):** [`docs/USAGE.md`](docs/USAGE.md).

---

## Why I built it

- **One tool, both directions, end to end.** Generate OpenAPI from your code, and generate code (client
  SDKs) from that — in one place, one pass, instead of stitching a chain of tools together.
- **Fast.** A native binary with no warm-up.
- **Incremental.** It regenerates only what actually changed — built for a save-and-see loop, not full
  rebuilds.
- **No runtime.** A single binary. No Docker, no JVM, nothing to stand up.
- **Configured by code, not YAML — and built for AI agents.** The customization surface is *code*, not a
  config file with a handful of human-friendly flags. The point isn't minimal-config "ease of use" for
  humans; it's that an AI agent can write code to adapt the entire parse-and-generate lifecycle to your
  project by extending the framework directly.
- **Modern SDKs, easy to shape.** Generate SDKs that follow modern idioms for each target — and change
  *how* they're generated through that same configuration-as-code model.

---

## How it works

```
your API code
   │  read the routes, handlers, and types directly — your code is the source of truth
   ▼
internal API model   ── language- & router-agnostic: methods, paths, params, schemas, provenance
   │
   ├─▶ OpenAPI 3.1 document
   └─▶ client SDK   (typed client + models + errors; it compiles)
```

The first supported frontend reads **Go + Gin** via `go/types`. A native engine builds a deterministic
model (identical input → byte-identical output) and generates the OpenAPI document and the SDK. A
project-local **`.gnr8/`** workspace tracks what it generated (so it never overwrites your edits),
regenerates only what changed, can watch for source edits, and a `doctor` command reports anything it
couldn't represent.

---

## Quick look

A small Gin service (the first supported frontend) — [`examples/bookstore/`](examples/bookstore/):

```go
func registerRoutes(r *gin.Engine) {
    books := r.Group("/books")
    books.POST("",     createBook)
    books.GET("",      listBooks)   // ?genre= filter
    books.GET("/:id",  getBook)
    books.PUT("/:id",  updateBook)
    books.DELETE("/:id", deleteBook)
}

type Book struct {
    ID          string    `json:"id"`
    Title       string    `json:"title"`
    Genre       Genre     `json:"genre"`              // a code-defined enum → OpenAPI enum
    Price       float64   `json:"price"`
    PublishedAt time.Time `json:"publishedAt"`
    Subtitle    *string   `json:"subtitle,omitempty"` // optional
    Publisher   Publisher `json:"publisher"`          // nested → $ref
    Tags        []string  `json:"tags"`
}
```

Run it:

```bash
gnr8 init        # scaffold the .gnr8/ workspace
gnr8 generate    # write openapi.yaml + the client SDK
```

Out comes OpenAPI 3.1:

```yaml
paths:
  '/books/':
    get:
      operationId: listBooks
      parameters:
        - { name: genre, in: query, required: false, schema: { type: string } }
      responses:
        '200': { content: { application/json: { schema: { $ref: '#/components/schemas/BookList' } } } }
```

…and an SDK you can call:

```go
func (c *Client) CreateBook(ctx context.Context, in CreateBookRequest) (Book, error)
func (c *Client) GetBook(ctx context.Context, id string) (Book, error)
```

The full input **and** the real generated output are committed in
[`examples/bookstore/`](examples/bookstore/) — see [its README](examples/bookstore/README.md) for the
side-by-side.

---

## Try the example

```bash
cargo build --release -p gnr8
cd examples/bookstore
../../target/release/gnr8 generate
# see generated/openapi.yaml and generated/sdk/
```

---

## Status

**Today:** Go + Gin, one route group per service, working end to end → OpenAPI 3.1 + a compiling client
SDK; the `.gnr8/` lifecycle (`init` / `generate` / `check` / `watch` / `doctor`), incremental
regeneration, and edit-protection.

**By design, additive:** the internal model is language- and router-agnostic, so additional source
frameworks and SDK targets extend it rather than reshape it. The deeper configuration-as-code surface
(adapting the lifecycle by extending the framework in code) is the direction it's being built toward;
today a small config drives the knobs that aren't expressible in the source (mount/base path, title,
security).

---

## Principles (see [`CLAUDE.md`](CLAUDE.md))

1. Everything is derived from **your code and your config** — one deterministic source per fact.
2. **Self-contained:** a single native binary, no runtime to install.
3. **Deterministic:** identical input → byte-identical output.
4. **Extensible in code:** adapt the lifecycle by extending the framework, not by toggling flags.

---

## Repo layout

| Path | What |
|------|------|
| `crates/gnr8-core/` | the engine: model, OpenAPI lowering, SDK generation, lifecycle, diagnostics |
| `crates/gnr8/` | the `gnr8` CLI (`init`, `generate`, `check`, `inspect`, `watch`, `doctor`) |
| `goextract/` | the Go frontend that reads Gin source via `go/types` |
| `examples/bookstore/` | a runnable example + its real generated output |
| `fixtures/goalservice/` | the test fixture (a realistic Gin service) driving the contract tests |
| `docs/USAGE.md` | full reference — CLI, config, patterns, type mapping, recipes |
| `docs/` | `demo.md` (walkthrough), `evidence.md` (what's verified) |

Build & verify: `make check` (format, lint, tests) · `make gates` (the full contract suite).
