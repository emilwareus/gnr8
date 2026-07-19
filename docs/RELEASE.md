# Release

The release process is intentionally shaped like `exlint`:

- `Release dry-run` runs on pushes/PRs and exercises crates.io dry-run packaging plus per-platform CLI
  archives.
- `Release` is a manual `workflow_dispatch` from `main`: bump the release version, commit to `main`,
  create `vX.Y.Z`, optionally publish the single public `gnr8` crate, then upload CLI archives and
  checksums to the GitHub Release.

## Local Dry Run

```bash
./scripts/release-local-check.sh
```

This runs `make check`, builds a host archive, writes a `.sha256`, and runs `DRY_RUN=1
./scripts/publish-crates.sh`.

Build one archive directly:

```bash
TARGET="$(rustc -vV | sed -n 's/^host: //p')" \
ASSET_OS=macos \
ASSET_ARCH=aarch64 \
scripts/package-release.sh
```

The archive lands under `target/release-local-dist/dist/` and uses the same names as CI:

- `gnr8-linux-x86_64.tar.gz`
- `gnr8-linux-aarch64.tar.gz`
- `gnr8-macos-x86_64.tar.gz`
- `gnr8-macos-aarch64.tar.gz`
- `gnr8-windows-x86_64.tar.gz`

Each archive also gets a matching `.sha256` file.

## Archive Layout

Each archive contains:

- `bin/gnr8`
- `share/gnr8/crates/gnr8-core`
- `share/gnr8/goextract`
- `share/gnr8/pyextract`
- `share/gnr8/tsextract`

The `share/gnr8` tree is required because source extraction shells out to the Go/Python/TypeScript
sidecars, and archive installs can scaffold `.gnr8` with a local path dependency for offline use.
`gnr8` discovers this tree automatically from the archive layout; `GNR8_RESOURCE_DIR` can override it.

The CLI and engine use focused open-source dependencies for commodity concerns such as serialization,
CLI parsing, and file watching. gnr8 owns the source-to-OpenAPI-to-SDK pipeline itself; generated SDKs
remain standard-library-only.

## GitHub Release

1. Make sure `main` is green.
2. Open **Actions → Release → Run workflow** on `main`.
3. Leave `publish_crates=true` to publish exactly one crates.io package: `gnr8`.
4. Leave `publish_cli=true` to upload the CLI archives.
5. The workflow bumps the release version in `Cargo.toml`, refreshes `Cargo.lock`, commits to `main`,
   creates tag `vX.Y.Z`, publishes `gnr8` when requested, builds assets, and creates/updates the
   GitHub Release.

## Install Script

Users can install the latest archive with:

```bash
curl -fsSL https://raw.githubusercontent.com/emilwareus/gnr8/main/scripts/install.sh | bash
```

Environment overrides:

- `GNR8_REPO=owner/repo`
- `GNR8_RELEASE_TAG=v0.1.0`
- `GNR8_INSTALL_ROOT=$HOME/.local/gnr8`
- `GNR8_BIN_DIR=$HOME/.local/bin`

## Required User Toolchains

Users need Rust/cargo because `gnr8 generate` compiles the project-local `.gnr8` generation crate.
They also need the source language toolchain for the service they analyze:

- Go services: `go`
- FastAPI/Flask services: `python3`
- NestJS services: `node` plus the target project's own `typescript` dev dependency

Generated Python SDKs use Pydantic v2 models by default. Consumers who need stdlib-only Python models
can configure `PySdk::new().dataclasses()` in `.gnr8/src/main.rs`.
