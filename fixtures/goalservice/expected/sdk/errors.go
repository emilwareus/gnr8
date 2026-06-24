// EXPECTED SDK SHAPE — acceptance target, not compiled; gnr8 must generate
// equivalents in Phase 3 (D-05).
//
// The typed API error (CONTEXT D-05): a single APIError type implementing the
// error interface, carrying the HTTP status and the decoded server message so
// callers can branch on status (e.g. 404 vs 400) without string matching. It is
// illustrative — it is NOT part of the Go build and need not compile.
package goalservice

import "fmt"

// APIError is returned by operation methods on non-2xx responses. It exposes the
// HTTP status and the decoded HttpError body (message/slug/hints).
type APIError struct {
	StatusCode int
	Message    string
	Slug       string
	Hints      []string
}

// Error implements the error interface.
func (e *APIError) Error() string {
	return fmt.Sprintf("goalservice: %d %s (%s)", e.StatusCode, e.Message, e.Slug)
}

// IsNotFound reports whether the error is a 404 — illustrative helper for the
// typed-error ergonomics callers get instead of inspecting raw responses.
func (e *APIError) IsNotFound() bool {
	return e.StatusCode == 404
}
