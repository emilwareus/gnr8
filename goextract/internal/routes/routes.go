// Package routes recognizes the Gin route table of the target module (GO-04).
//
// Recognition is SEMANTIC, not textual: for every `*ast.CallExpr` whose Fun is a
// selector (`x.METHOD(...)`), it resolves the method's identity through
// `go/types` `Info.Selections` and gates on the receiver package path
// (`github.com/gin-gonic/gin`) — so `import grouter "...gin"` aliasing or a
// different gin version is irrelevant (RESEARCH Pattern 2, Anti-Pattern: never
// string-match the import alias).
//
// The fixture registers routes as:
//
//	api := h.Router.Group("/" + basePath) // dynamic prefix -> not folded
//	api.Use(h.AuthMiddleware)             // secures the whole group (D-04/D-14)
//	api.POST("/", h.createGoal)           // group-relative "/"
//	api.GET("/list", h.listGoals)         // "/list"
//	api.PUT("/:uuid", h.updateGoal)       // "/:uuid" -> "/{uuid}"
//	api.DELETE("/:uuid", h.deleteGoal)    // "/:uuid" -> "/{uuid}"
//
// The group prefix is a non-constant `"/" + basePath`, so routes are recorded
// group-relative with Gin `:param` normalized to OpenAPI `{param}`; the concrete
// `/goal` prefix is supplied later by the Rust lowering layer (it is not scraped
// from any annotation — CLAUDE.md rule 1). No Gin terms leak into the emitted
// facts — only router-agnostic HTTP routes.
package routes

import (
	"go/ast"
	"go/token"
	gotypes "go/types"
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
)

// GinPkgPath is the receiver package the recognizer gates on. Recognition keys on
// this resolved package path, never on the source identifier/alias (T-02-06).
// Exported so the handler analyzer (Task 2) shares the exact identity gate.
const GinPkgPath = "github.com/gin-gonic/gin"

// ginPkgPath is the unexported alias kept for in-package readability.
const ginPkgPath = GinPkgPath

// httpMethods is the set of *gin.RouterGroup methods that register a route.
var httpMethods = map[string]bool{
	"GET": true, "POST": true, "PUT": true, "DELETE": true,
	"PATCH": true, "HEAD": true, "OPTIONS": true,
}

// Route is one recognized HTTP route plus the handler symbol and enclosing group,
// so the handler analyzer can match a handler FuncDecl by symbol. Secured records
// whether the enclosing group carried a Use(middleware) call — pure code
// recognition, NOT a security fact: security for the generated API comes from the
// user's gnr8 config, never from the source (CLAUDE.md rule 4).
type Route struct {
	Method  string           // "POST"
	Path    string           // group-relative, normalized: "/", "/list", "/{uuid}"
	Handler string           // selector name of the handler arg, e.g. "createGoal"
	Group   string           // deepest static route group segment, e.g. "books"
	Secured bool             // the enclosing group had a Use(middleware) call
	Span    facts.SourceSpan // provenance of the METHOD registration call
}

// Recognize walks every target package's syntax and returns the recognized routes
// with group security propagated. Output order is not guaranteed; callers sort
// (facts.Marshal) before emit.
func Recognize(res *load.Result) []Route {
	return RecognizeWithDiagnostics(res, nil)
}

// RecognizeWithDiagnostics is Recognize plus diagnostics for unsupported route
// registration patterns. Existing callers that only need routes can use
// Recognize; the CLI sidecar passes the accumulator so dynamic route paths are
// visible during brownfield migration.
func RecognizeWithDiagnostics(res *load.Result, diags *diag.Accumulator) []Route {
	var out []Route
	for _, pkg := range res.Packages {
		if pkg.TypesInfo == nil {
			continue
		}
		for _, file := range pkg.Syntax {
			out = append(out, recognizeFile(file, pkg.TypesInfo, res.Fset, diags)...)
		}
	}
	return out
}

// groupInfo tracks, per receiver object (the `api` group variable), whether the
// group has been secured by a Use(...) call. Routes registered on a secured group
// inherit secured=true.
type groupInfo struct {
	secured bool
	prefix  string
}

type rawGroup struct {
	parent       gotypes.Object
	parentPrefix string
	prefix       string
	static       bool
	span         facts.SourceSpan
}

// recognizeFile collects routes from a single file. It performs two passes over the
// receiver objects so that a Use(...) appearing on the same group object (in any
// order) marks every route on that object secured (D-14): pass 1 records which
// group objects are secured; pass 2 emits routes with the resolved security.
func recognizeFile(file *ast.File, info *gotypes.Info, fset *token.FileSet, diags *diag.Accumulator) []Route {
	groups := map[gotypes.Object]*groupInfo{}
	rawGroups := map[gotypes.Object]rawGroup{}

	// Pass 1: find Group(...) assignments and Use(...) calls. Group prefix
	// resolution happens after the walk so nested groups are order-independent.
	ast.Inspect(file, func(n ast.Node) bool {
		recordGroupAssignment(info, rawGroups, fset, n)
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := ginMethod(info, call)
		if !ok || recvPkg != ginPkgPath || name != "Use" {
			return true
		}
		if obj := receiverObject(info, call); obj != nil {
			groupOf(groups, obj).secured = true
		}
		return true
	})
	for obj := range rawGroups {
		raw := rawGroups[obj]
		if diags != nil && !raw.static {
			diags.Warn("unsupported Gin route pattern: dynamic Gin group prefix; prefix skipped rather than guessed (GO-04)", raw.span.File, raw.span.StartLine)
		}
		g := groupOf(groups, obj)
		g.prefix = resolveGroupPrefix(obj, rawGroups, map[gotypes.Object]bool{})
	}

	// Pass 2: emit one Route per recognized METHOD(path, handler) call.
	var out []Route
	ast.Inspect(file, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		name, recvPkg, ok := ginMethod(info, call)
		if !ok || recvPkg != ginPkgPath || !httpMethods[name] {
			return true
		}
		if len(call.Args) < 2 {
			return true // not a (path, handler) registration; skip defensively.
		}
		pathLit, ok := stringLiteral(call.Args[0])
		if !ok {
			if diags != nil {
				span := spanOf(fset, call.Pos(), call.End())
				diags.UnsupportedRoutePattern("dynamic route path for "+name+" registration", span.File, span.StartLine)
			}
			return true // dynamic path arg; not folded here.
		}
		handler := handlerSymbol(call.Args[len(call.Args)-1])
		if handler == "" {
			if diags != nil {
				span := spanOf(fset, call.Pos(), call.End())
				diags.UnsupportedRoutePattern("route handler for "+name+" "+pathLit+" is not a named function or method", span.File, span.StartLine)
			}
			return true
		}

		secured := false
		prefix := receiverPrefix(info, groups, call)
		if obj := receiverObject(info, call); obj != nil {
			if g, seen := groups[obj]; seen {
				secured = g.secured
			} else if prefix == "" && receiverIsGinRouterGroup(info, call) && diags != nil {
				span := spanOf(fset, call.Pos(), call.End())
				diags.Warn("unsupported Gin route pattern: route registered on router group parameter; prefix cannot be inferred across helper calls, so the route is emitted relative (GO-04)", span.File, span.StartLine)
			}
		}

		out = append(out, Route{
			Method:  name,
			Path:    joinPaths(prefix, normalizePath(pathLit)),
			Handler: handler,
			Group:   groupNameFromPrefix(prefix),
			Secured: secured,
			Span:    spanOf(fset, call.Pos(), call.End()),
		})
		return true
	})
	return out
}

func recordGroupAssignment(info *gotypes.Info, rawGroups map[gotypes.Object]rawGroup, fset *token.FileSet, n ast.Node) {
	switch node := n.(type) {
	case *ast.AssignStmt:
		for i, rhs := range node.Rhs {
			if i >= len(node.Lhs) {
				continue
			}
			obj := assignedIdentObject(info, node.Lhs[i])
			if obj == nil {
				continue
			}
			if group, ok := groupFromExpr(info, rhs, fset); ok {
				rawGroups[obj] = group
			}
		}
	case *ast.ValueSpec:
		for i, rhs := range node.Values {
			if i >= len(node.Names) {
				continue
			}
			obj := info.ObjectOf(node.Names[i])
			if obj == nil {
				continue
			}
			if group, ok := groupFromExpr(info, rhs, fset); ok {
				rawGroups[obj] = group
			}
		}
	}
}

func assignedIdentObject(info *gotypes.Info, expr ast.Expr) gotypes.Object {
	ident, ok := expr.(*ast.Ident)
	if !ok {
		return nil
	}
	return info.ObjectOf(ident)
}

func groupFromExpr(info *gotypes.Info, expr ast.Expr, fset *token.FileSet) (rawGroup, bool) {
	call, ok := expr.(*ast.CallExpr)
	if !ok {
		return rawGroup{}, false
	}
	name, recvPkg, ok := ginMethod(info, call)
	if !ok || recvPkg != ginPkgPath || name != "Group" {
		return rawGroup{}, false
	}
	prefix, static := "", false
	if len(call.Args) > 0 {
		prefix, static = stringLiteral(call.Args[0])
	}
	parentPrefix := ""
	if sel, ok := call.Fun.(*ast.SelectorExpr); ok {
		if parentGroup, ok := groupFromExpr(info, sel.X, fset); ok {
			parentPrefix = resolvedRawGroupPrefix(parentGroup, nil)
		}
	}
	var span facts.SourceSpan
	if fset != nil {
		span = spanOf(fset, call.Pos(), call.End())
	}
	return rawGroup{
		parent:       receiverObject(info, call),
		parentPrefix: parentPrefix,
		prefix:       normalizePath(prefix),
		static:       static,
		span:         span,
	}, true
}

func resolveGroupPrefix(obj gotypes.Object, rawGroups map[gotypes.Object]rawGroup, visiting map[gotypes.Object]bool) string {
	if obj == nil || visiting[obj] {
		return ""
	}
	group, ok := rawGroups[obj]
	if !ok {
		return ""
	}
	visiting[obj] = true
	parent := resolveGroupPrefix(group.parent, rawGroups, visiting)
	delete(visiting, obj)
	return joinPaths(parent, resolvedRawGroupPrefix(group, nil))
}

func receiverPrefix(info *gotypes.Info, groups map[gotypes.Object]*groupInfo, call *ast.CallExpr) string {
	if obj := receiverObject(info, call); obj != nil {
		if g, ok := groups[obj]; ok {
			return g.prefix
		}
	}
	sel, ok := call.Fun.(*ast.SelectorExpr)
	if !ok {
		return ""
	}
	return prefixFromExpr(info, groups, sel.X)
}

func prefixFromExpr(info *gotypes.Info, groups map[gotypes.Object]*groupInfo, expr ast.Expr) string {
	switch e := expr.(type) {
	case *ast.Ident:
		if obj := info.ObjectOf(e); obj != nil {
			if g, ok := groups[obj]; ok {
				return g.prefix
			}
		}
	case *ast.CallExpr:
		group, ok := groupFromExpr(info, e, nil)
		if !ok {
			return ""
		}
		parent := ""
		if group.parent != nil {
			if g, ok := groups[group.parent]; ok {
				parent = g.prefix
			}
		}
		return joinPaths(parent, resolvedRawGroupPrefix(group, nil))
	}
	return ""
}

func resolvedRawGroupPrefix(group rawGroup, parentPrefix *string) string {
	prefix := group.parentPrefix
	if parentPrefix != nil {
		prefix = joinPaths(*parentPrefix, prefix)
	}
	if !group.static {
		return prefix
	}
	return joinPaths(prefix, group.prefix)
}

func groupNameFromPrefix(prefix string) string {
	prefix = normalizePath(prefix)
	for _, seg := range reverseSegments(prefix) {
		if seg == "" || seg == "/" || strings.HasPrefix(seg, "{") {
			continue
		}
		return seg
	}
	return ""
}

func reverseSegments(path string) []string {
	segs := strings.Split(strings.Trim(path, "/"), "/")
	for i, j := 0, len(segs)-1; i < j; i, j = i+1, j-1 {
		segs[i], segs[j] = segs[j], segs[i]
	}
	return segs
}

// ginMethod is the in-package alias for GinMethod.
func ginMethod(info *gotypes.Info, call *ast.CallExpr) (name, recvPkgPath string, ok bool) {
	return GinMethod(info, call)
}

// GinMethod resolves a selector call to (methodName, receiverPkgPath, ok) using
// go/types selections — the version- and alias-robust identity check (Pattern 2).
// Only method-value selections on a typed receiver qualify. Shared by the route
// recognizer and the handler analyzer so both gate on the same resolved identity.
func GinMethod(info *gotypes.Info, call *ast.CallExpr) (name, recvPkgPath string, ok bool) {
	sel, isSel := call.Fun.(*ast.SelectorExpr)
	if !isSel {
		return "", "", false
	}
	s := info.Selections[sel]
	if s == nil || s.Kind() != gotypes.MethodVal {
		return "", "", false
	}
	fn, _ := s.Obj().(*gotypes.Func)
	if fn == nil {
		return "", "", false
	}
	if p := fn.Pkg(); p != nil {
		recvPkgPath = p.Path()
	}
	return fn.Name(), recvPkgPath, true
}

// receiverObject returns the *types.Object the selector's receiver expression
// denotes (the `api` group variable), so routes/Use on the same group object are
// correlated. Returns nil when the receiver is not a plain identifier.
func receiverObject(info *gotypes.Info, call *ast.CallExpr) gotypes.Object {
	sel, ok := call.Fun.(*ast.SelectorExpr)
	if !ok {
		return nil
	}
	ident, ok := sel.X.(*ast.Ident)
	if !ok {
		return nil
	}
	return info.ObjectOf(ident)
}

func receiverIsGinRouterGroup(info *gotypes.Info, call *ast.CallExpr) bool {
	sel, ok := call.Fun.(*ast.SelectorExpr)
	if !ok {
		return false
	}
	t := gotypes.Unalias(info.TypeOf(sel.X))
	if ptr, ok := t.(*gotypes.Pointer); ok {
		t = gotypes.Unalias(ptr.Elem())
	}
	named, ok := t.(*gotypes.Named)
	if !ok || named.Obj() == nil || named.Obj().Pkg() == nil {
		return false
	}
	return named.Obj().Pkg().Path() == ginPkgPath && named.Obj().Name() == "RouterGroup"
}

func groupOf(groups map[gotypes.Object]*groupInfo, obj gotypes.Object) *groupInfo {
	g, ok := groups[obj]
	if !ok {
		g = &groupInfo{}
		groups[obj] = g
	}
	return g
}

// handlerSymbol returns the trailing selector/identifier name of the handler arg,
// e.g. `h.createGoal` -> "createGoal", `createGoal` -> "createGoal".
func handlerSymbol(arg ast.Expr) string {
	switch e := arg.(type) {
	case *ast.SelectorExpr:
		return e.Sel.Name
	case *ast.Ident:
		return e.Name
	}
	return ""
}

// stringLiteral extracts the value of a basic string-literal expression.
func stringLiteral(arg ast.Expr) (string, bool) {
	lit, ok := arg.(*ast.BasicLit)
	if !ok || lit.Kind != token.STRING {
		return "", false
	}
	// Strip the surrounding quotes; route literals are simple, no escapes.
	return strings.Trim(lit.Value, "`\""), true
}

// normalizePath is the in-package alias for NormalizePath.
func normalizePath(p string) string { return NormalizePath(p) }

// NormalizePath converts a Gin path template to the OpenAPI form: each `:param`
// segment becomes `{param}` (Pitfall 5). Other segments pass through unchanged.
func NormalizePath(p string) string {
	if p == "" {
		return ""
	}
	if !strings.Contains(p, ":") {
		return p
	}
	segs := strings.Split(p, "/")
	for i, seg := range segs {
		if strings.HasPrefix(seg, ":") {
			segs[i] = "{" + seg[1:] + "}"
		}
	}
	return strings.Join(segs, "/")
}

func joinPaths(prefix, path string) string {
	prefix = normalizePath(prefix)
	path = normalizePath(path)
	if prefix == "" || prefix == "/" {
		if path == "" {
			return "/"
		}
		return path
	}
	if path == "" || path == "/" {
		return prefix
	}
	return strings.TrimRight(prefix, "/") + "/" + strings.TrimLeft(path, "/")
}

func spanOf(fset *token.FileSet, start, end token.Pos) facts.SourceSpan {
	if fset == nil || !start.IsValid() {
		return facts.SourceSpan{}
	}
	sp := fset.Position(start)
	ep := sp
	if end.IsValid() {
		ep = fset.Position(end)
	}
	return facts.SourceSpan{
		File:      sp.Filename,
		StartLine: uint32(sp.Line),
		EndLine:   uint32(ep.Line),
	}
}
