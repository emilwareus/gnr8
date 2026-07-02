package routes_test

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

// fixtureDir resolves the real goalservice fixture (analyzer input) from this
// test file's location (../../../fixtures/goalservice relative to internal/routes).
func fixtureDir(t *testing.T) string {
	t.Helper()
	abs, err := filepath.Abs(filepath.Join("..", "..", "..", "fixtures", "goalservice"))
	if err != nil {
		t.Fatalf("resolve fixture dir: %v", err)
	}
	return abs
}

func recognizeFixture(t *testing.T) []routes.Route {
	t.Helper()
	res, err := load.Load(fixtureDir(t))
	if err != nil {
		t.Fatalf("load fixture: %v", err)
	}
	return routes.Recognize(res)
}

// routeKey identifies a route for table lookups in tests.
type routeKey struct{ method, path string }

func index(rs []routes.Route) map[routeKey]routes.Route {
	m := make(map[routeKey]routes.Route, len(rs))
	for _, r := range rs {
		m[routeKey{r.Method, r.Path}] = r
	}
	return m
}

func TestRecognizesFourFixtureRoutes(t *testing.T) {
	rs := recognizeFixture(t)
	if len(rs) != 4 {
		t.Fatalf("expected exactly 4 routes, got %d: %+v", len(rs), rs)
	}

	want := []routes.Route{
		{Method: "POST", Path: "/", Handler: "createGoal", Secured: true},
		{Method: "GET", Path: "/list", Handler: "listGoals", Secured: true},
		{Method: "PUT", Path: "/{uuid}", Handler: "updateGoal", Secured: true},
		{Method: "DELETE", Path: "/{uuid}", Handler: "deleteGoal", Secured: true},
	}
	got := index(rs)
	for _, w := range want {
		r, ok := got[routeKey{w.Method, w.Path}]
		if !ok {
			t.Fatalf("missing route %s %s; got %+v", w.Method, w.Path, rs)
		}
		if r.Handler != w.Handler {
			t.Errorf("%s %s: handler want %q got %q", w.Method, w.Path, w.Handler, r.Handler)
		}
		if r.Secured != w.Secured {
			t.Errorf("%s %s: secured want %v got %v (Use(...) must propagate)", w.Method, w.Path, w.Secured, r.Secured)
		}
		if r.Span.File == "" || r.Span.StartLine == 0 {
			t.Errorf("%s %s: missing source span: %+v", w.Method, w.Path, r.Span)
		}
	}
}

// TestPathNormalization proves Gin `:param` segments become OpenAPI `{param}`.
func TestPathNormalization(t *testing.T) {
	rs := recognizeFixture(t)
	for _, r := range rs {
		if r.Method == "PUT" || r.Method == "DELETE" {
			if r.Path != "/{uuid}" {
				t.Errorf("%s: want normalized /{uuid}, got %q (:uuid must become {uuid})", r.Method, r.Path)
			}
		}
	}
}

// TestAliasedGinImportStillResolves loads a separate module that imports gin under
// an alias (`grouter`) and asserts recognition still finds the routes — proving the
// gate keys on the resolved receiver package path, not the import identifier text
// (threat T-02-06).
func TestAliasedGinImportStillResolves(t *testing.T) {
	dir, err := filepath.Abs(filepath.Join("testdata", "aliasedgin"))
	if err != nil {
		t.Fatalf("resolve aliasedgin dir: %v", err)
	}
	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load aliasedgin: %v", err)
	}
	rs := routes.Recognize(res)
	if len(rs) != 2 {
		t.Fatalf("expected 2 routes from aliased-gin app, got %d: %+v", len(rs), rs)
	}
	got := index(rs)

	post, ok := got[routeKey{"POST", "/"}]
	if !ok {
		t.Fatalf("aliased POST / not recognized; got %+v", rs)
	}
	if post.Handler != "create" || !post.Secured {
		t.Errorf("aliased POST /: want handler=create secured=true, got handler=%q secured=%v", post.Handler, post.Secured)
	}

	read, ok := got[routeKey{"GET", "/{id}"}]
	if !ok {
		t.Fatalf("aliased GET /{id} not recognized (param normalization under alias); got %+v", rs)
	}
	if read.Handler != "read" || !read.Secured {
		t.Errorf("aliased GET /{id}: want handler=read secured=true, got handler=%q secured=%v", read.Handler, read.Secured)
	}
}

func TestStaticGinGroupPrefixesAreComposedForModularServices(t *testing.T) {
	dir, err := filepath.Abs(filepath.Join("testdata", "modulargin"))
	if err != nil {
		t.Fatalf("resolve modulargin dir: %v", err)
	}
	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load modulargin: %v", err)
	}
	rs := routes.Recognize(res)
	got := index(rs)

	want := []routes.Route{
		{Method: "GET", Path: "/api/health", Handler: "health", Group: "api", Secured: true},
		{Method: "GET", Path: "/api/books", Handler: "listBooks", Group: "books", Secured: true},
		{Method: "GET", Path: "/api/books/{id}", Handler: "getBook", Group: "books", Secured: true},
		{Method: "GET", Path: "/api/admin/stats", Handler: "adminStats", Group: "admin", Secured: false},
		{Method: "GET", Path: "/api/ready", Handler: "ready", Group: "api", Secured: false},
	}
	if len(rs) != len(want) {
		t.Fatalf("expected %d modular routes, got %d: %+v", len(want), len(rs), rs)
	}
	for _, w := range want {
		r, ok := got[routeKey{w.Method, w.Path}]
		if !ok {
			t.Fatalf("missing modular route %s %s; got %+v", w.Method, w.Path, rs)
		}
		if r.Handler != w.Handler {
			t.Errorf("%s %s: handler want %q got %q", w.Method, w.Path, w.Handler, r.Handler)
		}
		if r.Group != w.Group {
			t.Errorf("%s %s: group want %q got %q", w.Method, w.Path, w.Group, r.Group)
		}
		if r.Secured != w.Secured {
			t.Errorf("%s %s: secured want %v got %v", w.Method, w.Path, w.Secured, r.Secured)
		}
	}
	assertRouteMiddleware(t, got[routeKey{"GET", "/api/books"}], []string{"Guard"})
	assertRouteMiddleware(t, got[routeKey{"GET", "/api/books/{id}"}], []string{"Guard"})
}

func TestMiddlewareSymbolsAreCapturedFromGroupsUseAndRoutes(t *testing.T) {
	dir := t.TempDir()
	mustWriteRouteTestFile(t, filepath.Join(dir, "go.mod"), `module example.com/middlewaregin

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWriteRouteTestFile(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWriteRouteTestFile(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Context struct{}
type Engine struct{}
type RouterGroup struct{}

func (e *Engine) Group(string, ...HandlerFunc) *RouterGroup { return nil }
func (e *Engine) GET(string, ...HandlerFunc) {}
func (g *RouterGroup) Use(...HandlerFunc) {}
func (g *RouterGroup) Group(string, ...HandlerFunc) *RouterGroup { return nil }
func (g *RouterGroup) GET(string, ...HandlerFunc) {}
func (g *RouterGroup) POST(string, ...HandlerFunc) {}
`)
	mustWriteRouteTestFile(t, filepath.Join(dir, "app.go"), `package middlewaregin

import "github.com/gin-gonic/gin"

type Auth struct{}
type Handler struct{}
type Server struct {
	R *gin.Engine
	H Handler
	A Auth
}

func (Auth) RequireActiveSchool() gin.HandlerFunc { return nil }
func (Auth) RequireActor() gin.HandlerFunc { return nil }
func (Auth) RequireCSRF() gin.HandlerFunc { return nil }

func (s Server) Register() {
	active := s.R.Group("/v1/schools/active", s.A.RequireActiveSchool())
	active.Use(s.A.RequireActor())
	active.GET("/files/:fileId/open", s.H.open)
	active.POST("/files", s.A.RequireCSRF(), s.H.create)
	s.R.GET("/v1/admin/export/:exportId", s.A.RequireActor(), s.H.export)
	s.R.Group("/inline", s.A.RequireActiveSchool()).Group("/files", s.A.RequireActor()).GET("/:fileId/open", s.H.open)
}

func (Handler) open(*gin.Context) {}
func (Handler) create(*gin.Context) {}
func (Handler) export(*gin.Context) {}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load middlewaregin: %v", err)
	}
	rs := routes.Recognize(res)
	got := index(rs)

	assertRouteMiddleware(t, got[routeKey{"GET", "/v1/schools/active/files/{fileId}/open"}], []string{"RequireActiveSchool", "RequireActor"})
	assertRouteMiddleware(t, got[routeKey{"POST", "/v1/schools/active/files"}], []string{"RequireActiveSchool", "RequireActor", "RequireCSRF"})
	assertRouteMiddleware(t, got[routeKey{"GET", "/v1/admin/export/{exportId}"}], []string{"RequireActor"})
	assertRouteMiddleware(t, got[routeKey{"GET", "/inline/files/{fileId}/open"}], []string{"RequireActiveSchool", "RequireActor"})
}

func mustWriteRouteTestFile(t *testing.T, path string, body string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}

func assertRouteMiddleware(t *testing.T, route routes.Route, want []string) {
	t.Helper()
	if !reflect.DeepEqual(route.Middleware, want) {
		t.Fatalf("%s %s middleware: want %v got %v", route.Method, route.Path, want, route.Middleware)
	}
	if !route.Secured {
		t.Fatalf("%s %s should be marked secured when middleware is present", route.Method, route.Path)
	}
}

func TestDynamicGinGroupPrefixProducesDiagnostic(t *testing.T) {
	dir, err := filepath.Abs(filepath.Join("testdata", "dynamicgin"))
	if err != nil {
		t.Fatalf("resolve dynamicgin dir: %v", err)
	}
	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load dynamicgin: %v", err)
	}
	diags := diag.New()
	rs := routes.RecognizeWithDiagnostics(res, diags)
	if len(rs) != 1 {
		t.Fatalf("expected route to remain discoverable without guessed prefix, got %d: %+v", len(rs), rs)
	}
	if rs[0].Path != "/ping" {
		t.Fatalf("dynamic prefix must not be guessed into the route path, got %q", rs[0].Path)
	}
	items := diags.Items()
	if len(items) != 1 {
		t.Fatalf("expected one dynamic-prefix diagnostic, got %+v", items)
	}
	if got := items[0].Message; got != "unsupported Gin route pattern: dynamic Gin group prefix; prefix skipped rather than guessed (GO-04)" {
		t.Fatalf("unexpected diagnostic: %q", got)
	}
}

func TestRouterGroupParameterProducesDiagnostic(t *testing.T) {
	dir, err := filepath.Abs(filepath.Join("testdata", "paramgin"))
	if err != nil {
		t.Fatalf("resolve paramgin dir: %v", err)
	}
	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load paramgin: %v", err)
	}
	diags := diag.New()
	rs := routes.RecognizeWithDiagnostics(res, diags)
	if len(rs) != 1 {
		t.Fatalf("expected one helper route, got %d: %+v", len(rs), rs)
	}
	if rs[0].Path != "/{id}" {
		t.Fatalf("router-group parameter prefix must not be guessed, got %q", rs[0].Path)
	}
	items := diags.Items()
	if len(items) != 1 {
		t.Fatalf("expected one router-group-parameter diagnostic, got %+v", items)
	}
	if got := items[0].Message; got != "unsupported Gin route pattern: route registered on router group parameter; prefix cannot be inferred across helper calls, so the route is emitted relative (GO-04)" {
		t.Fatalf("unexpected diagnostic: %q", got)
	}
}
