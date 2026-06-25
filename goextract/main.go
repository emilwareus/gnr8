// Command goextract loads a target Go module with the official go/packages loader
// in full-type mode, extracts router-agnostic HTTP/schema facts, and prints a
// single deterministic sorted JSON facts document on stdout. Errors go to stderr
// with a non-zero exit (the Rust subprocess driver maps them to typed CoreError).
//
// Usage:
//
//	goextract <target-dir>
//
// 02-01 extracts DTO struct/enum schemas + float64/free-form-map diagnostics.
// Routes/handlers (02-02) and the Rust ApiGraph/inspect (02-03) build on this.
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
	"github.com/gnr8/goextract/internal/types"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: goextract <target-dir>")
		os.Exit(1)
	}
	targetDir := os.Args[1]

	if err := run(targetDir, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "goextract:", err)
		os.Exit(1)
	}
}

// run loads the module, builds the facts document, and writes JSON to w. Any hard
// loader failure is returned as an error; per-package load errors become
// diagnostics (GO-06) so a partial graph is never silently emitted.
func run(targetDir string, w *os.File) error {
	res, err := load.Load(targetDir)
	if err != nil {
		return err
	}

	diags := diag.New()
	for _, le := range res.Errors {
		file, line := splitPos(le.Pos)
		diags.Warn("go/packages load error: "+le.Msg, file, line)
	}

	schemas := types.Extract(res, diags)

	module := moduleOf(res)

	// Recognize the Gin route table, then enrich each route with handler-inferred
	// request/response/param facts. buildRoutes owns the wiring + the per-route
	// diagnostics (untyped query params, dynamic responses). Every fact is derived
	// PURELY from Go code — there is no annotation source and no fallback path
	// (CLAUDE.md rules 1 & 3).
	//
	// The Analyzer carries the module prefix as per-invocation context (WR-03), so
	// the analysis is reentrant rather than depending on process-global setup
	// ordering. Building it also indexes the handlers, surfacing any duplicate-
	// bare-name collisions as diagnostics (WR-02).
	analyzer := handlers.NewAnalyzer(res, module, diags)
	recognized := routes.Recognize(res)
	routeFacts := buildRoutes(analyzer, recognized, diags)

	doc := facts.GoFacts{
		Module:      module,
		Routes:      routeFacts,
		Schemas:     schemas,
		Diagnostics: diags.Items(),
	}

	return facts.Marshal(doc, w)
}

// buildRoutes maps each recognized Gin route to a router-agnostic RouteFact.
//
// Every fact has exactly ONE code-derived source (CLAUDE.md rules 1 & 3): the
// method/path/handler come from the route recognizer, the operationId is the
// handler symbol, and the request body / responses / params come from analyzing
// the handler body. There is no annotation source and no fallback anywhere.
func buildRoutes(analyzer *handlers.Analyzer, recognized []routes.Route, diags *diag.Accumulator) []facts.RouteFact {
	out := make([]facts.RouteFact, 0, len(recognized))
	for _, r := range recognized {
		rf := facts.RouteFact{
			Method: r.Method,
			Path:   r.Path,
			// operationId is derived deterministically from the handler symbol in
			// code (e.g. "createGoal", "updateGoal") — there is no override source.
			Handler:     r.Handler,
			OperationID: r.Handler,
			Params:      []facts.ParamFact{},
			Responses:   []facts.ResponseFact{},
			Span:        r.Span,
		}

		// Code-inferred request/response/param facts — the only source.
		cf := analyzer.Analyze(r, diags)
		rf.RequestBody = cf.RequestBody
		rf.Responses = cf.Responses
		rf.Params = cf.Params

		out = append(out, rf)
	}
	return out
}

func moduleOf(res *load.Result) string {
	for _, pkg := range res.Packages {
		if pkg.Module != nil && pkg.Module.Main {
			return pkg.Module.Path
		}
	}
	return ""
}

// splitPos parses a "file:line:col" position string from go/packages into a file
// and line by splitting on the LAST two colons (so filenames containing ':' still
// resolve the line). Missing/unparsable parts degrade to ("", 0) so a load error
// still emits as a diagnostic (GO-06).
func splitPos(pos string) (string, uint32) {
	if pos == "" {
		return "", 0
	}
	parts := strings.Split(pos, ":")
	if len(parts) < 3 {
		return "", 0
	}
	lineStr := parts[len(parts)-2]
	line, err := strconv.Atoi(lineStr)
	if err != nil {
		return "", 0
	}
	file := strings.Join(parts[:len(parts)-2], ":")
	return file, uint32(line)
}
