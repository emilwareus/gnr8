---
phase: 04-typescript-source-tsextract
fixed_at: 2026-06-25T00:00:00Z
review_path: .planning/phases/04-typescript-source-tsextract/04-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 9
skipped: 0
status: all_fixed
---

# Phase 4: Code Review Fix Report

**Fixed at:** 2026-06-25
**Source review:** .planning/phases/04-typescript-source-tsextract/04-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 9 (4 critical + 5 warning; the 2 Info findings are out of the
  `critical_warning` scope and were not attempted)
- Fixed: 9
- Skipped: 0

**Acceptance verified after all fixes:**
- `make check` exits 0 (GREEN).
- The 6 multi-language acceptance snapshots stay GREEN with ZERO `.snap`/expected
  edits (FastAPI/Flask/NestJS x graph/openapi all `ok`; `git diff --name-only`
  since the phase base lists only the 4 source files + 4 new test/fixture files,
  no snapshot artifacts).
- `node tsextract/index.js fixtures/nestjs-bookstore` emits exactly 8 schemas + 4
  routes + 0 diagnostics, byte-identical across two runs.
- All 7 tsextract tests pass (`node tsextract/tests/*.test.js`), including the two
  new regression suites.
- `cargo test -p gnr8-core` green (0 failures).
- Sole npm dep remains `typescript` 5.9.3 (rule 2); no Rust crate added.
- Static-only + bright-line grep gates clean (the only grep hits are the load.js
  invariant comment and legitimate `ts.isStringLiteral*` TypeChecker usage).

## Fixed Issues

### CR-01: Type-alias to a non-union shape emits a dangling `$ref`

**Files modified:** `tsextract/types.js`, `tsextract/schemas.js`, `tsextract/load.js`
**Commit:** 8ec8cee
**Status:** fixed (requires human verification — registration/inline-mapping logic)
**Applied fix:** Introduced a SINGLE schema-bearing-alias predicate
`schemas.isSchemaBearingAlias` (string-literal-union OR object-union — exactly the
two shapes `_buildAliasSchema` can emit) and gated `_registerAlias` on it. A
non-schema-bearing alias now returns `null`, so `_mapResidual` falls through to map
the residual INLINE (alias-to-primitive -> the primitive; numeric/mixed-literal
union -> the existing union/enum/diagnose path), never a dangling `named` ref. One
source of truth shared by the registration site and the builder (rule 3). Verified
with a repro: `prim: AliasToPrim` -> inline `string`; `status: NumStatus` ->
diagnosed + omitted; no schema id minted for either alias.

### CR-02: Referenced stdlib/node_modules type registers an absolute-path schema id

**Files modified:** `tsextract/types.js`, `tsextract/schemas.js`, `tsextract/load.js`
**Commit:** 8ec8cee
**Status:** fixed
**Applied fix:** Lifted the `_underTarget` predicate into a shared
`load.underTarget(targetDir, absPath)` (replacing the inlined copy in schemas.js)
and applied it at the transitive registration sites `_registerAlias` and
`_registerClass`. A schema-bearing type whose declaration is outside the target
tree (TS lib / node_modules) is diagnosed + omitted, never minting an
absolute-path (machine-dependent, non-deterministic) schema id or a dangling ref.
Verified: no emitted schema id contains `node_modules` or starts with `..`; the
closure is byte-stable across two builds.

### CR-03: `Record<string,T>` / index-signature types misclassified

**Files modified:** `tsextract/types.js` (+ shared change in 8ec8cee)
**Commit:** 8ec8cee
**Status:** fixed
**Applied fix:** Added a string-index-signature branch to `_mapSingle` BEFORE the
class/interface branch, mapping `Record<string, T>` / index-signature objects to
the neutral `{type:"map", of:{key, value}}` (`Type::Map` in facts.rs), recursing on
the value type. A non-string (number-only) index is diagnosed + omitted (rule 3),
never guessed. Verified: `Record<string,string>` -> map(string,string);
`Record<string,number>` -> map(string,float64).

### CR-04: Computed/non-identifier property names emit raw source text

**Files modified:** `tsextract/schemas.js`
**Commit:** 81035fe
**Status:** fixed
**Applied fix:** Replaced `member.name.getText(sf)` with a `_propertyKey` helper
that branches on the name node kind: identifier / string-literal / numeric-literal
-> its UNQUOTED `.text`; a computed name whose expression is a static
string/numeric literal -> that literal's value; any other computed name -> `null`,
so `_buildClassSchema` diagnoses + omits the field (rule 3), never a guessed
`"[KEY]"` wire key. Verified: `['quoted-name']` -> `quoted-name`, `123` -> `123`,
`[DYN_KEY]` -> diagnosed + omitted.

### WR-01: `_responseRef` used a second, divergent named-ref discriminator

**Files modified:** `tsextract/types.js`, `tsextract/routes.js`
**Commit:** 7188ca1
**Status:** fixed (requires human verification — discriminator collapse)
**Applied fix:** Collapsed the dual path. Extracted the shared `_mapAnnotated` core
in types.js and added `mapReturnType(loaded, methodDecl, ...)` that resolves a
method return through the SAME annotation-node discriminator fields/params use
(reading `getTypeAtLocation(methodDecl.type)` — not the method's signature type).
`_responseRef` now calls `mapReturnType` and accepts only a `named` result as the
`TypeRef` ref_id. The divergent `t.aliasSymbol` read is gone (one discriminator,
one path). Verified: the fixture's `getBook(): BookOrError` stays byte-identical
(ref_id BookOrError); a nullable named return resolves via the single path.

### WR-02: Array (and union) return types silently dropped the response body

**Files modified:** `tsextract/types.js`, `tsextract/routes.js`
**Commit:** 7188ca1
**Status:** fixed
**Applied fix:** Since `ResponseFact.body` is a `TypeRef` (a bare ref_id), a
representable-but-not-named return shape (array/union/map/enum/primitive) now emits
a DISTINCT diagnostic naming the shape ("response type is a '<kind>', not a named
schema ...") and omits the body, separating it from the wholly-unresolvable-type
message. Verified: `Thing[]` -> distinct "array" diagnostic + body omitted.

### WR-03: Multiple HTTP-verb decorators silently dropped extra route(s)

**Files modified:** `tsextract/routes.js`
**Commit:** 72d53e3
**Status:** fixed
**Applied fix:** `_verbDecorator` keeps the first verb (one neutral route per
handler) but now emits a WARN naming each additional verb decorator, so the dropped
route is surfaced (rule 3) instead of vanishing. Verified: `@Get @Post` on one
method -> one GET route + a diagnostic naming `@Post`.

### WR-04: `@Body()` dropped a duplicate body without a diagnostic

**Files modified:** `tsextract/routes.js`
**Commit:** 72d53e3
**Status:** fixed
**Applied fix:** When a second `@Body` parameter is seen while `requestBody` is
already set, `_buildParams` now emits a WARN naming the ambiguity (first-wins is
surfaced, not silent). Verified with a two-`@Body` handler.

### WR-05: `_decoratorNumberArg` accepted out-of-range `@HttpCode`

**Files modified:** `tsextract/routes.js`
**Commit:** 72d53e3
**Status:** fixed
**Applied fix:** `_httpCodeOverride` now validates the override against the
plausible HTTP range (100-599; this also excludes negatives that
`Number.isInteger` would pass) and, when out of range, emits a WARN and returns
`null` so the deterministic method-derived status applies (the always-on default
rule, not a recovery fallback). This prevents a negative/out-of-range value
reaching the host `ResponseFact.status` u16 and crashing deserialize. Verified:
`@HttpCode(99)` / `@HttpCode(600)` -> diagnosed + ignored (status falls to the
method default); `@HttpCode(204)` honored.

## Regression Coverage Added

**Commit:** 106b460

- `tsextract/tests/edge-cases.test.js` + `tsextract/tests/fixtures/edge-cases/`:
  locks CR-01 (alias-to-primitive inline, numeric-union alias diagnosed),
  CR-02 (no node_modules/`..` schema id, no dangling refs, byte-stable closure),
  CR-03 (`Record<>` -> neutral map), CR-04 (quoted/numeric -> unquoted wire keys,
  computed non-literal diagnosed + omitted).
- `tsextract/tests/route-edges.test.js` + `tsextract/tests/fixtures/route-edges/`:
  locks WR-01 (named return via single path + nullable named diagnosed),
  WR-02 (distinct array diagnostic), WR-03 (second verb diagnosed),
  WR-04 (second @Body diagnosed), WR-05 (out-of-range @HttpCode diagnosed +
  method-default status, no out-of-range status emitted).

These fixtures are TEST-LOCAL (under `tsextract/tests/fixtures/`) and do not touch
the 6 multi-language acceptance fixtures, so the committed `.snap` files are
unaffected.

## Skipped Issues

None in scope.

**Out of scope (not attempted — `fix_scope` is `critical_warning`):**
- IN-01: unused `path` import + dead `_arms` export in types.js.
- IN-02: `_controllerPrefix` computed then discarded in routes.js.

---

_Fixed: 2026-06-25_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
