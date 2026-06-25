package goalservice

import "fmt"

// APIError is returned by operation methods on non-2xx responses. It exposes the
// HTTP status and the decoded error body (message/slug/hints).
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

// IsNotFound reports whether the error is a 404.
func (e *APIError) IsNotFound() bool {
	return e.StatusCode == 404
}
