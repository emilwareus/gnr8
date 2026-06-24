# Code-As-Config And CLI UX

## Research Question

What should the user experience look like if `gnr8` is configurable through code rather than YAML?

## Position

`gnr8` should be CLI-first and project-local. The UX should be closer to polint than to a traditional generator with a large YAML config file.

The core idea:

```text
gnr8 init
  -> creates .gnr8/
  -> user edits generation code
  -> gnr8 generate/watch executes that code
```

## Polint UX Reference

Polint describes itself as a framework where rule code lives in the user's repository. Its README says `polint init` creates `.polint.toml`, `.polint/rules/src/`, `.polint/cache/`, `.polint/output/`, and `.polint/.gitignore`, and that local rules use a public SDK.

Source: <https://github.com/emilwareus/polint>

Relevant UX lessons:

- The tool scaffolds local code.
- The user owns and reviews that code.
- Cache/output are local lifecycle artifacts.
- There are commands for creating new local units.
- There are machine-readable outputs for agent workflows.

For `gnr8`, the same pattern should apply, but without YAML/TOML as the user-facing customization layer.

## Proposed `.gnr8/` Layout

Candidate:

```text
.gnr8/
  src/
    main.rs
    routes.rs
    schemas.rs
    sdk_go.rs
  tests/
    fixtures/
    snapshots/
  cache/
  output/
    latest.json
    diagnostics.json
  generated/
    openapi/
    sdks/
  .gitignore
```

Alternative:

```text
.gnr8/
  generator/
    Cargo.toml
    src/
  fixtures/
  snapshots/
  cache/
  output/
```

Open question:

- Should generated SDKs live under `.gnr8/generated/` and then be copied out, or should `.gnr8/` only hold generator code and lifecycle state while outputs go to user-selected paths?

## CLI Shape

Candidate commands:

```text
gnr8 init
gnr8 generate
gnr8 watch
gnr8 new sdk go
gnr8 new route-recognizer chi
gnr8 new schema-mapper money
gnr8 inspect graph
gnr8 inspect routes
gnr8 inspect schemas
gnr8 doctor
gnr8 cache status
gnr8 cache clean
gnr8 snapshot update
gnr8 diff
```

Important UX principles:

- `init` scaffolds editable code, not a YAML file.
- `generate` runs local generator code.
- `watch` keeps the local generator hot.
- `inspect` explains what `gnr8` inferred.
- `doctor` explains missing semantic support, stale generated files, and unsupported framework patterns.
- JSON output should exist for agents and CI.

## Code Configuration Shape

Possible Rust API:

```rust
use gnr8::prelude::*;

fn main() -> gnr8::Result<()> {
    gnr8::project()
        .source(go::packages("./..."))
        .recognize(chi::routes())
        .schema(my_schema_rules())
        .openapi(openapi::v3_2().also(openapi::v3_1()))
        .sdk(go_sdk::client().module("example.com/acme/sdk"))
        .run()
}
```

This is illustrative research, not implementation.

## Why Not YAML

YAML is weak for:

- Conditional logic.
- Framework-specific route recognition.
- Type mapping policies.
- Custom SDK layout.
- Auth and retry behavior.
- Organization-specific naming.
- Reusable internal conventions.

Large YAML config files become a second programming language without compiler feedback.

## Lifecycle State

Even with code-as-config, the tool may need generated state:

- Lock file for generator API version.
- Cache keys.
- Snapshot metadata.
- Last-run diagnostics.
- Generated-file ownership map.
- Drift report.

Decision needed:

- Which files are checked in?
- Which are ignored?
- Which are regenerated?

## Research Tasks

1. Study polint's project-local code UX in more detail.
2. Draft `.gnr8/` layout variants.
3. Define checked-in vs ignored lifecycle artifacts.
4. Prototype command help text before implementation.
5. Define machine-readable outputs for agents and CI.
