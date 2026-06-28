# Guide: FastAPI or Flask Backend to Python SDK

Use this when the service is Python and the desired output is OpenAPI plus a Python SDK. Prefer this
guide for FastAPI services and for Flask services that already use typed request/response envelopes.

## FastAPI Start

```bash
gnr8 init --source fastapi --sdk python
```

## Flask Start

```bash
gnr8 init --source flask --sdk python
```

## Pipeline Example

```rust
use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(FastApi::new().inputs(["."]))
            .transform(SetBasePath::new("/api"))
            .transform(SetTitle::new("Python Service API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/python-service/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
```

For Flask, change the source line:

```rust
.source(Flask::new().inputs(["."]))
```

## Agent Checklist

- FastAPI extraction is static. Do not rely on importing or running the app during generation.
- Prefer typed parameters, Pydantic models, dataclasses, `response_model`, `status_code`, `Literal`,
  `Enum`, and explicit unions.
- For Flask, avoid untyped `request.json` and unannotated query reads. Add typed envelopes when
  diagnostics say gnr8 cannot infer a request or response shape.
- Keep Python SDK output under `generated/sdk` or another clearly generated path.
- If `doctor` reports diagnostics, improve annotations or add a transform instead of guessing.

## Validate

```bash
gnr8 generate
gnr8 doctor
gnr8 check
python3 -m py_compile generated/sdk/*.py
```

Read `generated/sdk/README.md` and `generated/sdk/reference.md` before writing consuming code.
