# taskflow — a gnr8 example that shows the power of code-as-config

A small Gin "tasks" service, plus a `.gnr8/` lifecycle that mixes gnr8's **built-in** stages with
**your own Rust** — a custom transform and a custom generator. The configuration is *code*, not TOML.

```
examples/taskflow/
├── main.go              # Gin server: one /tasks route group (+ an internal /tasks/_debug route)
├── models.go            # DTOs + the Status enum
├── .gnr8/
│   ├── Cargo.toml       # a tiny binary crate that depends on gnr8-core
│   └── src/main.rs      # THE CONFIG: built-ins + a custom Transform + a custom Target
└── generated/           # committed REAL output of `gnr8 generate`
    ├── openapi.yaml
    ├── sdk/*.go
    └── API.md           # written by the custom generator in .gnr8/src/main.rs
```

## The input (plain Go, zero annotations)

One Gin route group, registered with ordinary idioms gnr8 reads directly — `c.ShouldBindJSON`,
`c.Param`, `c.Query`, `c.JSON(http.StatusXxx, …)`. Note the internal `/_debug` route: it is real code,
but the `.gnr8/` lifecycle's custom transform drops it before generation.

```go
func registerRoutes(r *gin.Engine) {
	tasks := r.Group("/tasks")
	{
		tasks.POST("", createTask)
		tasks.GET("", listTasks)        // ?status= filter
		tasks.GET("/:id", getTask)
		tasks.PUT("/:id", updateTask)
		tasks.DELETE("/:id", deleteTask)
		tasks.GET("/_debug", debugTasks) // internal — dropped by the custom transform
	}
}
```

Typed DTOs — every field maps to a schema; `Status` is a code-defined enum, `Assignee` is nested:

```go
type Status string

const (
	StatusOpen       Status = "open"
	StatusInProgress Status = "in_progress"
	StatusDone       Status = "done"
)

type Task struct {
	ID       string    `json:"id"`
	Title    string    `json:"title"`
	Status   Status    `json:"status"`              // code-defined enum -> OpenAPI enum
	Priority int       `json:"priority"`
	DueAt    time.Time `json:"dueAt"`
	Notes    *string   `json:"notes,omitempty"`     // optional
	Assignee Assignee  `json:"assignee"`            // nested struct -> $ref
	Labels   []string  `json:"labels"`
}
```

## The config IS code: `.gnr8/src/main.rs`

There is no `config.toml`. `.gnr8/src/main.rs` is an ordinary Rust binary that composes a `Pipeline`
from four kinds of stage and hands it to the gnr8 runner. `gnr8 generate` **compiles and runs it**.
Built-ins and your own Rust compose freely:

```rust
use gnr8::graph::ApiGraph;
use gnr8::sdk::prelude::*;
use gnr8::CoreError;

// 1) A custom Transform — edit the IR in Rust before generation.
struct DropDebugRoutes;
impl Transform for DropDebugRoutes {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.operations.retain(|op| !op.path.contains("_debug"));
        Ok(())
    }
}

// 2) A custom Target — write your own generator in ~30 lines. Emits API.md.
struct ApiMarkdown { path: String }
impl Target for ApiMarkdown {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        let mut md = format!("# {}\n\n## Operations\n\n| Method | Path | Operation |\n|--|--|--|\n", ir.title);
        for op in &ir.operations {
            md.push_str(&format!("| {} | `{}` | {} |\n", op.method, op.path, op.id));
        }
        out.write(self.path.clone(), md);
        Ok(())
    }
}

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(GoGin::new().inputs(["."]))                                   // built-in source
            .transform(SetBasePath::new("/tasks"))                               // built-in transforms
            .transform(SetTitle::new("Taskflow API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .transform(DropDebugRoutes)                                          // <-- YOUR transform
            .target(OpenApi31::new().to("generated/openapi.yaml"))               // built-in targets
            .target(GoSdk::new().module("example.com/taskflow/sdk").to("generated/sdk"))
            .target(ApiMarkdown { path: "generated/API.md".into() })            // <-- YOUR generator
            .post(Header::generated()),                                          // built-in post-process
    )
}
```

(The real file in [`.gnr8/src/main.rs`](.gnr8/src/main.rs) is the same, with fuller comments and an
absolute-path helper so the Markdown paths match the spec.)

## The command

From this directory:

```sh
gnr8 generate
```

That compiles + runs `.gnr8/`, then writes `generated/openapi.yaml`, `generated/sdk/*.go`, and
`generated/API.md`. Running it again over unchanged source is a byte-identical no-op.

## The output (all three artifacts)

**OpenAPI** — paths mounted under `/tasks`, `status` is a `$ref` to the code-defined enum, security
from `ApplySecurity`. The dropped `/tasks/_debug` route is **absent**:

```yaml
paths:
  '/tasks/':
    get:
      operationId: listTasks
      parameters:
      - { name: status, in: query, required: false, schema: { type: string } }
      responses:
        '200': { content: { application/json: { schema: { $ref: '#/components/schemas/TaskList' } } } }
components:
  securitySchemes:
    ApiKeyAuth: { type: apiKey, in: header, name: X-API-Key }
  schemas:
    Status: { type: string, enum: [done, in_progress, open] }
```

**Go SDK** — a typed, `context`-first method per operation that builds the URL from the same `/tasks`
base path and sets the `X-API-Key` header:

```go
func (c *Client) CreateTask(ctx context.Context, in CreateTaskRequest) (Task, error)
func (c *Client) GetTask(ctx context.Context, id string) (Task, error)
func (c *Client) ListTasks(ctx context.Context, params ListTasksParams) (TaskList, error)
```

**API.md** — emitted by the custom `ApiMarkdown` target (no `_debug` row — the transform removed it):

```markdown
# Taskflow API

Base path: `/tasks`

## Operations

| Method | Path | Operation |
|--------|------|-----------|
| GET    | `/tasks/`     | listTasks  |
| POST   | `/tasks/`     | createTask |
| DELETE | `/tasks/{id}` | deleteTask |
| GET    | `/tasks/{id}` | getTask    |
| PUT    | `/tasks/{id}` | updateTask |
```

## What this showcases

- **Built-ins + your own Rust, composed freely.** The same pipeline mixes `GoGin`, `SetBasePath`,
  `SetTitle`, `ApplySecurity`, `OpenApi31`, `GoSdk`, and `Header` with a `Transform` and a `Target` you
  wrote inline. Implement one trait, add one `.transform(...)` / `.target(...)` — no forking a generator.
- **Edit the IR in code.** `DropDebugRoutes` mutates `ir.operations` directly; every later stage sees
  the filtered model, so the internal route never reaches the spec, the SDK, or `API.md`.
- **Write your own generator.** `ApiMarkdown` reads the frozen IR and `out.write`s a file — a complete
  custom emitter in ~30 lines, generated in the same pass as OpenAPI and the SDK.
- **No TOML.** `.gnr8/src/main.rs` is the entire configuration surface. `gnr8 generate` compiles and
  runs it; the host owns writing (ownership tracking, no-op detection, edit protection).
