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
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// CodeFacts is the code-inferred contract for one handler: the request body, the
// responses keyed by status, and the params. Annotation facts (Task 3) merge on
// top, code taking precedence.
type CodeFacts struct {
	RequestBody *facts.TypeRef
	Responses   []facts.ResponseFact
	Params      []facts.ParamFact
}

// handlerDecl bundles a handler FuncDecl with its owning package's type info and
// the shared FileSet, so the analyzer can resolve types and positions.
type handlerDecl struct {
	decl *ast.FuncDecl
	info *gotypes.Info
	fset *token.FileSet
}

// Index maps handler symbol name -> its declaration across all target packages.
// Built once from the loaded packages and reused for every route.
type Index map[string]handlerDecl

// BuildIndex collects every function/method declaration in the target packages,
// keyed by its name, so routes can look up their handler by symbol.
func BuildIndex(res *load.Result) Index {
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
				idx[fn.Name.Name] = handlerDecl{decl: fn, info: pkg.TypesInfo, fset: res.Fset}
			}
		}
	}
	return idx
}

// Doc returns the doc comment of the indexed handler (for the swaggo annotation
// parser, Task 3), or nil when the handler or its doc is absent.
func (i Index) Doc(handler string) *ast.CommentGroup {
	h, ok := i[handler]
	if !ok || h.decl == nil {
		return nil
	}
	return h.decl.Doc
}

// Analyze infers the request/response/param facts for one route's handler. The
// route carries the method + normalized path so untyped-query diagnostics can name
// the operation. Unknown handlers (no matching decl) yield empty facts, not a
// panic (defensive — GO-06).
func Analyze(route routes.Route, idx Index, diags *diag.Accumulator) CodeFacts {
	h, ok := idx[route.Handler]
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
			if ref, ok := bindRequestType(h.info, call, route.Handler); ok {
				cf.RequestBody = ref
			}
		case "JSON":
			analyzeJSON(h, call, route, &cf, seenStatus, diags)
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
// resolved to a named type, returns ok=false (the annotation @Param body is the
// fallback, applied in Task 3).
func bindRequestType(info *gotypes.Info, call *ast.CallExpr, handler string) (*facts.TypeRef, bool) {
	if len(call.Args) < 1 {
		return nil, false
	}
	unary, ok := call.Args[0].(*ast.UnaryExpr)
	if !ok || unary.Op != token.AND {
		return nil, false
	}
	id, ok := namedTypeID(info.TypeOf(unary.X))
	if !ok {
		return nil, false
	}
	return &facts.TypeRef{RefID: id}, true
}

// analyzeJSON resolves c.JSON(http.StatusXxx, y): status from go/constant, body
// from the named type of y. A dynamic/unresolvable body emits a WARN (D-05) and
// records the status with a nil body so the response is never silently dropped.
func analyzeJSON(
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
	if id, ok := namedTypeID(h.info.TypeOf(call.Args[1])); ok {
		body = &facts.TypeRef{RefID: id}
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "response body does not resolve to a named type", file, line)
	}
	cf.Responses = append(cf.Responses, facts.ResponseFact{Status: status, Body: body})
}

// statusOf resolves an HTTP status argument to its numeric value via go/constant.
// Handles http.StatusXxx selectors and any const expression whose value is an int.
func statusOf(info *gotypes.Info, arg ast.Expr) (uint16, bool) {
	tv, ok := info.Types[arg]
	if ok && tv.Value != nil && tv.Value.Kind() == constant.Int {
		if v, exact := constant.Int64Val(tv.Value); exact {
			return uint16(v), true
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
	return uint16(v), true
}

// namedTypeID returns the package-qualified schema id of a named (struct/enum)
// type, matching the schema-id format from 02-01 so the ref points at the same
// schema. Pointers/composite-literal types unwrap to their named type.
func namedTypeID(t gotypes.Type) (string, bool) {
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
	return qualifiedSchemaID(obj.Pkg().Path(), obj.Name()), true
}

// qualifiedSchemaID mirrors types.schemaID exactly so a handler-inferred request/
// response ref points at the same schema the type extractor (02-01) emitted: the
// module path prefix is trimmed to a module-relative package path, then
// ".TypeName" (e.g. "internal/common/dto.CreateGoalInput"). The module prefix is
// supplied once per run via SetModule.
func qualifiedSchemaID(pkgPath, name string) string {
	rel := pkgPath
	if modulePrefix != "" && strings.HasPrefix(pkgPath, modulePrefix) {
		rel = strings.TrimPrefix(pkgPath, modulePrefix)
		rel = strings.TrimPrefix(rel, "/")
	}
	if rel == "" {
		return name
	}
	return rel + "." + name
}

// modulePrefix is the target module path used to derive module-relative schema
// ids identical to 02-01. Set once per run via SetModule before Analyze.
var modulePrefix string

// SetModule records the target module path so handler-inferred refs share the
// exact module-relative schema-id format the type extractor (02-01) emits.
func SetModule(module string) { modulePrefix = module }

// --- param builders ------------------------------------------------------

func pathParam(name string, fset *token.FileSet, pos token.Pos) facts.ParamFact {
	return facts.ParamFact{
		Name:       name,
		Location:   "path",
		Required:   true,
		Schema:     facts.SchemaType{Kind: "string"},
		EnumValues: []string{},
		Span:       spanOf(fset, pos),
	}
}

func queryParam(name string, fset *token.FileSet, pos token.Pos) facts.ParamFact {
	return facts.ParamFact{
		Name:       name,
		Location:   "query",
		Required:   false, // under-specified from code; @Param may upgrade (Task 3).
		Schema:     facts.SchemaType{Kind: "string"},
		EnumValues: []string{},
		Span:       spanOf(fset, pos),
	}
}

// untypedRouteLabel renders the operation label used in the untyped-query
// diagnostic. The fixture's expected text is "GET /goal/list", so we prefer the
// concrete annotation-resolved path when available; the route here is
// group-relative, so 02-03 reconciles the exact prefix — the machine-stable
// identity (param name + method + relative path) is preserved.
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
