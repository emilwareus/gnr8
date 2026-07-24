<!-- generated-by: gsd-doc-writer -->
# CLI command reference

[Agent docs index](../agents/index.md)

Run commands from the application repository root. Global options are:

```text
--json          emit machine-readable output and suppress progress text
-v, --verbose   show more detail; repeat for additional verbosity
-h, --help      print help for the selected command
-V, --version   print the CLI version
```

For automation, put `--json` before the command, capture stdout as JSON, and treat stderr as human
diagnostics.

## Command summary

| Command | Purpose | Writes project files |
|---|---|---:|
| `init` | Scaffold the project-local Rust pipeline | yes |
| `guide` | Print a built-in scenario guide | no |
| `generate` | Run the pipeline and reconcile generated files | yes |
| `watch` | Regenerate after source changes | yes |
| `check` | Detect generated drift without writing | no |
| `inspect` | Explain extracted routes, schemas, or graph | no |
| `doctor` | Diagnose workspace, output, and pipeline health | no |
| `compat` | Compare OpenAPI or SDK public surfaces | no |

## `init`

```bash
gnr8 init [--source go-gin|fastapi|flask|nestjs] [--sdk go|python|typescript]
```

Creates missing files only:

- `.gnr8/Cargo.toml`
- `.gnr8/src/main.rs`
- `.gnr8/.gitignore`
- `.gnr8/README.md`

The command is idempotent and preserves existing files. The default source is `go-gin`. When `--sdk`
is omitted, the source default is Go for Go/Gin, Python for FastAPI/Flask, and TypeScript for NestJS.

```bash
gnr8 init --source nestjs --sdk typescript
```

After init, edit `.gnr8/src/main.rs`, then commit the generated `.gnr8/Cargo.lock` once generation has
resolved dependencies.

## `guide`

```bash
gnr8 guide [TOPIC]
```

Without a topic, lists available guides. Topics:

- `go-gin-to-python-typescript`
- `python-apis-to-python-sdk`
- `nestjs-to-typescript-sdk`

## `generate`

```bash
gnr8 generate [--force] [--accept-generated-baseline]
gnr8 --json generate
```

Runs the project-local pipeline, plans writes, preserves hand-edited generated files, removes stale
files previously owned by gnr8, and updates the ownership manifest.

- `--force` permits overwriting protected edits and can delete any file under a target output anchor
  that the current pipeline no longer produces. Keep output anchors dedicated to generated content.
- `--accept-generated-baseline` adopts the current generator result as an intentional migration
  baseline. It uses the same overwrite/prune path as `--force` and reports `baseline_adopted` in JSON.

JSON includes changed-file groups, counts, timings, diagnostics, cache mode, input/output identity,
baseline state, and cleanup guidance.

## `watch`

```bash
gnr8 watch [--debounce-ms 200]
```

Watches relevant source/configuration paths and reruns generation after a quiet period. The default
debounce is 200 ms; values below 10 ms are clamped to 10 ms. Stop with `Ctrl-C`. Use `check` in CI,
not `watch`.

## `check`

```bash
gnr8 check
gnr8 --json check
```

Runs the same pipeline and write planner as `generate` but changes nothing. Exit status is `1` when
generated artifacts are missing, stale, or protected by edits. A clean result exits `0`.

Developer and CI sequence:

```bash
gnr8 generate   # developer: inspect and commit the result
gnr8 check      # CI: fail on uncommitted generated drift
```

## `inspect`

```bash
gnr8 inspect routes [PATH]
gnr8 inspect schemas [PATH]
gnr8 inspect graph [PATH]
gnr8 --json inspect graph .
```

- `routes` shows operation IDs, methods, paths, parameters, and responses.
- `schemas` shows extracted schema identities and shapes.
- `graph` combines operations, schemas, and diagnostics.

When `.gnr8` exists, inspect uses its configured source pipeline. Without `.gnr8`, pass `PATH` to
inspect a supported source tree directly. JSON returns arrays for `routes` and `schemas`, and a graph
object for `graph`.

## `doctor`

```bash
gnr8 doctor
gnr8 --json doctor
```

Checks workspace setup, child protocol compatibility, pipeline execution, output freshness, protected
edits, and generated OpenAPI readiness. Analysis warnings are informational by themselves. Exit `1`
means at least one actionable lifecycle or output problem exists.

## Exit behavior

| Status | Meaning |
|---:|---|
| `0` | command completed and its gate passed |
| `1` | generated drift or an actionable doctor finding |
| other nonzero | invalid invocation or execution/configuration failure |

Do not infer success from parseable JSON alone; always inspect the process status.
