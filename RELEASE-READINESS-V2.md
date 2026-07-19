# gnr8 release-readiness assessment v2

**Assessed:** 2026-07-19  
**Branch:** `release-readiness`  
**Pull request:** https://github.com/emilwareus/gnr8/pull/41  
**Verdict:** ready for a constrained early-access release only after the PR's complete release gate is
green; not yet ready for broad or drop-in migration marketing.

## Executive assessment

The audited P0 defects have been addressed. gnr8 now has an honest and coherent initial contract:
it owns the source-to-SDK chain, accepts bounded commodity dependencies, emits standard-library-only
SDKs, rejects missing or ambiguous facts instead of silently recovering, and documents the actual
Rust and source-language toolchain requirements. Active product code, tests, generated examples, and
current documentation no longer contain compatibility behavior for another generator.

That is enough to market an early-access product for typed, statically discoverable Go/Gin,
Python/FastAPI or Flask, and TypeScript/NestJS services. It is not enough to promise general framework
understanding, arbitrary OpenAPI migration, behavioral parity with mature generators, or a
self-contained/runtime-free install.

The release decision remains conditional because this workspace has no Go toolchain. The final local
`make check` completed formatting and clippy, then stopped with 358 passing Rust library tests and four
`GoToolchainMissing` failures. Go-backed extraction, generated Go compilation/runtime behavior,
example regeneration, and the full gate need to pass in PR CI or on a Go-equipped clean machine. A
skipped or unavailable suite is not positive evidence.

## What is now release credible

- Public claims distinguish ownership of the chain from dependency count and state the real build and
  source-toolchain prerequisites.
- All active coupling to OpenAPI Generator profiles, scanners, config files, package conventions,
  compatibility tests, and migration guidance has been removed.
- Helper/resource/core-dependency resolution and missing response facts use one declared source and
  fail with diagnostics instead of entering environment-dependent recovery chains.
- Static FastAPI router, Flask blueprint, and NestJS controller/registration prefixes are preserved;
  dynamic ambiguity is diagnosed.
- Common FastAPI async/list return signatures and dependency injection, plus NestJS Promise/list
  responses, have regression coverage.
- Go SDK generation preserves `float64` and nullable string JSON semantics in committed runtime tests.
- `doctor` treats extraction errors as failures and unknown Go handlers or missing response facts emit
  actionable diagnostics.
- TypeScript scalar-array query parameters use repeated keys; unsupported structured query encodings
  fail before emitting a misleading client.
- The release workflow validates the release commit before atomically pushing `main` and its tag, and
  an unpacked archive smoke covers `init` → `generate` → `doctor` → `check` in an unrelated project.

## Remaining adoption blockers

These are not reasons to undo the P0 work. They define the boundary between an honest early-access
release and a broad market launch.

1. **OpenAPI import fidelity (P1.1).** Resolve referenced request bodies/responses; retain parameter
   serialization, docs, deprecation, examples, tags, default/range responses, and reject lossy media
   collapse explicitly. Validate against a real anonymized Swagger 2.0/OpenAPI 3.0/3.1 corpus and
   generated consumers.
2. **Framework parameter fidelity (P1.2).** Model headers, cookies, aliases, defaults, explicit
   query/path/body markers, and serialization metadata end to end. Framework recognition must account
   for imported symbols rather than matching decorator/constructor names alone.
3. **Status and error extraction (P1.3).** Extract common multiple-success and typed-error declarations
   such as FastAPI `responses=` and Flask returned status tuples, or issue blocking diagnostics that
   require an explicit `.gnr8` transform.
4. **TypeScript response decoding (P1.4).** Unify success/error body reading so empty or malformed JSON
   and content-type mismatches produce stable SDK errors with raw response context instead of leaking
   transport-specific exceptions.
5. **Clean-machine release proof (remaining P1.5).** The archive now contains a valid staged workspace
   and has a host smoke test, but every release platform still needs an unpacked, low-cache test plus a
   generated consumer compile/run and a clear prerequisite preflight.
6. **Behavioral parity tests (remaining P1.6).** The new regressions cover the audited shapes, but the
   three SDKs still need live HTTP tests for arrays, auth, retries, hooks, empty/malformed bodies,
   errors, and media types. Release reporting must distinguish executed from toolchain-skipped tests.
7. **Issue-tracker reconciliation (P1.7).** Audit issues #16, #17, #18, and #23 against current tests;
   close fixed portions and split or reprioritize residual gaps so public issue state matches the code.

Streaming/SSE ergonomics, watch-mode performance budgets, richer user extension artifacts, and
archiving stale planning documents remain useful P2 work, but they do not block the constrained
positioning below.

## Go-to-market boundary

Safe positioning after PR #41 is green:

> gnr8 turns typed, statically discoverable API source in Go, Python, and TypeScript into deterministic
> OpenAPI 3.1 and standard-library-only client SDKs. Its project-local Rust pipeline makes naming,
> authentication, errors, packaging, and layout customizable in code, while unsupported or ambiguous
> source patterns fail with diagnostics instead of guessed contracts.

Do not claim that gnr8 is a drop-in replacement for another generator, has no open-source
dependencies, installs as a single self-contained binary, needs no runtime/toolchain, understands
arbitrary framework code, or safely migrates arbitrary OpenAPI documents.

## Release decision

1. Resolve the branch against current `main` without reintroducing removed compatibility behavior.
2. Require PR #41's complete release dry-run and every Go-backed test to pass without skips.
3. Review the produced archives and published prerequisite wording once more.
4. If those checks pass, ship as an early-access release with the constrained positioning above.
5. Keep broad launch and migration claims blocked on P1.1–P1.6 evidence, with P1.7 completed before
   actively directing prospective users to the public repository.
