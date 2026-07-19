package diag

import "testing"

func TestDynamicResponseCarriesRouteIdentity(t *testing.T) {
	diagnostics := New()
	diagnostics.DynamicResponse("POST", "/widgets", "createWidget", "dynamic body", "handlers.go", 42)

	items := diagnostics.Items()
	if len(items) != 1 {
		t.Fatalf("got %d diagnostics, want 1", len(items))
	}
	diagnostic := items[0]
	if diagnostic.Code != "response.schema.unresolved" {
		t.Fatalf("code = %q, want response.schema.unresolved", diagnostic.Code)
	}
	if diagnostic.Category != categoryResponse {
		t.Fatalf("category = %q, want %q", diagnostic.Category, categoryResponse)
	}
	if diagnostic.Operation != "POST /widgets" {
		t.Fatalf("operation = %q, want POST /widgets", diagnostic.Operation)
	}
	if diagnostic.Subject != "createWidget" {
		t.Fatalf("subject = %q, want createWidget", diagnostic.Subject)
	}
}

func TestSchemaMetadataCarriesSchemaAndField(t *testing.T) {
	diagnostics := New()
	diagnostics.SchemaMetadataUnresolved("Widget", "Limit", "bad default", "dto.go", 9)

	diagnostic := diagnostics.Items()[0]
	if diagnostic.Code != "schema.metadata.unresolved" || diagnostic.Category != categorySchema {
		t.Fatalf("unexpected identity: %q/%q", diagnostic.Code, diagnostic.Category)
	}
	if diagnostic.Schema != "Widget" || diagnostic.Subject != "Limit" {
		t.Fatalf("unexpected schema subject: %q/%q", diagnostic.Schema, diagnostic.Subject)
	}
}
