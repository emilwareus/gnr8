# Installing gnr8

## Release archive layout

```text
gnr8-<os>-<arch>/
  bin/gnr8
  share/gnr8/
    Cargo.toml
    crates/gnr8-core/
    goextract/
    pyextract/
    tsextract/
```

The official installer extracts under `~/.local/gnr8` and creates:

```text
~/.local/bin/gnr8 -> ~/.local/gnr8/bin/gnr8
```

Resource discovery resolves through the **real** executable path, so invoking the symlink works with
`GNR8_RESOURCE_DIR` unset:

```bash
unset GNR8_RESOURCE_DIR
gnr8 --version
gnr8 doctor
gnr8 check
```

Override only when needed:

```bash
export GNR8_RESOURCE_DIR="$HOME/.local/gnr8/share/gnr8"
```

Exactly one location is selected, then validated — `$GNR8_RESOURCE_DIR` when set, otherwise
`share/gnr8` beside the real executable. No other location is searched. If the selected directory is
incomplete, `gnr8` fails and names that directory rather than quietly falling back to another
install, so a stale copy can never feed sidecars to a binary that did not ship it.

## Portable `.gnr8` Cargo dependency

Packaged `gnr8 init` writes:

```toml
[dependencies]
gnr8 = "=0.1.22"   # exact published crate version
```

The crates.io package provides the Rust API. Sidecars (`goextract`, `pyextract`, `tsextract`) come
from the CLI install. Keep the CLI on `PATH` (or set `GNR8_RESOURCE_DIR`) when building or running
the generator.

In-repo gnr8 development still scaffolds a local `path` dependency so contributors stay offline.

## Clean generation

```bash
rm -rf generated/go generated/typescript generated/python
mkdir -p generated/go generated/typescript generated/python
gnr8 generate --force
```

Generated SDKs must compile from an empty output directory with no leftover helpers.

## Deterministic regeneration

```bash
gnr8 generate --force
git diff --exit-code
gnr8 generate
git diff --exit-code
```

Both must produce no tracked changes for identical inputs.

## Doctor severity

| Status | Meaning | Exit |
|---|---|---|
| ready | toolchain checks passed | 0 |
| ready with warnings | passed, non-fatal diagnostics (for example Pydantic deprecations) | 0 |
| NOT READY | compile/import/build failed | 1 |
