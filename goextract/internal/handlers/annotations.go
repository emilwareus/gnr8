// This file implements the swaggo `// @...` doc-comment parser — the annotation
// ESCAPE HATCH (GO-05, RESEARCH finding A1). Code inference (handlers.go) is
// PRIMARY; annotations FILL GAPS and add metadata the code path cannot recover:
//
//	@Summary <text>                          -> route summary
//	@Description <text>                       -> (captured; first line wins)
//	@Tags a,b,c                               -> route tags
//	@ID <id>                                  -> operation id (e.g. goalUuidPut)
//	@Security <scheme>                        -> secured + scheme name (ApiKeyAuth)
//	@Router /path [method]                    -> authoritative path/method override
//	@Param <name> <in> <type> <req> "desc" Enums(a,b,c)
//	                                          -> a ParamFact (query/path/body);
//	                                             body params fall back to request_body
//	@Success/@Failure <status> {object} <type> "desc"
//	                                          -> ResponseFact, FILLING gaps only
//
// Parsing is defensive (T-02-09): unrecognized or malformed `@` lines are skipped,
// never panicking; values become structured facts, never executed (T-02-08).
package handlers

import (
	"strconv"
	"strings"

	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// AnnotationFacts holds the parsed swaggo facts for one handler before merge.
type AnnotationFacts struct {
	Summary         *string
	Description     *string
	Tags            []string
	OperationID     *string
	SecuritySchemes []string
	RouterPath      *string
	RouterMethod    *string
	Params          []facts.ParamFact
	RequestBody     *facts.TypeRef
	Responses       []facts.ResponseFact
}

// ParseAnnotations extracts swaggo facts from a handler's doc comment. A nil/empty
// doc yields zero facts. The Analyzer's module-relative selector->package map
// (WR-03) qualifies `{object} dto.X` type refs into the same module-relative
// schema ids the type extractor emits.
func (a *Analyzer) ParseAnnotations(handler string) AnnotationFacts {
	af := AnnotationFacts{Tags: []string{}, SecuritySchemes: []string{}}
	doc := a.idx.Doc(handler)
	if doc == nil {
		return af
	}
	for _, c := range doc.List {
		line := strings.TrimSpace(strings.TrimPrefix(strings.TrimPrefix(c.Text, "//"), "/*"))
		if !strings.HasPrefix(line, "@") {
			continue
		}
		a.parseAnnotationLine(line, &af)
	}
	return af
}

// parseAnnotationLine dispatches one `@directive ...` line into af. Unknown
// directives are ignored (defensive parsing).
func (a *Analyzer) parseAnnotationLine(line string, af *AnnotationFacts) {
	directive, rest := splitFirst(line)
	rest = strings.TrimSpace(rest)
	switch directive {
	case "@Summary":
		af.Summary = optString(rest)
	case "@Description":
		if af.Description == nil {
			af.Description = optString(rest)
		}
	case "@Tags":
		af.Tags = append(af.Tags, splitCSV(rest)...)
	case "@ID":
		af.OperationID = optString(rest)
	case "@Security":
		if scheme := firstToken(rest); scheme != "" {
			af.SecuritySchemes = append(af.SecuritySchemes, scheme)
		}
	case "@Router":
		parseRouter(rest, af)
	case "@Param":
		a.parseParam(rest, af)
	case "@Success", "@Failure":
		a.parseResponse(rest, af)
	}
}

// parseRouter parses `/path [method]` -> normalized path override + method.
func parseRouter(rest string, af *AnnotationFacts) {
	path := firstToken(rest)
	if path == "" {
		return
	}
	norm := routes.NormalizePath(path)
	af.RouterPath = &norm
	if open := strings.Index(rest, "["); open >= 0 {
		if close := strings.Index(rest[open:], "]"); close > 0 {
			method := strings.ToUpper(strings.TrimSpace(rest[open+1 : open+close]))
			if method != "" {
				af.RouterMethod = &method
			}
		}
	}
}

// parseParam parses `<name> <in> <type> <required> "desc" [Enums(a,b,c)]`.
// A `body` param sets the request_body fallback; others become ParamFacts.
func (a *Analyzer) parseParam(rest string, af *AnnotationFacts) {
	fields := tokenize(rest)
	if len(fields) < 4 {
		return // malformed; skip (T-02-09).
	}
	name, in, typ, reqStr := fields[0], fields[1], fields[2], fields[3]
	required, _ := strconv.ParseBool(reqStr)

	if in == "body" {
		// type is a dto.X qualified ref; record as request_body fallback.
		if id := a.schemaRefFromAnnotation(typ); id != "" {
			af.RequestBody = &facts.TypeRef{RefID: id}
		}
		return
	}

	p := facts.ParamFact{
		Name:        name,
		Location:    in,
		Required:    required,
		Schema:      facts.SchemaType{Kind: annotationKind(typ)},
		Description: quotedDescription(rest),
		EnumValues:  parseEnums(rest),
	}
	af.Params = append(af.Params, p)
}

// parseResponse parses `<status> {object} <type> "desc"` into a ResponseFact used
// to FILL a missing/typeless response (never to clobber a code-resolved one).
func (a *Analyzer) parseResponse(rest string, af *AnnotationFacts) {
	fields := tokenize(rest)
	if len(fields) < 1 {
		return
	}
	status, err := strconv.Atoi(fields[0])
	if err != nil || status < 0 || status > 599 {
		return
	}
	var body *facts.TypeRef
	if typ := objectType(fields); typ != "" {
		if id := a.schemaRefFromAnnotation(typ); id != "" {
			body = &facts.TypeRef{RefID: id}
		}
	}
	af.Responses = append(af.Responses, facts.ResponseFact{
		Status:      uint16(status),
		Body:        body,
		Description: quotedDescription(rest),
	})
}

// MergeAnnotations applies the annotation escape-hatch onto a code-inferred route
// fact: code is PRIMARY (request body, code-resolved responses, code params keep
// their values); annotations FILL gaps (summary, tags, operation id, security,
// @Router override, query param types/required/enums, missing responses, body
// fallback) and never overwrite a code-resolved fact (TARGET-API.md thesis).
func MergeAnnotations(rf *facts.RouteFact, af AnnotationFacts) {
	if rf.Summary == nil {
		rf.Summary = af.Summary
	}
	if rf.OperationID == nil {
		rf.OperationID = af.OperationID
	}
	if len(rf.Tags) == 0 {
		rf.Tags = append(rf.Tags, af.Tags...)
	}
	if len(af.SecuritySchemes) > 0 {
		rf.SecuritySchemes = append(rf.SecuritySchemes, af.SecuritySchemes...)
		rf.Secured = true // an explicit @Security requirement secures the op (D-14).
	}
	if af.RouterPath != nil {
		rf.RouterPath = af.RouterPath
	}
	// request_body: code-inferred wins; annotation @Param body fills a gap.
	if rf.RequestBody == nil && af.RequestBody != nil {
		rf.RequestBody = af.RequestBody
	}

	mergeParams(rf, af.Params)
	mergeResponses(rf, af.Responses)
}

// mergeParams upgrades existing code params with annotation metadata (required,
// description, enums, schema type) by (name, location), and appends annotation
// params the code path did not discover (e.g. a documented param never read).
func mergeParams(rf *facts.RouteFact, annParams []facts.ParamFact) {
	for _, ap := range annParams {
		idx := -1
		for i := range rf.Params {
			if rf.Params[i].Name == ap.Name && rf.Params[i].Location == ap.Location {
				idx = i
				break
			}
		}
		if idx == -1 {
			rf.Params = append(rf.Params, ap)
			continue
		}
		p := &rf.Params[idx]
		// Annotation supplies required-ness + docs + enums the code path lacked.
		p.Required = p.Required || ap.Required
		if p.Description == nil {
			p.Description = ap.Description
		}
		if len(p.EnumValues) == 0 && len(ap.EnumValues) > 0 {
			p.EnumValues = ap.EnumValues
		}
		if ap.Schema.Kind != "" && p.Schema.Kind == "" {
			p.Schema = ap.Schema
		}
	}
}

// mergeResponses fills responses the code path could not resolve: a status the
// code never emitted is added; a code status whose body was dynamic (nil) is
// backfilled from the annotation. A code-resolved body is NEVER overwritten.
func mergeResponses(rf *facts.RouteFact, annResponses []facts.ResponseFact) {
	for _, ar := range annResponses {
		idx := -1
		for i := range rf.Responses {
			if rf.Responses[i].Status == ar.Status {
				idx = i
				break
			}
		}
		if idx == -1 {
			rf.Responses = append(rf.Responses, ar)
			continue
		}
		r := &rf.Responses[idx]
		if r.Body == nil {
			r.Body = ar.Body // backfill a dynamic/unresolved code response.
		}
		if r.Description == nil {
			r.Description = ar.Description
		}
	}
}

// --- annotation token helpers -------------------------------------------

// schemaRefFromAnnotation turns a `dto.UpdateGoalInput` annotation type into the
// module-relative schema id. swaggo writes the local package selector (`dto.X`);
// the fixture's dto package is `internal/common/dto`, so we resolve the suffix by
// matching the package selector against the known module-relative dto path. The
// selector->package map is the Analyzer's per-invocation context (WR-03).
func (a *Analyzer) schemaRefFromAnnotation(typ string) string {
	typ = strings.TrimPrefix(typ, "{object}")
	typ = strings.TrimSpace(typ)
	dot := strings.LastIndex(typ, ".")
	if dot < 0 {
		return ""
	}
	sel, name := typ[:dot], typ[dot+1:]
	if name == "" {
		return ""
	}
	// Map the short package selector to its module-relative path. The fixture uses
	// `dto` -> `internal/common/dto`. Generic resolution (any pkg selector) is a
	// post-PoC concern; for now resolve the one selector the fixture annotates.
	if pkg, ok := a.annPkgPaths[sel]; ok {
		return pkg + "." + name
	}
	return sel + "." + name
}

// AnnotationPackagesFromResult builds the swaggo selector -> module-relative path
// map from the loaded target packages: each package's source name (the `package X`
// clause, e.g. `dto`) maps to its module-relative import path (e.g.
// `internal/common/dto`). When two target packages share a source name, the
// shorter (more canonical) path wins deterministically.
func AnnotationPackagesFromResult(res *load.Result, module string) map[string]string {
	out := map[string]string{}
	for _, pkg := range res.Packages {
		if pkg.Types == nil || pkg.Name == "" {
			continue
		}
		rel := pkg.PkgPath
		if module != "" && strings.HasPrefix(rel, module) {
			rel = strings.TrimPrefix(strings.TrimPrefix(rel, module), "/")
		} else {
			continue // only target-module packages are annotation ref targets.
		}
		if existing, ok := out[pkg.Name]; ok && len(existing) <= len(rel) {
			continue
		}
		out[pkg.Name] = rel
	}
	return out
}

// annotationKind maps a swaggo scalar type to a SchemaType kind.
func annotationKind(typ string) string {
	switch typ {
	case "integer", "int", "int64", "int32":
		return "integer"
	case "number", "float", "float64", "float32":
		return "number"
	case "boolean", "bool":
		return "boolean"
	default:
		return "string"
	}
}

// parseEnums extracts the closed value set from an inline `Enums(a,b,c)` clause,
// sorted for determinism. Returns an empty slice when absent.
func parseEnums(rest string) []string {
	open := strings.Index(rest, "Enums(")
	if open < 0 {
		return []string{}
	}
	start := open + len("Enums(")
	close := strings.Index(rest[start:], ")")
	if close < 0 {
		return []string{}
	}
	values := splitCSV(rest[start : start+close])
	// sort for determinism (caller also sorts, but keep it stable here).
	for i := 1; i < len(values); i++ {
		for j := i; j > 0 && values[j] < values[j-1]; j-- {
			values[j], values[j-1] = values[j-1], values[j]
		}
	}
	return values
}

// quotedDescription returns the first double-quoted segment of a line (the swaggo
// description), or nil when none is present.
func quotedDescription(s string) *string {
	open := strings.Index(s, `"`)
	if open < 0 {
		return nil
	}
	close := strings.Index(s[open+1:], `"`)
	if close < 0 {
		return nil
	}
	desc := s[open+1 : open+1+close]
	return optString(desc)
}

// objectType returns the type token following `{object}` in a @Success/@Failure
// field list, e.g. ["200","{object}","dto.X","desc"] -> "dto.X".
func objectType(fields []string) string {
	for i, f := range fields {
		if f == "{object}" && i+1 < len(fields) {
			return fields[i+1]
		}
	}
	return ""
}

// tokenize splits on whitespace but keeps a trailing quoted description intact by
// stopping field collection at the first quote. Enums(...) and the leading tokens
// remain space-separated.
func tokenize(s string) []string {
	// Cut off the quoted description so it does not pollute the positional fields.
	if q := strings.Index(s, `"`); q >= 0 {
		s = s[:q]
	}
	return strings.Fields(s)
}

func splitFirst(s string) (string, string) {
	if i := strings.IndexAny(s, " \t"); i >= 0 {
		return s[:i], s[i+1:]
	}
	return s, ""
}

func firstToken(s string) string {
	fields := strings.Fields(s)
	if len(fields) == 0 {
		return ""
	}
	return fields[0]
}

func splitCSV(s string) []string {
	var out []string
	for _, part := range strings.Split(s, ",") {
		if p := strings.TrimSpace(part); p != "" {
			out = append(out, p)
		}
	}
	return out
}

func optString(s string) *string {
	s = strings.TrimSpace(s)
	if s == "" {
		return nil
	}
	v := s
	return &v
}
