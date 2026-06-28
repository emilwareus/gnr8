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
	"go/token"
	gotypes "go/types"
	"reflect"
	"sort"
	"strconv"
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
)

// well-known package paths for type mapping (RESEARCH Pattern 6).
const (
	uuidPkgPath = "github.com/google/uuid"
	timePkgPath = "time"
	jsonPkgPath = "encoding/json"
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
		if under.NumFields() > 0 && !structHasJSONTag(under) {
			return facts.SchemaFact{}, false
		}
		span := spanOf(fset, named.Obj().Pos())
		fields := extractFields(named.Obj().Name(), under, modulePath, fset, diags)
		return facts.SchemaFact{
			ID:   schemaID(named, modulePath),
			Name: named.Obj().Name(),
			Body: facts.ObjectType(fields),
			Span: span,
		}, true
	case *gotypes.Basic:
		body := mapType(under, namedSchemaCtx(named, modulePath, fset, diags))
		if under.Kind() == gotypes.String {
			if values := enumValues(named, scope); len(values) > 0 {
				body = facts.EnumType(values)
			}
		}
		return facts.SchemaFact{
			ID:   schemaID(named, modulePath),
			Name: named.Obj().Name(),
			Body: body,
			Span: spanOf(fset, named.Obj().Pos()),
		}, true
	case *gotypes.Slice, *gotypes.Array, *gotypes.Map:
		return facts.SchemaFact{
			ID:   schemaID(named, modulePath),
			Name: named.Obj().Name(),
			Body: mapType(under, namedSchemaCtx(named, modulePath, fset, diags)),
			Span: spanOf(fset, named.Obj().Pos()),
		}, true
	default:
		return facts.SchemaFact{}, false
	}
}

func namedSchemaCtx(
	named *gotypes.Named,
	modulePath string,
	fset *token.FileSet,
	diags *diag.Accumulator,
) mapCtx {
	file, line := positionOf(fset, named.Obj().Pos())
	return mapCtx{
		structName:   named.Obj().Name(),
		fieldName:    named.Obj().Name(),
		declaredType: typeString(named),
		modulePath:   modulePath,
		file:         file,
		line:         line,
		diags:        diags,
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
		required := bindingHasRequired(tag.Get("binding"))
		// The two independent axes: optional = the key may be absent (a pointer or
		// `,omitempty`); nullable = the value may be explicitly null (a pointer can
		// hold nil). A non-pointer `,omitempty` field is optional-but-not-nullable.
		optional := isPointer(f.Type()) || omitempty
		nullable := isPointer(f.Type())
		meta := fieldMetaFromTags(structName, f.Name(), tag, st.Tag(i), schema, file, line, diags)

		fields = append(fields, facts.FieldFact{
			JSONName:    jsonName,
			Required:    required,
			Optional:    optional,
			Nullable:    nullable,
			Schema:      schema,
			Description: optString(tag.Get("description")),
			Example:     optString(tag.Get("example")),
			Meta:        meta,
		})
	}
	return fields
}

func bindingHasRequired(binding string) bool {
	for _, token := range strings.Split(binding, ",") {
		if strings.TrimSpace(token) == "required" {
			return true
		}
	}
	return false
}

func fieldMetaFromTags(
	structName string,
	fieldName string,
	tag reflect.StructTag,
	rawTag string,
	schema facts.Type,
	file string,
	line uint32,
	diags *diag.Accumulator,
) *facts.FieldMeta {
	meta := &facts.FieldMeta{}

	if constraints := constraintsFromBinding(structName, fieldName, tag.Get("binding"), schema, file, line, diags); !constraintsEmpty(constraints) {
		meta.Constraints = constraints
	}

	if rawDefault, ok := tag.Lookup("default"); ok {
		meta.Default = literalForSchema(rawDefault, schema, structName, fieldName, file, line, diags)
	}

	extensions := make([]facts.Extension, 0)
	if placeholder, ok := tag.Lookup("placeholder"); ok {
		extensions = append(extensions, facts.Extension{
			Name:  "x-gnr8-placeholder",
			Value: stringLiteral(placeholder),
		})
	}
	if render, ok := tag.Lookup("render"); ok {
		extensions = append(extensions, facts.Extension{
			Name:  "x-gnr8-render",
			Value: stringLiteral(render),
		})
	}

	rawTags := parseStructTag(rawTag)
	keys := make([]string, 0, len(rawTags))
	for key := range rawTags {
		if strings.HasPrefix(key, "x-") {
			keys = append(keys, key)
		}
	}
	sort.Strings(keys)
	for _, key := range keys {
		extensions = append(extensions, facts.Extension{
			Name:  key,
			Value: inferLiteral(rawTags[key]),
		})
	}
	if len(extensions) > 0 {
		meta.Extensions = extensions
	}

	if meta.Constraints == nil && meta.Default == nil && len(meta.Extensions) == 0 {
		return nil
	}
	return meta
}

func constraintsFromBinding(
	structName string,
	fieldName string,
	binding string,
	schema facts.Type,
	file string,
	line uint32,
	diags *diag.Accumulator,
) *facts.Constraints {
	constraints := &facts.Constraints{}
	if binding == "" {
		return constraints
	}
	for _, token := range strings.Split(binding, ",") {
		token = strings.TrimSpace(token)
		if token == "" || token == "required" || token == "omitempty" || token == "dive" {
			continue
		}
		name, value, hasValue := strings.Cut(token, "=")
		name = strings.TrimSpace(name)
		value = strings.TrimSpace(value)
		switch name {
		case "min":
			if !hasValue || !applyMinMaxConstraint(constraints, "min", value, schema) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
			}
		case "max":
			if !hasValue || !applyMinMaxConstraint(constraints, "max", value, schema) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
			}
		case "gte":
			if !hasValue || !validNumber(value) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			constraints.Minimum = stringPtr(value)
		case "lte":
			if !hasValue || !validNumber(value) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			constraints.Maximum = stringPtr(value)
		case "gt":
			if !hasValue || !validNumber(value) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			constraints.ExclusiveMinimum = stringPtr(value)
		case "lt":
			if !hasValue || !validNumber(value) {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			constraints.ExclusiveMaximum = stringPtr(value)
		case "oneof":
			if !hasValue {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			values := strings.Fields(value)
			if len(values) == 0 {
				unsupportedBinding(diags, structName, fieldName, token, file, line)
				continue
			}
			constraints.EnumValues = values
		default:
			unsupportedBinding(diags, structName, fieldName, token, file, line)
		}
	}
	return constraints
}

func applyMinMaxConstraint(c *facts.Constraints, name string, value string, schema facts.Type) bool {
	if schemaIsStringLike(schema) {
		parsed, err := strconv.ParseUint(value, 10, 64)
		if err != nil {
			return false
		}
		if name == "min" {
			c.MinLength = &parsed
		} else {
			c.MaxLength = &parsed
		}
		return true
	}
	if schemaIsNumeric(schema) {
		if !validNumber(value) {
			return false
		}
		if name == "min" {
			c.Minimum = stringPtr(value)
		} else {
			c.Maximum = stringPtr(value)
		}
		return true
	}
	return false
}

func constraintsEmpty(c *facts.Constraints) bool {
	return c == nil ||
		(c.MinLength == nil &&
			c.MaxLength == nil &&
			c.Minimum == nil &&
			c.Maximum == nil &&
			c.ExclusiveMinimum == nil &&
			c.ExclusiveMaximum == nil &&
			c.Pattern == nil &&
			len(c.EnumValues) == 0)
}

func unsupportedBinding(diags *diag.Accumulator, structName, fieldName, token, file string, line uint32) {
	diags.Warn(
		"unsupported binding tag on "+structName+"."+fieldName+": "+strconv.Quote(token)+
			" ignored by gnr8 metadata extraction (GO-06)",
		file,
		line,
	)
}

func literalForSchema(
	value string,
	schema facts.Type,
	structName string,
	fieldName string,
	file string,
	line uint32,
	diags *diag.Accumulator,
) *facts.LiteralValue {
	if value == "null" {
		lit := nullLiteral()
		return &lit
	}
	if schemaIsBool(schema) {
		parsed, err := strconv.ParseBool(value)
		if err != nil {
			diags.Warn(
				"default tag on "+structName+"."+fieldName+" is not a valid bool: "+strconv.Quote(value),
				file,
				line,
			)
			lit := stringLiteral(value)
			return &lit
		}
		lit := boolLiteral(parsed)
		return &lit
	}
	if schemaIsNumeric(schema) {
		if !validNumber(value) {
			diags.Warn(
				"default tag on "+structName+"."+fieldName+" is not a valid number: "+strconv.Quote(value),
				file,
				line,
			)
			lit := stringLiteral(value)
			return &lit
		}
		lit := numberLiteral(value)
		return &lit
	}
	lit := stringLiteral(value)
	return &lit
}

func inferLiteral(value string) facts.LiteralValue {
	switch value {
	case "null":
		return nullLiteral()
	case "true":
		return boolLiteral(true)
	case "false":
		return boolLiteral(false)
	default:
		if validNumber(value) {
			return numberLiteral(value)
		}
		return stringLiteral(value)
	}
}

func stringLiteral(value string) facts.LiteralValue {
	return facts.LiteralValue{Type: "string", Value: value}
}

func numberLiteral(value string) facts.LiteralValue {
	return facts.LiteralValue{Type: "number", Value: value}
}

func boolLiteral(value bool) facts.LiteralValue {
	return facts.LiteralValue{Type: "bool", Value: value}
}

func nullLiteral() facts.LiteralValue {
	return facts.LiteralValue{Type: "null"}
}

func schemaIsStringLike(schema facts.Type) bool {
	if schema.Type == facts.TypeWellKnown {
		return true
	}
	if schema.Type != facts.TypePrimitive {
		return false
	}
	if prim, ok := schema.Of.(*facts.Prim); ok && prim != nil {
		return prim.Prim == facts.PrimString || prim.Prim == facts.PrimBytes
	}
	return false
}

func schemaIsNumeric(schema facts.Type) bool {
	if schema.Type != facts.TypePrimitive {
		return false
	}
	if prim, ok := schema.Of.(*facts.Prim); ok && prim != nil {
		return prim.Prim == facts.PrimInt || prim.Prim == facts.PrimFloat
	}
	return false
}

func schemaIsBool(schema facts.Type) bool {
	if schema.Type != facts.TypePrimitive {
		return false
	}
	if prim, ok := schema.Of.(*facts.Prim); ok && prim != nil {
		return prim.Prim == facts.PrimBool
	}
	return false
}

func validNumber(value string) bool {
	_, err := strconv.ParseFloat(value, 64)
	return err == nil
}

func stringPtr(value string) *string {
	return &value
}

func parseStructTag(raw string) map[string]string {
	out := map[string]string{}
	for raw != "" {
		raw = strings.TrimLeft(raw, " ")
		if raw == "" {
			break
		}
		i := 0
		for i < len(raw) && raw[i] > ' ' && raw[i] != ':' && raw[i] != '"' && raw[i] != 0x7f {
			i++
		}
		if i == 0 || i+1 >= len(raw) || raw[i] != ':' || raw[i+1] != '"' {
			break
		}
		key := raw[:i]
		raw = raw[i+1:]
		i = 1
		for i < len(raw) {
			if raw[i] == '\\' {
				i += 2
				continue
			}
			if raw[i] == '"' {
				break
			}
			i++
		}
		if i >= len(raw) {
			break
		}
		quoted := raw[:i+1]
		value, err := strconv.Unquote(quoted)
		if err == nil {
			out[key] = value
		}
		raw = raw[i+1:]
	}
	return out
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

// mapType lowers a Go type into the neutral facts.Type vocabulary, incl. well-known
// types and the float64 / free-form-map diagnostics (RESEARCH Pattern 6).
func mapType(t gotypes.Type, ctx mapCtx) facts.Type {
	switch u := gotypes.Unalias(t).(type) {
	case *gotypes.Pointer:
		// Nullability/optionality are recorded on the field; the type describes the elem.
		return mapType(u.Elem(), ctx)
	case *gotypes.Slice:
		return facts.ArrayType(mapType(u.Elem(), ctx))
	case *gotypes.Array:
		return facts.ArrayType(mapType(u.Elem(), ctx))
	case *gotypes.Map:
		if _, ok := gotypes.Unalias(u.Elem()).(*gotypes.Interface); ok {
			ctx.diags.FreeFormMap(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
			return facts.AnyType()
		}
		key := mapType(u.Key(), ctx)
		value := mapType(u.Elem(), ctx)
		return facts.MapTypeOf(key, value)
	case *gotypes.Named:
		return mapNamed(u, ctx)
	case *gotypes.Basic:
		return mapBasic(u, ctx)
	default:
		return facts.AnyType()
	}
}

func mapNamed(u *gotypes.Named, ctx mapCtx) facts.Type {
	obj := u.Obj()
	pkgPath := ""
	if obj.Pkg() != nil {
		pkgPath = obj.Pkg().Path()
	}
	switch {
	case pkgPath == uuidPkgPath && obj.Name() == "UUID":
		return facts.WellKnownType(facts.WellKnownUUID)
	case pkgPath == timePkgPath && obj.Name() == "Time":
		return facts.WellKnownType(facts.WellKnownDateTime)
	case pkgPath == jsonPkgPath && obj.Name() == "RawMessage":
		return facts.AnyType()
	case pkgPath == jsonPkgPath && obj.Name() == "Number":
		return facts.PrimitiveType(facts.FloatPrim(64))
	}
	// A named string (with or without a const set) refs its own schema; the enum
	// values are resolved by the enum SchemaFact (see Extract). A non-string named
	// type is a struct ref. Both are stable, package-qualified ids.
	return facts.NamedType(schemaID(u, ctx.modulePath))
}

func mapBasic(u *gotypes.Basic, ctx mapCtx) facts.Type {
	switch u.Kind() {
	case gotypes.Bool:
		return facts.PrimitiveType(facts.BoolPrim())
	case gotypes.String:
		return facts.PrimitiveType(facts.StringPrim())
	case gotypes.Int, gotypes.Int8, gotypes.Int16, gotypes.Int32:
		return facts.PrimitiveType(facts.IntPrim(32, true))
	case gotypes.Int64:
		return facts.PrimitiveType(facts.IntPrim(64, true))
	case gotypes.Uint, gotypes.Uint8, gotypes.Uint16, gotypes.Uint32:
		return facts.PrimitiveType(facts.IntPrim(32, false))
	case gotypes.Uint64:
		// Carry the `signed` axis faithfully: an unsigned source type is NOT a
		// signed int. The neutral Prim::Int { signed } exists precisely so a
		// target can distinguish uint64 from int64 (one source of truth per fact).
		return facts.PrimitiveType(facts.IntPrim(64, false))
	case gotypes.Float32, gotypes.Float64:
		// float64 -> float32 narrowing warning (TARGET-API.md §5.2). Report the
		// field identity, the DECLARED type (e.g. "*float64"), and its position.
		ctx.diags.Floatf(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
		return facts.PrimitiveType(facts.FloatPrim(32))
	default:
		// An unsupported basic kind (complex64/128, uintptr, untyped constants,
		// ...) has no faithful neutral primitive. Emit a diagnostic and fall back
		// to the HONEST free-form `any` rather than fabricating a `string` fact
		// with no evidence (GO-06 / CLAUDE.md rule 3: diagnose, never guess).
		ctx.diags.UnsupportedType(ctx.structName, ctx.fieldName, ctx.declaredType, ctx.file, ctx.line)
		return facts.AnyType()
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
	// "any" under go 1.18+ aliasing rules (the normalization is done by TypeString
	// itself). Keep it qualified-free for stability. Return the string directly —
	// it is already a string, so wrapping it in fmt.Sprintf("%s", ...) is a no-op
	// allocation that go vet's simplify (S1025) flags.
	return gotypes.TypeString(t, func(p *gotypes.Package) string { return p.Name() })
}
