package types_test

import (
	"os"
	"path/filepath"
	"sort"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/types"
)

// fixtureDir resolves the real goalservice fixture from this test file's location
// (../../../fixtures/goalservice relative to goextract/internal/types).
func fixtureDir(t *testing.T) string {
	t.Helper()
	abs, err := filepath.Abs(filepath.Join("..", "..", "..", "fixtures", "goalservice"))
	if err != nil {
		t.Fatalf("resolve fixture dir: %v", err)
	}
	return abs
}

func extractFixture(t *testing.T) ([]facts.SchemaFact, []facts.DiagnosticFact) {
	t.Helper()
	res, err := load.Load(fixtureDir(t))
	if err != nil {
		t.Fatalf("load fixture: %v", err)
	}
	diags := diag.New()
	schemas := types.Extract(res, diags)
	return schemas, diags.Items()
}

func schemaByName(schemas []facts.SchemaFact, name string) (facts.SchemaFact, bool) {
	for _, s := range schemas {
		if s.Name == name {
			return s, true
		}
	}
	return facts.SchemaFact{}, false
}

// objectFields returns a named schema's object fields, or nil if its body is not an
// object (the neutral Type body carries fields as Type{Type: "object", Of: []FieldFact}).
func objectFields(s facts.SchemaFact) []facts.FieldFact {
	if s.Body.Type != facts.TypeObject {
		return nil
	}
	fields, _ := s.Body.Of.([]facts.FieldFact)
	return fields
}

// enumMembers returns a named schema's enum members, or nil if its body is not an enum.
func enumMembers(s facts.SchemaFact) []string {
	if s.Body.Type != facts.TypeEnum {
		return nil
	}
	members, _ := s.Body.Of.([]string)
	return members
}

func fieldByJSON(s facts.SchemaFact, jsonName string) (facts.FieldFact, bool) {
	for _, f := range objectFields(s) {
		if f.JSONName == jsonName {
			return f, true
		}
	}
	return facts.FieldFact{}, false
}

// primName returns the Prim tag of a primitive Type (e.g. "string", "int"), or "".
func primName(ty facts.Type) string {
	if ty.Type != facts.TypePrimitive {
		return ""
	}
	if p, ok := ty.Of.(*facts.Prim); ok {
		return p.Prim
	}
	return ""
}

// wellKnownName returns the canonical name of a well_known Type (e.g. "uuid"), or "".
func wellKnownName(ty facts.Type) string {
	if ty.Type != facts.TypeWellKnown {
		return ""
	}
	name, _ := ty.Of.(string)
	return name
}

func TestExtractObjectAndEnumCounts(t *testing.T) {
	schemas, _ := extractFixture(t)

	wantObjects := []string{
		"CreateGoalInput", "UpdateGoalInput", "GoalResponse", "ListGoalsOutput",
		"GoalAnalyticsQuery", "HttpError", "CommandMessage", "CommandMessageWithUUID",
	}
	var objects, enums []string
	for _, s := range schemas {
		switch s.Body.Type {
		case facts.TypeObject:
			objects = append(objects, s.Name)
		case facts.TypeEnum:
			enums = append(enums, s.Name)
		default:
			t.Errorf("unexpected schema body kind %q for %s", s.Body.Type, s.Name)
		}
	}
	sort.Strings(objects)
	sort.Strings(wantObjects)

	if len(objects) != len(wantObjects) {
		t.Fatalf("expected %d object schemas, got %d: %v", len(wantObjects), len(objects), objects)
	}
	for i := range wantObjects {
		if objects[i] != wantObjects[i] {
			t.Errorf("object schema mismatch at %d: want %s got %s", i, wantObjects[i], objects[i])
		}
	}

	if len(enums) != 1 || enums[0] != "TargetDirection" {
		t.Fatalf("expected exactly the TargetDirection enum, got %v", enums)
	}

	dir, ok := schemaByName(schemas, "TargetDirection")
	if !ok {
		t.Fatal("TargetDirection enum not found")
	}
	members := enumMembers(dir)
	if len(members) != 2 || members[0] != "gte" || members[1] != "lte" {
		t.Errorf("expected sorted enum members [gte lte], got %v", members)
	}
}

func TestCreateGoalInputFields(t *testing.T) {
	schemas, _ := extractFixture(t)
	s, ok := schemaByName(schemas, "CreateGoalInput")
	if !ok {
		t.Fatal("CreateGoalInput not found")
	}

	// name: required, not optional, string.
	name, ok := fieldByJSON(s, "name")
	if !ok {
		t.Fatal("field 'name' not found")
	}
	if !name.Required || name.Optional {
		t.Errorf("name: want required=true optional=false, got required=%v optional=%v", name.Required, name.Optional)
	}
	if got := primName(name.Schema); got != facts.PrimString {
		t.Errorf("name type: want primitive string, got %q (%+v)", got, name.Schema)
	}

	// targetValue: optional+nullable (a pointer), float primitive.
	tv, ok := fieldByJSON(s, "targetValue")
	if !ok {
		t.Fatal("field 'targetValue' not found")
	}
	if !tv.Optional || tv.Required {
		t.Errorf("targetValue: want optional=true required=false, got optional=%v required=%v", tv.Optional, tv.Required)
	}
	if !tv.Nullable {
		t.Errorf("targetValue: a pointer field must be nullable, got nullable=%v", tv.Nullable)
	}
	if got := primName(tv.Schema); got != facts.PrimFloat {
		t.Errorf("targetValue type: want primitive float, got %q (%+v)", got, tv.Schema)
	}

	// workflowChainIds: array of well-known uuid.
	wc, ok := fieldByJSON(s, "workflowChainIds")
	if !ok {
		t.Fatal("field 'workflowChainIds' not found")
	}
	if wc.Schema.Type != facts.TypeArray {
		t.Fatalf("workflowChainIds: want array, got %+v", wc.Schema)
	}
	elem, ok := wc.Schema.Of.(*facts.Type)
	if !ok || elem == nil {
		t.Fatalf("workflowChainIds: array element missing, got %+v", wc.Schema.Of)
	}
	if got := wellKnownName(*elem); got != facts.WellKnownUUID {
		t.Errorf("workflowChainIds element: want well-known uuid, got %q (%+v)", got, *elem)
	}

	// analyticsQuery: named ref to GoalAnalyticsQuery schema.
	aq, ok := fieldByJSON(s, "analyticsQuery")
	if !ok {
		t.Fatal("field 'analyticsQuery' not found")
	}
	if aq.Schema.Type != facts.TypeNamed {
		t.Fatalf("analyticsQuery: want named ref, got %+v", aq.Schema)
	}
	if id, _ := aq.Schema.Of.(string); id != "internal/common/dto.GoalAnalyticsQuery" {
		t.Errorf("analyticsQuery named id: want internal/common/dto.GoalAnalyticsQuery, got %v", aq.Schema.Of)
	}
}

func TestValidateRequiredTagsMarkFieldsRequired(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(
		filepath.Join(dir, "go.mod"),
		[]byte("module example.com/validatefixture\n\ngo 1.22\n"),
		0o644,
	); err != nil {
		t.Fatalf("write go.mod: %v", err)
	}
	if err := os.WriteFile(
		filepath.Join(dir, "models.go"),
		[]byte(`package validatefixture

type FileRef struct {
	FileID   string  `+"`json:\"fileId\" validate:\"required\"`"+`
	Filename string  `+"`json:\"filename\" validate:\"required,email\"`"+`
	Label    string  `+"`json:\"label,omitempty\"`"+`
	Note     *string `+"`json:\"note\"`"+`
}
`),
		0o644,
	); err != nil {
		t.Fatalf("write models.go: %v", err)
	}

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load validate fixture: %v", err)
	}
	diags := diag.New()
	schemas := types.Extract(res, diags)
	s, ok := schemaByName(schemas, "FileRef")
	if !ok {
		t.Fatal("FileRef not found")
	}

	fileID, ok := fieldByJSON(s, "fileId")
	if !ok {
		t.Fatal("field 'fileId' not found")
	}
	if !fileID.Required {
		t.Fatal("validate:\"required\" should mark fileId required")
	}
	filename, ok := fieldByJSON(s, "filename")
	if !ok {
		t.Fatal("field 'filename' not found")
	}
	if !filename.Required {
		t.Fatal("validate:\"required,email\" should mark filename required")
	}
	label, ok := fieldByJSON(s, "label")
	if !ok {
		t.Fatal("field 'label' not found")
	}
	if !label.Optional || label.Nullable {
		t.Fatalf("omitempty should be optional but not nullable, got optional=%v nullable=%v", label.Optional, label.Nullable)
	}
	note, ok := fieldByJSON(s, "note")
	if !ok {
		t.Fatal("field 'note' not found")
	}
	if !note.Nullable || note.Required {
		t.Fatalf("pointer should be nullable without forcing required, got nullable=%v required=%v", note.Nullable, note.Required)
	}
}

func TestEmbeddedFlattening(t *testing.T) {
	schemas, _ := extractFixture(t)
	s, ok := schemaByName(schemas, "CommandMessageWithUUID")
	if !ok {
		t.Fatal("CommandMessageWithUUID not found")
	}
	if _, ok := fieldByJSON(s, "message"); !ok {
		t.Error("embedded CommandMessage.message not flattened into CommandMessageWithUUID")
	}
	if _, ok := fieldByJSON(s, "uuid"); !ok {
		t.Error("uuid field missing from CommandMessageWithUUID")
	}
}

func TestGoalResponseWellKnownAndFreeFormMap(t *testing.T) {
	schemas, diags := extractFixture(t)
	s, ok := schemaByName(schemas, "GoalResponse")
	if !ok {
		t.Fatal("GoalResponse not found")
	}

	createdAt, ok := fieldByJSON(s, "createdAt")
	if !ok {
		t.Fatal("createdAt field not found")
	}
	if got := wellKnownName(createdAt.Schema); got != facts.WellKnownDateTime {
		t.Errorf("createdAt: want well-known date_time, got %q (%+v)", got, createdAt.Schema)
	}

	metadata, ok := fieldByJSON(s, "metadata")
	if !ok {
		t.Fatal("metadata field not found")
	}
	// A free-form Go map lowers to the neutral Any type (explicitly lossy).
	if metadata.Schema.Type != facts.TypeAny {
		t.Errorf("metadata: want any (free-form), got %+v", metadata.Schema)
	}

	// A free-form-map diagnostic for GoalResponse.Metadata must exist.
	if !hasDiag(diags, "free-form map field", "Metadata") {
		t.Errorf("expected free-form-map diagnostic for GoalResponse.Metadata, got %v", diags)
	}
}

func TestFloat64Diagnostics(t *testing.T) {
	_, diags := extractFixture(t)
	owners := []string{"CreateGoalInput", "UpdateGoalInput", "GoalResponse"}
	for _, owner := range owners {
		if !hasDiag(diags, "float64 -> float32 narrowing", owner+".TargetValue") {
			t.Errorf("expected float64 narrowing diagnostic for %s.TargetValue, got %v", owner, diags)
		}
	}
	// Every diagnostic must carry a file:line.
	for _, d := range diags {
		if d.File == "" || d.Line == 0 {
			t.Errorf("diagnostic missing file:line: %+v", d)
		}
	}
}

func hasDiag(diags []facts.DiagnosticFact, ruleSubstr, identitySubstr string) bool {
	for _, d := range diags {
		if containsAll(d.Message, ruleSubstr, identitySubstr) {
			return true
		}
	}
	return false
}

func containsAll(s string, subs ...string) bool {
	for _, sub := range subs {
		if !contains(s, sub) {
			return false
		}
	}
	return true
}

func contains(s, sub string) bool {
	return len(sub) == 0 || (len(s) >= len(sub) && indexOf(s, sub) >= 0)
}

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}
