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
- `gnr8-macos-aarch64.tar.gz`
- `gnr8-windows-x86_64.tar.gz`

Each archive also gets a matching `.sha256` file.
Intel macOS has no prebuilt archive because its GitHub-hosted build cannot reliably finish within the
five-minute job deadline; Intel Mac users build the CLI from a source checkout
(`cargo build --release -p gnr8-cli`). `cargo install gnr8` is not an option: the published `gnr8`
crate is the thin code-as-config SDK and ships no binary.

## Archive Layout

Each archive contains:

- `bin/gnr8`
- `share/gnr8/crates/gnr8-sdk` (the thin SDK an offline `.gnr8` can point a `path` dependency at)
- `share/gnr8/crates/gnr8` (the CLI crate source)
- `share/gnr8/goextract`
- `share/gnr8/pyextract`
- `share/gnr8/tsextract`

`crates/gnr8-core` — the host engine — is deliberately **not** shipped: it is already compiled into
`bin/gnr8`, and a project's `.gnr8` crate must never be able to depend on it.

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
2. Move the release's entries from `## Unreleased` into `## X.Y.Z — YYYY-MM-DD`, leaving
   `## Unreleased` empty. Commit that changelog update to `main`. Check it locally with the bump you
   intend to select:

   ```bash
   VERSION="$(python3 scripts/bump-workspace-version.py --dry-run)"
   python3 scripts/release-notes.py check "$VERSION"
   ```

   Add `--minor` to the first command when preparing a minor release.
3. Open **Actions → Release → Run workflow** on `main`.
4. Choose `bump`. **Use `minor` whenever the release removes or changes public Rust API.** Cargo
   treats `0.x.y → 0.x.(y+1)` as a *compatible* upgrade, so a patch bump would hand a breaking
   change to every downstream `gnr8 = "0.1"` without warning. `CHANGELOG.md` records which releases
   were breaking; check it before choosing.
5. Leave `publish_crates=true` to publish exactly one crates.io package: `gnr8`.
6. Leave `publish_cli=true` to upload the CLI archives.
   CLI publication requires `publish_crates=true`: generated projects pin the exact release version,
   so the workflow waits for crates.io publication to succeed before it creates the GitHub Release.
7. The workflow computes and stages the version bump, verifies that `Unreleased` is empty and its
   dated changelog section exists before any commit or tag, refreshes the root and example lockfiles,
   compile-checks that version-only commit, and pushes it with `vX.Y.Z`. The required focused CI jobs
   on `main` provide the behavioral gate; the dry-run workflow has already checked packaging and the
   unpacked archive. Publishing and platform asset jobs then run independently with the same
   five-minute deadline. The GitHub Release body keeps the installation instructions and is rendered
   from the same dated changelog section.

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

## Go Toolchain Policy

A Go version appears in four places in this repository, and they mean four different things. They are
bumped on different schedules, for different reasons, and conflating them is how a routine version
bump turns into a breaking change.

| Surface | Where | What it means | Bump when |
|---|---|---|---|
| **CI + dev toolchain** | `go-version:` in `.github/workflows/ci.yml` and `generated-sdk-check.yml` | Which compiler runs our own gates | Every stable Go release |
| **`goextract` floor** | `go` directive in `goextract/go.mod` | **The minimum Go a gnr8 user must have to analyze a Go service** | Only when goextract needs a newer language or stdlib feature |
| **Generated-SDK floor** | `GoSdk::go_version` default (`crates/gnr8-core/src/sdk/builtins.rs`) | The minimum Go a *consumer of a generated SDK* must have | Almost never — deliberately conservative |
| **Examples + fixtures** | `go` directive in `examples/*/go.mod`, `fixtures/*/go.mod` | Demo/test modules only; no user-facing meaning | Opportunistically, or never |

### Why the `goextract` floor is user-facing

`scripts/package-release.sh` ships `goextract/` to users **as source**, and `analyze::helper` compiles
it with **the user's own `go`** (one cached `go build`, keyed on the source and the toolchain). The
`go` directive in `goextract/go.mod` is therefore gnr8's published minimum supported Go version, not a
developer convenience.

That build is pinned to the toolchain the *analyzed* module selects, not the one `goextract/go.mod`
would pick on its own: `go/packages` runs `go list` inside the target, so a service declaring a newer
Go than the machine's `PATH` carries is type-checked by that newer release, and a `go/types` older
than the code it reads rejects every file gated on it. The pin preserves the caller's `GOTOOLCHAIN`
mode: `auto` may raise the build to the floor above, `path` can raise it only from `PATH`, and `local`
or an exact selection remains fixed.

Raising that directive forces every user onto that release or triggers a toolchain download on their
machine, and hard-fails outright under `GOTOOLCHAIN=local` — which is a configuration real users run,
and which our own `crates/gnr8-core/tests/sdk_lint.rs` depends on. Treat a bump to this directive as a
**breaking change**: it needs a CHANGELOG entry and a note in "Required User Toolchains" above.

The pin is checked rather than assumed. `goextract` reports the toolchain it was compiled with in
its facts document, and the driver refuses those facts when the analyzed module selects a newer
language version — a helper behind the module cannot type-check it, and `go/packages` reports the
whole module as per-package load errors rather than failing. Refusing there keeps a degraded
extraction out of the graph, the cache, and every generated document. The check is one-directional:
a helper AHEAD of the module is fine, because Go is forward compatible.

The asymmetry that makes this cheap to get right: Go is forward compatible. A module declaring
`go 1.26` builds correctly under a 1.27 toolchain, so **CI can lead the floor indefinitely**. There is
no reason to move the floor just because a new Go shipped.

### The action defaults to `stable`

`action.yml` sets `go-version: "stable"`, so consumers of the gnr8 GitHub Action build `goextract` with
whatever Go is current on the day their workflow runs. Our CI pin should track the latest stable
release so that our gates are not testing an *older* Go than our users are given. When these two drift,
CI is the thing that is wrong.

### Bump checklist (CI/dev toolchain)

1. Update `go-version:` in `.github/workflows/ci.yml` (both jobs) and `generated-sdk-check.yml`.
2. Update the "present on dev + CI (go X.Y)" comments in `crates/gnr8-core/tests/{determinism,
   sdk_compile,snapshot_sdk}.rs` and `crates/gnr8/src/render.rs`. Leave comments that describe a
   written `go.mod` floor (e.g. `sdk_compile.rs`'s hermetic `go 1.26` module) alone — those are floors,
   not toolchains.
3. Run `make check` on the new toolchain. `fixture-build`, `goextract-build`, and `examples-check` all
   shell out to the local `go`, so a local run is a genuine rehearsal of CI. It needs `PyYAML`, which
   CI installs as its own step but `make` does not — without it `lower::yaml`'s round-trip test
   hard-fails on a missing module rather than skipping.
4. Leave `goextract/go.mod`, the `GoSdk` default, and `docs/demo.md` untouched. `demo.md` records the
   toolchain a captured demo run actually used; it changes only when the demo is re-captured.
