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

	// 02-02: recognize the Gin route table (Task 1), then enrich each route with
	// handler-inferred request/response/param facts (Task 2) and swaggo
	// annotation facts (Task 3). buildRoutes owns the wiring + the per-route
	// diagnostics (untyped query params, dynamic responses).
	handlers.SetModule(module) // refs share the 02-01 module-relative schema id.
	handlers.SetAnnotationPackages(handlers.AnnotationPackagesFromResult(res, module))
	recognized := routes.Recognize(res)
	idx := handlers.BuildIndex(res)
	routeFacts := buildRoutes(recognized, idx, diags)

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
// 02-02 Task 1 emits the route table skeleton (method, group-relative normalized
// path, handler, secured, span). Tasks 2 and 3 enrich each fact in place with
// handler-inferred request/response/param facts and swaggo annotation facts; the
// enrichment is layered here so the wiring order (code primary, annotation
// escape-hatch) stays explicit.
func buildRoutes(recognized []routes.Route, idx handlers.Index, diags *diag.Accumulator) []facts.RouteFact {
	out := make([]facts.RouteFact, 0, len(recognized))
	for _, r := range recognized {
		rf := facts.RouteFact{
			Method:          r.Method,
			Path:            r.Path,
			Handler:         r.Handler,
			Tags:            []string{},
			Secured:         r.Secured,
			SecuritySchemes: []string{},
			Params:          []facts.ParamFact{},
			Responses:       []facts.ResponseFact{},
			Span:            r.Span,
		}

		// Task 2: code-inferred request/response/param facts (primary).
		cf := handlers.Analyze(r, idx, diags)
		rf.RequestBody = cf.RequestBody
		rf.Responses = cf.Responses
		rf.Params = cf.Params

		// Task 3: swaggo annotation facts (escape hatch) fill gaps + add
		// metadata (summary, tags, @ID, @Security, @Router, query enums,
		// missing responses) without clobbering code-resolved facts.
		handlers.MergeAnnotations(&rf, idx.ParseAnnotations(r.Handler))

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
