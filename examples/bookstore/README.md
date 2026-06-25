# bookstore — a gnr8 example

Point gnr8 at a plain Gin service and get an **OpenAPI 3.1** document plus a
**Go SDK** — every fact is derived from the Go code itself.

```
examples/bookstore/
├── main.go            # Gin server: one /books route group + handlers
├── models.go          # DTOs + the Genre enum
├── .gnr8/config.toml  # base_path, output paths, security scheme
└── generated/         # committed output of `gnr8 generate`
    ├── openapi.yaml
    └── sdk/*.go
```

## The input (plain Go)

A single Gin route group, registered with ordinary Gin idioms gnr8 reads
directly — `c.ShouldBindJSON`, `c.Param`, `c.Query`, `c.JSON(http.StatusXxx, …)`:

```go
func registerRoutes(r *gin.Engine) {
	books := r.Group("/books")
	{
		books.POST("", createBook)
		books.GET("", listBooks)
		books.GET("/:id", getBook)
		books.PUT("/:id", updateBook)
		books.DELETE("/:id", deleteBook)
	}
}
```

Typed DTOs — every field maps to a schema, every `$ref` resolves:

```go
type Book struct {
	ID          string    `json:"id"`
	Title       string    `json:"title"`
	Author      string    `json:"author"`
	Genre       Genre     `json:"genre"`        // code-defined enum
	Price       float64   `json:"price"`
	PublishedAt time.Time `json:"publishedAt"`
	Subtitle    *string   `json:"subtitle,omitempty"`
	Publisher   Publisher `json:"publisher"`    // nested struct -> $ref
	Tags        []string  `json:"tags"`
}

type Genre string

const (
	GenreFiction    Genre = "fiction"
	GenreNonfiction Genre = "nonfiction"
	GenreSciFi      Genre = "scifi"
	// ...
)
```

## The command

From this directory:

```sh
gnr8 generate
```

That writes `generated/openapi.yaml` and `generated/sdk/*.go`. Running it again
over unchanged source is a byte-identical no-op.

## The output

**OpenAPI** — paths are mounted under `/books` (from config), the `genre` field
is a `$ref` to the code-defined enum, and security comes from config:

```yaml
paths:
  '/books/':
    post:
      operationId: createBook
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateBookRequest'
      responses:
        '201': { description: Response 201, content: { application/json: { schema: { $ref: '#/components/schemas/Book' } } } }
        '400': { description: Response 400, content: { application/json: { schema: { $ref: '#/components/schemas/ErrorResponse' } } } }
components:
  securitySchemes:
    ApiKeyAuth:
      type: apiKey
      in: header
      name: X-API-Key
  schemas:
    Genre:
      type: string
      enum: [fiction, mystery, nonfiction, romance, scifi]
```

**Go SDK** — a typed, `context`-first method per operation that builds the
request URL from the same `/books` base path and sets the `X-API-Key` header:

```go
func (c *Client) CreateBook(ctx context.Context, in CreateBookRequest) (Book, error) {
	var out Book
	payload, err := json.Marshal(in)
	// ...
	reqURL := c.baseURL + "/books/"
	req, _ := http.NewRequestWithContext(ctx, "POST", reqURL, reqBody)
	req.Header.Set("Content-Type", "application/json")
	if c.apiKey != "" {
		req.Header.Set("X-API-Key", c.apiKey)
	}
	// ... decode 201 into Book, non-2xx into the typed error
}
```

## What this showcases

- **Zero annotations (code-first).** Routes, request/response types, status
  codes, and parameters all come from the Go AST and types — never comments.
- **Code-defined `Genre` enum.** gnr8 reads the `const` set straight from
  `go/types` and emits an OpenAPI string enum.
- **Security from config.** Auth lives in middleware, not handler signatures, so
  gnr8 never scrapes it. The `X-API-Key` scheme is declared in
  `.gnr8/config.toml` — the single source of truth — and flows into both the
  OpenAPI `securitySchemes` and the SDK's header.
- **Base path from config.** The Gin group prefix is often a runtime value the
  analyzer can't see, so `base_path = "/books"` is declared in config and joined
  to every path in both the spec and the SDK.
