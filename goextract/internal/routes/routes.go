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
// `/goal` prefix is supplied later from the swaggo `@Router` annotation (02-03).
// No Gin terms leak into the emitted facts — only router-agnostic HTTP routes.
package routes

import (
	"go/ast"
	"go/token"
	gotypes "go/types"
	"strings"

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
// so the handler analyzer (Task 2) can match a handler FuncDecl by symbol and the
// annotation parser (Task 3) can attach doc-comment facts.
type Route struct {
	Method  string           // "POST"
	Path    string           // group-relative, normalized: "/", "/list", "/{uuid}"
	Handler string           // selector name of the handler arg, e.g. "createGoal"
	Secured bool             // the enclosing group had a Use(middleware) call
	Span    facts.SourceSpan // provenance of the METHOD registration call
}

// Recognize walks every target package's syntax and returns the recognized routes
// with group security propagated. Output order is not guaranteed; callers sort
// (facts.Marshal) before emit.
func Recognize(res *load.Result) []Route {
	var out []Route
	for _, pkg := range res.Packages {
		if pkg.TypesInfo == nil {
			continue
		}
		for _, file := range pkg.Syntax {
			out = append(out, recognizeFile(file, pkg.TypesInfo, res.Fset)...)
		}
	}
	return out
}

// groupInfo tracks, per receiver object (the `api` group variable), whether the
// group has been secured by a Use(...) call. Routes registered on a secured group
// inherit secured=true.
type groupInfo struct {
	secured bool
}

// recognizeFile collects routes from a single file. It performs two passes over the
// receiver objects so that a Use(...) appearing on the same group object (in any
// order) marks every route on that object secured (D-14): pass 1 records which
// group objects are secured; pass 2 emits routes with the resolved security.
func recognizeFile(file *ast.File, info *gotypes.Info, fset *token.FileSet) []Route {
	groups := map[gotypes.Object]*groupInfo{}

	// Pass 1: find Use(...) calls and mark their receiver group secured.
	ast.Inspect(file, func(n ast.Node) bool {
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
			return true // dynamic path arg; not folded here.
		}
		handler := handlerSymbol(call.Args[len(call.Args)-1])

		secured := false
		if obj := receiverObject(info, call); obj != nil {
			if g, seen := groups[obj]; seen {
				secured = g.secured
			}
		}

		out = append(out, Route{
			Method:  name,
			Path:    normalizePath(pathLit),
			Handler: handler,
			Secured: secured,
			Span:    spanOf(fset, call.Pos(), call.End()),
		})
		return true
	})
	return out
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
// Exported so the annotation parser (Task 3) normalizes `@Router` paths the same
// way the code recognizer normalizes registration paths.
func NormalizePath(p string) string {
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
