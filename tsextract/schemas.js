"use strict";

// Schema builder: DTO class / type-alias -> neutral SchemaFact — the TypeScript
// twin of `pyextract/schemas.py`. It emits one SchemaFact per:
//
//   * a class -> an `object` body of FieldFacts carrying the four optional/nullable
//     axes (each property's type mapped via types.mapType);
//   * a string-literal-union alias used as a named ref (e.g. `BookFormat`) -> an
//     `enum` body of SORTED member values;
//   * an object-union alias (e.g. `BookOrError = BookDto | OutOfStockDto`) -> a
//     `union` body of named refs in SOURCE order.
//
// A string-literal-union alias is NEVER a standalone schema unless it is used as a
// named ref somewhere (the snapshot has a BookFormat schema because `format` refs
// it, but no SortOrder schema because `sort` only ever inlines it). This mirrors
// pyextract: only schema-BEARING, REFERENCED types are emitted.
//
// TRANSITIVE collection (RESEARCH Pattern 4): from every seed root, follow named
// refs through field types AND union arms to a fixpoint, so a type reachable only
// via a union arm (OutOfStockDto, via BookOrError) is still emitted. The seed
// roots for THIS wave are the DIRECT exported DTO classes/aliases (routes, which
// provide the real roots, land in 04-03).
//
// A SchemaFact has keys `id, name, body, span`. A FieldFact has EXACTLY
// `json_name, required, optional, nullable, schema, description, example` —
// `description`/`example` always null. `required = !optional` (nullable does NOT
// affect required — RESEARCH Pitfall 3).

const ts = require("typescript");

const load = require("./load");
const types = require("./types");

// A fixpoint registry of schema-bearing declarations keyed by stable id. `add`
// enqueues a declaration discovered as a named ref; `drain` yields each pending
// entry exactly once until no new ones appear.
class Registry {
  constructor() {
    this._byId = new Map();
    this._pending = [];
  }

  add(id, entry) {
    if (this._byId.has(id)) {
      return;
    }
    this._byId.set(id, entry);
    this._pending.push(Object.assign({ id: id }, entry));
  }

  hasPending() {
    return this._pending.length > 0;
  }

  drain() {
    const out = this._pending;
    this._pending = [];
    return out;
  }
}

// Build the full sorted-by-id list of SchemaFact objects for a loaded program.
function buildSchemas(loaded, diags) {
  const registry = new Registry();

  // Seed roots: every exported DTO class + every union-bearing alias declared in
  // the target. (Direct roots this wave; routes seed in 04-03.)
  _seedRoots(loaded, registry);

  const facts = [];
  const built = new Set();
  // Process to a fixpoint: building a schema may register further refs.
  while (registry.hasPending()) {
    for (const entry of registry.drain()) {
      if (built.has(entry.id)) {
        continue;
      }
      built.add(entry.id);
      const fact = _buildSchema(loaded, entry, diags, registry);
      if (fact !== null) {
        facts.push(fact);
      }
    }
  }

  facts.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  return facts;
}

// Seed every exported class + union-bearing alias as a collection root.
function _seedRoots(loaded, registry) {
  for (const sf of loaded.program.getSourceFiles()) {
    if (sf.isDeclarationFile) {
      continue;
    }
    // Only files under the target (exclude the TS lib + node_modules).
    if (!_underTarget(loaded, sf.fileName)) {
      continue;
    }
    sf.forEachChild((node) => {
      if (ts.isClassDeclaration(node) && node.name && _isDtoClass(node)) {
        const id = load.schemaId(loaded.targetDir, sf.fileName, node.name.text);
        registry.add(id, {
          kind: "class",
          decl: node,
          file: sf.fileName,
          name: node.name.text,
        });
      } else if (ts.isTypeAliasDeclaration(node) && node.name) {
        // Only an object-union alias is a standalone root (a string-literal-union
        // alias only becomes a schema if REFERENCED as a named ref, which the
        // type mapper registers). This mirrors pyextract _build_alias_schema.
        if (_isObjectUnionAlias(loaded, node)) {
          const id = load.schemaId(loaded.targetDir, sf.fileName, node.name.text);
          registry.add(id, {
            kind: "alias",
            decl: node,
            file: sf.fileName,
            name: node.name.text,
          });
        }
      }
    });
  }
}

// Whether a class is a DTO (data) class eligible to be a DIRECT seed root: it
// carries NO class-level decorator and declares NO methods (a routing controller
// carries `@Controller` + method handlers, so it is excluded — it is recognized
// for ROUTING facts in 04-03, never as a data schema). This gate applies ONLY to
// direct-root seeding; a class REFERENCED by a DTO field/union arm is registered
// as schema-bearing through the type mapper regardless, since a referenced type
// genuinely needs its schema emitted.
function _isDtoClass(classDecl) {
  const decorators =
    ts.canHaveDecorators && ts.canHaveDecorators(classDecl)
      ? ts.getDecorators(classDecl) || []
      : [];
  if (decorators.length > 0) {
    return false;
  }
  const hasMethod = classDecl.members.some((m) => ts.isMethodDeclaration(m));
  return !hasMethod;
}

function _underTarget(loaded, fileName) {
  const rel = load.relFile(loaded.targetDir, fileName);
  return !rel.startsWith("..") && !rel.includes("node_modules");
}

// Whether a type-alias resolves to a union whose arms are object (class) types
// (e.g. `BookOrError = BookDto | OutOfStockDto`). A string-literal-union alias
// (BookFormat/SortOrder) is NOT an object-union and is excluded here.
function _isObjectUnionAlias(loaded, aliasDecl) {
  const checker = loaded.checker;
  const t = checker.getTypeAtLocation(aliasDecl);
  if (!(t.isUnion && t.isUnion())) {
    return false;
  }
  const arms = t.types;
  if (arms.length < 2) {
    return false;
  }
  return arms.every(
    (a) =>
      a.flags & ts.TypeFlags.Object &&
      a.symbol &&
      (ts.isClassDeclaration(_firstDecl(a.symbol)) ||
        ts.isInterfaceDeclaration(_firstDecl(a.symbol)))
  );
}

function _firstDecl(sym) {
  const decls = sym.getDeclarations ? sym.getDeclarations() : sym.declarations;
  return (decls && decls[0]) || {};
}

// Build one SchemaFact from a registry entry, or null if not schema-bearing.
function _buildSchema(loaded, entry, diags, registry) {
  if (entry.kind === "class") {
    return _buildClassSchema(loaded, entry, diags, registry);
  }
  if (entry.kind === "alias") {
    return _buildAliasSchema(loaded, entry, diags, registry);
  }
  return null;
}

function _buildClassSchema(loaded, entry, diags, registry) {
  const classDecl = entry.decl;
  const sf = classDecl.getSourceFile();
  const fields = [];
  for (const member of classDecl.members) {
    if (!ts.isPropertyDeclaration(member) || !member.name) {
      continue;
    }
    const jsonName = member.name.getText(sf);
    const mapped = types.mapType(loaded, member, diags, registry);
    if (mapped.schema === null) {
      continue; // rule 3: unresolvable field omitted (diagnostic already recorded)
    }
    fields.push({
      json_name: jsonName,
      required: !mapped.optional,
      optional: mapped.optional,
      nullable: mapped.nullable,
      schema: mapped.schema,
      description: null,
      example: null,
    });
  }
  return {
    id: entry.id,
    name: entry.name,
    body: { type: "object", of: fields },
    span: load.span(sf, classDecl, loaded.targetDir),
  };
}

function _buildAliasSchema(loaded, entry, diags, registry) {
  const aliasDecl = entry.decl;
  const sf = aliasDecl.getSourceFile();
  const checker = loaded.checker;
  const t = checker.getTypeAtLocation(aliasDecl);
  const arms = t.isUnion && t.isUnion() ? t.types : [t];

  // String-literal-union alias (referenced as a named ref) -> enum body, sorted.
  const allStringLiterals =
    arms.length >= 1 && arms.every((a) => a.flags & ts.TypeFlags.StringLiteral);
  if (allStringLiterals) {
    const members = arms.map((a) => a.value).sort();
    return {
      id: entry.id,
      name: entry.name,
      body: { type: "enum", of: members },
      span: load.span(sf, aliasDecl, loaded.targetDir),
    };
  }

  // Object-union alias -> union body of named refs (SOURCE order), registering
  // each arm's class for transitive collection.
  const members = [];
  for (const arm of arms) {
    const sym = arm.symbol;
    const decls = sym && (sym.getDeclarations ? sym.getDeclarations() : sym.declarations);
    const decl = decls && decls[0];
    if (!decl || (!ts.isClassDeclaration(decl) && !ts.isInterfaceDeclaration(decl))) {
      diags.warn(
        "unsupported union-alias arm in '" +
          entry.name +
          "': not a named class; fact omitted (no fallback)",
        load.relFile(loaded.targetDir, sf.fileName),
        sf.getLineAndCharacterOfPosition(aliasDecl.getStart(sf)).line + 1
      );
      return null;
    }
    const armSf = decl.getSourceFile();
    const id = load.schemaId(loaded.targetDir, armSf.fileName, sym.getName());
    registry.add(id, {
      kind: "class",
      decl: decl,
      file: armSf.fileName,
      name: sym.getName(),
    });
    members.push({ type: "named", of: id });
  }
  return {
    id: entry.id,
    name: entry.name,
    body: { type: "union", of: members },
    span: load.span(sf, aliasDecl, loaded.targetDir),
  };
}

module.exports = { buildSchemas, Registry };
