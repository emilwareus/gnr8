<!-- generated-by: gsd-doc-writer -->
# Artifacts, lifecycle, and CI

The project-local `.gnr8` binary is an untrusted pure generator. The installed gnr8 CLI validates its
versioned JSON bundle, computes a safe write plan, and is the only component that mutates application
outputs.

## Host/child boundary

```text
gnr8 host
  └─ cargo run --quiet --manifest-path .gnr8/Cargo.toml -- __emit
       └─ Source → Transform → Target → PostProcess
            └─ JSON ArtifactBundle on stdout
  └─ validate handshake, paths, ownership, hashes
  └─ write/check plan
```

The current bundle protocol is version 3 and carries:

- protocol, host CLI, child core version, and capability fingerprint;
- sorted artifacts with producer/ownership/rewrite history;
- structured diagnostics and target output anchors;
- artifact cache key, input roots, and input file stamps.

Handshake environment variables are `GNR8_HOST_PROTOCOL_VERSION`, `GNR8_HOST_CLI_VERSION`, and
`GNR8_HOST_CAPABILITY_FINGERPRINT`. A mismatch fails before output is trusted and instructs the user
to align the installed CLI with `.gnr8/Cargo.lock`.

## Artifact ownership inside the pipeline

| Method | Precondition | Recorded transition |
|---|---|---|
| `Artifacts::create(path, text)` | path does not exist | `created` |
| `Artifacts::overlay(path, text)` | path already exists | full replacement, `overlaid` |
| `Artifacts::rewrite(path, fn)` | path already exists | in-place transform, `rewritten` |

Artifacts stay sorted by path. Every transition records the prior and new producer. A target should
normally create; a post-processor should rewrite. Collisions or missing overlay/rewrite targets fail.

## Host write safety

gnr8 stores last-written path/hash records in `.gnr8/cache/manifest.json` (gitignored). The planner
uses the generated hash, recorded hash, and disk hash to distinguish:

- new/stale output safe to write;
- byte-identical output (no-op);
- output previously owned by gnr8 and safe to delete after configuration removal;
- user-edited or unowned output that must be protected.

`gnr8 generate` writes safe changes and reports protected files. `gnr8 check` computes the same plan
without writing. A missing/corrupt cache degrades to an empty manifest instead of panicking.

All artifact paths must be safe project-relative paths: absolute paths, parent traversal, and unsafe
output-anchor relationships are rejected.

## Force and baseline adoption

- `gnr8 generate --force` permits overwriting protected edits and may prune generated-looking unowned
  files below output anchors. Review the cleanup report first.
- `gnr8 generate --accept-generated-baseline` records the current generator result as an intentional
  migration baseline and reports baseline adoption in JSON.

Neither flag changes extraction semantics. Durable fixes still belong in service source or
`.gnr8/src/main.rs`.

## Post-processing

```rust
.post(Header::generated())
.post(
    FormatCommand::new("gofmt")
        .args(["-w", "generated/sdk/client.go", "generated/sdk/models.go"]),
)
```

`Header::generated` adds the generated marker to Go artifacts. `FormatCommand` runs against a
temporary copy of the declared artifacts, then rewrites changed files back into the set. It cannot
silently create or remove undeclared artifact paths. Missing tools or nonzero commands fail the
pipeline. Arguments are passed directly without shell expansion, so list exact artifact paths or use
an explicit script/program that performs discovery.

## Caches

| Path | Purpose | Commit? |
|---|---|---:|
| `.gnr8/Cargo.lock` | exact generator dependency graph | yes |
| `.gnr8/target/` | compiled project-local generator | no |
| `.gnr8/cache/manifest.json` | generated ownership hashes | no |
| `.gnr8/cache/sources/` | source analysis cache | no |
| `.gnr8/cache/artifacts/` | artifact text/metadata cache | no |
| `.gnr8/cache/verified-noop.json` | hot no-op validation stamp | no |

Cache hits may skip compilation, extraction, or rendering after validating inputs/outputs. Deleting
cache is safe; the next run recomputes it.

## GitHub Action

```yaml
name: generated
on: [pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: emilwareus/gnr8@vX.Y.Z # replace with an exact release tag
        with:
          working-directories: |
            services/books
            services/orders
          version: lock
          setup-go: "true"
          setup-python: "true"
          setup-node: "true"
```

Action inputs:

| Input | Default | Meaning |
|---|---|---|
| `working-directories` | `.` | newline-separated roots containing `.gnr8/Cargo.toml`; blank/comment lines ignored |
| `gnr8-binary` | empty | executable to use; overrides install method |
| `install-method` | `release` | `release`, `source`, or `path` |
| `version` | `lock` | exact release or version resolved from every `.gnr8/Cargo.lock` |
| `extra-args` | empty | shell-split arguments passed to `gnr8 check` |
| `cache` | `true` | cache `.gnr8/cache` and `.gnr8/target` |
| `cache-key-prefix` | `gnr8` | cache-key prefix |
| `setup-rust` / `rust-toolchain` | `true` / `stable` | generator toolchain |
| `setup-go` / `go-version` | `false` / `stable` | Go source toolchain |
| `setup-python` / `python-version` | `false` / `3.x` | Python source toolchain |
| `setup-node` / `node-version` | `false` / `lts/*` | NestJS source toolchain |

Outputs are `binary` (resolved executable path) and `cache-hit`.

The release installer rejects `latest`: generated checks must use an exact version. `version: lock`
uses `cargo tree --locked` to find the direct normal `gnr8` dependency. Every working directory must
resolve the same exact version; an explicitly requested version must equal it. Commit lockfiles.

Install modes:

- `release`: download the exact GitHub release archive.
- `source`: build this Action checkout's CLI.
- `path`: find `gnr8` on `PATH` (or set `gnr8-binary`).

Enable only source-language setup steps needed by the configured services. NestJS still needs the
target project's `typescript` dependency.

## CI gates in this repository

For gnr8 contributors, `make check` is the full local/CI gate:

```bash
make check
```

It runs Rust formatting, clippy with warnings denied, all Rust tests, Go/Python/TypeScript sidecar
tests, fixture builds/vet, Action resolver tests, and deterministic example regeneration/checks.

For application repositories, the normal gate is narrower:

```bash
gnr8 check
# plus the generated SDK's native compiler/tests
```

Related: [CLI commands](../cli/commands.md), [Pipeline configuration](../pipeline/configuration.md),
and [Release process](../RELEASE.md).
