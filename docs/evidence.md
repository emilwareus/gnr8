# Release-candidate evidence

**Captured:** 2026-07-23

**Scope:** release-readiness remediation branch

**Status:** evidence log, not a production-readiness declaration

gnr8 currently supports statically discoverable Go/Gin, Python FastAPI, Python Flask typed-envelope,
and TypeScript NestJS class-DTO sources. It emits OpenAPI 3.1 and standard-library HTTP clients for
Go, Python, and TypeScript. Rust/Cargo and the analyzed source language's toolchain remain required;
the first `.gnr8` build may require crates.io access.

## Remediation evidence

The release-readiness work added or strengthened these executable contracts:

- gnr8 exposes one owned native configuration, graph, emitter, package structure, and test path.
- Resource, workspace, helper, and OpenAPI-lowering recovery chains now return explicit diagnostics.
- Static FastAPI router/Flask blueprint and NestJS controller prefixes are preserved; dynamic or
  ambiguous prefixes are diagnosed instead of guessed.
- FastAPI async return annotations, collection responses, intentional `-> None` responses, and
  dependency injection are covered by extractor tests.
- NestJS `Promise<T>`, `Promise<T[]>`, and direct array responses are covered by extractor tests.
- Go SDK `float64` width and nullable string JSON behavior have regression coverage.
- Doctor treats error-severity extraction diagnostics as actionable and retains the detailed child
  error. Unknown handlers, missing response facts, and Go package-load errors emit `ERROR` diagnostics.
- TypeScript scalar-array query parameters use repeated keys; required headers/cookies stay out of
  the URL, and `allowReserved` is verified by a generated-client runtime test. Structured query
  shapes without an explicit wire encoding fail generation.
- The host supplies the complete protocol/version/capability handshake before the child begins
  extraction or generation.

## Checks run in this workspace

These checks passed during the remediation session:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | PASS |
| Rust library unit tests | 413 PASS |
| CLI unit tests, including handshake and doctor health policy | 42 PASS |
| Rust integration, runtime, compile, and snapshot tests | PASS |
| Python extractor suite | 109 PASS |
| TypeScript extractor suites | PASS |
| Go fixture and extractor build/vet/test suites | PASS |
| Composite-action version tests | PASS |
| Forced generation plus drift checks for all five examples | PASS |
| Complete `make check` gate | PASS |

All required toolchains were available for this run. No blocking test was skipped or filtered. The
single ignored test is the intentionally opt-in, timing-dependent filesystem watch smoke;
deterministic watch-loop tests remain part of the blocking CLI suite.

## Release gate

Focused CI jobs cover formatting, linting, Rust packages, language sidecars, and each committed
example, with a hard five-minute deadline per job. The release dry-run separately packages every CLI
platform, unpacks and exercises the Linux archive through `init` → `generate` → `doctor` → `check`,
and verifies the crates.io package. The manual release refreshes lockfiles and compile-checks the
version-only commit before atomically pushing `main` and the version tag.

The final market-readiness verdict and remaining gaps belong in `RELEASE-READINESS-V2.md`; this page
records what was actually tested and where the current environment could not provide evidence.
