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
	formFields := map[string]facts.FieldFact{}
	formHasFile := false
	contentTypeHint := ""

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
			if ref, _, ok := a.bindRequestType(h.info, call); ok {
				cf.RequestBody = ref
			}
		case "ShouldBind", "Bind", "ShouldBindWith", "BindWith":
			if ref, bound, ok := a.bindRequestType(h.info, call); ok {
				cf.RequestBody = ref
				cf.RequestBodyContentType = bindContentType(name, h.info, call, bound)
			}
		case "JSON":
			a.analyzeJSON(h, call, route, &cf, seenStatus, diags)
		case "Status", "AbortWithStatus":
			a.analyzeStatus(h, call, route, &cf, seenStatus, diags)
		case "Header":
			if key, ok := stringArg(call, 0); ok && strings.EqualFold(key, "Content-Type") {
				if value, ok := stringArg(call, 1); ok {
					contentTypeHint = value
				}
			}
		case "File", "FileAttachment":
			a.analyzeBinaryStatus(&cf, seenStatus, 200, contentTypeHint)
		case "Data":
			a.analyzeData(h, call, route, &cf, seenStatus, diags)
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
	return cf
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
	if id, schema, ok := a.syntheticJSONResponse(h, call.Args[1], route.Handler, status); ok {
		body = &facts.TypeRef{RefID: id}
		cf.Schemas = append(cf.Schemas, schema)
	} else if id, ok := a.namedTypeID(h.info.TypeOf(call.Args[1])); ok {
		body = &facts.TypeRef{RefID: id}
	} else {
		file, line := positionOf(h.fset, call.Pos())
		diags.DynamicResponse(route.Handler, "response body does not resolve to a named type", file, line)
	}
	cf.Responses = append(cf.Responses, facts.ResponseFact{Status: status, Body: body})
}

func (a *Analyzer) analyzeStatus(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
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
	a.addResponse(cf, seenStatus, facts.ResponseFact{Status: status})
}

func (a *Analyzer) analyzeBinaryStatus(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	status uint16,
	contentType string,
) {
	a.addResponse(cf, seenStatus, facts.ResponseFact{
		Status:      status,
		BodyKind:    "binary",
		ContentType: responseContentType(contentType),
	})
}

func (a *Analyzer) analyzeData(
	h handlerDecl,
	call *ast.CallExpr,
	route routes.Route,
	cf *CodeFacts,
	seenStatus map[uint16]bool,
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
	contentType := ""
	if value, ok := stringArg(call, 1); ok {
		contentType = value
	}
	a.addResponse(cf, seenStatus, facts.ResponseFact{
		Status:      status,
		BodyKind:    "binary",
		ContentType: responseContentType(contentType),
	})
}

func (a *Analyzer) addResponse(
	cf *CodeFacts,
	seenStatus map[uint16]bool,
	response facts.ResponseFact,
) {
	if seenStatus[response.Status] {
		return
	}
	seenStatus[response.Status] = true
	cf.Responses = append(cf.Responses, response)
}

func responseContentType(contentType string) string {
	if contentType == "" {
		return "application/octet-stream"
	}
	return contentType
}

func isByteSlice(t gotypes.Type) bool {
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
