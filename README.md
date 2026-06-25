# gnr8

**Point it at your Go API. Get an OpenAPI document and a working Go SDK — from the code itself, no annotations.**

`gnr8` reads a real Go (Gin) service, builds an internal model of its routes and types, and generates
a valid **OpenAPI 3.1** document and a **compiling Go SDK**. It re-generates on save, won't clobber your
edits, and tells you what it can't represent and why. It owns the whole pipeline end to end — it does
**not** wrap Swagger, swaggo, oapi-codegen, or openapi-generator.

> Status: **proof of concept (v1.0).** Today it supports **Go + Gin, one route group per service**, end
> to end. It is deliberately narrow and deliberately self-reliant — see [Status & limits](#status--limits).

---

## Why I built it

I was tired of the OpenAPI/SDK toolchain. Generating a spec and clients for a Go service usually means
sprinkling hundreds of `// @swaggo` annotation comments that drift from the real code, gluing together
three or four tools, and waiting on a slow regenerate loop that doesn't fit a save-and-see workflow. The
annotations *are* a second source of truth, and second sources of truth rot.

So gnr8 takes a different stance:

- **The code is the source of truth.** Routes, request/response types, parameters and schemas come from
  the Go source and its types — not from comments. Delete every annotation and the output is the same.
- **Own the whole pipeline.** Extraction, the internal graph, OpenAPI lowering, SDK generation,
  diagnostics, and the watch loop are all ours. We're a *replacement* for the fragmented toolchain, not
  a consumer of it. (We're working toward depending on nothing but the language's standard library.)
- **Anything the code genuinely can't express, you configure in your engine config — not by scraping
  another tool.** (e.g. auth schemes live in middleware, not handler signatures, so you declare them.)

---

## How it works

```
Go (Gin) source
   │  go/types — read routes, handlers, structs, tags (code-first; no annotations)
   ▼
Internal API graph   ── router-agnostic: methods, paths, params, request/response schemas, provenance
   │
   ├─▶ OpenAPI 3.1 document   (lowered from the graph)
   └─▶ Go SDK                 (Client + typed methods + models + typed errors; it compiles)
```

- A small **Go helper** loads and type-checks your module, recognizes the Gin route registration and
  handler idioms (`Group`, `METHOD`, `ShouldBindJSON`, `c.Param`, `c.Query`, `c.JSON`), and emits stable
  facts.
- A **Rust engine** turns those facts into a deterministic internal graph, then lowers it to OpenAPI and
  generates the SDK. Identical input ⇒ byte-identical output.
- A project-local **`.gnr8/`** workspace holds your config; generation tracks what it wrote (so it never
  silently overwrites a file you edited), skips unchanged files, and can watch for source edits.
- `gnr8 doctor` aggregates what it couldn't represent (with `file:line`) so nothing fails silently.

---

## Quick look

A plain Gin service — **no annotations** ([`examples/bookstore/`](examples/bookstore/)):

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
    Genre       Genre     `json:"genre"`              // code-defined enum → OpenAPI enum
    Price       float64   `json:"price"`
    PublishedAt time.Time `json:"publishedAt"`
    Subtitle    *string   `json:"subtitle,omitempty"` // optional
    Publisher   Publisher `json:"publisher"`          // nested → $ref
    Tags        []string  `json:"tags"`
}
```

Run it:

```bash
gnr8 init        # scaffold .gnr8/ (config: inputs, base_path, output paths, security)
gnr8 generate    # write openapi.yaml + the Go SDK
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

…and a Go SDK you can call:

```go
func (c *Client) CreateBook(ctx context.Context, in CreateBookRequest) (Book, error)
func (c *Client) GetBook(ctx context.Context, id string) (Book, error)
```

The full input **and** the real generated output are committed in
[`examples/bookstore/`](examples/bookstore/) — read [its README](examples/bookstore/README.md) for the
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

## Status & limits

**Works today:** Go + Gin, **one route group per service**, code-first → OpenAPI 3.1 + a compiling,
test-exercised Go SDK; the `.gnr8/` lifecycle (`init` / `generate` / `check` / `watch` / `doctor`),
no-op regeneration, and edit-protection.

**Not yet (honest):**
- **One route group per service.** Multi-group / multi-domain services (several base paths) aren't
  supported yet — that's the next milestone.
- **Gin + Go only.** The internal graph is router-agnostic by design, so other Go routers (and other
  languages) are additive — but unbuilt.
- Facts the source can't carry (security schemes, the mount/base path) come from `.gnr8/` config.

**Self-reliance is a work in progress.** The product rule is to depend on nothing but the language
standard library (and prefer our own code even over that). A few third-party libraries remain and are
being retired — see [`CLAUDE.md`](CLAUDE.md).

---

## Principles (see [`CLAUDE.md`](CLAUDE.md))

1. Never couple to another tool's conventions or formats (no swaggo, no openapi-generator).
2. No third-party dependencies — standard library only, and prefer our own code.
3. No fallback / dual control-flow paths — exactly one deterministic source per fact.
4. What the source can't express comes from your engine config, never from scraping.

---

## Repo layout

| Path | What |
|------|------|
| `crates/gnr8-core/` | the engine: graph, OpenAPI lowering, SDK generation, lifecycle, diagnostics |
| `crates/gnr8/` | the `gnr8` CLI (`init`, `generate`, `check`, `inspect`, `watch`, `doctor`) |
| `goextract/` | the Go helper that reads Gin source via `go/types` |
| `examples/bookstore/` | a runnable, annotation-free example + its real generated output |
| `fixtures/goalservice/` | the test fixture (a realistic Gin service) driving the contract tests |
| `docs/` | `demo.md` (walkthrough), `evidence.md` (what's verified) |

Build & verify: `make check` (format, lint, tests) · `make gates` (the full contract suite).
