// Package facts defines the JSON facts document that goextract emits on stdout —
// the Rust<->Go contract boundary (CONTEXT D-02). Every slice is sorted by a stable
// key before marshalling so that two runs on unchanged source are byte-identical
// (GRAPH-02 / Pitfall 1: never range a Go map into output order).
//
// The json tags here MUST match the serde field names in
// crates/gnr8-core/src/analyze/facts.rs exactly. This plan (02-01) owns the
// routes-free part of the schema; Routes is defined now (as an always-present,
// non-nil slice) so the key is stable, and 02-02 fills it in.
package facts

import (
	"encoding/json"
	"io"
	"sort"
)

// GoFacts is the top-level facts document for a single target module.
type GoFacts struct {
	Module      string           `json:"module"`
	Routes      []RouteFact      `json:"routes"`
	Schemas     []SchemaFact     `json:"schemas"`
	Diagnostics []DiagnosticFact `json:"diagnostics"`
}

// RouteFact describes one HTTP route. 02-02 fills these from the Gin recognizer
// (Task 1) + handler analysis (Task 2) + swaggo annotations (Task 3).
//
// Path storage decision (02-02; RESEARCH Open Q1): the group prefix in the
// fixture is `"/" + basePath` (a non-constant BinaryExpr) the helper cannot
// constant-fold, so Path holds the CODE-derived, group-relative, normalized
// template (`/`, `/list`, `/{uuid}`). RouterPath holds the authoritative
// @Router annotation override (`/list`, `/{uuid}`) when present. Emitting both
// lets 02-03 render the absolute `/goal/...` deterministically without this
// plan guessing the dynamic prefix (router-agnostic facts only).
type RouteFact struct {
	Method          string         `json:"method"`
	Path            string         `json:"path"`
	RouterPath      *string        `json:"router_path"`
	Handler         string         `json:"handler"`
	OperationID     *string        `json:"operation_id"`
	Summary         *string        `json:"summary"`
	Tags            []string       `json:"tags"`
	Secured         bool           `json:"secured"`
	SecuritySchemes []string       `json:"security_schemes"`
	Params          []ParamFact    `json:"params"`
	RequestBody     *TypeRef       `json:"request_body"`
	Responses       []ResponseFact `json:"responses"`
	Span            SourceSpan     `json:"span"`
}

// ParamFact describes a path or query parameter (filled by 02-02).
type ParamFact struct {
	Name        string     `json:"name"`
	Location    string     `json:"location"`
	Required    bool       `json:"required"`
	Schema      SchemaType `json:"schema"`
	Description *string    `json:"description"`
	EnumValues  []string   `json:"enum_values"`
	Span        SourceSpan `json:"span"`
}

// ResponseFact describes one response keyed by HTTP status (filled by 02-02).
type ResponseFact struct {
	Status      uint16   `json:"status"`
	Body        *TypeRef `json:"body"`
	Description *string  `json:"description"`
}

// SchemaFact is one extracted named type: an object struct or a string enum.
type SchemaFact struct {
	ID         string      `json:"id"`
	Name       string      `json:"name"`
	Kind       string      `json:"kind"` // "object" | "enum"
	Fields     []FieldFact `json:"fields"`
	EnumValues []string    `json:"enum_values"`
	Span       SourceSpan  `json:"span"`
}

// FieldFact is one field of an object schema.
type FieldFact struct {
	JSONName    string     `json:"json_name"`
	Required    bool       `json:"required"`
	Optional    bool       `json:"optional"`
	Schema      SchemaType `json:"schema"`
	Description *string    `json:"description"`
	Example     *string    `json:"example"`
}

// SchemaType is a router-/OpenAPI-agnostic primitive description of a Go type.
type SchemaType struct {
	Kind                 string      `json:"kind"` // string|integer|number|boolean|array|object|ref
	Format               *string     `json:"format"`
	Items                *SchemaType `json:"items"`
	RefID                *string     `json:"ref_id"`
	AdditionalProperties *bool       `json:"additional_properties"`
}

// TypeRef is a reference to a schema by its stable id.
type TypeRef struct {
	RefID string `json:"ref_id"`
}

// DiagnosticFact is one warning/error with a source location (D-10 / GO-06).
type DiagnosticFact struct {
	Severity string `json:"severity"`
	Message  string `json:"message"`
	File     string `json:"file"`
	Line     uint32 `json:"line"`
}

// SourceSpan is the file + line range provenance attached to every node (D-07).
type SourceSpan struct {
	File      string `json:"file"`
	StartLine uint32 `json:"start_line"`
	EndLine   uint32 `json:"end_line"`
}

// Marshal sorts every slice in doc by a stable key and writes pretty-printed JSON
// to w. Sorting before encoding is what makes the output deterministic on
// unchanged source (GRAPH-02). It NEVER ranges a Go map into output order.
//
// Sort keys:
//   - Schemas by Id
//   - each schema's Fields by JsonName
//   - each schema's EnumValues lexically
//   - Diagnostics by (File, Line, Message)
//   - Routes by (Path, Method) — empty this plan, but keep the discipline
func Marshal(doc GoFacts, w io.Writer) error {
	sortDoc(&doc)

	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	// SetEscapeHTML(false) keeps message text (e.g. "->") readable and stable.
	enc.SetEscapeHTML(false)
	return enc.Encode(doc)
}

func sortDoc(doc *GoFacts) {
	sort.Slice(doc.Schemas, func(i, j int) bool {
		return doc.Schemas[i].ID < doc.Schemas[j].ID
	})
	for i := range doc.Schemas {
		s := &doc.Schemas[i]
		sort.Slice(s.Fields, func(a, b int) bool {
			return s.Fields[a].JSONName < s.Fields[b].JSONName
		})
		sort.Strings(s.EnumValues)
	}

	sort.Slice(doc.Diagnostics, func(i, j int) bool {
		di, dj := doc.Diagnostics[i], doc.Diagnostics[j]
		if di.File != dj.File {
			return di.File < dj.File
		}
		if di.Line != dj.Line {
			return di.Line < dj.Line
		}
		return di.Message < dj.Message
	})

	sort.Slice(doc.Routes, func(i, j int) bool {
		ri, rj := doc.Routes[i], doc.Routes[j]
		if ri.Path != rj.Path {
			return ri.Path < rj.Path
		}
		return ri.Method < rj.Method
	})
	for i := range doc.Routes {
		sortRoute(&doc.Routes[i])
	}
}

// sortRoute stably orders every sub-slice of a route so two runs on unchanged
// source are byte-identical (GRAPH-02): params by (name, location), each param's
// enum values lexically, responses by status, and tags/security schemes lexically.
func sortRoute(r *RouteFact) {
	sort.Slice(r.Params, func(a, b int) bool {
		if r.Params[a].Name != r.Params[b].Name {
			return r.Params[a].Name < r.Params[b].Name
		}
		return r.Params[a].Location < r.Params[b].Location
	})
	for i := range r.Params {
		sort.Strings(r.Params[i].EnumValues)
	}
	sort.Slice(r.Responses, func(a, b int) bool {
		return r.Responses[a].Status < r.Responses[b].Status
	})
	sort.Strings(r.Tags)
	sort.Strings(r.SecuritySchemes)
}
