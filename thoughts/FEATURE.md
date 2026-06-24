# Feature Log

Discovery-phase feature ledger for `gnr8`.

Status values:

- `candidate`: plausible feature, not yet accepted.
- `researching`: needs deeper evidence or design.
- `accepted`: accepted direction, still not implemented.
- `deferred`: intentionally out of the first slice.

## Product Core

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-001 | accepted | Native code-first API extraction | Build and own the Go-code-to-API-graph path. Do not wrap Swaggo, oapi-codegen, OpenAPI Generator, or similar tools as the core. |
| F-002 | accepted | OpenAPI as generated artifact | OpenAPI is an output, not the source of truth or internal model. |
| F-003 | accepted | SDK generation from internal graph | SDKs should be generated from the internal API graph, not by shelling out to an external OpenAPI SDK generator. |
| F-004 | accepted | Code-as-config | User customization must be code, not YAML/TOML/JSON configuration. |
| F-005 | researching | `.gnr8/` project workspace | Store user-owned generation code, lifecycle state, generated reports, fixtures, and cache under `.gnr8/`. |
| F-006 | researching | CLI-first UX | The likely primary UX is `gnr8 init`, `gnr8 generate`, `gnr8 watch`, and lifecycle management commands. |
| F-007 | accepted | Simplicity guardrails | Keep the first implementation small. Avoid plugin systems, macro APIs, graph databases, or multi-language runtime machinery until a vertical slice proves the shape. |

## Go To OpenAPI

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-101 | accepted | Parse Go code for route and schema facts | Prefer code structure over comment strings. Comments are escape hatches only. |
| F-102 | researching | Route recognizers | Pluggable recognizers for net/http, chi, gin, gorilla/mux, echo, fiber, and custom internal routers. |
| F-103 | researching | Handler contract inference | Infer request and response schemas from typed handlers, decoder calls, response writer calls, framework context calls, and adapter functions. |
| F-104 | researching | Go type-to-schema mapping | Need native support for primitives, aliases, pointers, slices, maps, embedded structs, validation tags, enum-like consts, and generics. |
| F-105 | candidate | Escape-hatch annotations | Allow minimal annotations only where static analysis cannot prove intent. Do not make annotations the normal path. |

## SDK Generation

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-201 | researching | Idiomatic Go SDK structure | Research client/resource/model/error/retry/pagination layout before generation. |
| F-202 | researching | SDK customization hooks | User code should customize naming, auth, retries, transport, layout, pagination, and error handling. |
| F-203 | researching | Multi-language target backends | Research TypeScript, Python, and Rust SDK targets now, but implement only after Go proves the core model. |

## Incrementality And Lifecycle

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-301 | researching | Watch mode | Save-time generation must be a core workflow, not a bolt-on. |
| F-302 | researching | Incremental graph invalidation | Changed file should only invalidate affected route/schema/output graph nodes. |
| F-303 | researching | Generation lifecycle management | `.gnr8/` should hold code, tests, fixtures, cache, output reports, and maybe generated SDK ownership metadata. |
| F-304 | candidate | Baselines or drift reports | Track generated-output drift and user-edited customization boundaries. |
| F-305 | researching | Realistic fixture test suite | Validate extraction and generation against realistic Go services, not toy-only examples. |

## Multi-Language Sources

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-401 | researching | TypeScript source frontend | Research Express/Fastify/Hono/Nest-style extraction using TypeScript compiler facts. |
| F-402 | researching | Python source frontend | Research FastAPI/Flask/Django-style extraction using Python AST plus optional framework/runtime metadata. |
| F-403 | researching | Rust source frontend | Research Axum/Actix/Poem-style extraction using Rust syntax and semantic tooling. |

## Engineering Quality

| ID | Status | Feature | Notes |
| --- | --- | --- | --- |
| F-501 | accepted | Rust implementation guardrails | Follow vendored `rust-best-practices`: typed errors, clippy discipline, benchmark before optimizing, focused unit/integration/snapshot tests. |
| F-502 | researching | PoC roadmap | Keep a rough phase plan for Go-to-Go proof of concept under `thoughts/ROADMAP.md`. |
