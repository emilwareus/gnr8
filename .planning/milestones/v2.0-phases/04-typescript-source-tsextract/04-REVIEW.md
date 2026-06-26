---
phase: 04-typescript-source-tsextract
reviewed: 2026-06-25T00:00:00Z
depth: standard
files_reviewed: 12
files_reviewed_list:
  - tsextract/index.js
  - tsextract/load.js
  - tsextract/types.js
  - tsextract/schemas.js
  - tsextract/routes.js
  - tsextract/facts.js
  - tsextract/diagnostics.js
  - crates/gnr8-core/src/analyze/mod.rs
  - crates/gnr8-core/src/analyze/helper.rs
  - crates/gnr8-core/src/diagnostics/mod.rs
  - crates/gnr8-core/src/error.rs
  - crates/gnr8-core/src/sdk/builtins.rs
findings:
  critical: 4
  warning: 5
  info: 2
  total: 11
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-06-25
**Depth:** standard
**Files Reviewed:** 12
**Status:** issues_found

## Summary

The tsextract sidecar is well-architected against the CLAUDE.md invariants on the
axes the nestjs-bookstore fixture exercises: its sole npm dependency is `typescript`
(rule-2 carve-out honored — verified `package.json` + every `require`), it never
executes the target (`ts.createProgram` + `noEmit`, no `require`/`eval`/`vm`/
`child_process` of target modules), it reads only `@nestjs/common` routing
decorators (never swagger/zod/class-validator), and the Rust seam mirrors the
pyextract typed-error discipline with a single deterministic 3-arm language
classification (no `_ =>`, no fallback). The fixture round-trips and the output is
sorted/deterministic.

The defects are **latent extraction bugs on type shapes the single fixture does not
exercise** — exactly the risk the review priorities flagged. The central one: the
named-vs-inline discriminator (`_annotationAliasSymbol` + `_registerAlias`) treats
ANY type-alias annotation as a schema-bearing named ref, but the schema builder can
only BUILD string-literal-union and object-union aliases. Every other alias shape
(alias-to-primitive, numeric/mixed-literal union, mapped type / `Record<>`, generic
instantiation) becomes a `{"type":"named","of":<id>}` with **no corresponding
schema emitted** — a dangling `$ref` that hard-fails the host's lowering
(`CoreError::Lowering`), turning what should be a clean rule-3 diagnostic into a
pipeline crash. A second strand of the same gap: the transitive registration paths
(`_registerAlias`/`_registerClass`/`routes::_responseRef`) never apply the
`_underTarget` filter, so a field typed as a stdlib/node_modules type (`Record<>`,
a lib type) registers a schema id derived from an **absolute node_modules path** —
both a dangling ref AND a machine-dependent string that breaks byte-determinism.

All four CRITICAL findings are reproducible with a few-line DTO and were confirmed
by running the sidecar.

## Critical Issues

### CR-01: Type-alias to a non-union shape emits a dangling `$ref` (no schema), crashing host lowering

**File:** `tsextract/types.js:147-153, 267-277` (and `tsextract/schemas.js:230-283`)
**Issue:** `_annotationAliasSymbol` returns the alias symbol for ANY `TypeReference`
whose target is a `TypeAliasDeclaration`, and `_registerAlias` then registers it
**unconditionally** and returns a named-ref id. But `_buildAliasSchema` only knows
how to build two alias shapes: an all-string-literal union (→ `enum`) and an
all-object union (→ `union`). For every other alias shape it emits a WARN and
returns `null` — so **the field keeps a `named` ref to an id that is never added to
`schemas`**. The host's `lower::resolve_ref` treats a `ref_id` not among the schemas
as a fatal `CoreError::Lowering { "dangling $ref ..." }` (lower/mod.rs:495-505), so
a single such field crashes the entire generation instead of producing the clean
rule-3 "fact omitted" the diagnostic promises.

The `_registerAlias` doc comment even *claims* the guard exists ("Only aliases that
resolve to a string-literal union ... or an object union ... are schema-bearing; a
bare re-binding is not") — but the code does not implement it.

Reproduced (sidecar run):
```ts
export type AliasToPrim = string;       // -> P.y named:AliasToPrim, NO schema -> dangling
export type Status = 1 | 2 | 3;         // -> Has.s named:Status,    NO schema -> dangling
export class P { y: AliasToPrim; }
export class Has { s: Status; }
```
Both emit `{"type":"named","of":"...AliasToPrim"}` / `...Status` while `schemas`
contains no such id.

**Fix:** Make registration agree with what the builder can produce — classify the
alias's resolved shape ONCE before registering, and when it is not a buildable
schema shape, do NOT return a named ref. Emit the diagnostic and map the residual
inline (alias-to-primitive → the primitive; numeric/mixed union → enum-or-diag),
never leave a dangling ref:
```js
function _registerAlias(loaded, aliasSym, diags, registry, file, line) {
  const info = _declOf(aliasSym);
  if (!info || !ts.isTypeAliasDeclaration(info.decl)) return null;
  // Only schema-bearing alias shapes (string-literal union OR all-object union)
  // become named refs — mirror _buildAliasSchema's two accepted shapes exactly.
  if (!_isSchemaBearingAlias(loaded, info.decl)) {
    return null; // caller falls through to map the residual inline (no dangling ref)
  }
  const id = load.schemaId(loaded.targetDir, info.file, info.name);
  if (registry) registry.add(id, { kind: "alias", decl: info.decl, file: info.file, name: info.name });
  return id;
}
```
where `_isSchemaBearingAlias` reuses the same predicate `_buildAliasSchema`/
`schemas::_isObjectUnionAlias` apply. (One source of truth for "is this alias a
schema" — rule 3.)

### CR-02: Referenced stdlib / node_modules type registers an absolute-path schema id — dangling ref AND non-deterministic output

**File:** `tsextract/types.js:267-294` (`_registerAlias`/`_registerClass`), `tsextract/routes.js:187-241` (`_responseRef`)
**Issue:** `_seedRoots` (schemas.js:107) and `recognizeNestController` (routes.js:340)
both gate on `_underTarget`, but the **transitive** registration paths do not. A DTO
field (or method return) typed as a type that resolves into `node_modules`/the TS
lib — e.g. `Record<string, string>` (a global alias in `lib.es5.d.ts`) — flows
through `_annotationAliasSymbol` → `_registerAlias`, which builds a schema id from
the declaration's `getSourceFile().fileName` with **no under-target check**.

Reproduced (sidecar run on `meta: Record<string, string>`):
```json
{"type":"named","of":"../../home/vercel-sandbox/workspace/tsextract/node_modules/typescript/lib/lib.es5.d.Record"}
```
Three failures at once: (1) dangling `$ref` (no `Record` schema is buildable) →
`CoreError::Lowering` crash; (2) the schema id embeds a **machine-absolute path**,
so identical source produces different bytes on different machines — a direct
violation of the byte-determinism contract this sidecar exists to uphold; (3) it
reaches OUTSIDE the target tree, violating the "the target's source only" boundary
the loader and seeders otherwise enforce.

**Fix:** Apply the `_underTarget` gate at EVERY registration site, not just the
seed-root scan. A type whose declaration is outside the target is not a target
schema — emit a diagnostic and omit the fact (rule 3). Lift `_underTarget` into a
shared module (load.js) and call it in `_registerAlias`, `_registerClass`, and
`_responseRef` before computing any id:
```js
if (!load.underTarget(loaded, info.file)) {
  diags.warn("referenced type '" + info.name + "' is outside the target tree; fact omitted (no fallback)", file, line);
  return null;
}
```

### CR-03: `Record<string,T>` / index-signature types are misclassified as named refs instead of the neutral `map` type

**File:** `tsextract/types.js:200-260` (`_mapSingle`)
**Issue:** The neutral vocabulary has `Type::Map { key, value }` (facts.rs:155-161),
but `_mapSingle` has no map/index-signature branch. A `Record<string, T>` resolves
via its alias to a named ref (CR-02) or, if the alias were stripped, would fall
through to the final "unsupported type" diagnostic — either way the genuine `map`
fact is lost. goextract emits map facts (`map[string]any` → `additionalProperties`),
so the TS sidecar silently under-extracts a representable shape, and via CR-02 it
actively corrupts the graph.

**Fix:** Add an index-type/`Record` branch to `_mapSingle` BEFORE the object/class
branch, mapping `key`/`value` through `_mapSingle` into `{type:"map", of:{key,value}}`.
Detect via `checker.getIndexInfoOfType(t, ts.IndexKind.String)` (string-keyed map)
and recurse on the value type. A non-string key index is a diagnostic (rule 3),
never a guess.

### CR-04: Computed / non-identifier property names emit raw source text as `json_name` (guessed fact)

**File:** `tsextract/schemas.js:207` (`member.name.getText(sf)`)
**Issue:** `jsonName = member.name.getText(sf)` blindly stringifies the property-name
AST node. For a computed name the emitted `json_name` is the literal source
expression, not the wire key; for a quoted name it keeps the quotes/brackets.

Reproduced (sidecar run):
```ts
export class W { ['quoted-name']: string; 123: number; [KEY]: string; }
// -> json_name: "['quoted-name']", "123", "[KEY]"
```
`"[KEY]"` and `"['quoted-name']"` are wrong wire keys (the real keys are `dyn` and
`quoted-name`). A computed name whose value isn't a static literal cannot be
resolved statically and MUST be a diagnostic (rule 3), never a guessed
`"[KEY]"` fact that silently mis-generates the schema.

**Fix:** Branch on the name node kind: identifier → its text; string-literal /
numeric-literal → its `.text` (unquoted); computed property whose expression is a
string/numeric literal → that literal's value; any other computed name → emit a
WARN and skip the field (rule 3 — no guessed name). Example:
```js
const nameNode = member.name;
let jsonName;
if (ts.isIdentifier(nameNode) || ts.isStringLiteralLike(nameNode) || ts.isNumericLiteral(nameNode)) {
  jsonName = nameNode.text;
} else if (ts.isComputedPropertyName(nameNode) && ts.isStringLiteralLike(nameNode.expression)) {
  jsonName = nameNode.expression.text;
} else {
  diags.warn("computed property name cannot be statically resolved; field omitted (no fallback)", file, line);
  continue;
}
```

## Warnings

### WR-01: `_responseRef` uses a SECOND, divergent named-ref discriminator (`t.aliasSymbol`) instead of the shared annotation-node path

**File:** `tsextract/routes.js:199-233`
**Issue:** Every other named-ref decision routes through `_annotationAliasSymbol`
(the documented single discriminator, types.js:62), but `_responseRef` reads the
resolved `t.aliasSymbol` directly. types.js's own header explains *why* the resolved
aliasSymbol is unreliable (TS drops it whenever `| null`/`| undefined` is mixed in).
A return type like `getX(): BookOrError | null` therefore loses its aliasSymbol and
falls through to the object-branch (or the "unsupported response" diagnostic),
diverging from how the identical type maps as a field/param. This is the dual-path
the design explicitly forbids (rule 3) and a latent correctness gap for nullable
return types.

**Fix:** Route the response through the same `mapType(loaded, methodDecl, ...)` (the
method's `type` annotation node is available) and accept its named/union result,
exactly as `_buildParams` does for `@Body`. One discriminator, one path.

### WR-02: Array (and union) return types silently drop the response body

**File:** `tsextract/routes.js:187-241`
**Issue:** `_responseRef` only recognizes a class/interface object type or an
alias-union; a method returning `BookDto[]` (array) or an inline `A | B` union (no
alias) emits "unsupported response type" and omits the body entirely. Array-returning
list endpoints are extremely common, so the sidecar would under-extract most real
controllers. (Routing through `mapType` per WR-01 would also fix this, since
`mapType` handles arrays/unions.)

**Fix:** Resolve the response via `mapType` so array/union/named shapes are all
representable; the `ResponseFact.body` is a `TypeRef` (ref_id only), so an array
response still needs a representation decision — at minimum emit a diagnostic that
distinguishes "array response not yet representable as a TypeRef" from "unresolvable
type", rather than collapsing both to one message.

### WR-03: A method with multiple HTTP-verb decorators silently drops the extra route(s)

**File:** `tsextract/routes.js:137-150` (`_verbDecorator`)
**Issue:** "The FIRST verb decorator wins." NestJS does allow a handler to carry
several verb decorators (e.g. `@Get()` + `@Post()`), each registering a distinct
route. The current loop returns only the first and silently discards the rest with
no diagnostic — a real route vanishes from the graph with no signal.

**Fix:** Either iterate ALL verb decorators on the method and emit one RouteFact per
verb, or, if single-verb is an intentional restriction, emit a WARN when a second
verb decorator is present so the dropped route is not silent (rule 3).

### WR-04: `@Body()` only honors the first body, drops a duplicate without a diagnostic

**File:** `tsextract/routes.js:258-276`
**Issue:** When two parameters carry `@Body()`, the `if (requestBody === null)` guard
keeps the first and silently ignores the second. NestJS would 500 at runtime on a
genuinely conflicting body, but more importantly the silent drop hides a malformed
handler. A second `@Body()` should be a diagnostic, not a silent first-wins.

**Fix:** When `requestBody !== null` and another `@Body` is seen, emit a WARN naming
the handler (rule 3 — surface the ambiguity, do not silently pick).

### WR-05: `_decoratorNumberArg` accepts a negative `@HttpCode` as a valid status

**File:** `tsextract/routes.js:76-90`
**Issue:** `Number.isInteger(n)` is true for negatives, so `@HttpCode(-1)` would be
emitted as `status: -1`. The host `ResponseFact.status` is a `u16` (facts.rs:87), so
a negative value fails deserialization with `CoreError::FactsParse` — a confusing
crash far from the cause, rather than a clean diagnostic at extraction time.

**Fix:** Constrain to a plausible HTTP status range and diagnose otherwise:
```js
if (Number.isInteger(n) && n >= 100 && n <= 599) return n;
// else: emit a WARN and ignore the override (fall back to the method-derived status is a fallback — instead omit/diag)
```
(Note: per rule 3 an out-of-range override should be a diagnostic, not silently
swapped for the method default.)

## Info

### IN-01: Unused `path` import and dead `_arms` export in types.js

**File:** `tsextract/types.js:32` (`const path = require("path")`), `:296`
**Issue:** `path` is required but never used in types.js (only `load`/`ts` are).
`_arms` is exported (`module.exports = { mapType, _declOf, _arms }`) but no other
module imports it (only `_declOf` is used, by routes.js). Dead surface.

**Fix:** Remove the unused `path` require and drop `_arms` from the exports (keep it
module-local) unless a test needs it.

### IN-02: `_controllerPrefix` computed then discarded

**File:** `tsextract/routes.js:354`
**Issue:** `const _controllerPrefix = _decoratorStringArg(controller);` is assigned
and never read. The comment explains it documents the bright line, but an
underscore-prefixed unused local is dead code; the intent is already covered by the
surrounding comment.

**Fix:** Drop the assignment (keep the comment), or actually thread the prefix into
provenance if a downstream consumer needs it.

---

_Reviewed: 2026-06-25_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
