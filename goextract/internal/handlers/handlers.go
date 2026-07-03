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
//	c.ShouldBind(&x)            -> form/multipart body when the bound DTO uses form tags
//	c.FormFile("name")          -> multipart file field on a synthesized request body
//	c.PostForm("name")          -> form string field on a synthesized request body
//	c.JSON(http.StatusXxx, y)   -> responses[status] = TypeOf(y); status via go/constant
//	c.Param("name")             -> path param (string, required)
//	c.Query("name")             -> query param (string) + untyped-query WARN diagnostic
//	c.DefaultQuery("n", "d")    -> optional query param (string) + default
//	c.GetQuery("name")          -> optional query param (string)
//
// Status numbers come from `go/constant` on the resolved `*types.Const` (e.g.
// http.StatusCreated -> 201), never a hardcoded name->number map.
package handlers

import (
	"go/ast"
	"go/constant"
	"go/token"
	gotypes "go/types"
	"reflect"
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
	RequestBody            *facts.TypeRef
	RequestBodyRequired    bool
	RequestBodyContentType string
	Responses              []facts.ResponseFact
	Params                 []facts.ParamFact
	Schemas                []facts.SchemaFact
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
	idx           Index
	declsByObject map[string]handlerDecl
	modulePrefix  string
	collisions    []handlerCollision
}

type handlerCollision struct {
	name    string
	message string
	file    string
	line    uint32
}

// NewAnalyzer builds an Analyzer from the loaded packages and the target module
// path. The module prefix qualifies handler-inferred schema refs into the 02-01
// module-relative format. Duplicate-name collisions are retained so callers can
// report only the ones that recognized routes actually reference (WR-02).
func NewAnalyzer(res *load.Result, module string, diags *diag.Accumulator) *Analyzer {
	idx, collisions := buildIndex(res)
	return &Analyzer{
		idx:           idx,
		declsByObject: buildDeclObjectIndex(res),
		modulePrefix:  module,
		collisions:    collisions,
	}
}

// Index exposes the underlying handler index (for callers that look up docs or
// build their own per-route flow).
func (a *Analyzer) Index() Index { return a.idx }

// ReportRouteHandlerCollisions emits duplicate-name diagnostics only for names
// that recognized routes actually reference. Helper/helper collisions that never
// affect route extraction are not actionable and stay silent.
func (a *Analyzer) ReportRouteHandlerCollisions(recognized []routes.Route, diags *diag.Accumulator) {
	if diags == nil || len(a.collisions) == 0 {
		return
	}
	routeHandlers := map[string]bool{}
	for _, route := range recognized {
		if route.HandlerKey == "" {
			routeHandlers[route.Handler] = true
		}
	}
	for _, collision := range a.collisions {
		if routeHandlers[collision.name] {
			diags.Warn(collision.message, collision.file, collision.line)
		}
	}
}

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
	idx, collisions := buildIndex(res)
	if diags != nil {
		for _, collision := range collisions {
			diags.Warn(collision.message, collision.file, collision.line)
		}
	}
	return idx
}

func buildIndex(res *load.Result) (Index, []handlerCollision) {
	idx := make(Index)
	var collisions []handlerCollision
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
				file, line := positionOf(loser.fset, declPos(loser.decl))
				collisions = append(collisions, handlerCollision{
					name: fn.Name.Name,
					message: "duplicate handler name '" + fn.Name.Name + "': also declared at " +
						loser.identityKey() + "; route lookups by bare name are ambiguous, " +
						"keeping " + winner.identityKey() + " deterministically — qualify the " +
						"handler or disambiguate the route (WR-02)",
					file: file,
					line: line,
				})
			}
		}
	}
	return idx, collisions
}

func buildDeclObjectIndex(res *load.Result) map[string]handlerDecl {
	out := make(map[string]handlerDecl)
	if res == nil {
		return out
	}
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
				decl := handlerDecl{decl: fn, info: pkg.TypesInfo, fset: res.Fset, pkgPath: pkg.PkgPath}
				key := ""
				if obj, ok := pkg.TypesInfo.Defs[fn.Name].(*gotypes.Func); ok {
					key = routes.FuncObjectKey(obj)
				}
				if key == "" {
					key = funcDeclObjectKey(pkg.PkgPath, fn.Name.Pos())
				}
				out[key] = decl
			}
		}
	}
	return out
}

func funcDeclObjectKey(pkgPath string, pos token.Pos) string {
	if pkgPath == "" || !pos.IsValid() {
		return ""
	}
	return pkgPath + "@" + strconv.FormatInt(int64(pos), 10)
}

func funcObjectKey(fn *gotypes.Func) string {
	return routes.FuncObjectKey(fn)
}

func (a *Analyzer) samePackageCallee(h handlerDecl, fn *gotypes.Func) (handlerDecl, bool) {
	if fn == nil || fn.Pkg() == nil || fn.Pkg().Path() != h.pkgPath {
		return handlerDecl{}, false
	}
	callee, ok := a.declsByObject[funcObjectKey(fn)]
	if !ok || callee.pkgPath != h.pkgPath || callee.decl == nil || callee.decl.Body == nil {
		return handlerDecl{}, false
	}
	return callee, true
}

// Analyze infers the request/response/param facts for one route's handler. The
// route carries the method + normalized path so untyped-query diagnostics can name
// the operation. Unknown handlers (no matching decl) yield empty facts, not a
// panic (defensive — GO-06). The module prefix used to qualify schema refs is read
// from the Analyzer's per-invocation context (WR-03), not a package global.
func (a *Analyzer) Analyze(route routes.Route, diags *diag.Accumulator) CodeFacts {
	h, ok := a.handlerForRoute(route)
	if !ok || h.decl == nil || h.decl.Body == nil {
		return CodeFacts{RequestBodyRequired: true, Responses: []facts.ResponseFact{}, Params: []facts.ParamFact{}}
	}

	cf := CodeFacts{RequestBodyRequired: true, Responses: []facts.ResponseFact{}, Params: []facts.ParamFact{}}
	seenParam := map[string]bool{}
	seenStatus := map[uint16]bool{}
	provisionalStatus := map[uint16]bool{}
	formFields := map[string]facts.FieldFact{}
	formHasFile := false
	contentTypeHint := ""
	optionalBindPositions := collectOptionalBindPositions(h)
	hasBodyBind := false
	allBodyBindsOptional := true
	delegatedResponseSeen := map[string]bool{}
	delegatedBodySeen := map[string]bool{}

	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(h.info, call)
		if !ok || recvPkg != routes.GinPkgPath {
			if ref, schema, ok := a.bodyFromGenericJSONHelper(h, call); ok {
				cf.RequestBody = ref
				cf.RequestBodyContentType = "application/json"
				cf.Schemas = append(cf.Schemas, schema...)
				hasBodyBind = true
				allBodyBindsOptional = false
			}
			a.analyzeDelegatedRequestBodies(h, call, &cf, delegatedBodySeen, &hasBodyBind, &allBodyBindsOptional)
			a.analyzeDelegatedResponses(h, call, route, &cf, seenStatus, provisionalStatus, diags, delegatedResponseSeen, contentTypeHint)
			a.analyzePathHelperCall(h, call, &cf, seenParam)
			a.analyzeQueryHelperCall(h, call, route, &cf, seenParam)
			return true
		}
		switch name {
		case "ShouldBindJSON", "BindJSON":
			if ref, _, ok := a.bindRequestType(h.info, call); ok {
				cf.RequestBody = ref
				cf.RequestBodyContentType = "application/json"
				hasBodyBind = true
				if !optionalBindPositions[call.Pos()] {
					allBodyBindsOptional = false
				}
			}
		case "ShouldBind", "Bind", "ShouldBindWith", "BindWith":
			if ref, bound, ok := a.bindRequestType(h.info, call); ok {
				cf.RequestBody = ref
				hasBodyBind = true
				if !optionalBindPositions[call.Pos()] {
					allBodyBindsOptional = false
				}
				cf.RequestBodyContentType = bindContentType(name, h.info, call, bound)
			}
		case "JSON":
			a.analyzeJSON(h, call, route, &cf, seenStatus, provisionalStatus, diags)
		case "Status":
			a.analyzeStatus(h, call, route, &cf, seenStatus, provisionalStatus, true, diags)
		case "AbortWithStatus":
			a.analyzeStatus(h, call, route, &cf, seenStatus, provisionalStatus, false, diags)
		case "Header":
			if key, ok := stringArg(call, 0); ok && strings.EqualFold(key, "Content-Type") {
				if value, ok := a.stringValueOf(h, call.Args[1]); ok {
					contentTypeHint = value
				}
			}
		case "File", "FileAttachment":
			a.analyzeBinaryStatus(&cf, seenStatus, provisionalStatus, 200, contentTypeHint)
		case "Data":
			a.analyzeData(h, call, route, &cf, seenStatus, provisionalStatus, diags)
		case "DataFromReader":
			a.analyzeDataFromReader(h, call, route, &cf, seenStatus, provisionalStatus, diags)
		case "SSEvent":
			a.addSSEResponse(&cf, seenStatus, provisionalStatus)
		case "Stream":
			if streamCallContainsSSEvent(h.info, call) {
				a.addSSEResponse(&cf, seenStatus, provisionalStatus)
			}
		case "Param":
			if pname, ok := stringArg(call, 0); ok && !seenParam["path/"+pname] {
				seenParam["path/"+pname] = true
				cf.Params = append(cf.Params, pathParam(pname, h.fset, call.Pos()))
			}
		case "Query", "DefaultQuery", "GetQuery":
			if pname, ok := stringArg(call, 0); ok && !seenParam["query/"+pname] {
				seenParam["query/"+pname] = true
				file, line := positionOf(h.fset, call.Pos())
				cf.Params = append(cf.Params, queryParamFromGinCall(h.info, name, pname, h.fset, call))
				if name == "Query" {
					diags.UntypedQueryParam(pname, route.Method, untypedRouteLabel(route), file, line)
				}
			}
		case "FormFile":
			if fname, ok := stringArg(call, 0); ok {
				formFields[fname] = formField(fname, facts.PrimitiveType(facts.BytesPrim()), true)
				formHasFile = true
			} else {
				file, line := positionOf(h.fset, call.Pos())
				diags.Warn("unsupported multipart source pattern: FormFile field name is dynamic (GO-05)", file, line)
			}
		case "PostForm", "DefaultPostForm":
			if fname, ok := stringArg(call, 0); ok {
				if _, seen := formFields[fname]; !seen {
					formFields[fname] = formField(fname, facts.PrimitiveType(facts.StringPrim()), false)
				}
			} else {
				file, line := positionOf(h.fset, call.Pos())
				diags.Warn("unsupported form source pattern: form field name is dynamic (GO-05)", file, line)
			}
		case "MultipartForm":
			file, line := positionOf(h.fset, call.Pos())
			diags.Warn("unsupported multipart source pattern: MultipartForm map access cannot be fully extracted; use typed ShouldBind or direct FormFile/PostForm calls (GO-05)", file, line)
		}
		return true
	})
	if cf.RequestBody == nil && len(formFields) > 0 {
		schemaID, schemaName := syntheticFormSchemaIdentity(route.Handler)
		fields := make([]facts.FieldFact, 0, len(formFields))
		for _, field := range formFields {
			fields = append(fields, field)
		}
		cf.RequestBody = &facts.TypeRef{RefID: schemaID}
		cf.RequestBodyContentType = "application/x-www-form-urlencoded"
		if formHasFile {
			cf.RequestBodyContentType = "multipart/form-data"
		}
		cf.Schemas = append(cf.Schemas, facts.SchemaFact{
			ID:   schemaID,
			Name: schemaName,
			Body: facts.ObjectType(fields),
			Span: spanOf(h.fset, declPos(h.decl)),
		})
	}
	if cf.RequestBody == nil {
		if schema := a.rawJSONRequestSchema(h, route.Handler); schema != nil {
			cf.RequestBody = &facts.TypeRef{RefID: schema.ID}
			cf.RequestBodyContentType = "application/json"
			cf.Schemas = append(cf.Schemas, *schema)
		}
	}
	if hasBodyBind {
		cf.RequestBodyRequired = !allBodyBindsOptional
	}
	return cf
}

func (a *Analyzer) handlerForRoute(route routes.Route) (handlerDecl, bool) {
	if route.HandlerKey != "" {
		h, ok := a.declsByObject[route.HandlerKey]
		return h, ok
	}
	h, ok := a.idx[route.Handler]
	return h, ok
}

func collectOptionalBindPositions(h handlerDecl) map[token.Pos]bool {
	out := map[token.Pos]bool{}
	if h.decl == nil || h.decl.Body == nil {
		return out
	}
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		ifStmt, ok := n.(*ast.IfStmt)
		if !ok || !isOptionalBodyGuard(h.info, ifStmt.Cond) {
			return true
		}
		ast.Inspect(ifStmt.Body, func(child ast.Node) bool {
			call, ok := child.(*ast.CallExpr)
			if !ok || !isGinBindCall(h.info, call) {
				return true
			}
			out[call.Pos()] = true
			return true
		})
		return true
	})
	return out
}

func isOptionalBodyGuard(info *gotypes.Info, expr ast.Expr) bool {
	return isBodyPresentPredicate(info, expr)
}

func isBodyPresentPredicate(info *gotypes.Info, expr ast.Expr) bool {
	switch node := expr.(type) {
	case *ast.BinaryExpr:
		switch node.Op {
		case token.LOR:
			return isBodyPresentPredicate(info, node.X) && isBodyPresentPredicate(info, node.Y)
		case token.LAND:
			return isBodyPresentPredicate(info, node.X) || isBodyPresentPredicate(info, node.Y)
		case token.GTR:
			return isContentLengthExpr(info, node.X) && isZeroLiteral(node.Y)
		case token.LSS:
			return isZeroLiteral(node.X) && isContentLengthExpr(info, node.Y)
		case token.NEQ:
			return (isContentLengthExpr(info, node.X) && isZeroLiteral(node.Y)) ||
				(isZeroLiteral(node.X) && isContentLengthExpr(info, node.Y)) ||
				(isContentLengthHeaderCall(info, node.X) && isEmptyStringLiteral(node.Y)) ||
				(isEmptyStringLiteral(node.X) && isContentLengthHeaderCall(info, node.Y))
		}
	case *ast.ParenExpr:
		return isBodyPresentPredicate(info, node.X)
	}
	return false
}

func isContentLengthExpr(info *gotypes.Info, expr ast.Expr) bool {
	switch node := expr.(type) {
	case *ast.SelectorExpr:
		return isGinRequestContentLength(info, node)
	case *ast.CallExpr:
		if selectorName(node.Fun) == "len" && len(node.Args) == 1 {
			return isContentLengthHeaderValueExpr(info, node.Args[0])
		}
	}
	return false
}

func isGinRequestContentLength(info *gotypes.Info, selector *ast.SelectorExpr) bool {
	if info == nil {
		return false
	}
	if selector == nil || selector.Sel == nil || selector.Sel.Name != "ContentLength" {
		return false
	}
	requestSelector, ok := selector.X.(*ast.SelectorExpr)
	if !ok || requestSelector.Sel == nil || requestSelector.Sel.Name != "Request" {
		return false
	}
	return isGinContextType(info.TypeOf(requestSelector.X)) &&
		isNamedType(info.TypeOf(requestSelector), "net/http", "Request")
}

func mentionsContentLengthHeader(info *gotypes.Info, expr ast.Expr) bool {
	if info == nil {
		return false
	}
	found := false
	ast.Inspect(expr, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok || !isContentLengthHeaderCall(info, call) {
			return true
		}
		found = true
		return false
	})
	return found
}

func isContentLengthHeaderCall(info *gotypes.Info, expr ast.Expr) bool {
	if info == nil {
		return false
	}
	call, ok := expr.(*ast.CallExpr)
	if !ok {
		return false
	}
	name, recvPkg, ok := routes.GinMethod(info, call)
	if !ok || recvPkg != routes.GinPkgPath || name != "GetHeader" {
		return false
	}
	value, ok := stringArg(call, 0)
	return ok && strings.EqualFold(value, "Content-Length")
}

func isContentLengthHeaderValueExpr(info *gotypes.Info, expr ast.Expr) bool {
	if isContentLengthHeaderCall(info, expr) {
		return true
	}
	call, ok := expr.(*ast.CallExpr)
	if !ok || len(call.Args) != 1 {
		return false
	}
	fn := calledFuncObject(info, call.Fun)
	return fn != nil && fn.Pkg() != nil && fn.Pkg().Path() == "strings" && fn.Name() == "TrimSpace" &&
		isContentLengthHeaderValueExpr(info, call.Args[0])
}

func isGinContextType(t gotypes.Type) bool {
	return isNamedType(t, routes.GinPkgPath, "Context")
}

func isNamedType(t gotypes.Type, pkgPath, name string) bool {
	if t == nil {
		return false
	}
	if ptr, ok := gotypes.Unalias(t).(*gotypes.Pointer); ok {
		t = ptr.Elem()
	}
	named, ok := gotypes.Unalias(t).(*gotypes.Named)
	if !ok || named.Obj() == nil || named.Obj().Pkg() == nil {
		return false
	}
	return named.Obj().Pkg().Path() == pkgPath && named.Obj().Name() == name
}

func isZeroLiteral(expr ast.Expr) bool {
	lit, ok := expr.(*ast.BasicLit)
	return ok && lit.Kind == token.INT && lit.Value == "0"
}

func isEmptyStringLiteral(expr ast.Expr) bool {
	lit, ok := expr.(*ast.BasicLit)
	if !ok || lit.Kind != token.STRING {
		return false
	}
	value, err := strconv.Unquote(lit.Value)
	return err == nil && value == ""
}

func isGinBindCall(info *gotypes.Info, call *ast.CallExpr) bool {
	name, recvPkg, ok := routes.GinMethod(info, call)
	if !ok || recvPkg != routes.GinPkgPath {
		return false
	}
	switch name {
	case "ShouldBindJSON", "BindJSON", "ShouldBind", "Bind", "ShouldBindWith", "BindWith":
		return true
	default:
		return false
	}
}

func (a *Analyzer) analyzeQueryHelperCall(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenParam map[string]bool,
) {
	if param, ok := a.queryParamFromSamePackageHelper(h, call); ok {
		if !seenParam["query/"+param.Name] {
			seenParam["query/"+param.Name] = true
			cf.Params = append(cf.Params, param)
		}
		return
	}
	query, ok := firstQueryCall(h.info, call)
	if !ok || query == call {
		return
	}
	pname, ok := stringArg(query, 0)
	if !ok || seenParam["query/"+pname] {
		return
	}
	if nestedQueryHelperOutranks(h.info, call, query) {
		return
	}
	param, ok := a.queryParamFromHelper(h, call, query, pname)
	if !ok {
		return
	}
	seenParam["query/"+pname] = true
	cf.Params = append(cf.Params, param)
	_ = route
}

func (a *Analyzer) queryParamFromHelper(
	h handlerDecl,
	helper *ast.CallExpr,
	query *ast.CallExpr,
	name string,
) (facts.ParamFact, bool) {
	schema, ok := queryHelperSchema(h.info.TypeOf(helper), helper)
	if !ok {
		return facts.ParamFact{}, false
	}
	required := queryHelperRequired(h.info, helper, query)
	if fn := calledFuncObject(h.info, helper.Fun); fn != nil {
		if callee, ok := a.samePackageCallee(h, fn); ok {
			required = queryHelperRequiredFromDirectCallee(h.info, helper, callee, query)
		}
	}
	return facts.ParamFact{
		Name:     name,
		Location: "query",
		Required: required,
		Schema:   schema,
		Default:  firstLiteral(queryDefaultValue(h.info, query), queryHelperDefault(h.info, helper, query)),
		Span:     spanOf(h.fset, query.Pos()),
	}, true
}

func (a *Analyzer) queryParamFromSamePackageHelper(h handlerDecl, call *ast.CallExpr) (facts.ParamFact, bool) {
	if !callPassesGinContext(h.info, call) {
		return facts.ParamFact{}, false
	}
	fn := calledFuncObject(h.info, call.Fun)
	callee, ok := a.samePackageCallee(h, fn)
	if !ok {
		return facts.ParamFact{}, false
	}
	sig, ok := gotypes.Unalias(h.info.TypeOf(call.Fun)).(*gotypes.Signature)
	if !ok || sig.Params() == nil {
		return facts.ParamFact{}, false
	}
	schema, ok := queryHelperSchema(h.info.TypeOf(call), call)
	if !ok {
		return facts.ParamFact{}, false
	}
	for i := 0; i < len(call.Args) && i < sig.Params().Len(); i++ {
		pname, ok := stringArg(call, i)
		if !ok {
			continue
		}
		query, ok := helperQueryCallWithVar(callee, sig.Params().At(i))
		if !ok {
			continue
		}
		return facts.ParamFact{
			Name:     pname,
			Location: "query",
			Required: queryHelperRequiredFromCallee(h.info, call, callee, query),
			Schema:   schema,
			Default:  firstLiteral(queryDefaultValue(callee.info, query), queryHelperDefaultFromArgs(h.info, call, sig)),
			Span:     spanOf(h.fset, call.Pos()),
		}, true
	}
	return facts.ParamFact{}, false
}

func helperQueryCallWithVar(h handlerDecl, keyVar *gotypes.Var) (*ast.CallExpr, bool) {
	if h.decl == nil || h.decl.Body == nil || keyVar == nil {
		return nil, false
	}
	var found *ast.CallExpr
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found != nil {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(h.info, call)
		if !ok || recvPkg != routes.GinPkgPath || !isGinQueryMethod(name) || len(call.Args) == 0 {
			return true
		}
		id, ok := call.Args[0].(*ast.Ident)
		if ok && h.info.ObjectOf(id) == keyVar {
			found = call
			return false
		}
		return true
	})
	return found, found != nil
}

func (a *Analyzer) analyzePathHelperCall(
	h handlerDecl,
	call *ast.CallExpr,
	cf *CodeFacts,
	seenParam map[string]bool,
) {
	pname, schema, ok := a.pathParamFromHelper(h, call)
	if !ok || seenParam["path/"+pname] {
		return
	}
	seenParam["path/"+pname] = true
	cf.Params = append(cf.Params, facts.ParamFact{
		Name:     pname,
		Location: "path",
		Required: true,
		Schema:   schema,
		Span:     spanOf(h.fset, call.Pos()),
	})
}

func (a *Analyzer) pathParamFromHelper(
	h handlerDecl,
	call *ast.CallExpr,
) (string, facts.Type, bool) {
	fn := calledFuncObject(h.info, call.Fun)
	callee, ok := a.samePackageCallee(h, fn)
	if !ok {
		return "", facts.Type{}, false
	}
	sig, ok := gotypes.Unalias(h.info.TypeOf(call.Fun)).(*gotypes.Signature)
	if !ok || sig.Params() == nil {
		return "", facts.Type{}, false
	}
	schema, ok := queryHelperSchema(h.info.TypeOf(call), call)
	if !ok {
		return "", facts.Type{}, false
	}
	for i := 0; i < len(call.Args) && i < sig.Params().Len(); i++ {
		name, ok := stringArg(call, i)
		if !ok {
			continue
		}
		if helperReadsGinParamWithVar(callee, sig.Params().At(i)) {
			return name, schema, true
		}
	}
	return "", facts.Type{}, false
}

func helperReadsGinParamWithVar(h handlerDecl, keyVar *gotypes.Var) bool {
	if h.decl == nil || h.decl.Body == nil || keyVar == nil {
		return false
	}
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(h.info, call)
		if !ok || recvPkg != routes.GinPkgPath || name != "Param" || len(call.Args) == 0 {
			return true
		}
		id, ok := call.Args[0].(*ast.Ident)
		if ok && h.info.ObjectOf(id) == keyVar {
			found = true
			return false
		}
		return true
	})
	return found
}

func (a *Analyzer) analyzeDelegatedResponses(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	diags *diag.Accumulator,
	seenHelpers map[string]bool,
	inheritedContentTypeHint string,
) {
	callee, ok := a.delegatedGinContextHelper(h, call)
	if !ok {
		return
	}
	key := callee.identityKey()
	if seenHelpers[key] {
		return
	}
	seenHelpers[key] = true

	contentTypeHint := inheritedContentTypeHint
	ast.Inspect(callee.decl.Body, func(n ast.Node) bool {
		nested, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(callee.info, nested)
		if !ok || recvPkg != routes.GinPkgPath {
			a.analyzeDelegatedResponses(callee, nested, route, cf, seenStatus, provisionalStatus, diags, seenHelpers, contentTypeHint)
			return true
		}
		switch name {
		case "JSON":
			a.analyzeJSON(callee, nested, route, cf, seenStatus, provisionalStatus, diags)
		case "Status":
			a.analyzeStatus(callee, nested, route, cf, seenStatus, provisionalStatus, true, diags)
		case "AbortWithStatus":
			a.analyzeStatus(callee, nested, route, cf, seenStatus, provisionalStatus, false, diags)
		case "Header":
			if key, ok := stringArg(nested, 0); ok && strings.EqualFold(key, "Content-Type") {
				if value, ok := a.stringValueOf(callee, nested.Args[1]); ok {
					contentTypeHint = value
				}
			}
		case "File", "FileAttachment":
			a.analyzeBinaryStatus(cf, seenStatus, provisionalStatus, 200, contentTypeHint)
		case "Data":
			a.analyzeData(callee, nested, route, cf, seenStatus, provisionalStatus, diags)
		case "DataFromReader":
			a.analyzeDataFromReader(callee, nested, route, cf, seenStatus, provisionalStatus, diags)
		case "SSEvent":
			a.addSSEResponse(cf, seenStatus, provisionalStatus)
		case "Stream":
			if streamCallContainsSSEvent(callee.info, nested) {
				a.addSSEResponse(cf, seenStatus, provisionalStatus)
			}
		}
		return true
	})
}

func (a *Analyzer) analyzeDelegatedRequestBodies(
	h handlerDecl,
	call *ast.CallExpr,
	cf *CodeFacts,
	seenHelpers map[string]bool,
	hasBodyBind *bool,
	allBodyBindsOptional *bool,
) {
	if callUsesTypeArgs(call) {
		return
	}
	callee, ok := a.delegatedGinContextHelper(h, call)
	if !ok {
		return
	}
	key := callee.identityKey()
	if seenHelpers[key] {
		return
	}
	seenHelpers[key] = true

	optionalBindPositions := collectOptionalBindPositions(callee)
	ast.Inspect(callee.decl.Body, func(n ast.Node) bool {
		nested, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(callee.info, nested)
		if !ok || recvPkg != routes.GinPkgPath {
			a.analyzeDelegatedRequestBodies(callee, nested, cf, seenHelpers, hasBodyBind, allBodyBindsOptional)
			return true
		}
		switch name {
		case "ShouldBindJSON", "BindJSON":
			if ref, _, ok := a.bindRequestType(callee.info, nested); ok {
				cf.RequestBody = ref
				cf.RequestBodyContentType = "application/json"
				*hasBodyBind = true
				if !optionalBindPositions[nested.Pos()] {
					*allBodyBindsOptional = false
				}
			}
		case "ShouldBind", "Bind", "ShouldBindWith", "BindWith":
			if ref, bound, ok := a.bindRequestType(callee.info, nested); ok {
				cf.RequestBody = ref
				cf.RequestBodyContentType = bindContentType(name, callee.info, nested, bound)
				*hasBodyBind = true
				if !optionalBindPositions[nested.Pos()] {
					*allBodyBindsOptional = false
				}
			}
		}
		return true
	})
}

func callUsesTypeArgs(call *ast.CallExpr) bool {
	if call == nil {
		return false
	}
	switch call.Fun.(type) {
	case *ast.IndexExpr, *ast.IndexListExpr:
		return true
	default:
		return false
	}
}

func (a *Analyzer) delegatedGinContextHelper(h handlerDecl, call *ast.CallExpr) (handlerDecl, bool) {
	if !callPassesGinContext(h.info, call) {
		return handlerDecl{}, false
	}
	fn := calledFuncObject(h.info, call.Fun)
	callee, ok := a.samePackageCallee(h, fn)
	if !ok {
		return handlerDecl{}, false
	}
	return callee, true
}

func (a *Analyzer) bodyFromGenericJSONHelper(
	h handlerDecl,
	call *ast.CallExpr,
) (*facts.TypeRef, []facts.SchemaFact, bool) {
	if !callPassesGinContext(h.info, call) {
		return nil, nil, false
	}
	fn := calledFuncObject(h.info, call.Fun)
	callee, ok := a.samePackageCallee(h, fn)
	if !ok || !helperBindsJSONTypeParam(callee) {
		return nil, nil, false
	}
	typeArgs := callTypeArgs(h.info, call)
	if len(typeArgs) != 1 {
		return nil, nil, false
	}
	id, ok := a.namedTypeID(typeArgs[0])
	if !ok {
		return nil, nil, false
	}
	return &facts.TypeRef{RefID: id}, nil, true
}

func helperBindsJSONTypeParam(h handlerDecl) bool {
	if h.decl == nil || h.decl.Body == nil {
		return false
	}
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(h.info, call)
		if ok && recvPkg == routes.GinPkgPath && (name == "ShouldBindJSON" || name == "BindJSON") && bindArgIsTypeParam(h.info, call) {
			found = true
			return false
		}
		return true
	})
	return found
}

func bindArgIsTypeParam(info *gotypes.Info, call *ast.CallExpr) bool {
	if info == nil || call == nil || len(call.Args) == 0 {
		return false
	}
	unary, ok := call.Args[0].(*ast.UnaryExpr)
	if !ok || unary.Op != token.AND {
		return false
	}
	_, ok = gotypes.Unalias(info.TypeOf(unary.X)).(*gotypes.TypeParam)
	return ok
}

func callTypeArgs(info *gotypes.Info, call *ast.CallExpr) []gotypes.Type {
	if info == nil || call == nil {
		return nil
	}
	var exprs []ast.Expr
	switch fun := call.Fun.(type) {
	case *ast.IndexExpr:
		exprs = []ast.Expr{fun.Index}
	case *ast.IndexListExpr:
		exprs = fun.Indices
	default:
		return nil
	}
	out := make([]gotypes.Type, 0, len(exprs))
	for _, expr := range exprs {
		t := info.TypeOf(expr)
		if t == nil {
			return nil
		}
		out = append(out, t)
	}
	return out
}

func callPassesGinContext(info *gotypes.Info, call *ast.CallExpr) bool {
	for _, arg := range call.Args {
		if isGinContextType(info.TypeOf(arg)) {
			return true
		}
	}
	return false
}

func nestedQueryHelperOutranks(info *gotypes.Info, helper *ast.CallExpr, query *ast.CallExpr) bool {
	found := false
	currentPriority := queryHelperPriority(info, helper, query)
	ast.Inspect(helper, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok || call == helper || call == query {
			return true
		}
		if nestedQuery, ok := firstQueryCall(info, call); ok && nestedQuery == query {
			if name, recvPkg, ok := routes.GinMethod(info, call); ok && recvPkg == routes.GinPkgPath && isGinQueryMethod(name) {
				return true
			}
			if queryHelperPriority(info, call, query) > currentPriority {
				found = true
				return false
			}
		}
		return true
	})
	return found
}

func queryHelperPriority(info *gotypes.Info, helper *ast.CallExpr, query *ast.CallExpr) int {
	if info == nil || helper == nil {
		return 0
	}
	schema, ok := queryHelperSchema(info.TypeOf(helper), helper)
	if !ok {
		return 0
	}
	if !isPrimitiveStringType(schema) {
		return 4
	}
	if queryDefaultValue(info, query) != nil || queryHelperDefault(info, helper, query) != nil || helperReturnsError(info, helper) {
		return 3
	}
	name := strings.ToLower(selectorName(helper.Fun))
	if name != "trimspace" && (strings.Contains(name, "optional") || strings.Contains(name, "required")) {
		return 2
	}
	return 1
}

func isPrimitiveStringType(schema facts.Type) bool {
	if schema.Type != facts.TypePrimitive {
		return false
	}
	prim, ok := schema.Of.(*facts.Prim)
	return ok && prim.Prim == facts.PrimString
}

func helperReturnsError(info *gotypes.Info, helper *ast.CallExpr) bool {
	if info == nil || helper == nil {
		return false
	}
	t := info.TypeOf(helper)
	if tuple, ok := gotypes.Unalias(t).(*gotypes.Tuple); ok && tuple.Len() > 1 {
		last := tuple.At(tuple.Len() - 1)
		return last != nil && isErrorType(last.Type())
	}
	return false
}

func firstQueryCall(info *gotypes.Info, root ast.Expr) (*ast.CallExpr, bool) {
	var found *ast.CallExpr
	ast.Inspect(root, func(n ast.Node) bool {
		if found != nil {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(info, call)
		if ok && recvPkg == routes.GinPkgPath && isGinQueryMethod(name) {
			found = call
			return false
		}
		return true
	})
	return found, found != nil
}

func isGinQueryMethod(name string) bool {
	switch name {
	case "Query", "DefaultQuery", "GetQuery":
		return true
	default:
		return false
	}
}

func queryDefaultValue(info *gotypes.Info, query *ast.CallExpr) *facts.LiteralValue {
	if query == nil || selectorName(query.Fun) != "DefaultQuery" || len(query.Args) < 2 {
		return nil
	}
	return literalValue(info, query.Args[1])
}

func queryHelperSchema(t gotypes.Type, helper *ast.CallExpr) (facts.Type, bool) {
	if t == nil {
		if selectorName(helper.Fun) == "TrimSpace" {
			return facts.PrimitiveType(facts.StringPrim()), true
		}
		return facts.Type{}, false
	}
	if tuple, ok := gotypes.Unalias(t).(*gotypes.Tuple); ok && tuple.Len() > 0 {
		t = tuple.At(0).Type()
	}
	if ptr, ok := gotypes.Unalias(t).(*gotypes.Pointer); ok {
		t = ptr.Elem()
	}
	if named, ok := gotypes.Unalias(t).(*gotypes.Named); ok {
		if obj := named.Obj(); obj != nil && obj.Pkg() != nil {
			if obj.Pkg().Path() == "github.com/google/uuid" && obj.Name() == "UUID" {
				return facts.WellKnownType(facts.WellKnownUUID), true
			}
			if obj.Pkg().Path() == "time" && obj.Name() == "Time" {
				return facts.WellKnownType(facts.WellKnownDateTime), true
			}
		}
		t = named.Underlying()
	}
	if basic, ok := gotypes.Unalias(t).(*gotypes.Basic); ok {
		switch basic.Kind() {
		case gotypes.String:
			return facts.PrimitiveType(facts.StringPrim()), true
		case gotypes.Bool:
			return facts.PrimitiveType(facts.BoolPrim()), true
		case gotypes.Int, gotypes.Int8, gotypes.Int16, gotypes.Int32, gotypes.Int64:
			return facts.PrimitiveType(facts.IntPrim(64, true)), true
		case gotypes.Uint, gotypes.Uint8, gotypes.Uint16, gotypes.Uint32, gotypes.Uint64:
			return facts.PrimitiveType(facts.IntPrim(64, false)), true
		case gotypes.Float32:
			return facts.PrimitiveType(facts.FloatPrim(32)), true
		case gotypes.Float64:
			return facts.PrimitiveType(facts.FloatPrim(64)), true
		}
	}
	if selectorName(helper.Fun) == "TrimSpace" {
		return facts.PrimitiveType(facts.StringPrim()), true
	}
	return facts.Type{}, false
}

func queryHelperRequired(info *gotypes.Info, helper *ast.CallExpr, query *ast.CallExpr) bool {
	name := selectorName(helper.Fun)
	if strings.Contains(strings.ToLower(name), "optional") {
		return false
	}
	if queryHelperDefault(info, helper, query) != nil {
		return false
	}
	if queryDefaultValue(info, query) != nil {
		return false
	}
	if name == "TrimSpace" || strings.Contains(strings.ToLower(name), "required") {
		return true
	}
	t := info.TypeOf(helper)
	if t == nil {
		return false
	}
	if tuple, ok := gotypes.Unalias(t).(*gotypes.Tuple); ok && tuple.Len() > 1 {
		last := tuple.At(tuple.Len() - 1)
		return last != nil && isErrorType(last.Type())
	}
	return false
}

func queryHelperRequiredFromCallee(info *gotypes.Info, helper *ast.CallExpr, callee handlerDecl, query *ast.CallExpr) bool {
	lowerName := strings.ToLower(selectorName(helper.Fun))
	if strings.Contains(lowerName, "optional") {
		return false
	}
	if queryHelperDefaultFromArgs(info, helper, signatureOf(info, helper.Fun)) != nil {
		return false
	}
	if queryDefaultValue(callee.info, query) != nil {
		return false
	}
	if selectorName(query.Fun) == "DefaultQuery" || selectorName(query.Fun) == "GetQuery" {
		return false
	}
	if strings.Contains(lowerName, "required") {
		return true
	}
	if helperEmptyStringTolerant(callee, query) {
		return false
	}
	return helperReturnsError(info, helper)
}

func queryHelperRequiredFromDirectCallee(info *gotypes.Info, helper *ast.CallExpr, callee handlerDecl, query *ast.CallExpr) bool {
	lowerName := strings.ToLower(selectorName(helper.Fun))
	if strings.Contains(lowerName, "optional") {
		return false
	}
	if queryHelperDefault(info, helper, query) != nil {
		return false
	}
	if queryDefaultValue(info, query) != nil {
		return false
	}
	if selectorName(query.Fun) == "DefaultQuery" || selectorName(query.Fun) == "GetQuery" {
		return false
	}
	if strings.Contains(lowerName, "required") {
		return true
	}
	sig := signatureOf(info, helper.Fun)
	if sig != nil && sig.Params() != nil {
		for i, arg := range helper.Args {
			if i >= sig.Params().Len() || !exprContainsCall(arg, query) {
				continue
			}
			if helperParamEmptyStringTolerant(callee, sig.Params().At(i)) {
				return false
			}
		}
	}
	return helperReturnsError(info, helper)
}

func signatureOf(info *gotypes.Info, expr ast.Expr) *gotypes.Signature {
	sig, _ := gotypes.Unalias(info.TypeOf(expr)).(*gotypes.Signature)
	return sig
}

func queryHelperDefault(info *gotypes.Info, helper *ast.CallExpr, query *ast.CallExpr) *facts.LiteralValue {
	sig, ok := gotypes.Unalias(info.TypeOf(helper.Fun)).(*gotypes.Signature)
	if !ok || sig.Params() == nil {
		return nil
	}
	afterQuery := query == nil
	for i, arg := range helper.Args {
		if !afterQuery {
			if arg == query || exprContainsCall(arg, query) {
				afterQuery = true
			}
			continue
		}
		if i >= sig.Params().Len() || !isDefaultParamName(sig.Params().At(i).Name()) {
			continue
		}
		if value := literalValue(info, arg); value != nil {
			return value
		}
	}
	return nil
}

func queryHelperDefaultFromArgs(info *gotypes.Info, helper *ast.CallExpr, sig *gotypes.Signature) *facts.LiteralValue {
	if sig == nil || sig.Params() == nil {
		return nil
	}
	for i, arg := range helper.Args {
		if i >= sig.Params().Len() || !isDefaultParamName(sig.Params().At(i).Name()) {
			continue
		}
		if value := literalValue(info, arg); value != nil {
			return value
		}
	}
	return nil
}

func helperEmptyStringTolerant(h handlerDecl, query *ast.CallExpr) bool {
	queryVars := queryResultVars(h, query)
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		ifStmt, ok := n.(*ast.IfStmt)
		if !ok || !exprChecksEmptyString(h.info, ifStmt.Cond, queryVars) {
			return true
		}
		for _, stmt := range ifStmt.Body.List {
			ret, ok := stmt.(*ast.ReturnStmt)
			if !ok || len(ret.Results) == 0 {
				continue
			}
			if returnTreatsEmptyStringAsOptional(ret) {
				found = true
				return false
			}
		}
		return true
	})
	return found
}

func helperParamEmptyStringTolerant(h handlerDecl, param *gotypes.Var) bool {
	if h.decl == nil || h.decl.Body == nil || param == nil {
		return false
	}
	queryVars := map[gotypes.Object]bool{param: true}
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		ifStmt, ok := n.(*ast.IfStmt)
		if !ok || !exprChecksEmptyString(h.info, ifStmt.Cond, queryVars) {
			return true
		}
		for _, stmt := range ifStmt.Body.List {
			ret, ok := stmt.(*ast.ReturnStmt)
			if !ok || len(ret.Results) == 0 {
				continue
			}
			if returnTreatsEmptyStringAsOptional(ret) {
				found = true
				return false
			}
		}
		return true
	})
	return found
}

func returnTreatsEmptyStringAsOptional(ret *ast.ReturnStmt) bool {
	if ret == nil || len(ret.Results) < 2 || !isNilIdent(ret.Results[len(ret.Results)-1]) {
		return false
	}
	for _, result := range ret.Results[:len(ret.Results)-1] {
		if isNilIdent(result) {
			return true
		}
	}
	return false
}

func queryResultVars(h handlerDecl, query *ast.CallExpr) map[gotypes.Object]bool {
	out := map[gotypes.Object]bool{}
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		assign, ok := n.(*ast.AssignStmt)
		if !ok {
			return true
		}
		for i, rhs := range assign.Rhs {
			if i >= len(assign.Lhs) || !exprContainsCall(rhs, query) {
				continue
			}
			id, ok := assign.Lhs[i].(*ast.Ident)
			if ok {
				if obj := h.info.ObjectOf(id); obj != nil {
					out[obj] = true
				}
			}
		}
		return true
	})
	return out
}

func exprChecksEmptyString(info *gotypes.Info, expr ast.Expr, queryVars map[gotypes.Object]bool) bool {
	switch node := expr.(type) {
	case *ast.BinaryExpr:
		if node.Op == token.EQL || node.Op == token.NEQ {
			return (exprIsQueryValue(info, node.X, queryVars) && isEmptyStringLiteral(node.Y)) ||
				(isEmptyStringLiteral(node.X) && exprIsQueryValue(info, node.Y, queryVars))
		}
		return exprChecksEmptyString(info, node.X, queryVars) || exprChecksEmptyString(info, node.Y, queryVars)
	case *ast.ParenExpr:
		return exprChecksEmptyString(info, node.X, queryVars)
	}
	return false
}

func exprIsQueryValue(info *gotypes.Info, expr ast.Expr, queryVars map[gotypes.Object]bool) bool {
	if call, ok := expr.(*ast.CallExpr); ok {
		if selectorName(call.Fun) == "TrimSpace" && len(call.Args) == 1 {
			return exprIsQueryValue(info, call.Args[0], queryVars)
		}
		name, recvPkg, ok := routes.GinMethod(info, call)
		return ok && recvPkg == routes.GinPkgPath && isGinQueryMethod(name)
	}
	id, ok := expr.(*ast.Ident)
	return ok && queryVars[info.ObjectOf(id)]
}

func isNilIdent(expr ast.Expr) bool {
	id, ok := expr.(*ast.Ident)
	return ok && id.Name == "nil"
}

func exprContainsCall(expr ast.Expr, target *ast.CallExpr) bool {
	if expr == nil || target == nil {
		return false
	}
	found := false
	ast.Inspect(expr, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if ok && call == target {
			found = true
			return false
		}
		return true
	})
	return found
}

func isDefaultParamName(name string) bool {
	lower := strings.ToLower(name)
	return strings.Contains(lower, "default") || strings.Contains(lower, "fallback")
}

func literalValue(info *gotypes.Info, expr ast.Expr) *facts.LiteralValue {
	if value := constValueOf(info, expr); value != nil {
		return literalValueFromConstant(value)
	}
	switch value := expr.(type) {
	case *ast.BasicLit:
		switch value.Kind {
		case token.STRING:
			unquoted, err := strconv.Unquote(value.Value)
			if err != nil {
				return nil
			}
			return &facts.LiteralValue{Type: "string", Value: unquoted}
		case token.INT, token.FLOAT:
			return &facts.LiteralValue{Type: "number", Value: value.Value}
		}
	case *ast.Ident:
		if value.Name == "true" {
			return &facts.LiteralValue{Type: "bool", Value: true}
		}
		if value.Name == "false" {
			return &facts.LiteralValue{Type: "bool", Value: false}
		}
	}
	return nil
}

func constValueOf(info *gotypes.Info, expr ast.Expr) constant.Value {
	if info == nil || expr == nil {
		return nil
	}
	if tv, ok := info.Types[expr]; ok && tv.Value != nil {
		return tv.Value
	}
	if ident := leafIdent(expr); ident != nil {
		if c, ok := info.ObjectOf(ident).(*gotypes.Const); ok {
			return c.Val()
		}
	}
	return nil
}

func literalValueFromConstant(value constant.Value) *facts.LiteralValue {
	switch value.Kind() {
	case constant.String:
		return &facts.LiteralValue{Type: "string", Value: constant.StringVal(value)}
	case constant.Bool:
		return &facts.LiteralValue{Type: "bool", Value: constant.BoolVal(value)}
	case constant.Int:
		return &facts.LiteralValue{Type: "number", Value: value.ExactString()}
	case constant.Float:
		number := value.ExactString()
		if strings.Contains(number, "/") {
			if f, ok := constant.Float64Val(value); ok {
				number = strconv.FormatFloat(f, 'g', -1, 64)
			}
		}
		return &facts.LiteralValue{Type: "number", Value: number}
	default:
		return nil
	}
}

func firstLiteral(values ...*facts.LiteralValue) *facts.LiteralValue {
	for _, value := range values {
		if value != nil {
			return value
		}
	}
	return nil
}

func isErrorType(t gotypes.Type) bool {
	named, ok := gotypes.Unalias(t).(*gotypes.Named)
	if !ok || named.Obj() == nil {
		return false
	}
	return named.Obj().Name() == "error" && named.Obj().Pkg() == nil
}

func selectorName(expr ast.Expr) string {
	switch fun := expr.(type) {
	case *ast.SelectorExpr:
		return fun.Sel.Name
	case *ast.Ident:
		return fun.Name
	default:
		return ""
	}
}

// bindRequestType resolves ShouldBindJSON(&x) / ShouldBind(&x) -> a TypeRef to the bound named type.
// arg[0] must be &ident (an *ast.UnaryExpr with Op AND). When the type cannot be
// resolved to a named type, returns ok=false and the route simply has no request
// body — there is no secondary source (CLAUDE.md rule 3).
func (a *Analyzer) bindRequestType(info *gotypes.Info, call *ast.CallExpr) (*facts.TypeRef, gotypes.Type, bool) {
	if len(call.Args) < 1 {
		return nil, nil, false
	}
	unary, ok := call.Args[0].(*ast.UnaryExpr)
	if !ok || unary.Op != token.AND {
		return nil, nil, false
	}
	bound := info.TypeOf(unary.X)
	id, ok := a.namedTypeID(bound)
	if !ok {
		return nil, nil, false
	}
	return &facts.TypeRef{RefID: id}, bound, true
}

func bindContentType(method string, info *gotypes.Info, call *ast.CallExpr, bound gotypes.Type) string {
	switch explicitBindingName(info, call) {
	case "FormMultipart":
		return "multipart/form-data"
	case "Form", "FormPost":
		return "application/x-www-form-urlencoded"
	case "JSON":
		return ""
	}
	if typeContainsMultipartFile(bound) {
		return "multipart/form-data"
	}
	if typeHasFormTag(bound) {
		return "application/x-www-form-urlencoded"
	}
	if method == "Bind" || method == "ShouldBind" {
		return ""
	}
	return ""
}

func explicitBindingName(info *gotypes.Info, call *ast.CallExpr) string {
	if len(call.Args) < 2 {
		return ""
	}
	switch arg := call.Args[1].(type) {
	case *ast.SelectorExpr:
		return arg.Sel.Name
	case *ast.Ident:
		if obj := info.ObjectOf(arg); obj != nil {
			return obj.Name()
		}
		return arg.Name
	default:
		return ""
	}
}

func formField(name string, schema facts.Type, required bool) facts.FieldFact {
	return facts.FieldFact{
		JSONName: name,
		Required: required,
		Optional: !required,
		Nullable: false,
		Schema:   schema,
	}
}

func syntheticFormSchemaIdentity(handler string) (id string, name string) {
	base := exportedIdentifier(handler)
	if base == "" {
		base = "Request"
	}
	name = base + "FormRequest"
	return "__synthetic." + name, name
}

func syntheticRawJSONRequestSchemaIdentity(handler string) (id string, name string) {
	base := exportedIdentifier(handler)
	if base == "" {
		base = "Request"
	}
	name = base + "RawJSONRequest"
	return "__synthetic." + name, name
}

func syntheticResponseSchemaIdentity(handler string, status uint16) (id string, name string) {
	base := exportedIdentifier(handler)
	if base == "" {
		base = "Response"
	}
	name = base + strconv.FormatUint(uint64(status), 10) + "Response"
	return "__synthetic." + name, name
}

func exportedIdentifier(name string) string {
	if name == "" {
		return ""
	}
	return strings.ToUpper(name[:1]) + name[1:]
}

func typeContainsMultipartFile(t gotypes.Type) bool {
	return typeContainsMultipartFileSeen(t, map[string]bool{})
}

func typeContainsMultipartFileSeen(t gotypes.Type, seen map[string]bool) bool {
	if t == nil {
		return false
	}
	key := t.String()
	if seen[key] {
		return false
	}
	seen[key] = true
	switch u := gotypes.Unalias(t).(type) {
	case *gotypes.Pointer:
		return typeContainsMultipartFileSeen(u.Elem(), seen)
	case *gotypes.Slice:
		return typeContainsMultipartFileSeen(u.Elem(), seen)
	case *gotypes.Array:
		return typeContainsMultipartFileSeen(u.Elem(), seen)
	case *gotypes.Named:
		obj := u.Obj()
		if obj != nil && obj.Pkg() != nil && obj.Pkg().Path() == "mime/multipart" && obj.Name() == "FileHeader" {
			return true
		}
		return typeContainsMultipartFileSeen(u.Underlying(), seen)
	case *gotypes.Struct:
		for i := 0; i < u.NumFields(); i++ {
			if typeContainsMultipartFileSeen(u.Field(i).Type(), seen) {
				return true
			}
		}
	}
	return false
}

func typeHasFormTag(t gotypes.Type) bool {
	return typeHasFormTagSeen(t, map[string]bool{})
}

func typeHasFormTagSeen(t gotypes.Type, seen map[string]bool) bool {
	if t == nil {
		return false
	}
	key := t.String()
	if seen[key] {
		return false
	}
	seen[key] = true
	switch u := gotypes.Unalias(t).(type) {
	case *gotypes.Pointer:
		return typeHasFormTagSeen(u.Elem(), seen)
	case *gotypes.Named:
		return typeHasFormTagSeen(u.Underlying(), seen)
	case *gotypes.Struct:
		for i := 0; i < u.NumFields(); i++ {
			tag := reflect.StructTag(u.Tag(i))
			if raw, ok := tag.Lookup("form"); ok && raw != "" && raw != "-" {
				return true
			}
			if u.Field(i).Embedded() && typeHasFormTagSeen(u.Field(i).Type(), seen) {
				return true
			}
		}
	}
	return false
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
	provisionalStatus map[uint16]bool,
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

	var body *facts.TypeRef
	if id, schema, ok := a.syntheticJSONResponse(h, call.Args[1], route.Handler, status); ok {
		body = &facts.TypeRef{RefID: id}
		cf.Schemas = append(cf.Schemas, schema)
	} else if id, ok := a.namedTypeID(h.info.TypeOf(call.Args[1])); ok {
		body = &facts.TypeRef{RefID: id}
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "response body does not resolve to a named type", file, line)
	}
	file, line := positionOf(h.fset, call.Pos())
	a.addJSONResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{
		Status:       status,
		Body:         body,
		ContentTypes: []string{"application/json"},
	}, route, diags, file, line)
}

func (a *Analyzer) analyzeStatus(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	provisional bool,
	diags *diag.Accumulator,
) {
	if len(call.Args) < 1 {
		return
	}
	status, ok := statusOf(h.info, call.Args[0])
	if !ok {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "non-constant HTTP status", file, line)
		return
	}
	a.addResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{Status: status}, provisional)
}

func (a *Analyzer) analyzeBinaryStatus(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	status uint16,
	contentType string,
) {
	a.addResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{
		Status:       status,
		BodyKind:     "binary",
		ContentType:  responseContentType(contentType),
		ContentTypes: responseContentTypes(responseContentType(contentType)),
	}, false)
}

func (a *Analyzer) addSSEResponse(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
) {
	a.addResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{
		Status:       200,
		BodyKind:     "sse",
		ContentType:  "text/event-stream",
		ContentTypes: []string{"text/event-stream"},
	}, false)
}

func streamCallContainsSSEvent(info *gotypes.Info, stream *ast.CallExpr) bool {
	found := false
	for _, arg := range stream.Args {
		ast.Inspect(arg, func(n ast.Node) bool {
			if found {
				return false
			}
			call, ok := n.(*ast.CallExpr)
			if !ok {
				return true
			}
			name, recvPkg, ok := routes.GinMethod(info, call)
			if ok && recvPkg == routes.GinPkgPath && name == "SSEvent" {
				found = true
				return false
			}
			return true
		})
	}
	return found
}

func (a *Analyzer) analyzeData(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	diags *diag.Accumulator,
) {
	if len(call.Args) < 3 {
		return
	}
	status, ok := statusOf(h.info, call.Args[0])
	if !ok {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "non-constant HTTP status", file, line)
		return
	}
	if !isByteSlice(h.info.TypeOf(call.Args[2])) {
		file, line := positionOf(h.fset, call.Pos())
		diags.Warn("unsupported binary response pattern: Gin Data payload is not []byte (GO-05)", file, line)
		return
	}
	contentType := "application/octet-stream"
	if value, ok := a.stringValueOf(h, call.Args[1]); ok {
		contentType = responseContentType(value)
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.Warn("unsupported binary response pattern: Gin Data content type is dynamic; defaulting to application/octet-stream (GO-05)", file, line)
	}
	a.addResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{
		Status:       status,
		BodyKind:     "binary",
		ContentType:  contentType,
		ContentTypes: responseContentTypes(contentType),
	}, false)
}

func (a *Analyzer) analyzeDataFromReader(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	diags *diag.Accumulator,
) {
	if len(call.Args) < 3 {
		return
	}
	status, ok := statusOf(h.info, call.Args[0])
	if !ok {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "non-constant HTTP status", file, line)
		return
	}
	contentType := "application/octet-stream"
	if value, ok := a.stringValueOf(h, call.Args[2]); ok {
		contentType = responseContentType(value)
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.Warn("unsupported binary response pattern: Gin DataFromReader content type is dynamic; defaulting to application/octet-stream (GO-05)", file, line)
	}
	a.addResponse(cf, seenStatus, provisionalStatus, facts.ResponseFact{
		Status:       status,
		BodyKind:     "binary",
		ContentType:  contentType,
		ContentTypes: responseContentTypes(contentType),
	}, false)
}

func (a *Analyzer) addResponse(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	response facts.ResponseFact,
	provisional bool,
) {
	if seenStatus[response.Status] {
		if provisionalStatus[response.Status] && !provisional {
			for i := range cf.Responses {
				if cf.Responses[i].Status == response.Status {
					cf.Responses[i] = response
					break
				}
			}
			provisionalStatus[response.Status] = false
		}
		return
	}
	seenStatus[response.Status] = true
	provisionalStatus[response.Status] = provisional
	cf.Responses = append(cf.Responses, response)
}

func (a *Analyzer) addJSONResponse(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	provisionalStatus map[uint16]bool,
	response facts.ResponseFact,
	route routes.Route,
	diags *diag.Accumulator,
	file string,
	line uint32,
) {
	if !seenStatus[response.Status] {
		a.addResponse(cf, seenStatus, provisionalStatus, response, false)
		return
	}
	idx := responseIndex(cf.Responses, response.Status)
	if idx < 0 {
		return
	}
	if provisionalStatus[response.Status] {
		cf.Responses[idx] = response
		provisionalStatus[response.Status] = false
		return
	}
	if response.Status < 200 || response.Status > 299 {
		return
	}
	existing := cf.Responses[idx]
	existingPriority := successResponsePriority(existing)
	newPriority := successResponsePriority(response)
	switch {
	case newPriority > existingPriority:
		cf.Responses[idx] = response
	case newPriority == existingPriority && newPriority >= 3 && response.Body != nil && existing.Body != nil && response.Body.RefID != existing.Body.RefID:
		if diags != nil {
			diags.Warn("conflicting typed success responses for "+route.Handler+" status "+strconv.FormatUint(uint64(response.Status), 10)+"; keeping first response body "+existing.Body.RefID+" (GO-05)", file, line)
		}
	}
}

func responseIndex(responses []facts.ResponseFact, status uint16) int {
	for i := range responses {
		if responses[i].Status == status {
			return i
		}
	}
	return -1
}

func successResponsePriority(response facts.ResponseFact) int {
	if response.Body == nil {
		return 0
	}
	if isErrorishRef(response.Body.RefID) {
		return 1
	}
	if strings.HasPrefix(response.Body.RefID, "__synthetic.") {
		return 2
	}
	return 3
}

func isErrorishRef(ref string) bool {
	parts := strings.Split(ref, ".")
	name := strings.ToLower(parts[len(parts)-1])
	return strings.Contains(name, "error") || strings.Contains(name, "problem") || strings.Contains(name, "failure")
}

func responseContentType(contentType string) string {
	if contentType == "" {
		return "application/octet-stream"
	}
	return contentType
}

func responseContentTypes(contentType string) []string {
	if contentType == "" {
		return nil
	}
	return []string{contentType}
}

func (a *Analyzer) stringValueOf(h handlerDecl, expr ast.Expr) (string, bool) {
	return a.stringValueOfSeen(h, expr, map[string]bool{})
}

func (a *Analyzer) stringValueOfSeen(h handlerDecl, expr ast.Expr, seen map[string]bool) (string, bool) {
	if lit, ok := expr.(*ast.BasicLit); ok && lit.Kind == token.STRING {
		value, err := strconv.Unquote(lit.Value)
		if err == nil {
			return value, true
		}
		return strings.Trim(lit.Value, "`\""), true
	}
	if tv, ok := h.info.Types[expr]; ok && tv.Value != nil && tv.Value.Kind() == constant.String {
		return constant.StringVal(tv.Value), true
	}
	if ident := leafIdent(expr); ident != nil {
		if c, ok := h.info.ObjectOf(ident).(*gotypes.Const); ok && c.Val() != nil && c.Val().Kind() == constant.String {
			return constant.StringVal(c.Val()), true
		}
	}
	call, ok := expr.(*ast.CallExpr)
	if !ok || len(call.Args) != 0 {
		return "", false
	}
	if _, ok := call.Fun.(*ast.Ident); !ok {
		return "", false
	}
	fn := calledFuncObject(h.info, call.Fun)
	callee, ok := a.samePackageCallee(h, fn)
	if !ok {
		return "", false
	}
	key := funcObjectKey(fn)
	if key == "" || seen[key] {
		return "", false
	}
	seen[key] = true
	if len(callee.decl.Body.List) != 1 {
		return "", false
	}
	ret, ok := callee.decl.Body.List[0].(*ast.ReturnStmt)
	if !ok || len(ret.Results) != 1 {
		return "", false
	}
	return a.stringValueOfSeen(callee, ret.Results[0], seen)
}

func calledFuncObject(info *gotypes.Info, fun ast.Expr) *gotypes.Func {
	if info == nil {
		return nil
	}
	switch f := fun.(type) {
	case *ast.IndexExpr:
		return calledFuncObject(info, f.X)
	case *ast.IndexListExpr:
		return calledFuncObject(info, f.X)
	case *ast.Ident:
		fn, _ := info.ObjectOf(f).(*gotypes.Func)
		return fn
	case *ast.SelectorExpr:
		if sel := info.Selections[f]; sel != nil {
			fn, _ := sel.Obj().(*gotypes.Func)
			return fn
		}
		fn, _ := info.ObjectOf(f.Sel).(*gotypes.Func)
		return fn
	default:
		return nil
	}
}

func (a *Analyzer) rawJSONRequestSchema(h handlerDecl, handler string) *facts.SchemaFact {
	if !handlerUsesRawJSONBody(h) {
		return nil
	}
	id, name := syntheticRawJSONRequestSchemaIdentity(handler)
	return &facts.SchemaFact{
		ID:   id,
		Name: name,
		Body: facts.AnyType(),
		Span: spanOf(h.fset, declPos(h.decl)),
	}
}

func handlerUsesRawJSONBody(h handlerDecl) bool {
	if h.decl == nil || h.decl.Body == nil {
		return false
	}
	rawVars := rawDataVars(h)
	if len(rawVars) == 0 {
		return false
	}
	if rawDataUsedByEncodingJSON(h, rawVars) {
		return true
	}
	return hasJSONContentTypeEvidence(h)
}

func rawDataVars(h handlerDecl) map[gotypes.Object]bool {
	out := map[gotypes.Object]bool{}
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		switch node := n.(type) {
		case *ast.AssignStmt:
			for i, rhs := range node.Rhs {
				if i >= len(node.Lhs) || !isGinGetRawDataCall(h.info, rhs) {
					continue
				}
				if id, ok := node.Lhs[i].(*ast.Ident); ok {
					if obj := h.info.ObjectOf(id); obj != nil {
						out[obj] = true
					}
				}
			}
		case *ast.ValueSpec:
			for i, rhs := range node.Values {
				if i >= len(node.Names) || !isGinGetRawDataCall(h.info, rhs) {
					continue
				}
				if obj := h.info.ObjectOf(node.Names[i]); obj != nil {
					out[obj] = true
				}
			}
		}
		return true
	})
	return out
}

func isGinGetRawDataCall(info *gotypes.Info, expr ast.Expr) bool {
	call, ok := expr.(*ast.CallExpr)
	if !ok {
		return false
	}
	name, recvPkg, ok := routes.GinMethod(info, call)
	return ok && recvPkg == routes.GinPkgPath && name == "GetRawData"
}

func rawDataUsedByEncodingJSON(h handlerDecl, rawVars map[gotypes.Object]bool) bool {
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		fn := calledFuncObject(h.info, call.Fun)
		if fn == nil || fn.Pkg() == nil || fn.Pkg().Path() != "encoding/json" {
			return true
		}
		switch fn.Name() {
		case "Unmarshal", "Valid":
			if len(call.Args) > 0 && exprUsesObject(h.info, call.Args[0], rawVars) {
				found = true
				return false
			}
		case "Compact", "Indent", "HTMLEscape":
			if len(call.Args) > 1 && exprUsesObject(h.info, call.Args[1], rawVars) {
				found = true
				return false
			}
		}
		return true
	})
	return found
}

func exprUsesObject(info *gotypes.Info, expr ast.Expr, targets map[gotypes.Object]bool) bool {
	found := false
	ast.Inspect(expr, func(n ast.Node) bool {
		if found {
			return false
		}
		id, ok := n.(*ast.Ident)
		if !ok {
			return true
		}
		if targets[info.ObjectOf(id)] {
			found = true
			return false
		}
		return true
	})
	return found
}

func hasJSONContentTypeEvidence(h handlerDecl) bool {
	found := false
	ast.Inspect(h.decl.Body, func(n ast.Node) bool {
		if found {
			return false
		}
		if expr, ok := n.(*ast.BinaryExpr); ok {
			if exprMentionsGinContentType(h.info, expr) && exprHasStringLiteral(expr, "application/json") {
				found = true
				return false
			}
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		if callSubtreeHasStringLiteral(call, "application/json") && exprMentionsGinContentType(h.info, call) {
			found = true
			return false
		}
		return true
	})
	return found
}

func exprMentionsGinContentType(info *gotypes.Info, expr ast.Expr) bool {
	found := false
	ast.Inspect(expr, func(n ast.Node) bool {
		if found {
			return false
		}
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := routes.GinMethod(info, call)
		if ok && recvPkg == routes.GinPkgPath && name == "ContentType" {
			found = true
			return false
		}
		if ok && recvPkg == routes.GinPkgPath && name == "GetHeader" {
			key, ok := stringArg(call, 0)
			if ok && strings.EqualFold(key, "Content-Type") {
				found = true
				return false
			}
		}
		return true
	})
	return found
}

func exprHasStringLiteral(expr ast.Expr, value string) bool {
	return callSubtreeHasStringLiteral(expr, value)
}

func callSubtreeHasStringLiteral(root ast.Node, value string) bool {
	found := false
	ast.Inspect(root, func(n ast.Node) bool {
		if found {
			return false
		}
		lit, ok := n.(*ast.BasicLit)
		if !ok || lit.Kind != token.STRING {
			return true
		}
		unquoted, err := strconv.Unquote(lit.Value)
		if err == nil && strings.EqualFold(unquoted, value) {
			found = true
			return false
		}
		return true
	})
	return found
}

func isByteSlice(t gotypes.Type) bool {
	if t == nil {
		return false
	}
	if named, ok := gotypes.Unalias(t).(*gotypes.Named); ok {
		t = named.Underlying()
	}
	slice, ok := gotypes.Unalias(t).(*gotypes.Slice)
	if !ok {
		return false
	}
	basic, ok := gotypes.Unalias(slice.Elem()).(*gotypes.Basic)
	return ok && basic.Kind() == gotypes.Uint8
}

func (a *Analyzer) syntheticJSONResponse(
	h handlerDecl,
	expr ast.Expr,
	handler string,
	status uint16,
) (string, facts.SchemaFact, bool) {
	if schema, ok := a.arrayResponseSchema(h.info.TypeOf(expr), handler, status, h.fset, expr.Pos()); ok {
		return schema.ID, schema, true
	}
	if !isGinH(h.info.TypeOf(expr)) {
		return "", facts.SchemaFact{}, false
	}
	schema := a.ginHResponseSchema(h, expr, handler, status)
	return schema.ID, schema, true
}

func (a *Analyzer) arrayResponseSchema(
	t gotypes.Type,
	handler string,
	status uint16,
	fset *token.FileSet,
	pos token.Pos,
) (facts.SchemaFact, bool) {
	var elem gotypes.Type
	switch u := gotypes.Unalias(t).(type) {
	case *gotypes.Slice:
		elem = u.Elem()
	case *gotypes.Array:
		elem = u.Elem()
	default:
		return facts.SchemaFact{}, false
	}
	elemType, ok := a.responseType(elem)
	if !ok {
		return facts.SchemaFact{}, false
	}
	id, name := syntheticResponseSchemaIdentity(handler, status)
	return facts.SchemaFact{
		ID:   id,
		Name: name,
		Body: facts.ArrayType(elemType),
		Span: spanOf(fset, pos),
	}, true
}

func (a *Analyzer) ginHResponseSchema(
	h handlerDecl,
	expr ast.Expr,
	handler string,
	status uint16,
) facts.SchemaFact {
	id, name := syntheticResponseSchemaIdentity(handler, status)
	body := facts.MapTypeOf(facts.PrimitiveType(facts.StringPrim()), facts.AnyType())
	if lit, ok := expr.(*ast.CompositeLit); ok {
		if fields, ok := ginHLiteralFields(lit); ok {
			body = facts.ObjectType(fields)
		}
	}
	return facts.SchemaFact{
		ID:   id,
		Name: name,
		Body: body,
		Span: spanOf(h.fset, expr.Pos()),
	}
}

func ginHLiteralFields(lit *ast.CompositeLit) ([]facts.FieldFact, bool) {
	fields := make([]facts.FieldFact, 0, len(lit.Elts))
	for _, elt := range lit.Elts {
		kv, ok := elt.(*ast.KeyValueExpr)
		if !ok {
			return nil, false
		}
		key, ok := stringKey(kv.Key)
		if !ok {
			return nil, false
		}
		schema, ok := literalSchema(kv.Value)
		if !ok {
			return nil, false
		}
		fields = append(fields, facts.FieldFact{
			JSONName: key,
			Required: true,
			Optional: false,
			Nullable: false,
			Schema:   schema,
		})
	}
	return fields, true
}

func stringKey(expr ast.Expr) (string, bool) {
	lit, ok := expr.(*ast.BasicLit)
	if !ok || lit.Kind != token.STRING {
		return "", false
	}
	value, err := strconv.Unquote(lit.Value)
	if err != nil {
		return "", false
	}
	return value, true
}

func literalSchema(expr ast.Expr) (facts.Type, bool) {
	switch value := expr.(type) {
	case *ast.BasicLit:
		switch value.Kind {
		case token.STRING:
			return facts.PrimitiveType(facts.StringPrim()), true
		case token.INT:
			return facts.PrimitiveType(facts.IntPrim(64, true)), true
		case token.FLOAT:
			return facts.PrimitiveType(facts.FloatPrim(64)), true
		}
	case *ast.Ident:
		if value.Name == "true" || value.Name == "false" {
			return facts.PrimitiveType(facts.BoolPrim()), true
		}
	}
	return facts.Type{}, false
}

func (a *Analyzer) responseType(t gotypes.Type) (facts.Type, bool) {
	if t == nil {
		return facts.Type{}, false
	}
	if p, ok := gotypes.Unalias(t).(*gotypes.Pointer); ok {
		t = p.Elem()
	}
	if id, ok := a.namedTypeID(t); ok {
		return facts.NamedType(id), true
	}
	if basic, ok := gotypes.Unalias(t).(*gotypes.Basic); ok {
		switch basic.Kind() {
		case gotypes.String:
			return facts.PrimitiveType(facts.StringPrim()), true
		case gotypes.Bool:
			return facts.PrimitiveType(facts.BoolPrim()), true
		case gotypes.Int, gotypes.Int8, gotypes.Int16, gotypes.Int32, gotypes.Int64:
			return facts.PrimitiveType(facts.IntPrim(64, true)), true
		case gotypes.Uint, gotypes.Uint8, gotypes.Uint16, gotypes.Uint32, gotypes.Uint64:
			return facts.PrimitiveType(facts.IntPrim(64, false)), true
		case gotypes.Float32:
			return facts.PrimitiveType(facts.FloatPrim(32)), true
		case gotypes.Float64:
			return facts.PrimitiveType(facts.FloatPrim(64)), true
		}
	}
	return facts.Type{}, false
}

func isGinH(t gotypes.Type) bool {
	named, ok := gotypes.Unalias(t).(*gotypes.Named)
	if !ok || named.Obj() == nil || named.Obj().Pkg() == nil {
		return false
	}
	return named.Obj().Pkg().Path() == routes.GinPkgPath && named.Obj().Name() == "H"
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

func queryParamFromGinCall(info *gotypes.Info, method, name string, fset *token.FileSet, call *ast.CallExpr) facts.ParamFact {
	param := queryParam(name, fset, call.Pos())
	if method == "DefaultQuery" {
		param.Default = queryDefaultValue(info, call)
	}
	return param
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
