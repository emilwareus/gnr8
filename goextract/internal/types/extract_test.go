package types_test

import (
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

func fieldByJSON(s facts.SchemaFact, jsonName string) (facts.FieldFact, bool) {
	for _, f := range s.Fields {
		if f.JSONName == jsonName {
			return f, true
		}
	}
	return facts.FieldFact{}, false
}

func TestExtractObjectAndEnumCounts(t *testing.T) {
	schemas, _ := extractFixture(t)

	wantObjects := []string{
		"CreateGoalInput", "UpdateGoalInput", "GoalResponse", "ListGoalsOutput",
		"GoalAnalyticsQuery", "HttpError", "CommandMessage", "CommandMessageWithUUID",
	}
	var objects, enums []string
	for _, s := range schemas {
		switch s.Kind {
		case "object":
			objects = append(objects, s.Name)
		case "enum":
			enums = append(enums, s.Name)
		default:
			t.Errorf("unexpected schema kind %q for %s", s.Kind, s.Name)
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
	if len(dir.EnumValues) != 2 || dir.EnumValues[0] != "gte" || dir.EnumValues[1] != "lte" {
		t.Errorf("expected sorted enum values [gte lte], got %v", dir.EnumValues)
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
	if name.Schema.Kind != "string" {
		t.Errorf("name kind: want string, got %s", name.Schema.Kind)
	}

	// targetValue: optional (pointer+omitempty), number.
	tv, ok := fieldByJSON(s, "targetValue")
	if !ok {
		t.Fatal("field 'targetValue' not found")
	}
	if !tv.Optional || tv.Required {
		t.Errorf("targetValue: want optional=true required=false, got optional=%v required=%v", tv.Optional, tv.Required)
	}
	if tv.Schema.Kind != "number" {
		t.Errorf("targetValue kind: want number, got %s", tv.Schema.Kind)
	}

	// workflowChainIds: array of string format uuid.
	wc, ok := fieldByJSON(s, "workflowChainIds")
	if !ok {
		t.Fatal("field 'workflowChainIds' not found")
	}
	if wc.Schema.Kind != "array" || wc.Schema.Items == nil {
		t.Fatalf("workflowChainIds: want array with items, got %+v", wc.Schema)
	}
	if wc.Schema.Items.Kind != "string" || wc.Schema.Items.Format == nil || *wc.Schema.Items.Format != "uuid" {
		t.Errorf("workflowChainIds items: want string/uuid, got %+v", wc.Schema.Items)
	}

	// analyticsQuery: ref to GoalAnalyticsQuery schema.
	aq, ok := fieldByJSON(s, "analyticsQuery")
	if !ok {
		t.Fatal("field 'analyticsQuery' not found")
	}
	if aq.Schema.Kind != "ref" || aq.Schema.RefID == nil {
		t.Fatalf("analyticsQuery: want ref, got %+v", aq.Schema)
	}
	if *aq.Schema.RefID != "internal/common/dto.GoalAnalyticsQuery" {
		t.Errorf("analyticsQuery ref_id: want internal/common/dto.GoalAnalyticsQuery, got %s", *aq.Schema.RefID)
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
	if createdAt.Schema.Kind != "string" || createdAt.Schema.Format == nil || *createdAt.Schema.Format != "date-time" {
		t.Errorf("createdAt: want string/date-time, got %+v", createdAt.Schema)
	}

	metadata, ok := fieldByJSON(s, "metadata")
	if !ok {
		t.Fatal("metadata field not found")
	}
	if metadata.Schema.Kind != "object" || metadata.Schema.AdditionalProperties == nil || !*metadata.Schema.AdditionalProperties {
		t.Errorf("metadata: want object additionalProperties=true, got %+v", metadata.Schema)
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
