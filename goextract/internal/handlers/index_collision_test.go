package handlers_test

import (
	"go/ast"
	"go/parser"
	"go/token"
	gotypes "go/types"
	"strings"
	"testing"

	"golang.org/x/tools/go/packages"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// parsePkg parses a single source string into a *packages.Package with the given
// import path, a non-nil (empty) TypesInfo, and the shared FileSet so BuildIndex
// can index its FuncDecls and resolve their positions. It deliberately avoids the
// full type checker: BuildIndex only needs the syntax + package path + fset.
func parsePkg(t *testing.T, fset *token.FileSet, pkgPath, filename, src string) *packages.Package {
	t.Helper()
	f, err := parser.ParseFile(fset, filename, src, parser.ParseComments)
	if err != nil {
		t.Fatalf("parse %s: %v", filename, err)
	}
	return &packages.Package{
		PkgPath:   pkgPath,
		Syntax:    []*ast.File{f},
		TypesInfo: &gotypes.Info{},
	}
}

// collidingResult builds a synthetic two-package load.Result where both packages
// declare a handler named `Handle`, simulating the cross-package bare-name
// collision WR-02 guards against. order controls which package is visited first,
// so a test can prove the surviving decl is independent of load order.
func collidingResult(t *testing.T, order int) *load.Result {
	t.Helper()
	fset := token.NewFileSet()
	srcA := "package a\n\nfunc Handle() {}\n"
	srcB := "package b\n\nfunc Handle() {}\n"
	pkgA := parsePkg(t, fset, "example.com/m/a", "a/handler.go", srcA)
	pkgB := parsePkg(t, fset, "example.com/m/b", "b/handler.go", srcB)
	pkgs := []*packages.Package{pkgA, pkgB}
	if order == 1 {
		pkgs = []*packages.Package{pkgB, pkgA}
	}
	return &load.Result{Packages: pkgs, Fset: fset}
}

// dupMessages collects the duplicate-handler-name diagnostic messages from an
// accumulator's items.
func dupMessages(items []diagItem) []string {
	var out []string
	for _, d := range items {
		if strings.Contains(d.Message, "duplicate handler name 'Handle'") {
			out = append(out, d.Message)
		}
	}
	return out
}

// diagItem mirrors the public DiagnosticFact fields the test reads.
type diagItem = facts.DiagnosticFact

// TestBuildIndexDuplicateNameEmitsDiagnostic proves a cross-package bare-name
// collision is surfaced as a diagnostic rather than silently last-write-wins.
func TestBuildIndexDuplicateNameEmitsDiagnostic(t *testing.T) {
	diags := diag.New()
	idx := handlers.BuildIndex(collidingResult(t, 0), diags)

	if _, ok := idx["Handle"]; !ok {
		t.Fatal("Handle must remain indexed after a collision")
	}

	dupes := dupMessages(diags.Items())
	if len(dupes) != 1 {
		t.Fatalf("want exactly 1 duplicate-handler diagnostic, got %d: %v", len(dupes), dupes)
	}
	if !strings.Contains(dupes[0], "(WR-02)") {
		t.Errorf("collision diagnostic should cite the rule id (WR-02): %q", dupes[0])
	}
}

// TestBuildIndexDuplicateSurvivorIsDeterministic proves the kept/dropped decls do
// not depend on package load order (GRAPH-02). The diagnostic names the LOSER
// (its fully-qualified identity), so a stable loser across swapped orders proves a
// stable winner. The loser must always be pkg b (example.com/m/b sorts after a).
func TestBuildIndexDuplicateSurvivorIsDeterministic(t *testing.T) {
	d0 := diag.New()
	handlers.BuildIndex(collidingResult(t, 0), d0)
	d1 := diag.New()
	handlers.BuildIndex(collidingResult(t, 1), d1)

	loser0 := loserIdentity(t, dupMessages(d0.Items()))
	loser1 := loserIdentity(t, dupMessages(d1.Items()))

	if loser0 != loser1 {
		t.Errorf("dropped decl differs by load order: order0=%q order1=%q", loser0, loser1)
	}
	if !strings.Contains(loser0, "example.com/m/b") {
		t.Errorf("deterministic loser should be pkg b (sorts after a), got %q", loser0)
	}
}

func TestAnalyzerReportsOnlyRouteReferencedDuplicateNames(t *testing.T) {
	analyzer := handlers.NewAnalyzer(collidingResult(t, 0), "example.com/m", nil)

	unreferenced := diag.New()
	analyzer.ReportRouteHandlerCollisions([]routes.Route{{Handler: "Other"}}, unreferenced)
	if dupes := dupMessages(unreferenced.Items()); len(dupes) != 0 {
		t.Fatalf("helper-only duplicate should stay silent, got %d diagnostics: %v", len(dupes), dupes)
	}

	referenced := diag.New()
	analyzer.ReportRouteHandlerCollisions([]routes.Route{{Handler: "Handle"}}, referenced)
	dupes := dupMessages(referenced.Items())
	if len(dupes) != 1 {
		t.Fatalf("route-referenced duplicate should warn once, got %d diagnostics: %v", len(dupes), dupes)
	}
}

// loserIdentity extracts the loser's identity from the single collision message
// ("... also declared at <identity>; ..."), failing if not exactly one message.
func loserIdentity(t *testing.T, dupes []string) string {
	t.Helper()
	if len(dupes) != 1 {
		t.Fatalf("want exactly 1 duplicate diagnostic, got %d: %v", len(dupes), dupes)
	}
	const pre = "also declared at "
	i := strings.Index(dupes[0], pre)
	if i < 0 {
		t.Fatalf("diagnostic missing %q: %q", pre, dupes[0])
	}
	rest := dupes[0][i+len(pre):]
	if j := strings.Index(rest, ";"); j >= 0 {
		rest = rest[:j]
	}
	return strings.TrimSpace(rest)
}
