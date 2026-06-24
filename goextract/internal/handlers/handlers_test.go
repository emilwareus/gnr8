package handlers_test

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

func fixtureDir(t *testing.T) string {
	t.Helper()
	abs, err := filepath.Abs(filepath.Join("..", "..", "..", "fixtures", "goalservice"))
	if err != nil {
		t.Fatalf("resolve fixture dir: %v", err)
	}
	return abs
}

// analyzeFixture loads the fixture, recognizes routes, builds the handler index,
// sets the module prefix (so refs use 02-01 schema ids), and returns per-handler
// code facts plus the accumulated diagnostics.
func analyzeFixture(t *testing.T) (map[string]handlers.CodeFacts, []facts.DiagnosticFact) {
	t.Helper()
	res, err := load.Load(fixtureDir(t))
	if err != nil {
		t.Fatalf("load fixture: %v", err)
	}
	for _, pkg := range res.Packages {
		if pkg.Module != nil && pkg.Module.Main {
			handlers.SetModule(pkg.Module.Path)
		}
	}
	idx := handlers.BuildIndex(res)
	diags := diag.New()
	out := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		out[r.Handler] = handlers.Analyze(r, idx, diags)
	}
	return out, diags.Items()
}

func TestCreateGoalRequestAndResponses(t *testing.T) {
	facts, _ := analyzeFixture(t)
	cf, ok := facts["createGoal"]
	if !ok {
		t.Fatal("createGoal not analyzed")
	}

	if cf.RequestBody == nil || !strings.HasSuffix(cf.RequestBody.RefID, "dto.CreateGoalInput") {
		t.Fatalf("createGoal request body: want dto.CreateGoalInput ref, got %+v", cf.RequestBody)
	}

	want := map[uint16]string{201: "dto.CommandMessageWithUUID", 400: "dto.HttpError"}
	if len(cf.Responses) != len(want) {
		t.Fatalf("createGoal: want %d responses, got %d: %+v", len(want), len(cf.Responses), cf.Responses)
	}
	for _, r := range cf.Responses {
		suffix, ok := want[r.Status]
		if !ok {
			t.Errorf("createGoal: unexpected status %d", r.Status)
			continue
		}
		if r.Body == nil || !strings.HasSuffix(r.Body.RefID, suffix) {
			t.Errorf("createGoal %d: want body %s, got %+v", r.Status, suffix, r.Body)
		}
	}
}

// TestStatusFromGoConstant proves status numbers come from go/constant resolution
// of http.StatusXxx, not a hardcoded name->number map: 201, 400, 200 all appear
// exactly as the net/http constants define them.
func TestStatusFromGoConstant(t *testing.T) {
	facts, _ := analyzeFixture(t)

	create := statuses(facts["createGoal"].Responses)
	if !equalUint16(create, []uint16{201, 400}) {
		t.Errorf("createGoal statuses: want [201 400], got %v", create)
	}
	list := statuses(facts["listGoals"].Responses)
	if !equalUint16(list, []uint16{200}) {
		t.Errorf("listGoals statuses: want [200], got %v", list)
	}
}

func TestPathParamForUUIDHandlers(t *testing.T) {
	facts, _ := analyzeFixture(t)
	for _, h := range []string{"updateGoal", "deleteGoal"} {
		cf := facts[h]
		p, ok := paramByName(cf.Params, "uuid")
		if !ok {
			t.Fatalf("%s: missing path param 'uuid': %+v", h, cf.Params)
		}
		if p.Location != "path" || !p.Required || p.Schema.Kind != "string" {
			t.Errorf("%s uuid: want path/required/string, got loc=%s req=%v kind=%s", h, p.Location, p.Required, p.Schema.Kind)
		}
	}
}

func TestListGoalsQueryParamsAndDiagnostics(t *testing.T) {
	facts, diags := analyzeFixture(t)
	cf := facts["listGoals"]

	wantQuery := []string{"aggregation", "cursor", "page_size"}
	var gotQuery []string
	for _, p := range cf.Params {
		if p.Location == "query" {
			gotQuery = append(gotQuery, p.Name)
		}
	}
	if !equalStrings(sortedCopy(gotQuery), wantQuery) {
		t.Fatalf("listGoals query params: want %v, got %v", wantQuery, gotQuery)
	}

	// Exactly 3 untyped-query diagnostics, one per query param, each with file:line.
	var untyped []facts2Diag
	for _, d := range diags {
		if strings.Contains(d.Message, "untyped query param") {
			untyped = append(untyped, facts2Diag{d.Message, d.File, d.Line})
		}
	}
	if len(untyped) != 3 {
		t.Fatalf("want exactly 3 untyped-query diagnostics, got %d: %+v", len(untyped), untyped)
	}
	for _, name := range wantQuery {
		found := false
		for _, d := range untyped {
			if strings.Contains(d.msg, "'"+name+"'") {
				found = true
				if d.file == "" || d.line == 0 {
					t.Errorf("untyped-query diag for %s missing file:line: %+v", name, d)
				}
			}
		}
		if !found {
			t.Errorf("no untyped-query diagnostic for query param %q", name)
		}
	}
}

// --- helpers -------------------------------------------------------------

type facts2Diag struct {
	msg, file string
	line      uint32
}

func statuses(rs []facts.ResponseFact) []uint16 {
	out := make([]uint16, 0, len(rs))
	for _, r := range rs {
		out = append(out, r.Status)
	}
	// responses are appended in source order; sort for stable comparison.
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j] < out[j-1]; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}

func paramByName(ps []facts.ParamFact, name string) (facts.ParamFact, bool) {
	for _, p := range ps {
		if p.Name == name {
			return p, true
		}
	}
	return facts.ParamFact{}, false
}

func equalUint16(a, b []uint16) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func sortedCopy(in []string) []string {
	out := append([]string(nil), in...)
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j] < out[j-1]; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}
