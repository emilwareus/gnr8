---
phase: 05-typescript-target-tssdk
reviewed: 2026-06-26T00:00:00Z
depth: standard
files_reviewed: 5
files_reviewed_list:
  - crates/gnr8-core/src/tssdk/mod.rs
  - crates/gnr8-core/src/tssdk/emit.rs
  - crates/gnr8-core/src/tssdk/bundle.rs
  - crates/gnr8-core/src/sdk/builtins.rs
  - crates/gnr8-core/tests/tssdk_compile.rs
findings:
  critical: 2
  warning: 5
  info: 4
  total: 11
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-06-26
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

Reviewed the new IR→TypeScript SDK emitter (`tssdk/{mod,emit,bundle}.rs`), the `TsSdk`
target addition in `sdk/builtins.rs`, and the hermetic `tsc --noEmit` acceptance test.
The implementation is a faithful structural twin of the Go/Python SDK targets, the
`ts_type` match is genuinely exhaustive (no `_ =>` catch-all, all neutral `Type` variants
handled — rule 3 satisfied), package name and base_path flow from the single source of
truth (`sdk_package` / `ir.base_path`, no re-derivation), determinism is preserved (fixed
file order, graph-sorted iteration, no `HashMap`/`HashSet` walks), and there are no
production `unwrap`/`expect`/`panic` and no third-party imports in either the Rust code or
the emitted SDK. CLAUDE.md rules 1–4 hold.

However, the codegen has **two latent correctness defects on shapes the single
`nestjs-bookstore` fixture does not exercise** — exactly the failure class the prompt
flagged as load-bearing. Both produce invalid or wrong TypeScript that the hermetic `tsc`
gate cannot catch because its one fixture carries only identifier-safe keys and
identifier-safe enum members. The first (unescaped string-literal-union members) can even
silently corrupt the API contract. There are also several robustness warnings around
identifier sanitization, non-string query-param coercion, and the error-body decode path.

## Critical Issues

### CR-01: Enum / string-literal-union members are emitted unescaped — invalid TS or silent contract corruption

**File:** `crates/gnr8-core/src/tssdk/emit.rs:146-150` (inline enum in `ts_type`),
`crates/gnr8-core/src/tssdk/emit.rs:267-275` (`emit_enum_alias`)

**Issue:** Both the inline-enum arm and the named-enum alias wrap each member with a naive
`format!("\"{m}\"")`, interpolating the raw wire string directly into a double-quoted TS
string literal with **no escaping**. The doc comment at line 264-266 explicitly asserts
"there are no member identifiers to sanitize," but that conflates *identifier* escaping
(genuinely not needed) with *string-literal* escaping (very much needed). Any enum member
whose wire value contains a `"`, `\`, newline, or other control character produces broken
or wrong output:

- A member containing a double quote, e.g. wire value `a"b`, emits
  `export type X = "a"b";` — a **syntax error** that fails `tsc`.
- A member containing a backslash, e.g. `a\b`, emits `"a\b"` where `\b` is the TS
  backspace escape — the literal type silently becomes `"a<BS>b"`, so the SDK's compile-time
  contract **no longer matches the wire value**. This is a data-correctness defect, not just
  a compile error, and it passes `tsc` cleanly.

Enum values flow from arbitrary source-language string constants (Go `const`, Python enum
values, TS string-literal union members) and are NOT constrained to identifier-safe ASCII —
JSON enum values routinely contain hyphens, slashes, dots, spaces, and occasionally quotes.
The Go/Python twins do not hit this because Go emits typed constants and Python escapes via
its string machinery; the TS target is the only one interpolating raw into a quoted literal.
The `nestjs-bookstore` fixture only has `"hardcover"`/`"paperback"`, so the `tsc` gate is
blind to it.

**Fix:** Escape each member for a TS double-quoted string literal before interpolation
(at minimum `\` and `"`, plus newline/CR/control chars). Add a small helper and use it in
both sites:

```rust
/// Escape a wire string for a TypeScript double-quoted string literal.
fn ts_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
```
Then `members.iter().map(|m| ts_string_literal(m))` in both `ts_type` (line 147-149) and
`emit_enum_alias` (line 273). Add a regression test with an enum member containing `"` and
`\`.

### CR-02: Object field property names are emitted unquoted — invalid TS for non-identifier wire keys

**File:** `crates/gnr8-core/src/tssdk/emit.rs:296-303` (`emit_interface`)

**Issue:** `emit_interface` writes each field as `  {json_name}{opt}: {hint};` with the raw
`json_name` placed directly as an interface member name. The inline comment at line 297-299
acknowledges this and bets on "the bookstore fixtures only carry identifier-safe keys" —
but that is a property of the one fixture, not of the IR. `json_name` is the **wire key**
and can legitimately be any JSON string: `kebab-case` headers/fields, dotted keys, keys with
spaces, keys starting with a digit, or empty. For any such key the emitter produces invalid
TypeScript:

- wire key `content-type` → `  content-type?: string;` — `tsc` parse error (subtraction in
  a member position).
- wire key `123abc` → `  123abc: number;` — invalid member name.
- wire key `user name` → `  user name: string;` — invalid.

TS *does* allow these as quoted member names (`"content-type"?: string;`), so the fix is to
quote any key that is not a plain identifier. The comment even states the correct fact ("TS
accepts any string literal as a member") but then declines to act on it. Because every wire
key in the fixture is identifier-safe, the `tsc` gate cannot catch this class.

**Fix:** Quote the property name unless it is a valid bare identifier (and escape it via the
CR-01 helper when quoting):

```rust
let key = if is_ident(&field.json_name) {
    field.json_name.clone()
} else {
    ts_string_literal(&field.json_name) // quoted + escaped
};
writeln!(out, "  {key}{opt}: {hint};", ).map_err(sink)?;
```
where `is_ident` checks: non-empty, first char `A-Za-z_$`, rest `A-Za-z0-9_$`. Add a
regression test with a `kebab-case` and a leading-digit `json_name`.

## Warnings

### WR-01: `camel` produces colliding/invalid identifiers for non-ASCII or empty param names; collision check only catches a subset

**File:** `crates/gnr8-core/src/tssdk/emit.rs:74-89` (`camel`), used by `resolve_op_args`
(`emit.rs:518-531`)

**Issue:** `camel` lower/upper-cases via `to_ascii_lowercase`/`to_ascii_uppercase` and never
sanitizes. Two distinct failure modes on shapes the fixture lacks:
1. A param name that is empty after `split_words` (e.g. name `"_"` or `"-"`) yields an empty
   `camel("")` → emits `: T` with no identifier → invalid TS. `resolve_op_args` would not
   flag a single such param.
2. A param name with non-ASCII letters (e.g. `"naïve"`) is left as-is by the ASCII casing —
   technically a valid TS identifier, but path-token matching in `emit_operation`
   (lines 575-586) compares the *raw* `p.name` against path tokens while the *emitted* arg
   uses `camel(p.name)`; for a name that camelizes to something different, the path-token
   interpolation in `emit_op_path` (line 676-680) re-derives via `camel(token)` only when the
   zip lookup fails, masking mismatches inconsistently.

The `RESERVED_ARGS = ["body"]` / collision pass (lines 494-531) catches camelCase *clashes*
between params but not empty-identifier or TS-reserved-word param names (e.g. a param literally
named `delete` or `class` becomes a valid-looking but reserved identifier — TS allows reserved
words as parameter names in non-strict positions but it is fragile).

**Fix:** Reject (typed `SdkGen` error) or sanitize an empty `camel` result, and consider
extending the reserved set beyond `body` to the TS reserved words that are illegal as binding
names. At minimum, error when `camel(&p.name).is_empty()`.

### WR-02: Non-string query params are stringified with `String(value)`, losing/ misformatting booleans and risking `[object Object]`

**File:** `crates/gnr8-core/src/tssdk/emit.rs:705,709` (`emit_op_query`)

**Issue:** Every query param is serialized as `query.set("k", String(ident))` regardless of
its declared schema type. For a primitive `boolean`/`number` this is acceptable, but the param
type emitted into the signature is the *full* `ts_type` (lines 619-625), which can be an array
(`string[]`), a `Record<string, V>` (map), a named model, or a union. For an array param,
`String(["a","b"])` yields `"a,b"` (likely not the intended repeated-key encoding); for an
object/map/named-model param, `String(value)` yields `"[object Object]"`. The signature
advertises a rich type the serializer cannot faithfully encode.

**Fix:** Either constrain query params to scalar primitives at the schema level (typed error
for a non-scalar query param), or special-case array params to append each element
(`for (const v of arr) query.append("k", String(v))`). Today the mismatch silently produces a
wrong query string for any non-scalar query param.

### WR-03: `success_of` picks the first 2xx in graph order, not the lowest status — comment says "lowest 2xx" but code does not sort

**File:** `crates/gnr8-core/src/tssdk/emit.rs:372-407` (`success_of`)

**Issue:** The doc comment (line 372) says "primary success (lowest 2xx)" but the loop returns
the **first** 2xx response encountered in `op.responses` iteration order, not the numerically
lowest. If `op.responses` is not guaranteed sorted by status (the graph sorts schemas/fields,
but response ordering for a single operation should be verified), an operation declaring both
`202` and `200` in that source order would emit `if (res.status !== 202)` and treat a real
`200` as an error. This drives both the runtime status comparison (`emit_op_dispatch` line 744)
and the return type, so a wrong pick is a real behavioral bug, not cosmetic.

**Fix:** If responses are not already status-sorted upstream, select `min` by status:
`op.responses.iter().filter(|r| (200..300).contains(&r.status)).min_by_key(|r| r.status)`.
Otherwise, fix the comment and add a test asserting responses arrive status-sorted. The twin
`pysdk::emit::success_of` should be cross-checked for the same wording-vs-behavior gap.

### WR-04: Error-body decode `await res.json().catch(() => null)` swallows a non-Promise throw and mistypes the body

**File:** `crates/gnr8-core/src/tssdk/emit.rs:745-749` (`emit_op_dispatch`)

**Issue:** On a non-success status the SDK throws
`new ApiError(res.status, await res.json().catch(() => null))`. `res.json()` rejects on a
non-JSON / empty error body, and `.catch(() => null)` handles that — good. But the success
path (line 752) is `return (await res.json()) as models.{model}` with **no** `.catch`, so a
malformed success body throws a raw `SyntaxError` (not an `ApiError`), giving callers an
inconsistent error contract (some failures are `ApiError`, some are bare `SyntaxError`). A
204/empty-body success likewise throws. This is an asymmetry between the error and success
decode paths.

**Fix:** Be consistent: for a `void`/no-model success do not call `res.json()` at all (already
the case — good), but for a typed success consider whether an empty/invalid body should also
surface as a typed `ApiError` rather than a raw parse throw, or document that the success body
is trusted. At minimum note the asymmetry; ideally wrap the success parse so callers get one
error type.

### WR-05: `emit_index` re-exports model names without escaping/validation; collides with the CR-02 class

**File:** `crates/gnr8-core/src/tssdk/emit.rs:760-777` (`emit_index`)

**Issue:** `emit_index` re-exports every schema `name` bare inside `export type { ... }`.
Schema names are validated for *uniqueness* in `emit_models` (lines 219-231), but `emit_index`
runs independently and does not share that check, and neither validates that a schema `name` is
a legal TS identifier. A schema whose `name` is not identifier-safe (e.g. contains a hyphen
from an unsanitized source type name) emits `export type {\n  Foo-Bar,\n} from "./models";` —
invalid TS — and the same bad name would also have produced an invalid `export interface
Foo-Bar` in `models.ts`. The two passes can also drift: `emit_models` returns `Err` on a
duplicate name, but `emit_index` (called at `mod.rs:64-67`, *before* `emit_models` at line 71)
cannot fail and would happily emit a duplicate re-export if the order were ever changed.

**Fix:** Validate/sanitize schema names to legal TS identifiers at the single point they become
symbols (ideally in the graph or a shared helper), so `models.ts`, `client.ts`, and `index.ts`
all agree. Reject a non-identifier schema name with a typed `SdkGen` error.

## Info

### IN-01: `package`/`_package` parameter threaded through emitters but never used

**File:** `crates/gnr8-core/src/tssdk/emit.rs:211` (`emit_models`), `:311` (`emit_errors`),
`:337` (`emit_client`), `:464` (`emit_operations`), `:760` (`emit_index`)

**Issue:** Every emitter takes a `_package` it never reads (the TS files carry no package
clause). The doc comments justify this as "kept for call-site symmetry with the twin." It is
dead parameter surface that invites confusion (a reader expects the package name to influence
output). Acceptable for twin-parity, but worth a note.

**Fix:** Either drop the unused params or keep the explicit `_`-prefix + doc note (already
done) — no action required if twin-parity is the deliberate choice.

### IN-02: `emit_op_path` dead fallback branch (`map_or_else(|| camel(token), ...)`)

**File:** `crates/gnr8-core/src/tssdk/emit.rs:676-680`

**Issue:** The token-to-ident lookup falls back to `camel(token)` when the zip find fails — but
`emit_operation` already enforces (lines 575-586) that the token set exactly equals the path-param
set, so the `find` can never miss by the time `emit_op_path` runs. The fallback arm is unreachable
dead code that, if it ever did fire, would silently paper over the very invariant the set-equality
check protects (a soft second control path, mild tension with rule 3).

**Fix:** Replace the `map_or_else` fallback with a hard error or `expect`-free typed error so a
future invariant break surfaces instead of silently re-deriving via `camel`.

### IN-03: `bundle::parse` silently drops any text before the first marker

**File:** `crates/gnr8-core/src/tssdk/bundle.rs:73-91`

**Issue:** `parse` ignores any leading lines before the first marker (`current` is `None` until
the first marker). The doc says "there is none in practice," which is true for `generate` output,
but `write_to_dir` accepts an arbitrary bundle string (it is `pub`), so a malformed bundle could
lose content silently rather than erroring. Low risk given program-controlled input.

**Fix:** Optionally error if `parse` sees non-blank content before the first marker, or document
that `write_to_dir` trusts `generate`-shaped input only.

### IN-04: `tssdk_compile.rs` reuses the `GoBuild` error variant for tsc failures

**File:** `crates/gnr8-core/tests/tssdk_compile.rs:121-124,225`

**Issue:** A `tsc` non-zero exit maps to `CoreError::GoBuild { code, stderr }` — deliberately
reused per the plan's interfaces note (no new variant), but the name is now a misnomer in a
TypeScript context and the test pattern-matches `GoBuild` for a TS typecheck failure. Harmless
(test-only), but the variant name will mislead future readers.

**Fix:** Consider renaming the variant to a language-neutral `SubprocessBuild`/`ToolExit` carrier
in a later pass, or document the reuse at the variant definition.

---

_Reviewed: 2026-06-26_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
