package facts_test

import (
	"bytes"
	"encoding/json"
	"testing"

	"github.com/gnr8/goextract/internal/facts"
)

// unsortedDoc builds a GoFacts value whose slices are intentionally out of order
// so the test proves Marshal sorts (GRAPH-02 discipline at the helper boundary).
func unsortedDoc() facts.GoFacts {
	return facts.GoFacts{
		Module: "github.com/acme/svc",
		Routes: []facts.RouteFact{}, // empty -> must marshal as [], not null
		Schemas: []facts.SchemaFact{
			{
				ID:   "internal/dto.Zebra",
				Name: "Zebra",
				Kind: "object",
				Fields: []facts.FieldFact{
					{JSONName: "yankee", Schema: facts.SchemaType{Kind: "string"}},
					{JSONName: "alpha", Schema: facts.SchemaType{Kind: "string"}},
				},
				EnumValues: []string{},
				Span:       facts.SourceSpan{File: "z.go", StartLine: 1, EndLine: 1},
			},
			{
				ID:         "internal/dto.Apple",
				Name:       "Apple",
				Kind:       "enum",
				Fields:     []facts.FieldFact{},
				EnumValues: []string{"lte", "gte"},
				Span:       facts.SourceSpan{File: "a.go", StartLine: 1, EndLine: 1},
			},
		},
		Diagnostics: []facts.DiagnosticFact{
			{Severity: "WARN", Message: "second", File: "b.go", Line: 10},
			{Severity: "WARN", Message: "first", File: "a.go", Line: 5},
		},
	}
}

func TestDeterminism(t *testing.T) {
	var buf1, buf2 bytes.Buffer
	if err := facts.Marshal(unsortedDoc(), &buf1); err != nil {
		t.Fatalf("marshal 1: %v", err)
	}
	if err := facts.Marshal(unsortedDoc(), &buf2); err != nil {
		t.Fatalf("marshal 2: %v", err)
	}
	if !bytes.Equal(buf1.Bytes(), buf2.Bytes()) {
		t.Fatalf("non-deterministic output:\n--- run 1 ---\n%s\n--- run 2 ---\n%s", buf1.String(), buf2.String())
	}
}

func TestSchemasSortedByID(t *testing.T) {
	var buf bytes.Buffer
	if err := facts.Marshal(unsortedDoc(), &buf); err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var out facts.GoFacts
	if err := json.Unmarshal(buf.Bytes(), &out); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(out.Schemas) != 2 {
		t.Fatalf("expected 2 schemas, got %d", len(out.Schemas))
	}
	// Apple < Zebra by id.
	if out.Schemas[0].ID != "internal/dto.Apple" || out.Schemas[1].ID != "internal/dto.Zebra" {
		t.Errorf("schemas not sorted by id: %s, %s", out.Schemas[0].ID, out.Schemas[1].ID)
	}
	// Object fields sorted by json name: alpha < yankee.
	zebra := out.Schemas[1]
	if len(zebra.Fields) != 2 || zebra.Fields[0].JSONName != "alpha" || zebra.Fields[1].JSONName != "yankee" {
		t.Errorf("fields not sorted by json name: %+v", zebra.Fields)
	}
	// Enum values sorted: gte < lte.
	apple := out.Schemas[0]
	if len(apple.EnumValues) != 2 || apple.EnumValues[0] != "gte" || apple.EnumValues[1] != "lte" {
		t.Errorf("enum values not sorted: %v", apple.EnumValues)
	}
	// Diagnostics sorted by (file, line): a.go:5 before b.go:10.
	if len(out.Diagnostics) != 2 || out.Diagnostics[0].File != "a.go" || out.Diagnostics[1].File != "b.go" {
		t.Errorf("diagnostics not sorted: %+v", out.Diagnostics)
	}
}

func TestEmptySlicesMarshalAsArrays(t *testing.T) {
	var buf bytes.Buffer
	doc := facts.GoFacts{
		Module:      "m",
		Routes:      []facts.RouteFact{},
		Schemas:     []facts.SchemaFact{},
		Diagnostics: []facts.DiagnosticFact{},
	}
	if err := facts.Marshal(doc, &buf); err != nil {
		t.Fatalf("marshal: %v", err)
	}
	s := buf.String()
	// Empty slices must serialize as `[]`, never `null` (stable, non-nil keys).
	for _, key := range []string{`"routes": []`, `"schemas": []`, `"diagnostics": []`} {
		if !bytes.Contains(buf.Bytes(), []byte(key)) {
			t.Errorf("expected %q in output, got:\n%s", key, s)
		}
	}
	if bytes.Contains(buf.Bytes(), []byte("null")) {
		t.Errorf("output must not contain null for empty slices:\n%s", s)
	}
}
