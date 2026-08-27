<!-- generated-by: gsd-doc-writer -->
# Artifacts, lifecycle, and CI

[Agent docs index](../agents/index.md)

The installed gnr8 CLI owns the whole generation run: it analyzes source, executes every built-in
stage, validates artifact paths, computes a safe write plan, and is the only component that mutates
application outputs. The project-local `.gnr8` worker contributes exactly one thing — the stages you
wrote yourself.

## Host/worker boundary

```text
gnr8 host
  ├─ cargo build --target-dir .gnr8/target      (skipped while .gnr8/ is unchanged)
  ├─ run .gnr8/target/debug/<pkg>, cwd = project root
  │    └─ frame: b"GN8F" | len:u32be | BLAKE3(payload):32 | payload:JSON
  ├─ Hello  ──▶  Ready { protocol, sdk_version, capability_digest, plan }
  ├─ Source → Transform → Target → PostProcess, executed host-side per the plan
  │    └─ one frame round trip per Custom(...) stage
  ├─ Shutdown ──▶ Done
  ├─ validate artifact paths, ownership, hashes
  └─ write/check plan
```

The frame protocol is version 1 and carries:

- the ordered stage plan: built-in declarations inline, custom stages by index and label;
- the graph snapshot for a custom transform or target, and the artifact set for a custom target or
  post-processor;
- sorted artifacts with producer/ownership/rewrite history;
- structured diagnostics, target output anchors, and explicitly declared readiness targets.

Every frame is BLAKE3-digest-checked and bounded at 64 MiB. The handshake compares protocol version,
exact gnr8 version, and a capability digest; a mismatch fails before output is trusted and instructs
the user to align the installed CLI with `.gnr8/Cargo.toml`. A manifest that pins a pre-0.9 `gnr8` is
refused before anything is compiled, with `gnr8 init --upgrade` as the one-shot fix.

### Worker reuse and cargo

`.gnr8/cache/worker.json` records a fingerprint over every file under `.gnr8/` (excluding `target/`
and `cache/`), the host executable's own content hash, and the protocol constants — plus the built
binary's length and hash. When all of that matches, that binary **is** the build output of those
inputs, so cargo is not invoked at all. `gnr8 generate -v` reports `worker: reused` or `worker: built`.

### Trust

Building and running `.gnr8/` compiles and executes Rust from the repository — `build.rs`, proc macros,
and the pipeline's `main()` — with the invoking user's privileges. **It is not sandboxed.** Use
`--no-build` to refuse cargo, or `--no-execute` to refuse both building and running; `gnr8 inspect
routes <path>` analyzes a source tree without touching `.gnr8/`.

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
- byte-identical unowned output that can be adopted without a rewrite;
- user-edited or divergent unowned output that must be protected.

`gnr8 generate` writes safe changes and reports protected files. `gnr8 check` computes the same plan
without writing. A missing/corrupt cache degrades to an empty manifest instead of panicking. Generate
reconstructs ownership for identical outputs and exits non-zero if any divergent output remains
protected; check never creates ownership or a no-op cache entry.

All artifact paths use one portable project-relative form. They must be NFC-normalized UTF-8 with
canonical `/` separators; each component is limited to 255 UTF-8 bytes and 255 UTF-16 code units.
Empty, `.`/`..`, absolute, control-character, Windows-invalid-character, trailing-dot/space, and
Windows device-name components are rejected. The top-level `.gnr8` state directory and gnr8's
transaction names are reserved. Unicode case-fold-equivalent paths collide even on a case-sensitive
host, so a custom `Target` or `PostProcess` must emit one canonical spelling. Unsafe output-anchor
relationships are rejected by the same host boundary.

## Force

- `gnr8 generate --force` permits overwriting protected emitted paths. It may also remove a stale
  path that the ownership manifest records, even when that path was edited.
- Force never recursively cleans an output directory. Unowned support files and other neighbors are
  outside gnr8's ownership and remain untouched.

The flag does not change extraction semantics. Durable fixes still belong in service source or
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
| `.gnr8/cache/artifacts/` | reserved; cross-run artifact reuse is disabled | no |
| `.gnr8/cache/verified-noop.json` | reserved; ignored while pre-child skipping is disabled | no |

Source cache hits may skip extraction inside a normal child run after validating their bounded inputs.
For the Go source that bound is the **enclosing module**, not just the configured input dir, because
`go/packages` type-checks the input packages together with everything they import, using whatever `go`
is on PATH, so the module's build inputs and the toolchain identity are both part of the key. A module
rooted above the project root, a Go workspace that puts other modules in scope, or a tree that cannot
be enumerated exactly, is not cached at all. Every entry
records the key it was computed under and is discarded when that recording does not match the current
run, so a cache restored from another commit can only cost time, never change a verdict.
Deleting cache is safe; the next run recomputes it. Pre-child pipeline skipping is disabled:
Rust code-as-config may read environment, time, network, or arbitrary files while constructing the
pipeline, and the host cannot prove those inputs unchanged. Every command therefore runs the child;
Cargo's build cache and source-analysis caches retain the safe acceleration. Cross-run artifact reuse
is also disabled because target configuration may be derived from the same arbitrary Rust inputs. If a monitored input
changes during a run, the run is rejected and its disposable artifact-cache entry is removed; no
mixed-snapshot outputs are accepted.

## GitHub Action

```yaml
name: generated
on: [pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: emilwareus/gnr8@v0.1.21 # pin an exact released Action tag
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
| `setup-rust` / `rust-toolchain` | `true` / `auto` | generator toolchain; `auto` honors a repository-root `rust-toolchain.toml` (or `rust-toolchain`) pin and installs `stable` when there is none |
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

For gnr8 contributors, `make check` remains the complete local gate:

```bash
make check
```

It runs Rust formatting, clippy with warnings denied, all Rust tests, Go/Python/TypeScript sidecar
tests, fixture builds/vet, Action resolver tests, and deterministic example regeneration/checks.

CI runs the same confidence areas as focused jobs rather than invoking `make check` as one process.
Every job is capped at five total minutes, and generated-project checks are isolated per example. A
fast repository policy job rejects missing or larger job deadlines.

For application repositories, the normal gate is narrower:

```bash
gnr8 check
# plus the generated SDK's native compiler/tests
```

Related: [CLI commands](../cli/commands.md), [Pipeline configuration](../pipeline/configuration.md),
and [Release process](../RELEASE.md).
