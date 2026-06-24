package handlers_test

import (
	"strings"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// buildMergedRoutes replicates main.go's wiring: recognize routes, analyze
// handlers (code-primary), then merge swaggo annotations (escape hatch). Returns
// the fully-merged RouteFacts keyed by handler so tests can assert the merge.
func buildMergedRoutes(t *testing.T) map[string]facts.RouteFact {
	t.Helper()
	res, err := load.Load(fixtureDir(t))
	if err != nil {
		t.Fatalf("load fixture: %v", err)
	}
	var module string
	for _, pkg := range res.Packages {
		if pkg.Module != nil && pkg.Module.Main {
			module = pkg.Module.Path
		}
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, module, diags)
	out := map[string]facts.RouteFact{}
	for _, r := range routes.Recognize(res) {
		rf := facts.RouteFact{
			Method: r.Method, Path: r.Path, Handler: r.Handler,
			Tags: []string{}, Secured: r.Secured, SecuritySchemes: []string{},
			Params: []facts.ParamFact{}, Responses: []facts.ResponseFact{}, Span: r.Span,
		}
		cf := analyzer.Analyze(r, diags)
		rf.RequestBody = cf.RequestBody
		rf.Responses = cf.Responses
		rf.Params = cf.Params
		handlers.MergeAnnotations(&rf, analyzer.ParseAnnotations(r.Handler))
		out[r.Handler] = rf
	}
	return out
}

func TestAnnotationUpdateGoalOperationAndSecurity(t *testing.T) {
	merged := buildMergedRoutes(t)
	rf := merged["updateGoal"]

	if rf.OperationID == nil || *rf.OperationID != "goalUuidPut" {
		t.Errorf("updateGoal operation_id: want goalUuidPut (from @ID), got %v", rf.OperationID)
	}
	if rf.Summary == nil || *rf.Summary != "Update goal" {
		t.Errorf("updateGoal summary: want 'Update goal', got %v", rf.Summary)
	}
	if !containsStr(rf.SecuritySchemes, "ApiKeyAuth") {
		t.Errorf("updateGoal security: want ApiKeyAuth scheme, got %v", rf.SecuritySchemes)
	}
	if !rf.Secured {
		t.Error("updateGoal: @Security must mark the route secured")
	}
	if rf.RouterPath == nil || *rf.RouterPath != "/{uuid}" {
		t.Errorf("updateGoal @Router override: want /{uuid}, got %v", rf.RouterPath)
	}
	if rf.Method != "PUT" {
		t.Errorf("updateGoal method: want PUT, got %s", rf.Method)
	}
}

func TestAnnotationListGoalsAggregationEnum(t *testing.T) {
	merged := buildMergedRoutes(t)
	rf := merged["listGoals"]

	var agg *facts.ParamFact
	for i := range rf.Params {
		if rf.Params[i].Name == "aggregation" {
			agg = &rf.Params[i]
		}
	}
	if agg == nil {
		t.Fatalf("listGoals: aggregation param missing: %+v", rf.Params)
	}
	if !agg.Required {
		t.Error("aggregation: @Param 'true' must upgrade the code-derived param to required")
	}
	want := []string{"avg", "count", "max", "min", "sum"}
	if !equalStrings(agg.EnumValues, want) {
		t.Errorf("aggregation enum_values: want %v (sorted, from Enums annotation), got %v", want, agg.EnumValues)
	}
}

// TestAnnotationFillsResponseGapWithoutClobber proves the merge rule: code-resolved
// responses keep their bodies; an annotation-only status (404, never emitted by
// code) is added.
func TestAnnotationFillsResponseGapWithoutClobber(t *testing.T) {
	merged := buildMergedRoutes(t)
	rf := merged["updateGoal"]

	byStatus := map[uint16]facts.ResponseFact{}
	for _, r := range rf.Responses {
		byStatus[r.Status] = r
	}

	// 200 + 400 are code-resolved (c.JSON(StatusOK/StatusBadRequest, ...)); their
	// bodies must be the code-resolved named types, NOT overwritten.
	r200, ok := byStatus[200]
	if !ok || r200.Body == nil || !strings.HasSuffix(r200.Body.RefID, "dto.CommandMessage") {
		t.Errorf("updateGoal 200: want code-resolved dto.CommandMessage, got %+v", r200.Body)
	}
	r400, ok := byStatus[400]
	if !ok || r400.Body == nil || !strings.HasSuffix(r400.Body.RefID, "dto.HttpError") {
		t.Errorf("updateGoal 400: want code-resolved dto.HttpError, got %+v", r400.Body)
	}
	// 404 is annotation-ONLY (@Failure 404); the merge must ADD it.
	r404, ok := byStatus[404]
	if !ok {
		t.Fatalf("updateGoal 404: annotation-only response must be filled in; got statuses %v", keys(byStatus))
	}
	if r404.Body == nil || !strings.HasSuffix(r404.Body.RefID, "dto.HttpError") {
		t.Errorf("updateGoal 404: want dto.HttpError from @Failure, got %+v", r404.Body)
	}
}

// TestAnnotationCreateGoalHasNoAnnotationBlock confirms a fully code-inferable
// handler (createGoal, no @-block) is unaffected by the annotation pass.
func TestAnnotationCreateGoalUnaffected(t *testing.T) {
	merged := buildMergedRoutes(t)
	rf := merged["createGoal"]
	if rf.OperationID != nil {
		t.Errorf("createGoal: no @ID, operation_id should be nil, got %v", rf.OperationID)
	}
	if rf.RequestBody == nil || !strings.HasSuffix(rf.RequestBody.RefID, "dto.CreateGoalInput") {
		t.Errorf("createGoal request body should remain code-inferred, got %+v", rf.RequestBody)
	}
}

// --- helpers -------------------------------------------------------------

func containsStr(ss []string, want string) bool {
	for _, s := range ss {
		if s == want {
			return true
		}
	}
	return false
}

func keys(m map[uint16]facts.ResponseFact) []uint16 {
	out := make([]uint16, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}
