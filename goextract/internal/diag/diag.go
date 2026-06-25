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

// UntypedQueryParam records the untyped-query-param warning (TARGET-API.md §5.4):
// a c.Query("name") read with no binding struct, so the param's type/required-ness
// is under-specified by code alone. method + route identify the operation; the
// param name + rule are the machine-stable identity (the canonical rendered
// wording — which differs per param in expected/diagnostics.txt — is reconciled
// on the Rust side in 02-03).
func (a *Accumulator) UntypedQueryParam(name, method, route, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message: "untyped query param '" + name + "' on " + method + " " + route +
			": read via c.Query with no binding struct; param type/required-ness " +
			"under-specified, type inferred as string only (TARGET-API.md §5.4)",
		File: file,
		Line: line,
	})
}

// DynamicResponse records the dynamic/unresolvable-response warning (D-05 / GO-06):
// a c.JSON(...) whose status or body could not be resolved to a constant/named
// type. The response is diagnosed rather than guessed or silently dropped; there
// is no secondary source to recover it from (CLAUDE.md rule 3).
func (a *Accumulator) DynamicResponse(handler, reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message: "dynamic response in handler " + handler + ": " + reason +
			"; cannot infer a typed response from code (TARGET-API.md §5; D-05)",
		File: file,
		Line: line,
	})
}

// UnsupportedType records that a struct field's declared type has no faithful
// neutral primitive (e.g. complex64/128, uintptr, an untyped constant kind), so
// the extractor lowers it to free-form `any` rather than guessing a concrete
// type (GO-06 / CLAUDE.md rule 3: diagnose, never fabricate). goType is the
// rendered Go type. method/route identity here is the struct.field + declared
// type, mirroring Floatf/FreeFormMap.
func (a *Accumulator) UnsupportedType(structName, fieldName, goType, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Severity: severityWarn,
		Message: "unsupported field type: " + structName + "." + fieldName +
			" (" + goType + ") has no neutral primitive; lowered to free-form any " +
			"rather than guessing a concrete type (GO-06)",
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
