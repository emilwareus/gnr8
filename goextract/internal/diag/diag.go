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

const (
	categorySource           = "source"
	categoryRequestParameter = "request_parameter"
	categoryRequestBody      = "request_body"
	categoryResponse         = "response"
	categorySchema           = "schema"
)

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
		Code:     "schema.numeric.narrowing",
		Severity: severityWarn,
		Category: categorySchema,
		Message: "float64 -> float32 narrowing: field " + structName + "." + fieldName +
			" (" + goType + ") loses precision in the generated Go SDK; map to float64 or " +
			"surface a compatibility diagnostic (TARGET-API.md §5.2)",
		File:    file,
		Line:    line,
		EndLine: line,
		Schema:  structName,
		Subject: fieldName,
	})
}

// FreeFormMap records the free-form map warning for a struct field
// (TARGET-API.md §5.1): map[string]any lowers to additionalProperties: true.
func (a *Accumulator) FreeFormMap(structName, fieldName, goType, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     "schema.free_form_map",
		Severity: severityWarn,
		Category: categorySchema,
		Message: "free-form map field: " + structName + "." + fieldName +
			" (" + goType + ") lowers to additionalProperties: true; downstream generators " +
			"may mishandle untyped maps (TARGET-API.md §5.1)",
		File:    file,
		Line:    line,
		EndLine: line,
		Schema:  structName,
		Subject: fieldName,
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
		Code:     "request.parameter.unresolved",
		Severity: severityWarn,
		Category: categoryRequestParameter,
		Message: "untyped query param '" + name + "' on " + method + " " + route +
			": read via c.Query with no binding struct; param type/required-ness " +
			"under-specified, type inferred as string only (TARGET-API.md §5.4)",
		File:      file,
		Line:      line,
		EndLine:   line,
		Operation: method + " " + route,
		Subject:   name,
	})
}

// RequestParameterUnresolved records a helper boundary where a Gin context was
// passed but the module-owned implementation could not be traversed. Keeping
// this distinct from an untyped direct Query read lets CI deny incomplete call
// graph analysis with the same stable request.parameter.unresolved identity.
func (a *Accumulator) RequestParameterUnresolved(subject, method, route, reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     "request.parameter.unresolved",
		Severity: severityWarn,
		Category: categoryRequestParameter,
		Message: "request parameter analysis stopped at " + subject + " on " + method + " " + route +
			": " + reason + "; add an explicit parameter override or keep the helper within the loaded module",
		File:      file,
		Line:      line,
		EndLine:   line,
		Operation: method + " " + route,
		Subject:   subject,
	})
}

// RequestBodyUnresolved records a request-body shape or media type that Gin
// accepts at runtime but the extractor cannot represent faithfully. The stable
// code is intentionally distinct from request-parameter failures so callers can
// deny incomplete body extraction independently.
func (a *Accumulator) RequestBodyUnresolved(subject, method, route, reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     "request.body.unresolved",
		Severity: severityWarn,
		Category: categoryRequestBody,
		Message: "request body analysis is incomplete for " + subject + " on " + method + " " + route +
			": " + reason + "; add an explicit body override or use a statically typed binding",
		File:      file,
		Line:      line,
		EndLine:   line,
		Operation: method + " " + route,
		Subject:   subject,
	})
}

// DynamicResponse records the dynamic/unresolvable-response warning (D-05 / GO-06):
// a c.JSON(...) whose status or body could not be resolved to a constant/named
// type. The response is diagnosed rather than guessed or silently dropped; there
// is no secondary source to recover it from (CLAUDE.md rule 3).
func (a *Accumulator) DynamicResponse(handler, reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     "response.schema.unresolved",
		Severity: severityWarn,
		Category: categoryResponse,
		Message: "dynamic response in handler " + handler + ": " + reason +
			"; cannot infer a typed response from code (TARGET-API.md §5; D-05)",
		File:      file,
		Line:      line,
		EndLine:   line,
		Operation: handler,
	})
}

// ResponseMediaTypeUnresolved records a response whose status and body kind are
// known but whose runtime media type is dynamic. The operation identity allows
// a checked response override to retire exactly the diagnostic it resolves.
func (a *Accumulator) ResponseMediaTypeUnresolved(method, route, reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:      "response.media_type.unresolved",
		Severity:  severityWarn,
		Category:  categoryResponse,
		Message:   reason,
		File:      file,
		Line:      line,
		EndLine:   line,
		Operation: method + " " + route,
	})
}

// UnsupportedRoutePattern records a Gin route registration shape that cannot be
// lowered faithfully. The route is skipped rather than guessed so migration
// review can fix the source pattern or add an explicit custom Source/Transform.
func (a *Accumulator) UnsupportedRoutePattern(reason, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     "source.route.unresolved",
		Severity: severityWarn,
		Category: categorySource,
		Message: "unsupported Gin route pattern: " + reason +
			"; route skipped rather than guessed (GO-04)",
		File:    file,
		Line:    line,
		EndLine: line,
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
		Code:     "schema.type.unresolved",
		Severity: severityWarn,
		Category: categorySchema,
		Message: "unsupported field type: " + structName + "." + fieldName +
			" (" + goType + ") has no neutral primitive; lowered to free-form any " +
			"rather than guessing a concrete type (GO-06)",
		File:    file,
		Line:    line,
		EndLine: line,
		Schema:  structName,
		Subject: fieldName,
	})
}

// Warn records a generic warning. Used for go/packages load errors (GO-06) and
// any rule that does not have a dedicated helper.
func (a *Accumulator) Warn(message, file string, line uint32) {
	a.WarnCode("source.unresolved", categorySource, message, file, line)
}

// WarnCode records a warning with an explicit stable code and category.
func (a *Accumulator) WarnCode(code, category, message, file string, line uint32) {
	a.items = append(a.items, facts.DiagnosticFact{
		Code:     code,
		Severity: severityWarn,
		Category: category,
		Message:  message,
		File:     file,
		Line:     line,
		EndLine:  line,
	})
}

// Items returns the accumulated diagnostics. The caller (facts.Marshal) sorts
// them into a stable order before emitting.
func (a *Accumulator) Items() []facts.DiagnosticFact {
	return a.items
}
