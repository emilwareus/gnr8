# Decision Log

Discovery-phase decisions for `gnr8`.

Format:

```text
Dx: <decision>
Status: proposed | accepted | superseded
Reason:
Consequences:
```

## D1: Stay In Research Before Implementation

Status: accepted

Reason:
The project is ambitious enough that premature scaffolding will freeze weak assumptions. The current phase is discovery and research hardening only.

Consequences:
No Rust crate, generated examples, SDK output, or runnable CLI should be checked in yet.

## D2: Own The Native Extraction And Generation Pipeline

Status: accepted

Reason:
The product promise is not to wrap fragmented infrastructure. Existing tools prove demand, but the project should own the code-to-API-graph extraction, OpenAPI lowering, and SDK generation pipeline.

Consequences:
Existing tools may be used for comparison, validation, or inspiration, but not as the core engine.

## D3: Code Is Configuration

Status: accepted

Reason:
YAML/TOML/JSON configuration will not be expressive enough for framework-specific extraction, SDK layout, naming policy, auth, retries, pagination, and internal conventions.

Consequences:
User customization should live in versioned code under `.gnr8/`. Static config files may exist only as generated metadata or lock/state files if necessary, not as the user-facing customization model.

## D4: Use Comments Only As Escape Hatches

Status: accepted

Reason:
Swaggo-style comment-driven docs can drift from actual source behavior and require duplicated strings. The product should infer as much as possible from Go code structure and types.

Consequences:
The first research focus is route, handler, and schema inference from code. Annotations are allowed only for cases where code cannot encode enough intent.

## D5: `.gnr8/` Is The Likely Project Workspace

Status: proposed

Reason:
The desired UX is similar to polint: user-owned project-local code and lifecycle artifacts live in a tool folder rather than hidden global state or large YAML files.

Consequences:
Research should define the `.gnr8/` layout before implementation, including generated code ownership boundaries, tests, cache, reports, and extension entrypoints.

## D6: OpenAPI Is An Artifact, Not The Internal Model

Status: accepted

Reason:
SDK generation and source diagnostics need information that OpenAPI cannot always represent cleanly, especially source provenance and language/framework facts.

Consequences:
The internal model should be an API graph. OpenAPI versions are lowerer targets with explicit compatibility/loss diagnostics.

## D7: Simpler Is Better Until Proven Otherwise

Status: accepted

Reason:
The project can easily overbuild itself into a framework, plugin runtime, graph engine, and compiler platform before proving the core product loop. The first useful version needs fewer abstractions, not more.

Consequences:
Prefer a small owned core with direct code paths. Delay macros, dynamic plugins, generalized multi-language runtime abstractions, graph databases, and complex lifecycle machinery until real use cases force them.

## D8: Design For Multiple Source And Target Languages, But Do Not Start Multi-Language

Status: accepted

Reason:
The product should eventually support source languages and target SDKs beyond Go, especially TypeScript, Python, and Rust. But implementing several frontends/backends before one vertical slice works would hide the hard problems instead of solving them.

Consequences:
Research multi-language support now. Keep the internal model language-neutral enough for TS/Python/Rust later, but implement one source and one target first.

## D9: Use Rust Best-Practice Guardrails For Implementation

Status: accepted

Reason:
The implementation will be Rust, and the repo now vendors Apollo's `rust-best-practices` skill. The guidance is compatible with the project's simplicity goals: borrow over clone, avoid panic in production, typed errors in libraries, clippy discipline, measured performance, and tests as living documentation.

Consequences:
Future implementation plans should call out Rust-specific guardrails: `thiserror` for library errors, `anyhow` only at binary boundaries, `cargo clippy --all-targets --all-features --locked -- -D warnings`, realistic fixture tests, small snapshots, and no clever type-state/generic machinery unless it prevents concrete bugs.
