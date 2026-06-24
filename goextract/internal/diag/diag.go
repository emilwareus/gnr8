// Package diag accumulates analysis diagnostics (severity + message + file:line)
// so that unsupported or lossy patterns surface as a diagnostic rather than a
// panic or a silent drop (GO-06 / D-10).
//
// The messages here keep a machine-stable rule + field identity (e.g.
// "CreateGoalInput.TargetValue (*float64)"). The canonical rendered wording that
// must reconcile with fixtures/goalservice/expected/diagnostics.txt is finalized
// on the Rust side in 02-03 — this package only needs to be stable and to carry
// the rule + identity + position.
package diag

import "github.com/gnr8/goextract/internal/facts"

const severityWarn = "WARN"

// Accumulator collects DiagnosticFact values during extraction.
type Accumulator struct {
	items []facts.DiagnosticFact
}

// New returns an empty accumulator.
func New() *Accumulator {
	return &Accumulator{items: []facts.DiagnosticFact{}}
}

// Floatf records the float64 -> float32 narrowing warning for a struct field
// (TARGET-API.md §5.2). goType is the rendered Go type (e.g. "*float64").
func (a *Accumulator) Floatf(structName, fieldName, goType, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message: "float64 -> float32 narrowing: field " + structName + "." + fieldName +
			" (" + goType + ") loses precision in the generated Go SDK; map to float64 or " +
			"surface a compatibility diagnostic (TARGET-API.md §5.2)",
		File: file,
		Line: line,
	})
}

// FreeFormMap records the free-form map warning for a struct field
// (TARGET-API.md §5.1): map[string]any lowers to additionalProperties: true.
func (a *Accumulator) FreeFormMap(structName, fieldName, goType, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message: "free-form map field: " + structName + "." + fieldName +
			" (" + goType + ") lowers to additionalProperties: true; downstream generators " +
			"may mishandle untyped maps (TARGET-API.md §5.1)",
		File: file,
		Line: line,
	})
}

// Warn records a generic warning. Used for go/packages load errors (GO-06) and
// any rule that does not have a dedicated helper.
func (a *Accumulator) Warn(message, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message:  message,
		File:     file,
		Line:     line,
	})
}

// Items returns the accumulated diagnostics. The caller (facts.Marshal) sorts
// them into a stable order before emitting.
func (a *Accumulator) Items() []facts.DiagnosticFact {
	return a.items
}
