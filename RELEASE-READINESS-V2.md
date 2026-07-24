# gnr8 release-readiness assessment v2

**Assessed:** 2026-07-23
**Branch:** `release-readiness`
**Pull request:** https://github.com/emilwareus/gnr8/pull/41
**Verdict:** locally ready to merge and ship as a constrained early-access release; the hosted PR
checks must still be green before merge. Not yet ready for broad or drop-in migration marketing.

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

The complete local release gate is green. `make check` passed formatting, strict clippy, 413 Rust
library tests, 42 CLI tests, all Rust integration and snapshot tests, the Go fixture and extractor
build/vet/test suites, 109 Python extractor tests, all TypeScript extractor tests, action-version
tests, and forced drift checks for every example. The only ignored test is the intentionally opt-in,
timing-dependent watch smoke; deterministic watch tests remain blocking. `release-local-check.sh`
then repeated that gate, built and unpacked the macOS arm64 archive, passed
`init` → `generate` → `doctor` → `check` in an unrelated project, and completed
`cargo publish --dry-run`. Hosted CI remains the final merge authority because it exercises the
repository's declared runner matrix and release workflow.

## What is now release credible

- Public claims distinguish ownership of the chain from dependency count and state the real build and
  source-toolchain prerequisites.
- All active coupling to OpenAPI Generator profiles, scanners, config files, package conventions,
  compatibility tests, and migration guidance has been removed.
- Helper/resource/core-dependency resolution and missing response facts use one declared source and
  fail with diagnostics instead of entering environment-dependent recovery chains.
- Static FastAPI router, Flask blueprint, and NestJS controller/registration prefixes are preserved;
  dynamic ambiguity is diagnosed.
- Common FastAPI async/list return signatures, intentional `-> None` responses, and dependency
  injection, plus NestJS Promise/list responses, have regression coverage.
- Go SDK generation preserves `float64` and nullable string JSON semantics in committed runtime tests.
- Empty Go operation sets emit compilable packages without unused imports, and generated examples
  carry the corrected nullable pointer types.
- `doctor` treats extraction errors as failures and unknown Go handlers or missing response facts emit
  actionable diagnostics.
- Forced generation cannot be hidden by the verified no-op cache, and Go extractor source changes
  invalidate both host and source-graph caches.
- TypeScript scalar-array query parameters use repeated keys, required headers/cookies stay out of the
  URL, and `allowReserved` behavior has a generated-client runtime test. Unsupported structured query
  encodings fail before emitting a misleading client.
- The host supplies a complete compatibility handshake before the child begins extraction or
  generation.
- Release version commits contain refreshed example lockfiles, and the full gate must leave the tested
  commit clean before packaging, tagging, or pushing.
- The release workflow validates the release commit before atomically pushing `main` and its tag, and
  an unpacked archive smoke covers `init` → `generate` → `doctor` → `check` in an unrelated project.

## Remaining adoption blockers

These are not reasons to undo the P0 work. They define the boundary between an honest early-access
release and a broad market launch.

1. **OpenAPI import fidelity (remaining P1.1).** Current `main` adds referenced request/response,
   parameter serialization, operation docs/examples/tags, and multi-media preservation. Default/range
   responses, explicit rejection of every lossy collapse, a real anonymized Swagger 2.0/OpenAPI
   3.0/3.1 corpus, and generated-consumer proof still remain.
2. **Framework parameter fidelity (remaining P1.2).** OpenAPI-imported headers, cookies,
   style/explode, aliases, and preserved fields are now modeled. Native framework extraction still
   needs broader header/cookie/alias/default/marker coverage, and framework recognition must account
   for imported symbols rather than matching decorator/constructor names alone.
3. **Status and error extraction (P1.3).** Extract common multiple-success and typed-error declarations
   such as FastAPI `responses=` and Flask returned status tuples, or issue blocking diagnostics that
   require an explicit `.gnr8` transform.
4. **TypeScript response decoding (P1.4).** Unify success/error body reading so empty or malformed JSON
   and content-type mismatches produce stable SDK errors with raw response context instead of leaking
   transport-specific exceptions.
5. **Clean-machine release proof (remaining P1.5).** The macOS arm64 archive has a local unpacked host
   smoke test, but every release platform still needs an unpacked, low-cache test plus a generated
   consumer compile/run and a clear prerequisite preflight.
6. **Behavioral parity tests (remaining P1.6).** Live HTTP tests now cover core auth, retries, hooks,
   pagination, media requests, and Go/Python round trips. Broader cross-SDK coverage for arrays,
   malformed or empty bodies, typed errors, and content-type mismatches remains before parity claims.
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

1. Keep PR #41 blocked until its hosted checks reproduce the green local release gate.
2. Review the hosted release artifacts and published prerequisite wording once more.
3. When those checks pass, merge and ship as an early-access release with the constrained positioning
   above.
4. Keep broad launch and migration claims blocked on P1.1–P1.6 evidence, with P1.7 completed before
   actively directing prospective users to the public repository.
