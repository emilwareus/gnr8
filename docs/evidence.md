# Release-candidate evidence

**Captured:** 2026-07-19

**Scope:** release-readiness remediation branch

**Status:** evidence log, not a production-readiness declaration

gnr8 currently supports statically discoverable Go/Gin, Python FastAPI, Python Flask typed-envelope,
and TypeScript NestJS class-DTO sources. It emits OpenAPI 3.1 and standard-library HTTP clients for
Go, Python, and TypeScript. Rust/Cargo and the analyzed source language's toolchain remain required;
the first `.gnr8` build may require crates.io access.

## Remediation evidence

The release-readiness work added or strengthened these executable contracts:

- Third-party generator compatibility profiles, scanners, CLI behavior, tests, and active
  documentation were removed. gnr8 exposes only its owned native generation surface.
- Resource, workspace, helper, and OpenAPI-lowering recovery chains now return explicit diagnostics.
- Static FastAPI router/Flask blueprint and NestJS controller prefixes are preserved; dynamic or
  ambiguous prefixes are diagnosed instead of guessed.
- FastAPI async return annotations, collection responses, and dependency injection are covered by
  extractor tests.
- NestJS `Promise<T>`, `Promise<T[]>`, and direct array responses are covered by extractor tests.
- Go SDK `float64` width and nullable string JSON behavior have regression coverage.
- Doctor treats error-severity extraction diagnostics as actionable and retains the detailed child
  error. Unknown handlers, missing response facts, and Go package-load errors emit `ERROR` diagnostics.
- TypeScript scalar-array query parameters use repeated keys; structured query shapes without an
  explicit wire encoding fail generation.

## Checks run in this workspace

These checks passed during the remediation session:

| Check | Result |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| CLI unit tests, including doctor health policy | 41 PASS |
| Python extractor suite after FastAPI changes | 104 PASS |
| TypeScript extractor suites after NestJS changes | PASS |
| TypeScript emitter tests | 50 PASS |
| Generated TypeScript SDK compile gates | 5 PASS |
| Focused route-prefix snapshots and sidecar tests | PASS |
| `make check` | BLOCKED after 358 Rust library tests passed; 4 Go-dependent tests could not start without `go` |

The local environment does not contain `go` or `gofmt`. The final `make check` run completed formatting
and clippy, then stopped in `cargo test --all-features`: 358 library tests passed and four tests failed
with `GoToolchainMissing`. Go extractor/fixture tests, Go-backed snapshots, generated Go
compile/runtime tests, example regeneration, and therefore the complete gate cannot be represented as
green here. The Go runtime regression tests are committed and run when the toolchain is present;
skipped tests are not counted as local execution evidence.

## Release gate

The release workflow prepares the version commit locally, runs `make check`, packages and unpacks a
host archive, exercises `init` → `generate` → `doctor` → `check` in an unrelated FastAPI project, and
performs a crates.io dry run. Only after all of those steps pass does it atomically push `main` and the
version tag. The branch/PR dry-run workflow exercises the same local release check without publishing.

The final market-readiness verdict and remaining gaps belong in `RELEASE-READINESS-V2.md`; this page
records what was actually tested and where the current environment could not provide evidence.
