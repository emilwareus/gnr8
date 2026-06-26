// Package handlers analyzes each route's handler function body to infer the
// request body, responses (by numeric HTTP status), and path/query parameters
// from the supported typed Gin patterns (GO-05), and to emit diagnostics for the
// lossy cases (untyped query params, dynamic/unresolvable responses) rather than
// guessing or silently dropping (GO-06 / D-05).
//
// Recognized gin-context calls (gated on the resolved receiver package path
// `github.com/gin-gonic/gin`, never the alias text — shared with the route
// recognizer via routes.GinMethod):
//
//	c.ShouldBindJSON(&x)        -> request body type = TypeOf(x)
//	c.JSON(http.StatusXxx, y)   -> responses[status] = TypeOf(y); status via go/constant
//	c.Param("name")             -> path param (string, required)
//	c.Query("name")             -> query param (string) + untyped-query WARN diagnostic
//
// Status numbers come from `go/constant` on the resolved `*types.Const` (e.g.
// http.StatusCreated -> 201), never a hardcoded name->number map.
package handlers

import (
	"go/ast"
	"go/constant"
	"go/token"
	gotypes "go/types"
	"strconv"
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// CodeFacts is the code-inferred contract for one handler: the request body, the
// responses keyed by status, and the params. This is the ONLY source of these
// facts — there is no annotation/fallback path (CLAUDE.md rules 1 & 3).
type CodeFacts struct {
	RequestBody *facts.TypeRef
	Responses   []facts.ResponseFact
	Params      []facts.ParamFact
}

// handlerDecl bundles a handler FuncDecl with its owning package's type info and
// the shared FileSet, so the analyzer can resolve types and positions. pkgPath is
// the owning package's import path, used to disambiguate same-named handlers
// across packages (WR-02).
type handlerDecl struct {
	decl    *ast.FuncDecl
	info    *gotypes.Info
	fset    *token.FileSet
	pkgPath string
}

// identityKey is the fully-qualified, position-stable identity of a handler decl
// ("<pkgPath>.<receiver>.<name>@<file>:<line>"), used as a deterministic
// tie-break when two handlers share a bare name (WR-02). It is independent of
// package/file load order, so the chosen survivor is reproducible (GRAPH-02).
func (h handlerDecl) identityKey() string {
	name := ""
	if h.decl != nil && h.decl.Name != nil {
		name = h.decl.Name.Name
	}
	recv := receiverTypeName(h.decl)
	file, line := positionOf(h.fset, declPos(h.decl))
	return h.pkgPath + "." + recv + "." + name + "@" + file + ":" + strconv.FormatUint(uint64(line), 10)
}

// receiverTypeName renders the method receiver's base type name (e.g. "Handler"
// for `func (h *Handler) Foo()`), or "" for a plain function, so two methods of
// the same name on different receivers have distinct identities (WR-02).
func receiverTypeName(fn *ast.FuncDecl) string {
	if fn == nil || fn.Recv == nil || len(fn.Recv.List) == 0 {
		return ""
	}
	t := fn.Recv.List[0].Type
	if star, ok := t.(*ast.StarExpr); ok {
		t = star.X
	}
	if id, ok := t.(*ast.Ident); ok {
		return id.Name
	}
	return ""
}

// declPos returns the declaration's position, or token.NoPos when absent.
func declPos(fn *ast.FuncDecl) token.Pos {
	if fn == nil {
		return token.NoPos
	}
	return fn.Pos()
}

// Index maps handler symbol name -> its declaration across all target packages.
// Built once from the loaded packages and reused for every route.
type Index map[string]handlerDecl

// Analyzer carries the per-invocation context the handler analysis needs — the
// module prefix used to derive module-relative schema ids — alongside the handler
// Index. Threading this state through a struct (instead of package-level globals)
// makes the helper reentrant: two analyses over different modules in one process
// no longer clobber each other's prefix (WR-03), and the setup ordering is
// enforced by construction rather than by call discipline.
type Analyzer struct {
	idx          Index
	modulePrefix string
}

// NewAnalyzer builds an Analyzer from the loaded packages and the target module
// path. The module prefix qualifies handler-inferred schema refs into the 02-01
// module-relative format. diags receives any duplicate-handler-name collision
// warnings (WR-02).
func NewAnalyzer(res *load.Result, module string, diags *diag.Accumulator) *Analyzer {
	return &Analyzer{
		idx:          BuildIndex(res, diags),
		modulePrefix: module,
	}
}

// Index exposes the underlying handler index (for callers that look up docs or
// build their own per-route flow).
func (a *Analyzer) Index() Index { return a.idx }

// BuildIndex collects every function/method declaration in the target packages,
// keyed by its name, so routes can look up their handler by symbol.
//
// A bare function name is not unique across packages/receivers, so two handlers
// named the same in different packages would otherwise resolve last-write-wins by
// load order — both a correctness bug (wrong body/responses attached to a route)
// and a GRAPH-02 determinism hazard. To remove the load-order dependency, the
// surviving decl for a colliding name is chosen deterministically by its
// fully-qualified identity (package path, then receiver + file:line position), and
// the collision is surfaced as a diagnostic so it is never silent (WR-02).
func BuildIndex(res *load.Result, diags *diag.Accumulator) Index {
	idx := make(Index)
	for _, pkg := range res.Packages {
		if pkg.TypesInfo == nil {
			continue
		}
		for _, file := range pkg.Syntax {
			for _, d := range file.Decls {
				fn, ok := d.(*ast.FuncDecl)
				if !ok || fn.Name == nil {
					continue
				}
				cand := handlerDecl{decl: fn, info: pkg.TypesInfo, fset: res.Fset, pkgPath: pkg.PkgPath}
				existing, dup := idx[fn.Name.Name]
				if !dup {
					idx[fn.Name.Name] = cand
					continue
				}
				// Deterministic tie-break: keep the candidate whose fully-qualified
				// identity sorts first, independent of package/file iteration order.
				winner, loser := existing, cand
				if cand.identityKey() < existing.identityKey() {
					winner, loser = cand, existing
				}
				idx[fn.Name.Name] = winner
				if diags != nil {
					file, line := positionOf(loser.fset, declPos(loser.decl))
					diags.Warn(
						"duplicate handler name '"+fn.Name.Name+"': also declared at "+
							loser.identityKey()+"; route lookups by bare name are ambiguous, "+
							"keeping "+winner.identityKey()+" deterministically — qualify the "+
							"handler or disambiguate the route (WR-02)",
						file, line,
					)
				}
			}
		}
	}
	return idx
}

// Analyze infers the request/response/param facts for one route's handler. The
// route carries the method + normalized path so untyped-query diagnostics can name
// the operation. Unknown handlers (no matching decl) yield empty facts, not a
// panic (defensive — GO-06). The module prefix used to qualify schema refs is read
// from the Analyzer's per-invocation context (WR-03), not a package global.
func (a *Analyzer) Analyze(route routes.Route, diags *diag.Accumulator) CodeFacts {
	h, ok := a.idx[route.Handler]
	if !ok || h.decl == nil || h.decl.Body == nil {
		return CodeFacts{Responses: []facts.ResponseFact{}, Params: []facts.ParamFact{}}
	}

	cf := CodeFacts{Responses: []facts.ResponseFact{}, Params: []facts.ParamFact{}}
	seenParam := map[string]bool{}
	seenStatus := map[uint16]bool{}

	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(h.info, call)
		if !ok || recvPkg != routes.GinPkgPath {
			return true
		}
		switch name {
		case "ShouldBindJSON", "BindJSON":
			if ref, ok := a.bindRequestType(h.info, call); ok {
				cf.RequestBody = ref
			}
		case "JSON":
			a.analyzeJSON(h, call, route, &cf, seenStatus, diags)
		case "Param":
			if pname, ok := stringArg(call, 0); ok && !seenParam["path/"+pname] {
				seenParam["path/"+pname] = true
				cf.Params = append(cf.Params, pathParam(pname, h.fset, call.Pos()))
			}
		case "Query":
			if pname, ok := stringArg(call, 0); ok && !seenParam["query/"+pname] {
				seenParam["query/"+pname] = true
				file, line := positionOf(h.fset, call.Pos())
				cf.Params = append(cf.Params, queryParam(pname, h.fset, call.Pos()))
				diags.UntypedQueryParam(pname, route.Method, untypedRouteLabel(route), file, line)
			}
		}
		return true
	})
	return cf
}

// bindRequestType resolves ShouldBindJSON(&x) -> a TypeRef to the bound named type.
// arg[0] must be &ident (an *ast.UnaryExpr with Op AND). When the type cannot be
// resolved to a named type, returns ok=false and the route simply has no request
// body — there is no secondary source (CLAUDE.md rule 3).
func (a *Analyzer) bindRequestType(info *gotypes.Info, call *ast.CallExpr) (*facts.TypeRef, bool) {
	if len(call.Args) < 1 {
		return nil, false
	}
	unary, ok := call.Args[0].(*ast.UnaryExpr)
	if !ok || unary.Op != token.AND {
		return nil, false
	}
	id, ok := a.namedTypeID(info.TypeOf(unary.X))
	if !ok {
		return nil, false
	}
	return &facts.TypeRef{RefID: id}, true
}

// analyzeJSON resolves c.JSON(http.StatusXxx, y): status from go/constant, body
// from the named type of y. A dynamic/unresolvable body emits a WARN (D-05) and
// records the status with a nil body so the response is never silently dropped.
func (a *Analyzer) analyzeJSON(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	diags *diag.Accumulator,
) {
	if len(call.Args) < 2 {
		return
	}
	status, ok := statusOf(h.info, call.Args[0])
	if !ok {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "non-constant HTTP status", file, line)
		return
	}
	if seenStatus[status] {
		return // first wins; identical status duplicates collapse deterministically.
	}
	seenStatus[status] = true

	var body *facts.TypeRef
	if id, ok := a.namedTypeID(h.info.TypeOf(call.Args[1])); ok {
		body = &facts.TypeRef{RefID: id}
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "response body does not resolve to a named type", file, line)
	}
	cf.Responses = append(cf.Responses, facts.ResponseFact{Status: status, Body: body})
}

// statusOf resolves an HTTP status argument to its numeric value via go/constant.
// Handles http.StatusXxx selectors and any const expression whose value is an int.
//
// The resolved value is bounds-checked to a sane HTTP status range (100..=599)
// before narrowing to uint16: a non-exact or out-of-range constant (e.g.
// c.JSON(70000, x), which would otherwise wrap silently to 4464) returns
// ok=false so the caller emits a dynamic-response diagnostic instead of emitting
// a corrupted status (GO-06).
func statusOf(info *gotypes.Info, arg ast.Expr) (uint16, bool) {
	tv, ok := info.Types[arg]
	if ok && tv.Value != nil && tv.Value.Kind() == constant.Int {
		if v, exact := constant.Int64Val(tv.Value); exact {
			return httpStatusInRange(v)
		}
	}
	// Fallback: resolve the identifier to a *types.Const and read its value.
	ident := leafIdent(arg)
	if ident == nil {
		return 0, false
	}
	c, ok := info.ObjectOf(ident).(*gotypes.Const)
	if !ok || c.Val() == nil || c.Val().Kind() != constant.Int {
		return 0, false
	}
	v, exact := constant.Int64Val(c.Val())
	if !exact {
		return 0, false
	}
	return httpStatusInRange(v)
}

// httpStatusInRange narrows a resolved int64 status to uint16 only when it is a
// valid HTTP status code (100..=599); otherwise it reports ok=false so the
// out-of-range value is diagnosed as a dynamic response rather than truncated.
func httpStatusInRange(v int64) (uint16, bool) {
	if v < 100 || v > 599 {
		return 0, false
	}
	return uint16(v), true
}

// namedTypeID returns the package-qualified schema id of a named (struct/enum)
// type, matching the schema-id format from 02-01 so the ref points at the same
// schema. Pointers/composite-literal types unwrap to their named type.
func (a *Analyzer) namedTypeID(t gotypes.Type) (string, bool) {
	if t == nil {
		return "", false
	}
	if p, ok := gotypes.Unalias(t).(*gotypes.Pointer); ok {
		t = p.Elem()
	}
	named, ok := gotypes.Unalias(t).(*gotypes.Named)
	if !ok {
		return "", false
	}
	obj := named.Obj()
	if obj == nil || obj.Pkg() == nil {
		return "", false
	}
	return a.qualifiedSchemaID(obj.Pkg().Path(), obj.Name()), true
}

// qualifiedSchemaID mirrors types.schemaID exactly so a handler-inferred request/
// response ref points at the same schema the type extractor (02-01) emitted: the
// module path prefix is trimmed to a module-relative package path, then
// ".TypeName" (e.g. "internal/common/dto.CreateGoalInput"). The module prefix is
// read from the Analyzer's per-invocation context (WR-03), not a package global.
func (a *Analyzer) qualifiedSchemaID(pkgPath, name string) string {
	rel := pkgPath
	if a.modulePrefix != "" && strings.HasPrefix(pkgPath, a.modulePrefix) {
		rel = strings.TrimPrefix(pkgPath, a.modulePrefix)
		rel = strings.TrimPrefix(rel, "/")
	}
	if rel == "" {
		return name
	}
	return rel + "." + name
}

// --- param builders ------------------------------------------------------

func pathParam(name string, fset *token.FileSet, pos token.Pos) facts.ParamFact {
	return facts.ParamFact{
		Name:     name,
		Location: "path",
		Required: true,
		Schema:   facts.PrimitiveType(facts.StringPrim()),
		Span:     spanOf(fset, pos),
	}
}

// queryParam builds a query parameter from a c.Query("name") read. Type defaults
// to string and required defaults to false; there is no annotation source to
// refine these (CLAUDE.md rules 1 & 3).
func queryParam(name string, fset *token.FileSet, pos token.Pos) facts.ParamFact {
	return facts.ParamFact{
		Name:     name,
		Location: "query",
		Required: false,
		Schema:   facts.PrimitiveType(facts.StringPrim()),
		Span:     spanOf(fset, pos),
	}
}

// untypedRouteLabel renders the operation label (method + group-relative path)
// used in the untyped-query diagnostic. The machine-stable identity (param name +
// method + relative path) is preserved.
func untypedRouteLabel(route routes.Route) string {
	return route.Path
}

// --- expression helpers --------------------------------------------------

// stringArg returns the string value of the i-th call argument when it is a
// string literal.
func stringArg(call *ast.CallExpr, i int) (string, bool) {
	if i >= len(call.Args) {
		return "", false
	}
	lit, ok := call.Args[i].(*ast.BasicLit)
	if !ok || lit.Kind != token.STRING {
		return "", false
	}
	return strings.Trim(lit.Value, "`\""), true
}

// leafIdent returns the trailing identifier of a selector/ident expression, e.g.
// `http.StatusCreated` -> the `StatusCreated` ident; `StatusOK` -> itself.
func leafIdent(e ast.Expr) *ast.Ident {
	switch x := e.(type) {
	case *ast.SelectorExpr:
		return x.Sel
	case *ast.Ident:
		return x
	}
	return nil
}

func spanOf(fset *token.FileSet, pos token.Pos) facts.SourceSpan {
	file, line := positionOf(fset, pos)
	return facts.SourceSpan{File: file, StartLine: line, EndLine: line}
}

func positionOf(fset *token.FileSet, pos token.Pos) (string, uint32) {
	if fset == nil || !pos.IsValid() {
		return "", 0
	}
	p := fset.Position(pos)
	return p.Filename, uint32(p.Line)
}
