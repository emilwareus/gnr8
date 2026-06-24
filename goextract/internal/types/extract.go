// Package types walks the reachable named types of the target module and lowers
// each DTO struct / enum to a router-agnostic facts.SchemaFact (GO-02, GO-03).
//
// Scope discipline (02-01): only named types DECLARED IN THE TARGET MODULE are
// considered, and a struct is treated as a DTO schema only when it (or an
// embedded struct) carries at least one `json:` tag. This excludes server/wiring
// structs such as HttpServer (no json tags) while capturing every dto.* type.
// Routes/handlers are 02-02; this package does not look at them.
package types

import (
	"fmt"
	"go/token"
	gotypes "go/types"
	"reflect"
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
)

// well-known package paths for type mapping (RESEARCH Pattern 6).
const (
	uuidPkgPath = "github.com/google/uuid"
	timePkgPath = "time"
)

// Extract returns one SchemaFact per DTO struct and per string-enum named type
// declared in the target module, plus any float64 / free-form-map diagnostics it
// discovers. Output order is not guaranteed here; facts.Marshal sorts everything.
func Extract(res *load.Result, diags *diag.Accumulator) []facts.SchemaFact {
	modulePath := mainModulePath(res)
	schemas := make([]facts.SchemaFact, 0)

	for _, pkg := range res.Packages {
		if !isTargetPackage(pkg.PkgPath, modulePath) || pkg.Types == nil {
			continue
		}
		scope := pkg.Types.Scope()
		for _, name := range scope.Names() {
			obj := scope.Lookup(name)
			tn, ok := obj.(*gotypes.TypeName)
			if !ok || tn.IsAlias() {
				continue
			}
			named, ok := gotypes.Unalias(tn.Type()).(*gotypes.Named)
			if !ok {
				continue
			}
			if fact, ok := schemaFor(named, modulePath, res.Fset, scope, diags); ok {
				schemas = append(schemas, fact)
			}
		}
	}
	return schemas
}

// schemaFor lowers one named type to a SchemaFact, or reports ok=false if the
// type is neither a DTO struct nor a string enum.
func schemaFor(
	named *gotypes.Named,
	modulePath string,
	fset *token.FileSet,
	scope *gotypes.Scope,
	diags *diag.Accumulator,
) (facts.SchemaFact, bool) {
	switch under := named.Underlying().(type) {
	case *gotypes.Struct:
		if !structHasJSONTag(under) {
			return facts.SchemaFact{}, false
		}
		span := spanOf(fset, named.Obj().Pos())
		fields := extractFields(named.Obj().Name(), under, modulePath, fset, diags)
		return facts.SchemaFact{
			ID:         schemaID(named, modulePath),
			Name:       named.Obj().Name(),
			Kind:       "object",
			Fields:     fields,
			EnumValues: []string{},
			Span:       span,
		}, true
	case *gotypes.Basic:
		if under.Kind() != gotypes.String {
			return facts.SchemaFact{}, false
		}
		values := enumValues(named, scope)
		if len(values) == 0 {
			return facts.SchemaFact{}, false
		}
		return facts.SchemaFact{
			ID:         schemaID(named, modulePath),
			Name:       named.Obj().Name(),
			Kind:       "enum",
			Fields:     []facts.FieldFact{},
			EnumValues: values,
			Span:       spanOf(fset, named.Obj().Pos()),
		}, true
	default:
		return facts.SchemaFact{}, false
	}
}

// extractFields walks struct fields, flattening embedded structs (Pattern 5).
func extractFields(
	structName string,
	st *gotypes.Struct,
	modulePath string,
	fset *token.FileSet,
	diags *diag.Accumulator,
) []facts.FieldFact {
	fields := make([]facts.FieldFact, 0, st.NumFields())
	for i := 0; i < st.NumFields(); i++ {
		f := st.Field(i)
		tag := reflect.StructTag(st.Tag(i))

		if f.Embedded() {
			if embedded, ok := embeddedStruct(f.Type()); ok {
				// Promote the embedded struct's fields, but attribute diagnostics
				// to the embedded type's own name (its float64 fields belong to it).
				fields = append(fields, extractFields(
					embeddedTypeName(f.Type()), embedded, modulePath, fset, diags)...)
			}
			continue
		}

		jsonName, omitempty, skip := parseJSONTag(tag, f.Name())
		if skip {
			continue
		}

		file, line := positionOf(fset, f.Pos())
		ctx := mapCtx{
			structName:   structName,
			fieldName:    f.Name(),
			declaredType: typeString(f.Type()), // the AS-WRITTEN type, e.g. "*float64"
			modulePath:   modulePath,
			file:         file,
			line:         line,
			diags:        diags,
		}
		schema := mapType(f.Type(), ctx)
		required := strings.Contains(tag.Get("binding"), "required")
		optional := isPointer(f.Type()) || omitempty

		fields = append(fields, facts.FieldFact{
			JSONName:    jsonName,
			Required:    required,
			Optional:    optional,
			Schema:      schema,
			Description: optString(tag.Get("description")),
			Example:     optString(tag.Get("example")),
		})
	}
	return fields
}

// mapCtx carries the per-field diagnostic identity (owning struct, field name,
// the as-written Go type, and the field's file:line) through the recursive
// mapType walk so float64 / free-form-map diagnostics render the DECLARED field
// type (e.g. "*float64") and the right position, not an unwrapped inner type.
type mapCtx struct {
	structName   string
	fieldName    string
	declaredType string
	modulePath   string
	file         string
	line         uint32
	diags        *diag.Accumulator
}

// mapType implements the Go-type -> SchemaType kind switch incl. well-known types
// and the float64 / free-form-map diagnostics (RESEARCH Pattern 6).
func mapType(t gotypes.Type, ctx mapCtx) facts.SchemaType {
	switch u := gotypes.Unalias(t).(type) {
	case *gotypes.Pointer:
		// Optionality is recorded on the field; the schema describes the elem.
		return mapType(u.Elem(), ctx)
	case *gotypes.Slice:
		items := mapType(u.Elem(), ctx)
		return facts.SchemaType{Kind: "array", Items: &items}
	case *gotypes.Map:
		// map[string]T -> object additionalProperties; warn on free-form maps.
		ctx.diags.FreeFormMap(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
		yes := true
		return facts.SchemaType{Kind: "object", AdditionalProperties: &yes}
	case *gotypes.Named:
		return mapNamed(u, ctx)
	case *gotypes.Basic:
		return mapBasic(u, ctx)
	default:
		return facts.SchemaType{Kind: "object"}
	}
}

func mapNamed(u *gotypes.Named, ctx mapCtx) facts.SchemaType {
	obj := u.Obj()
	pkgPath := ""
	if obj.Pkg() != nil {
		pkgPath = obj.Pkg().Path()
	}
	switch {
	case pkgPath == uuidPkgPath && obj.Name() == "UUID":
		return facts.SchemaType{Kind: "string", Format: optString("uuid")}
	case pkgPath == timePkgPath && obj.Name() == "Time":
		return facts.SchemaType{Kind: "string", Format: optString("date-time")}
	}
	// A named string (with or without a const set) refs its own schema; the enum
	// values are resolved by the enum SchemaFact (see Extract). A non-string named
	// type is a struct ref ($ref). Both are stable, package-qualified ids.
	id := schemaID(u, ctx.modulePath)
	return facts.SchemaType{Kind: "ref", RefID: &id}
}

func mapBasic(u *gotypes.Basic, ctx mapCtx) facts.SchemaType {
	switch u.Kind() {
	case gotypes.Bool:
		return facts.SchemaType{Kind: "boolean"}
	case gotypes.String:
		return facts.SchemaType{Kind: "string"}
	case gotypes.Int, gotypes.Int8, gotypes.Int16, gotypes.Int32, gotypes.Int64,
		gotypes.Uint, gotypes.Uint8, gotypes.Uint16, gotypes.Uint32, gotypes.Uint64:
		return facts.SchemaType{Kind: "integer", Format: optString("int64")}
	case gotypes.Float32, gotypes.Float64:
		// float64 -> float32 narrowing warning (TARGET-API.md §5.2). Report the
		// field identity, the DECLARED type (e.g. "*float64"), and its position.
		ctx.diags.Floatf(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
		return facts.SchemaType{Kind: "number"}
	default:
		return facts.SchemaType{Kind: "string"}
	}
}

// --- helpers -------------------------------------------------------------

func mainModulePath(res *load.Result) string {
	for _, pkg := range res.Packages {
		if pkg.Module != nil && pkg.Module.Main {
			return pkg.Module.Path
		}
	}
	// Fallback: longest common path prefix is unreliable; if no main module is
	// reported, return empty so isTargetPackage matches nothing rather than
	// pulling in stdlib/deps.
	return ""
}

func isTargetPackage(pkgPath, modulePath string) bool {
	if modulePath == "" {
		return false
	}
	if pkgPath != modulePath && !strings.HasPrefix(pkgPath, modulePath+"/") {
		return false
	}
	// Exclude the fixture's `expected/` tree: those packages (e.g. expected/sdk)
	// are hand-authored Phase-3 ACCEPTANCE SNAPSHOTS, not analyzer input. They
	// re-declare DTO names (CreateGoalInput, GoalResponse, ...) and would double
	// the schema set. Generated/expected output is never analysis input.
	rel := strings.TrimPrefix(strings.TrimPrefix(pkgPath, modulePath), "/")
	for _, seg := range strings.Split(rel, "/") {
		if seg == "expected" {
			return false
		}
	}
	return true
}

// schemaID is the package-qualified, module-relative type name, e.g.
// "internal/common/dto.CreateGoalInput".
func schemaID(named *gotypes.Named, modulePath string) string {
	obj := named.Obj()
	pkgPath := ""
	if obj.Pkg() != nil {
		pkgPath = obj.Pkg().Path()
	}
	rel := pkgPath
	if modulePath != "" && strings.HasPrefix(pkgPath, modulePath) {
		rel = strings.TrimPrefix(pkgPath, modulePath)
		rel = strings.TrimPrefix(rel, "/")
	}
	if rel == "" {
		return obj.Name()
	}
	return rel + "." + obj.Name()
}

func structHasJSONTag(st *gotypes.Struct) bool {
	for i := 0; i < st.NumFields(); i++ {
		tag := reflect.StructTag(st.Tag(i))
		if _, ok := tag.Lookup("json"); ok {
			return true
		}
		if st.Field(i).Embedded() {
			if embedded, ok := embeddedStruct(st.Field(i).Type()); ok {
				if structHasJSONTag(embedded) {
					return true
				}
			}
		}
	}
	return false
}

// enumValues collects the sorted-by-caller string const values whose type is the
// given named string type, scanning the package scope (RESEARCH Pattern 6).
func enumValues(named *gotypes.Named, scope *gotypes.Scope) []string {
	values := make([]string, 0)
	for _, name := range scope.Names() {
		c, ok := scope.Lookup(name).(*gotypes.Const)
		if !ok {
			continue
		}
		cn, ok := gotypes.Unalias(c.Type()).(*gotypes.Named)
		if !ok || cn.Obj() != named.Obj() {
			continue
		}
		// Const value is a quoted Go string literal; strip the quotes.
		values = append(values, strings.Trim(c.Val().ExactString(), `"`))
	}
	return values
}

func embeddedStruct(t gotypes.Type) (*gotypes.Struct, bool) {
	named, ok := gotypes.Unalias(deref(t)).(*gotypes.Named)
	if !ok {
		return nil, false
	}
	st, ok := named.Underlying().(*gotypes.Struct)
	return st, ok
}

func embeddedTypeName(t gotypes.Type) string {
	if named, ok := gotypes.Unalias(deref(t)).(*gotypes.Named); ok {
		return named.Obj().Name()
	}
	return ""
}

func deref(t gotypes.Type) gotypes.Type {
	if p, ok := t.(*gotypes.Pointer); ok {
		return p.Elem()
	}
	return t
}

func isPointer(t gotypes.Type) bool {
	_, ok := t.(*gotypes.Pointer)
	return ok
}

// parseJSONTag returns the effective json field name, whether omitempty is set,
// and whether the field is JSON-skipped (`json:"-"`). Falls back to the Go field
// name when no json tag is present.
func parseJSONTag(tag reflect.StructTag, goName string) (name string, omitempty, skip bool) {
	raw, ok := tag.Lookup("json")
	if !ok || raw == "" {
		return goName, false, false
	}
	parts := strings.Split(raw, ",")
	jsonName := parts[0]
	if jsonName == "-" && len(parts) == 1 {
		return "", false, true
	}
	if jsonName == "" {
		jsonName = goName
	}
	for _, opt := range parts[1:] {
		if opt == "omitempty" {
			omitempty = true
		}
	}
	return jsonName, omitempty, false
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

func optString(s string) *string {
	if s == "" {
		return nil
	}
	v := s
	return &v
}

func typeString(t gotypes.Type) string {
	// Render map[string]any as written; gotypes.TypeString renders interface{} as
	// "any" under go 1.18+ aliasing rules. Keep it qualified-free for stability.
	return fmt.Sprintf("%s", gotypes.TypeString(t, func(p *gotypes.Package) string { return p.Name() }))
}
