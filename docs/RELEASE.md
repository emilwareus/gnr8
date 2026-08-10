# Release

The release process is intentionally shaped like `exlint`:

- `Release dry-run` runs on pull requests and pushes to `main`. Separate jobs exercise crates.io
  packaging, per-platform CLI archives, and an unpacked Linux archive.
- `Release` is a manual `workflow_dispatch` from a green `main`: prepare and compile-check the
  version-only commit, push `main` and `vX.Y.Z`, optionally publish the public `gnr8` crate, and upload
  CLI archives/checksums.

Every GitHub Actions job has a hard five-minute deadline. The repository policy check rejects a
workflow that omits that deadline or reintroduces a monolithic full-suite command.

## Local Dry Run

```bash
./scripts/release-local-check.sh
```

This runs `make check`, builds a host archive, writes a `.sha256`, unpacks it outside the checkout,
and exercises `init`, clean Go/Python/TypeScript generation, SDK compilation, `doctor`, and `check`
against a static FastAPI fixture. It then runs `DRY_RUN=1 ./scripts/publish-crates.sh`.

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
- `share/gnr8/crates/gnr8` (keeps the staged Cargo workspace structurally complete)
- `share/gnr8/goextract`
- `share/gnr8/pyextract`
- `share/gnr8/tsextract`

The `share/gnr8` tree is required because source extraction shells out to the Go/Python/TypeScript
sidecars. `gnr8` discovers this tree from the archive layout — `share/gnr8` beside the real
executable, resolved through any install symlink — or from `GNR8_RESOURCE_DIR`. Exactly one location
is selected and then validated; an incomplete tree is an error naming that location, never a silent
search elsewhere.

A packaged `gnr8 init` scaffolds `.gnr8` with a `gnr8 = "=<version>"` pin so the generated
`Cargo.toml` is portable across machines, which is why `publish_crates` must succeed for a version
before users of that archive can build their generator crate. Only in-repo builds scaffold a path
dependency.

The CLI and engine use focused open-source dependencies for commodity concerns such as serialization,
CLI parsing, and file watching. gnr8 owns the source-to-OpenAPI-to-SDK pipeline itself; generated SDKs
remain standard-library-only.

## GitHub Release

1. Make sure `main` is green and contains only the intended release changes.
2. Open **Actions → Release → Run workflow** on `main`.
3. Choose `bump`. **Use `minor` whenever the release removes or changes public Rust API.** Cargo
   treats `0.x.y → 0.x.(y+1)` as a *compatible* upgrade, so a patch bump would hand a breaking
   change to every downstream `gnr8 = "0.1"` without warning. `CHANGELOG.md` records which releases
   were breaking; check it before choosing.
4. Leave `publish_crates=true` to publish exactly one crates.io package: `gnr8`.
5. Leave `publish_cli=true` to upload the CLI archives.
6. The workflow bumps the version, refreshes the root and example lockfiles, compile-checks that
   version-only commit, and pushes it with `vX.Y.Z`. The required focused CI jobs on `main` provide the
   behavioral gate; the dry-run workflow has already checked packaging and the unpacked archive.
   Publishing and platform asset jobs then run independently with the same five-minute deadline.

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
The first build also needs crates.io access unless Cargo already has every transitive Rust dependency
cached; the release archive does not vendor the Rust registry.
They also need the source language toolchain for the service they analyze:

- Go services: `go`
- FastAPI/Flask services: `python3`
- NestJS services: `node` plus the target project's own `typescript` dev dependency

Generated Python SDKs use Pydantic v2 models by default. Consumers who need stdlib-only Python models
can configure `PySdk::new().dataclasses()` in `.gnr8/src/main.rs`.

The extractor contract is static and deliberately bounded. Dynamic route prefixes/paths, unresolved
handlers or response shapes, and types without a declared wire representation are diagnosed or fail
explicitly. See [USAGE.md](USAGE.md) for the current per-frontend envelope.
