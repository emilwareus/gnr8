"use strict";

// NestJS route recognition -> neutral RouteFact dicts (the TypeScript twin of
// `pyextract/routes.py`'s recognizer).
//
// Recognition is STATIC and derived ENTIRELY from the SOURCE's own constructs
// (CLAUDE.md rule 1): a class carrying an `@Controller(...)` decorator, whose
// methods carry an HTTP-verb decorator (`@Get`/`@Post`/`@Put`/`@Patch`/`@Delete`),
// and whose parameters carry `@Param`/`@Query`/`@Body`. The ONLY decorators read
// are @nestjs/common's framework-native ROUTING decorators; nothing here ever
// reads a schema-annotation / validation-schema dialect on the class — the
// request/response/param SHAPES come from the TypeChecker over the method's typed
// signature (via types.mapType), exactly like the DTO schemas do.
//
// The `@Controller('books')` prefix is recorded for provenance ONLY and is NEVER
// folded into an operation path (rule 1): the neutral operation paths stay
// group-relative (`/`, `/{bookId}`); the base path is supplied later by the host
// (rule 4). NestJS `'/:bookId'` is converted to neutral `'/{bookId}'`.
//
// A RouteFact has EXACTLY the keys the host `RouteFact` DTO (deny_unknown_fields)
// requires: method, path, handler, operation_id, params, request_body, responses,
// span. A ParamFact has EXACTLY name, location, required, schema, span. A
// ResponseFact has EXACTLY status, body — body = {ref_id:<id>} or null.

const ts = require("typescript");

const load = require("./load");
const types = require("./types");

// The HTTP-verb method decorators -> neutral method names. Recognized by NAME
// (rule 1): these are @nestjs/common's framework-native routing decorators.
const _VERB_MAP = {
  Get: "GET",
  Post: "POST",
  Put: "PUT",
  Patch: "PATCH",
  Delete: "DELETE",
};

// The parameter decorators that bind a routing param/body (by NAME, rule 1).
const _PARAM_DECORATOR = "Param"; // -> location: path
const _QUERY_DECORATOR = "Query"; // -> location: query
const _BODY_DECORATOR = "Body"; // -> request_body (a TypeRef), NOT a param

// Return the simple callee name of a decorator (`@Get('/')` -> "Get"), or null
// for a non-call / non-identifier decorator.
function _decoratorName(decorator) {
  const expr = decorator.expression;
  if (!ts.isCallExpression(expr)) {
    return null;
  }
  const callee = expr.expression;
  if (ts.isIdentifier(callee)) {
    return callee.text;
  }
  return null;
}

// Return the first string-literal argument of a decorator call (`@Get('/')` ->
// "/", `@Param('bookId')` -> "bookId"), or null if there is none (`@Body()`).
function _decoratorStringArg(decorator) {
  const expr = decorator.expression;
  if (!ts.isCallExpression(expr)) {
    return null;
  }
  for (const arg of expr.arguments) {
    if (ts.isStringLiteralLike(arg)) {
      return arg.text;
    }
  }
  return null;
}

// Return the integer argument of a decorator call (`@HttpCode(204)` -> 204), or
// null if there is no numeric-literal argument.
function _decoratorNumberArg(decorator) {
  const expr = decorator.expression;
  if (!ts.isCallExpression(expr)) {
    return null;
  }
  for (const arg of expr.arguments) {
    if (ts.isNumericLiteral(arg)) {
      const n = Number(arg.text);
      if (Number.isInteger(n)) {
        return n;
      }
    }
  }
  return null;
}

// All decorators on a node (class/method/parameter), via the TS 4.8+/5.x helper
// (Pitfall 6 — `node.decorators` is gone; use `ts.getDecorators`).
function _decorators(node) {
  if (ts.canHaveDecorators && ts.canHaveDecorators(node)) {
    return ts.getDecorators(node) || [];
  }
  return [];
}

// Whether a class is a routing controller: it carries an `@Controller(...)`
// decorator. Recognized by NAME (rule 1).
function _controllerDecorator(classDecl) {
  for (const dec of _decorators(classDecl)) {
    if (_decoratorName(dec) === "Controller") {
      return dec;
    }
  }
  return null;
}

// Convert a NestJS route path to the neutral path: `:name` -> `{name}`. A bare
// `/` stays `/`. The `@Controller` prefix is NEVER prepended here (rule 1).
function _neutralPath(raw) {
  const out = [];
  let i = 0;
  while (i < raw.length) {
    const ch = raw[i];
    if (ch === ":") {
      // Read the param name (alphanumerics / underscore) and brace it.
      let j = i + 1;
      while (j < raw.length && /[A-Za-z0-9_]/.test(raw[j])) {
        j += 1;
      }
      out.push("{" + raw.slice(i + 1, j) + "}");
      i = j;
    } else {
      out.push(ch);
      i += 1;
    }
  }
  return out.join("");
}

// Find the HTTP-verb decorator on a method, returning {name, method, path} or
// null. The FIRST verb decorator wins (a method carries one routing verb).
function _verbDecorator(methodDecl) {
  for (const dec of _decorators(methodDecl)) {
    const name = _decoratorName(dec);
    if (name && Object.prototype.hasOwnProperty.call(_VERB_MAP, name)) {
      const raw = _decoratorStringArg(dec);
      return {
        method: _VERB_MAP[name],
        // A verb decorator with no string arg defaults to the group root `/`.
        path: _neutralPath(raw === null ? "/" : raw),
      };
    }
  }
  return null;
}

// Return the `@HttpCode(n)` override status if present on a method, else null.
// This is the SINGLE override on the method-derived rule (rule 3) — never a
// try-then-fallback chain.
function _httpCodeOverride(methodDecl) {
  for (const dec of _decorators(methodDecl)) {
    if (_decoratorName(dec) === "HttpCode") {
      return _decoratorNumberArg(dec);
    }
  }
  return null;
}

// Classify a parameter's routing decorator: returns {kind, name} where kind is
// "path" (@Param), "query" (@Query) or "body" (@Body), or null if the parameter
// carries no routing decorator. `name` is the decorator's string arg (the
// param name) or null (e.g. `@Body()`).
function _paramKind(paramDecl) {
  for (const dec of _decorators(paramDecl)) {
    const dname = _decoratorName(dec);
    if (dname === _PARAM_DECORATOR) {
      return { kind: "path", name: _decoratorStringArg(dec) };
    }
    if (dname === _QUERY_DECORATOR) {
      return { kind: "query", name: _decoratorStringArg(dec) };
    }
    if (dname === _BODY_DECORATOR) {
      return { kind: "body", name: null };
    }
  }
  return null;
}

// Resolve a method's return type to a schema ref id, registering the referenced
// declaration for transitive collection. Returns the id or null (with a
// diagnostic recorded) when the return type is not a named schema-bearing type.
function _responseRef(loaded, methodDecl, diags, registry) {
  if (!methodDecl.type) {
    return null;
  }
  const sf = methodDecl.getSourceFile();
  const file = load.relFile(loaded.targetDir, sf.fileName);
  const line = sf.getLineAndCharacterOfPosition(methodDecl.type.getStart(sf)).line + 1;
  const checker = loaded.checker;
  const t = checker.getTypeAtLocation(methodDecl.type);

  // An object-union alias (e.g. `BookOrError`) carries its aliasSymbol -> its
  // own union schema id. Register the alias for transitive collection.
  if (t.aliasSymbol) {
    const info = types._declOf(t.aliasSymbol);
    if (info && ts.isTypeAliasDeclaration(info.decl)) {
      const id = load.schemaId(loaded.targetDir, info.file, info.name);
      if (registry) {
        registry.add(id, {
          kind: "alias",
          decl: info.decl,
          file: info.file,
          name: info.name,
        });
      }
      return id;
    }
  }

  // A class/interface return type -> its named schema id. Register the class.
  if (t.flags & ts.TypeFlags.Object && t.symbol) {
    const info = types._declOf(t.symbol);
    if (
      info &&
      (ts.isClassDeclaration(info.decl) || ts.isInterfaceDeclaration(info.decl))
    ) {
      const id = load.schemaId(loaded.targetDir, info.file, info.name);
      if (registry) {
        registry.add(id, {
          kind: "class",
          decl: info.decl,
          file: info.file,
          name: info.name,
        });
      }
      return id;
    }
  }

  diags.warn(
    "unsupported response type: not a named class/union schema; response body omitted (no fallback)",
    file,
    line
  );
  return null;
}

// Build the params + request_body from a method's typed signature.
//   @Param -> location path, required true
//   @Query -> location query, required = NOT (questionToken OR default initializer)
//   @Body  -> request_body TypeRef (NOT a param)
function _buildParams(loaded, methodDecl, diags, registry) {
  const sf = methodDecl.getSourceFile();
  const params = [];
  let requestBody = null;

  for (const paramDecl of methodDecl.parameters) {
    const classified = _paramKind(paramDecl);
    if (classified === null) {
      continue; // a parameter with no routing decorator is not a route fact
    }

    if (classified.kind === "body") {
      // The request body's schema: map the parameter's typed declaration to a
      // named ref (registering the DTO for transitive collection). The body is a
      // TypeRef (just the id), not a full param.
      const mapped = types.mapType(loaded, paramDecl, diags, registry);
      if (mapped.schema !== null && mapped.schema.type === "named") {
        if (requestBody === null) {
          requestBody = { ref_id: mapped.schema.of };
        }
      } else if (mapped.schema !== null) {
        const line =
          sf.getLineAndCharacterOfPosition(paramDecl.getStart(sf)).line + 1;
        diags.warn(
          "@Body parameter is not a named DTO type; request body omitted (no fallback)",
          load.relFile(loaded.targetDir, sf.fileName),
          line
        );
      }
      continue;
    }

    // A @Param / @Query parameter -> a ParamFact. The neutral param name is the
    // decorator's string arg (the wire name).
    const name = classified.name;
    if (name === null) {
      const line =
        sf.getLineAndCharacterOfPosition(paramDecl.getStart(sf)).line + 1;
      diags.warn(
        "@" +
          (classified.kind === "path" ? "Param" : "Query") +
          " has no name argument; param omitted (no fallback)",
        load.relFile(loaded.targetDir, sf.fileName),
        line
      );
      continue;
    }

    const mapped = types.mapType(loaded, paramDecl, diags, registry);
    if (mapped.schema === null) {
      continue; // rule 3: unresolvable param omitted (diagnostic already recorded)
    }

    // required: a path param is always required; a query param is required
    // unless it is optional (`?`) or carries a default initializer.
    let required;
    if (classified.kind === "path") {
      required = true;
    } else {
      const optional = !!paramDecl.questionToken || !!paramDecl.initializer;
      required = !optional;
    }

    // The param span anchors to the parameter-name line (the chosen convention).
    const nameNode = paramDecl.name;
    const pline =
      sf.getLineAndCharacterOfPosition(nameNode.getStart(sf)).line + 1;
    params.push({
      name: name,
      location: classified.kind,
      required: required,
      schema: mapped.schema,
      span: {
        file: load.relFile(loaded.targetDir, sf.fileName),
        start_line: pline,
        end_line: pline,
      },
    });
  }

  return { params, requestBody };
}

// Recognize the NestJS controller(s) in a loaded program -> a list of RouteFact
// dicts, seeding `registry` with every referenced DTO for transitive collection.
function recognizeNestController(loaded, diags, registry) {
  const routes = [];

  for (const sf of loaded.program.getSourceFiles()) {
    if (sf.isDeclarationFile) {
      continue;
    }
    const rel = load.relFile(loaded.targetDir, sf.fileName);
    if (rel.startsWith("..") || rel.includes("node_modules")) {
      continue; // the target's source only
    }

    sf.forEachChild((node) => {
      if (!ts.isClassDeclaration(node) || !node.name) {
        return;
      }
      const controller = _controllerDecorator(node);
      if (controller === null) {
        return; // not a routing controller
      }
      // The @Controller(...) prefix is read for provenance only and NEVER folded
      // into an operation path (rule 1). Capturing it documents the bright line.
      const _controllerPrefix = _decoratorStringArg(controller);

      for (const member of node.members) {
        if (!ts.isMethodDeclaration(member) || !member.name) {
          continue;
        }
        const verb = _verbDecorator(member);
        if (verb === null) {
          continue; // a method with no HTTP-verb decorator is not a route
        }
        const handler = member.name.getText(sf);

        const { params, requestBody } = _buildParams(
          loaded,
          member,
          diags,
          registry
        );

        const bodyRef = _responseRef(loaded, member, diags, registry);

        // Status is METHOD-DERIVED (typed POST -> 201, else 200), overridden by
        // an explicit @HttpCode(n). A single deterministic rule (rule 3) — the
        // override is read first, the method-default applied otherwise; never a
        // try-typed-then-fallback chain.
        const override = _httpCodeOverride(member);
        const status =
          override !== null ? override : verb.method === "POST" ? 201 : 200;

        const responses = [
          {
            status: status,
            body: bodyRef === null ? null : { ref_id: bodyRef },
          },
        ];

        // The operation span anchors to the method-name line (the chosen
        // convention, matching param=name-line / schema=decl-name-line).
        const opLine =
          sf.getLineAndCharacterOfPosition(member.name.getStart(sf)).line + 1;

        routes.push({
          method: verb.method,
          path: verb.path,
          handler: handler,
          operation_id: handler,
          params: params,
          request_body: requestBody,
          responses: responses,
          span: {
            file: rel,
            start_line: opLine,
            end_line: opLine,
          },
        });
      }
    });
  }

  return routes;
}

module.exports = { recognizeNestController };
