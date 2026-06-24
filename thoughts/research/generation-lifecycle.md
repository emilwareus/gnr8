# Generation Lifecycle

## Research Question

How should `gnr8` manage generation over time so users can safely customize behavior, regenerate outputs, and avoid drift?

## Lifecycle Stages

### Initialize

`gnr8 init` should create the local generator workspace.

Research goals:

- What exactly is created under `.gnr8/`?
- Which files are user-owned?
- Which files are generated and safe to overwrite?
- Is there a lock file?

### Customize

Users edit generator code to define:

- Source packages.
- Router recognition.
- Type mapping.
- OpenAPI profiles.
- SDK targets.
- SDK layout.
- Auth/retry/pagination/error policies.

This is the core code-as-config experience.

### Generate

`gnr8 generate` should:

- Compile or load local generator code.
- Analyze source code.
- Build/update the API graph.
- Emit OpenAPI and SDK outputs.
- Write diagnostics and reports.
- Avoid rewriting unchanged files.

### Watch

`gnr8 watch` should:

- Keep parser and semantic caches warm.
- React to file saves.
- Recompute affected graph nodes.
- Update outputs quickly.
- Avoid loops from generated file changes.

### Inspect

Users need observability into inference:

- Which routes were found?
- Which handler produced which request/response schema?
- Which schemas were inferred?
- Which facts required fallback annotations?
- Which facts could not be inferred?

### Test

Local generator code needs tests:

- Fixture input services.
- Expected API graph snapshots.
- Expected OpenAPI snapshots.
- Expected SDK snapshots.
- Diagnostic snapshots.

### Upgrade

The generator API will change. The lifecycle needs:

- Version pinning.
- Migration commands.
- Compatibility diagnostics.
- Clear split between user code and tool-managed state.

## Generated File Ownership

Open questions:

- Should generated SDK files have ownership markers?
- How are user edits protected?
- Should generated SDKs be overwritten wholesale or patched?
- Should users extend SDKs through separate hand-written files?
- Should the tool maintain a manifest of generated files?

## Drift Detection

Potential drift states:

- Source code changed, generated outputs stale.
- Generator code changed, outputs stale.
- Generated files manually edited.
- OpenAPI output differs from API graph.
- SDK output differs from API graph.

Possible commands:

```text
gnr8 diff
gnr8 check
gnr8 generated status
gnr8 generated repair
```

## Research Tasks

1. Define `.gnr8/` lifecycle file ownership.
2. Define generated output manifest.
3. Define stale output detection.
4. Define local generator testing workflow.
5. Define upgrade/migration workflow.
