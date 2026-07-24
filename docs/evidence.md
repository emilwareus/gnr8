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

- Third-party generator compatibility profiles, scanners, CLI behavior, tests, and active
  documentation were removed. gnr8 exposes only its owned native generation surface.
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

The release workflow refreshes the root and independent example lockfiles before creating the version
commit, runs `make check`, and verifies that the tested commit remains clean. It then packages and
unpacks a host archive, exercises `init` → `generate` → `doctor` → `check` in an unrelated FastAPI
project, and performs a crates.io dry run. Only after all of those steps pass does it atomically push
`main` and the version tag. The branch/PR dry-run workflow exercises the same local release check
without publishing.

The final market-readiness verdict and remaining gaps belong in `RELEASE-READINESS-V2.md`; this page
records what was actually tested and where the current environment could not provide evidence.
