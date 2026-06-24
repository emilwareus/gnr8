// EXPECTED SDK SHAPE — acceptance target, not compiled; gnr8 must generate
// equivalents in Phase 3 (D-05).
//
// This sketches the functional-options Client (CONTEXT D-05): a single SDK
// package exposing a Client constructed via NewClient(baseURL, opts...), with a
// base URL, a customizable *http.Client, and shared request plumbing. It is
// illustrative — it is NOT part of the Go build and need not compile.
package goalservice

import (
	"net/http"
	"time"
)

// Client is the goalservice SDK entrypoint. Tag-grouped operation methods (see
// goals.go) hang off this type. Constructed with functional options.
type Client struct {
	baseURL    string
	httpClient *http.Client
	apiKey     string
}

// Option mutates a Client during construction (functional-options pattern).
type Option func(*Client)

// WithHTTPClient overrides the default *http.Client (timeouts, transport, etc.).
func WithHTTPClient(hc *http.Client) Option {
	return func(c *Client) { c.httpClient = hc }
}

// WithAPIKey sets the API key sent to satisfy the ApiKeyAuth security scheme
// (the route group's auth middleware lowers to this requirement).
func WithAPIKey(key string) Option {
	return func(c *Client) { c.apiKey = key }
}

// NewClient builds a Client for the given base URL, applying any options. A
// sensible default *http.Client is used unless WithHTTPClient overrides it.
func NewClient(baseURL string, opts ...Option) *Client {
	c := &Client{
		baseURL:    baseURL,
		httpClient: &http.Client{Timeout: 30 * time.Second},
	}
	for _, opt := range opts {
		opt(c)
	}
	return c
}
