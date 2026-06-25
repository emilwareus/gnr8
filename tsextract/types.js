"use strict";

// TS Type -> neutral Type — the TypeScript twin of `pyextract/types.py`.
//
// The neutral vocabulary is byte-fixed by `crates/gnr8-core/src/analyze/facts.rs`
// (`Type` / `Prim`) and the mapping table by the committed nestjs snapshot:
//
//   * `number`  -> {"type":"primitive","of":{"prim":"float","bits":64}}  (NEVER int — Pitfall 2)
//   * `string`  -> {"prim":"string"};  `boolean` -> {"prim":"bool"}
//   * `T[]`     -> {"type":"array","of":map(T)}
//   * `field?: T` / `T | undefined`   -> strip the undefined arm, set `optional`
//   * `T | null`                      -> strip the null arm, set `nullable`
//   * `field?: T | null`              -> strip BOTH, leaves a SINGLE type (NOT a union)
//   * `A | B` (no null/undefined, object arms) -> {"type":"union","of":[...]}  SOURCE order
//   * a class type, OR a type-alias used as the SOLE type (its aliasSymbol survives
//     on the full type) -> {"type":"named","of":"<id>"} + register the schema
//   * a residual bare string-literal union (aliasSymbol LOST because `| null`/`| undefined`
//     was mixed in) -> {"type":"enum","of":sorted([...])} (INLINE)
//   * an unresolvable type -> (null, diagnostic) — never an `any` fact as a guess (rule 3)
//
// THE named-vs-inline predicate (Open Question 1, pinned empirically against the
// snapshot — RESEARCH Pitfall 4): capture `aliasSymbol` from the FULL type BEFORE
// stripping null/undefined. `format: BookFormat` -> the full type carries
// `aliasSymbol = BookFormat` (no null/undefined to drop it) -> named ref. `sort?:
// SortOrder | null` -> TS synthesizes `SortOrder | null | undefined`, a fresh union
// whose `aliasSymbol` is gone -> after stripping, the residual literal union has no
// aliasSymbol -> inline enum. This is ONE discriminator, ONE path (rule 3).
//
// Every fact derives from the SOURCE's own TS types via the TypeChecker (rule 1):
// no third-party schema-annotation / validation library on the target is ever read.

const path = require("path");
const ts = require("typescript");

const load = require("./load");

function _prim(prim, extra) {
  return { type: "primitive", of: Object.assign({ prim: prim }, extra || {}) };
}

// Split a type into its union arms (or a singleton list for a non-union).
function _arms(t) {
  return t.isUnion && t.isUnion() ? t.types.slice() : [t];
}

// Resolve the declaration file + name of a symbol, or null.
function _declOf(sym) {
  if (!sym) return null;
  const decls = sym.getDeclarations ? sym.getDeclarations() : sym.declarations;
  const decl = decls && decls[0];
  if (!decl) return null;
  return { decl: decl, file: decl.getSourceFile().fileName, name: sym.getName() };
}

// Return the type-alias Symbol named by a declaration's SYNTACTIC type
// annotation, or null. Only a bare `TypeReference` (e.g. `format: BookFormat`,
// `fmt?: BookFormat`) whose target symbol is a type-alias counts; a union /
// primitive / array / inline annotation returns null (it is mapped from the
// resolved residual instead). This is the single discriminator for named-vs-inline
// (it survives `?` / `| null` because it reads what the author wrote, not the
// resolved type whose aliasSymbol TS drops once a null/undefined arm is present).
function _annotationAliasSymbol(node, checker) {
  const anno = node.type;
  if (!anno || !ts.isTypeReferenceNode(anno)) {
    return null;
  }
  let sym = checker.getSymbolAtLocation(anno.typeName);
  if (!sym) {
    return null;
  }
  // A type-alias used through an `import { X }` resolves to an alias symbol; follow
  // it to the underlying declaration's symbol.
  if (sym.flags & ts.SymbolFlags.Alias && checker.getAliasedSymbol) {
    sym = checker.getAliasedSymbol(sym);
  }
  const decls = sym.getDeclarations ? sym.getDeclarations() : sym.declarations;
  const decl = decls && decls[0];
  if (decl && ts.isTypeAliasDeclaration(decl)) {
    return sym;
  }
  return null;
}

// Map the resolved TS Type of `node` to a neutral Type, splitting out the
// optional/nullable axes. Returns `{ schema, optional, nullable }` where `schema`
// is the neutral Type dict, or `null` (with a diagnostic recorded) when the type
// is unresolvable (rule 3). `registry`, when provided, accumulates referenced
// schema-bearing declarations for transitive collection (see schemas.js).
function mapType(loaded, node, diags, registry) {
  const checker = loaded.checker;
  const full = checker.getTypeAtLocation(node);
  const sf = node.getSourceFile();
  const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
  const file = load.relFile(loaded.targetDir, sf.fileName);

  // THE named-vs-inline discriminator (Open Question 1), derived from the SINGLE
  // source that is reliable across every optional/nullable combination: the
  // SYNTACTIC annotation node the author wrote (rule 3, one path). When the
  // annotation is a bare `TypeReference` to a type-alias (`format: BookFormat`,
  // `fmt?: BookFormat`), the author named a type -> a named ref + its schema. When
  // the annotation is a union expression (`sort?: SortOrder | null`), the alias is
  // a member of a union the author wrote inline -> after stripping null/undefined
  // the residual literal union inlines as an enum. The resolved `aliasSymbol` is
  // NOT usable here: TS drops it whenever `| null`/`| undefined` is mixed in, so
  // `fmt?: BookFormat` (which MUST be a named ref) would lose it — only the
  // annotation node distinguishes `fmt?` (TypeReference) from `sort?` (UnionType).
  const fullAlias = _annotationAliasSymbol(node, checker);

  // Strip the optional (undefined) and nullable (null) arms to compute the axes.
  let optional = false;
  let nullable = false;
  const residual = _arms(full).filter((arm) => {
    if (arm.flags & ts.TypeFlags.Undefined) {
      optional = true;
      return false;
    }
    if (arm.flags & ts.TypeFlags.Null) {
      nullable = true;
      return false;
    }
    return true;
  });

  const schema = _mapResidual(
    loaded,
    residual,
    fullAlias,
    diags,
    registry,
    file,
    line
  );
  if (schema === null) {
    return { schema: null, optional, nullable };
  }
  return { schema, optional, nullable };
}

// Map the residual (post-strip) arm list to a neutral Type. `fullAlias` is the
// aliasSymbol of the pre-strip full type (null if none/dropped).
function _mapResidual(loaded, residual, fullAlias, diags, registry, file, line) {
  const checker = loaded.checker;

  // A surviving alias on the full type used as the sole type (no null/undefined
  // was mixed in to drop it): a NAMED ref (e.g. `format: BookFormat`). Register
  // the alias so its schema is emitted.
  if (fullAlias && residual.length >= 1) {
    const id = _registerAlias(loaded, fullAlias, diags, registry, file, line);
    if (id !== null) {
      return { type: "named", of: id };
    }
    // The alias is not schema-bearing here; fall through to map the residual.
  }

  // TS models `boolean` as the synthetic union `true | false` (two
  // BooleanLiteral arms). Collapse that residual to a single `bool` BEFORE the
  // generic union handling so `inStock?: boolean` is one primitive, not a union.
  if (
    residual.length >= 1 &&
    residual.every((a) => a.flags & ts.TypeFlags.BooleanLiteral)
  ) {
    return _prim("bool");
  }

  if (residual.length === 1) {
    return _mapSingle(loaded, residual[0], diags, registry, file, line);
  }

  if (residual.length > 1) {
    // A residual union. String-literal arms -> inline enum (sorted); object arms
    // -> union of mapped members (SOURCE order). Mixed/other -> diagnostic.
    const allStringLiterals = residual.every(
      (a) => a.flags & ts.TypeFlags.StringLiteral
    );
    if (allStringLiterals) {
      const members = residual.map((a) => a.value).sort();
      return { type: "enum", of: members };
    }
    const members = [];
    for (const arm of residual) {
      const mapped = _mapSingle(loaded, arm, diags, registry, file, line);
      if (mapped === null) {
        return null; // rule 3: an unresolvable arm omits the whole fact
      }
      members.push(mapped);
    }
    return { type: "union", of: members };
  }

  // Zero residual arms (e.g. a bare `null`/`undefined` type) — not a fact.
  diags.warn(
    "unsupported type: no non-null/undefined arms; fact omitted (no fallback)",
    file,
    line
  );
  return null;
}

// Map a single (non-union, post-strip) TS type to a neutral Type.
function _mapSingle(loaded, t, diags, registry, file, line) {
  const checker = loaded.checker;
  const flags = t.flags;

  // Primitives.
  if (flags & ts.TypeFlags.Number) {
    return _prim("float", { bits: 64 }); // TS number is IEEE double (Pitfall 2)
  }
  if (flags & ts.TypeFlags.String) {
    return _prim("string");
  }
  if (
    flags & ts.TypeFlags.Boolean ||
    flags & ts.TypeFlags.BooleanLiteral
  ) {
    return _prim("bool");
  }

  // Arrays: `T[]` — map the element type.
  if (checker.isArrayType && checker.isArrayType(t)) {
    const elemType = checker.getTypeArguments
      ? checker.getTypeArguments(t)[0]
      : t.typeArguments && t.typeArguments[0];
    if (!elemType) {
      diags.warn(
        "unsupported array type: element type unresolved; fact omitted (no fallback)",
        file,
        line
      );
      return null;
    }
    const elem = _mapSingle(loaded, elemType, diags, registry, file, line);
    if (elem === null) {
      return null;
    }
    return { type: "array", of: elem };
  }

  // A class/interface object type -> a named ref + register the class schema.
  if (flags & ts.TypeFlags.Object && t.symbol) {
    const id = _registerClass(loaded, t.symbol, diags, registry, file, line);
    if (id !== null) {
      return { type: "named", of: id };
    }
  }

  // A bare string-literal singleton (degenerate; an alias would have been caught
  // earlier). A single literal is still a (one-member) inline enum.
  if (flags & ts.TypeFlags.StringLiteral) {
    return { type: "enum", of: [t.value] };
  }

  diags.warn(
    "unsupported type '" +
      checker.typeToString(t) +
      "': not a primitive/array/class/enum; fact omitted (no fallback)",
    file,
    line
  );
  return null;
}

// Register (for transitive collection) the schema for an aliasSymbol used as a
// named ref, returning its schema id, or null if the alias is not schema-bearing
// here (e.g. it resolves to something unmappable). Only aliases that resolve to a
// string-literal union (enum schema) or an object union (union schema) are
// schema-bearing; a bare re-binding is not.
function _registerAlias(loaded, aliasSym, diags, registry, _file, _line) {
  const info = _declOf(aliasSym);
  if (!info || !ts.isTypeAliasDeclaration(info.decl)) {
    return null;
  }
  const id = load.schemaId(loaded.targetDir, info.file, info.name);
  if (registry) {
    registry.add(id, { kind: "alias", decl: info.decl, file: info.file, name: info.name });
  }
  return id;
}

// Register the schema for a class symbol used as a named ref, returning its
// schema id, or null if the symbol has no class declaration.
function _registerClass(loaded, sym, diags, registry, _file, _line) {
  const info = _declOf(sym);
  if (!info) {
    return null;
  }
  if (!ts.isClassDeclaration(info.decl) && !ts.isInterfaceDeclaration(info.decl)) {
    return null;
  }
  const id = load.schemaId(loaded.targetDir, info.file, info.name);
  if (registry) {
    registry.add(id, { kind: "class", decl: info.decl, file: info.file, name: info.name });
  }
  return id;
}

module.exports = { mapType, _declOf, _arms };
