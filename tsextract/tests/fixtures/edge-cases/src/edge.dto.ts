// Edge-case DTO shapes the multi-language acceptance fixture (nestjs-bookstore)
// deliberately does NOT exercise, used to LOCK the rule-3 behavior fixed in the
// 04 code-review pass (CR-01/CR-02/CR-03/CR-04). Each shape must produce either a
// correct deterministic neutral mapping OR a diagnose-and-omit — never a dangling
// $ref, never a node_modules-path id, never a guessed wire key.

// CR-01: a type-alias to a non-(string-literal-union / object-union) shape is NOT
// schema-bearing. It must be mapped INLINE (here: to the primitive), never left as
// a dangling `named` ref with no registered schema.
export type AliasToPrim = string;

// CR-01: a numeric/mixed-literal union alias is NOT a buildable schema shape
// (the host enum is string-only). It must diagnose + omit, never a dangling ref.
export type NumStatus = 1 | 2 | 3;

export class AliasCases {
  prim: AliasToPrim; // -> inline string primitive (no named ref)
  status: NumStatus; // -> diagnosed + omitted (no dangling ref)
  keep: string; // a survivor field, proving omission is per-field
}

// CR-02 + CR-03: a `Record<string, T>` resolves through the global `Record` alias
// declared in the TS lib (outside the target tree). It must map to the neutral
// `map` type, NEVER to a schema id derived from the absolute node_modules path
// (which would be a dangling ref AND machine-dependent, non-deterministic bytes).
export class MapCases {
  meta: Record<string, string>; // -> {type:map, key:string, value:string}
  counts: Record<string, number>; // -> {type:map, key:string, value:float64}
}

// CR-04: property names that are not plain identifiers. A quoted/numeric name
// yields its UNQUOTED wire key; a computed name whose value is not a static
// literal cannot be resolved and must diagnose + omit (never the raw source text).
declare const DYN_KEY: string;
export class NameCases {
  ["quoted-name"]: string; // -> wire key "quoted-name" (unquoted)
  123: number; // -> wire key "123"
  [DYN_KEY]: string; // -> diagnosed + omitted (NOT "[DYN_KEY]")
  plain: string; // a survivor field
}
