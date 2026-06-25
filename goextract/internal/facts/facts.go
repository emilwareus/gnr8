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

// RouteFact describes one HTTP route, derived PURELY from Go code (the Gin
// recognizer + handler-body analysis). There is exactly one code-derived source
// per fact and no annotation/fallback path anywhere (CLAUDE.md rules 1 & 3):
//
//   - OperationID is the handler function/method symbol (e.g. `createGoal`).
//   - Path is the code-derived, group-relative, normalized template (`/`,
//     `/list`, `/{uuid}`). The dynamic group prefix (`"/" + basePath`) is a
//     lowering concern on the Rust side, not a fact here.
//   - RequestBody / Responses / Params come from the recognized gin-context
//     calls in the handler body (see handlers.go).
//
// Security, summary, tags, router-path overrides, and param enum/required-from-
// annotation are DELIBERATELY ABSENT: those were doc-comment-annotation facts.
// Security now lives in the user's gnr8 config (CLAUDE.md rule 4); the rest are
// simply gone.
type RouteFact struct {
	Method      string         `json:"method"`
	Path        string         `json:"path"`
	Handler     string         `json:"handler"`
	OperationID string         `json:"operation_id"`
	Params      []ParamFact    `json:"params"`
	RequestBody *TypeRef       `json:"request_body"`
	Responses   []ResponseFact `json:"responses"`
	Span        SourceSpan     `json:"span"`
}

// ParamFact describes a path or query parameter, derived purely from code. Path
// params come from `:param` route segments / c.Param reads (required); query
// params come from c.Query reads (type string, not required). There is no enum
// or description — those were annotation-only and are gone.
type ParamFact struct {
	Name     string     `json:"name"`
	Location string     `json:"location"`
	Required bool       `json:"required"`
	Schema   SchemaType `json:"schema"`
	Span     SourceSpan `json:"span"`
}

// ResponseFact describes one response keyed by HTTP status, from c.JSON(status, x).
type ResponseFact struct {
	Status uint16   `json:"status"`
	Body   *TypeRef `json:"body"`
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
//   - Routes by (Path, Method)
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
// source are byte-identical (GRAPH-02): params by (name, location) and responses
// by status.
func sortRoute(r *RouteFact) {
	sort.Slice(r.Params, func(a, b int) bool {
		if r.Params[a].Name != r.Params[b].Name {
			return r.Params[a].Name < r.Params[b].Name
		}
		return r.Params[a].Location < r.Params[b].Location
	})
	sort.Slice(r.Responses, func(a, b int) bool {
		return r.Responses[a].Status < r.Responses[b].Status
	})
}
