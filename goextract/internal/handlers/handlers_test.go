package handlers_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

func fixtureDir(t *testing.T) string {
	t.Helper()
	abs, err := filepath.Abs(filepath.Join("..", "..", "..", "fixtures", "goalservice"))
	if err != nil {
		t.Fatalf("resolve fixture dir: %v", err)
	}
	return abs
}

// analyzeFixture loads the fixture, recognizes routes, builds an Analyzer (which
// indexes the handlers and carries the module prefix so refs use 02-01 schema
// ids), and returns per-handler code facts plus the accumulated diagnostics.
func analyzeFixture(t *testing.T) (map[string]handlers.CodeFacts, []facts.DiagnosticFact) {
	t.Helper()
	res, err := load.Load(fixtureDir(t))
	if err != nil {
		t.Fatalf("load fixture: %v", err)
	}
	var module string
	for _, pkg := range res.Packages {
		if pkg.Module != nil && pkg.Module.Main {
			module = pkg.Module.Path
		}
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, module, diags)
	out := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		out[r.Handler] = analyzer.Analyze(r, diags)
	}
	return out, diags.Items()
}

func TestCreateGoalRequestAndResponses(t *testing.T) {
	facts, _ := analyzeFixture(t)
	cf, ok := facts["createGoal"]
	if !ok {
		t.Fatal("createGoal not analyzed")
	}

	if cf.RequestBody == nil || !strings.HasSuffix(cf.RequestBody.RefID, "dto.CreateGoalInput") {
		t.Fatalf("createGoal request body: want dto.CreateGoalInput ref, got %+v", cf.RequestBody)
	}

	want := map[uint16]string{201: "dto.CommandMessageWithUUID", 400: "dto.HttpError"}
	if len(cf.Responses) != len(want) {
		t.Fatalf("createGoal: want %d responses, got %d: %+v", len(want), len(cf.Responses), cf.Responses)
	}
	for _, r := range cf.Responses {
		suffix, ok := want[r.Status]
		if !ok {
			t.Errorf("createGoal: unexpected status %d", r.Status)
			continue
		}
		if r.Body == nil || !strings.HasSuffix(r.Body.RefID, suffix) {
			t.Errorf("createGoal %d: want body %s, got %+v", r.Status, suffix, r.Body)
		}
	}
}

// TestStatusFromGoConstant proves status numbers come from go/constant resolution
// of http.StatusXxx, not a hardcoded name->number map: 201, 400, 200 all appear
// exactly as the net/http constants define them.
func TestStatusFromGoConstant(t *testing.T) {
	facts, _ := analyzeFixture(t)

	create := statuses(facts["createGoal"].Responses)
	if !equalUint16(create, []uint16{201, 400}) {
		t.Errorf("createGoal statuses: want [201 400], got %v", create)
	}
	list := statuses(facts["listGoals"].Responses)
	if !equalUint16(list, []uint16{200}) {
		t.Errorf("listGoals statuses: want [200], got %v", list)
	}
}

func TestPathParamForUUIDHandlers(t *testing.T) {
	facts, _ := analyzeFixture(t)
	for _, h := range []string{"updateGoal", "deleteGoal"} {
		cf := facts[h]
		p, ok := paramByName(cf.Params, "uuid")
		if !ok {
			t.Fatalf("%s: missing path param 'uuid': %+v", h, cf.Params)
		}
		// The neutral param type for a path string is a string primitive. Compare the
		// marshaled wire form (the local var `facts` shadows the package here).
		gotSchema, err := json.Marshal(p.Schema)
		if err != nil {
			t.Fatalf("%s uuid: marshal schema: %v", h, err)
		}
		const wantSchema = `{"type":"primitive","of":{"prim":"string"}}`
		if p.Location != "path" || !p.Required || string(gotSchema) != wantSchema {
			t.Errorf("%s uuid: want path/required/%s, got loc=%s req=%v schema=%s",
				h, wantSchema, p.Location, p.Required, gotSchema)
		}
	}
}

func TestListGoalsQueryParamsAndDiagnostics(t *testing.T) {
	facts, diags := analyzeFixture(t)
	cf := facts["listGoals"]

	wantQuery := []string{"aggregation", "cursor", "page_size"}
	var gotQuery []string
	for _, p := range cf.Params {
		if p.Location == "query" {
			gotQuery = append(gotQuery, p.Name)
		}
	}
	if !equalStrings(sortedCopy(gotQuery), wantQuery) {
		t.Fatalf("listGoals query params: want %v, got %v", wantQuery, gotQuery)
	}

	// Exactly 3 untyped-query diagnostics, one per query param, each with file:line.
	var untyped []facts2Diag
	for _, d := range diags {
		if strings.Contains(d.Message, "untyped query param") {
			untyped = append(untyped, facts2Diag{d.Message, d.File, d.Line})
		}
	}
	if len(untyped) != 3 {
		t.Fatalf("want exactly 3 untyped-query diagnostics, got %d: %+v", len(untyped), untyped)
	}
	for _, name := range wantQuery {
		found := false
		for _, d := range untyped {
			if strings.Contains(d.msg, "'"+name+"'") {
				found = true
				if d.file == "" || d.line == 0 {
					t.Errorf("untyped-query diag for %s missing file:line: %+v", name, d)
				}
			}
		}
		if !found {
			t.Errorf("no untyped-query diagnostic for query param %q", name)
		}
	}
}

func TestMultipartFormBodiesFromTypedBindAndDirectCalls(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/uploadhandlers

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBind(any) error { return nil }
func (c *Context) FormFile(string) (any, error) { return nil, nil }
func (c *Context) PostForm(string) string { return "" }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package uploadhandlers

import (
	"mime/multipart"

	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }

type UploadForm struct {
	File *multipart.FileHeader `+"`form:\"file\" binding:\"required\"`"+`
	Name string `+"`form:\"name\" validate:\"required\"`"+`
}

type UploadResult struct {
	ID string `+"`json:\"id\"`"+`
}

func (s Server) Register() {
	s.R.POST("/upload", s.uploadTyped)
	s.R.POST("/loose", s.uploadLoose)
}

func (s Server) uploadTyped(c *gin.Context) {
	var in UploadForm
	_ = c.ShouldBind(&in)
	c.JSON(201, UploadResult{})
}

func (s Server) uploadLoose(c *gin.Context) {
	_, _ = c.FormFile("file")
	_ = c.PostForm("name")
	c.JSON(200, UploadResult{})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load upload handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/uploadhandlers", diags)
	got := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		got[r.Handler] = analyzer.Analyze(r, diags)
	}

	typed := got["uploadTyped"]
	if typed.RequestBody == nil || !strings.HasSuffix(typed.RequestBody.RefID, "UploadForm") {
		t.Fatalf("typed upload body should reference UploadForm, got %+v", typed.RequestBody)
	}
	if typed.RequestBodyContentType != "multipart/form-data" {
		t.Fatalf("typed upload content type: want multipart/form-data got %q", typed.RequestBodyContentType)
	}

	loose := got["uploadLoose"]
	if loose.RequestBody == nil || loose.RequestBody.RefID != "__synthetic.UploadLooseFormRequest" {
		t.Fatalf("loose upload should synthesize request schema, got %+v", loose.RequestBody)
	}
	if loose.RequestBodyContentType != "multipart/form-data" {
		t.Fatalf("loose upload content type: want multipart/form-data got %q", loose.RequestBodyContentType)
	}
	if len(loose.Schemas) != 1 || loose.Schemas[0].Name != "UploadLooseFormRequest" {
		t.Fatalf("loose upload synthetic schemas: %+v", loose.Schemas)
	}
	fields, ok := loose.Schemas[0].Body.Of.([]facts.FieldFact)
	if !ok {
		t.Fatalf("synthetic schema body should be object fields, got %+v", loose.Schemas[0].Body)
	}
	seen := map[string]facts.FieldFact{}
	for _, field := range fields {
		seen[field.JSONName] = field
	}
	if primName(seen["file"].Schema) != facts.PrimBytes || !seen["file"].Required {
		t.Fatalf("synthetic file field should be required bytes, got %+v", seen["file"])
	}
	if primName(seen["name"].Schema) != facts.PrimString || seen["name"].Required {
		t.Fatalf("synthetic name field should be optional string, got %+v", seen["name"])
	}
}

func TestBranchingContentTypeHelperIsDynamic(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/downloadhandlers

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

import "io"

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) DataFromReader(int, int64, string, io.Reader, map[string]string) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package downloadhandlers

import (
	"strings"

	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }
type Other struct{}

var preferPDF bool

func (s Server) Register() {
	s.R.GET("/file", s.file)
	s.R.GET("/method-file", s.methodFile)
}

func (s Server) file(c *gin.Context) {
	c.DataFromReader(200, 12, dynamicContentType(), strings.NewReader("hello"), nil)
}

func (s Server) methodFile(c *gin.Context) {
	var other Other
	c.DataFromReader(200, 12, other.ContentType(), strings.NewReader("hello"), nil)
}

func ContentType() string {
	return "application/pdf"
}

func (Other) ContentType() string {
	return "text/plain"
}

func dynamicContentType() string {
	if preferPDF {
		return "application/pdf"
	}
	return "text/plain"
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load download handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/downloadhandlers", diags)
	got := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		got[r.Handler] = analyzer.Analyze(r, diags)
	}
	file := got["file"]
	if len(file.Responses) != 1 || file.Responses[0].ContentType != "application/octet-stream" {
		t.Fatalf("branching content type helper should fall back to octet-stream, got %+v", file.Responses)
	}
	methodFile := got["methodFile"]
	if len(methodFile.Responses) != 1 || methodFile.Responses[0].ContentType != "application/octet-stream" {
		t.Fatalf("method content type helper should not fold through a same-named top-level helper, got %+v", methodFile.Responses)
	}
	found := false
	for _, item := range diags.Items() {
		if strings.Contains(item.Message, "DataFromReader content type is dynamic") {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("branching content type helper should emit dynamic-content diagnostic, got %+v", diags.Items())
	}
}

func TestDelegatedBinaryResponseHelpersAreAnalyzed(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/delegatedbinary

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

import "io"

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Header(string, string) {}
func (c *Context) Data(int, string, []byte) {}
func (c *Context) DataFromReader(int, int64, string, io.Reader, map[string]string) {}
func (c *Context) File(string) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package delegatedbinary

import (
	"io"
	"strings"

	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }
type Other struct{}
type attachment struct {
	Reader io.Reader
	SizeBytes int64
	ContentType string
}

func (s Server) Register() {
	s.R.GET("/download", s.download)
	s.R.GET("/content", s.content)
	s.R.GET("/file", s.file)
}

func (s Server) download(c *gin.Context) {
	s.serveAttachment(c, false)
}

func (s Server) serveAttachment(c *gin.Context, inline bool) {
	attachment := loadAttachment(c)
	c.Header("Content-Disposition", "attachment")
	c.DataFromReader(200, attachment.SizeBytes, attachment.ContentType, attachment.Reader, nil)
}

func (Other) serveAttachment(c *gin.Context, inline bool) {}

func loadAttachment(c *gin.Context) attachment {
	return attachment{Reader: strings.NewReader("hello"), SizeBytes: 5}
}

func (s Server) content(c *gin.Context) {
	writeAttachmentContent(c, dynamicContentType(c), "file.txt", []byte("hello"))
}

func (s Server) file(c *gin.Context) {
	c.Header("Content-Type", "application/pdf")
	s.serveFile(c)
}

func (s Server) serveFile(c *gin.Context) {
	c.File("/tmp/report.pdf")
}

func writeAttachmentContent(c *gin.Context, contentType, filename string, body []byte) {
	if contentType == "" {
		contentType = "application/octet-stream"
	}
	c.Header("Content-Disposition", "attachment; filename="+filename)
	c.Data(200, contentType, body)
}

func dynamicContentType(c *gin.Context) string {
	return ""
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load delegated binary handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/delegatedbinary", diags)
	got := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		got[r.Handler] = analyzer.Analyze(r, diags)
	}

	assertBinaryOctetStreamResponse(t, got["download"].Responses)
	assertBinaryOctetStreamResponse(t, got["content"].Responses)
	assertBinaryResponse(t, got["file"].Responses, "application/pdf")
}

func TestPathParamHelperReturnTypeSetsSchema(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/pathhelpers

go 1.22

require (
	github.com/gin-gonic/gin v0.0.0
	github.com/google/uuid v0.0.0
)

replace github.com/gin-gonic/gin => ./ginstub
replace github.com/google/uuid => ./uuidstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	if err := os.Mkdir(filepath.Join(dir, "uuidstub"), 0o755); err != nil {
		t.Fatalf("mkdir uuidstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Param(string) string { return "" }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "uuidstub", "go.mod"), "module github.com/google/uuid\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "uuidstub", "uuid.go"), `package uuid

type UUID [16]byte

func Parse(string) (UUID, error) { return UUID{}, nil }
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package pathhelpers

import (
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

type Server struct{ R *gin.Engine }
type File struct {
	ID string `+"`json:\"id\"`"+`
}

func (s Server) Register() {
	s.R.GET("/files/:fileId", s.getFile)
}

func (s Server) getFile(c *gin.Context) {
	fileID, err := parsePathUUID(c, "fileId")
	_, _ = fileID, err
	c.JSON(200, File{})
}

func parsePathUUID(c *gin.Context, key string) (uuid.UUID, error) {
	return uuid.Parse(c.Param(key))
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load path helper handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/pathhelpers", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		if r.Handler == "getFile" {
			cf = analyzer.Analyze(r, diags)
		}
	}

	param, ok := paramByName(cf.Params, "fileId")
	if !ok {
		t.Fatalf("missing helper-derived fileId param: %+v", cf.Params)
	}
	if param.Location != "path" || !param.Required {
		t.Fatalf("fileId param should be required path param, got %+v", param)
	}
	if param.Schema.Type != facts.TypeWellKnown || param.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("fileId helper return type should infer uuid schema, got %+v", param.Schema)
	}
}

func TestDataApplicationJSONBytesAreRawBinaryResponses(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/rawjson

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Data(int, string, []byte) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package rawjson

import "github.com/gin-gonic/gin"

type Server struct{ R *gin.Engine }

func (s Server) Register() {
	s.R.GET("/export", s.export)
}

func (s Server) export(c *gin.Context) {
	payload := []byte(`+"`"+`{"status":"ready"}`+"`"+`)
	c.Data(200, "application/json", payload)
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load raw json handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/rawjson", diags)
	got := map[string]handlers.CodeFacts{}
	for _, r := range routes.Recognize(res) {
		got[r.Handler] = analyzer.Analyze(r, diags)
	}

	response := got["export"].Responses[0]
	if response.BodyKind != "binary" {
		t.Fatalf("c.Data application/json bytes must be marked binary, got %+v", response)
	}
	if response.ContentType != "application/json" {
		t.Fatalf("raw JSON response content type: got %+v", response)
	}
	if !equalStrings(response.ContentTypes, []string{"application/json"}) {
		t.Fatalf("raw JSON response content_types: got %+v", response)
	}
}

func TestRouteHandlerIdentityDisambiguatesSameNamedMethods(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/identityhandlers

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBindJSON(any) error { return nil }
func (c *Context) JSON(int, any) {}
`)
	for _, pkg := range []string{"a", "b"} {
		if err := os.MkdirAll(filepath.Join(dir, "internal", pkg, "ports"), 0o755); err != nil {
			t.Fatalf("mkdir package %s: %v", pkg, err)
		}
		typePrefix := strings.ToUpper(pkg)
		mustWrite(t, filepath.Join(dir, "internal", pkg, "ports", "handlers.go"), `package ports

import "github.com/gin-gonic/gin"

type Server struct {
	R *gin.Engine
	H Handler
}

type Handler struct{}
type `+typePrefix+`Request struct { Name string `+"`json:\"name\"`"+` }
type `+typePrefix+`Response struct { ID string `+"`json:\"id\"`"+` }

func (s Server) Register() {
	s.R.POST("/`+pkg+`", s.H.Handle)
}

func (Handler) Handle(c *gin.Context) {
	var input `+typePrefix+`Request
	_ = c.ShouldBindJSON(&input)
	c.JSON(200, `+typePrefix+`Response{})
}
`)
	}

	res, err := load.Load(dir, "./internal/.../ports")
	if err != nil {
		t.Fatalf("load identity handlers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/identityhandlers", diags)
	recognized := routes.Recognize(res)
	analyzer.ReportRouteHandlerCollisions(recognized, diags)
	got := map[string]handlers.CodeFacts{}
	for _, r := range recognized {
		if r.HandlerKey == "" {
			t.Fatalf("recognized route %s %s should carry handler identity: %+v", r.Method, r.Path, r)
		}
		got[r.Path] = analyzer.Analyze(r, diags)
	}
	for _, item := range diags.Items() {
		if strings.Contains(item.Message, "duplicate handler name 'Handle'") {
			t.Fatalf("resolved route identities should not warn about bare-name Handle collisions: %+v", diags.Items())
		}
	}
	assertBodySuffix(t, got["/a"].RequestBody, "internal/a/ports.ARequest")
	assertResponseSuffix(t, got["/a"].Responses, 200, "internal/a/ports.AResponse")
	assertBodySuffix(t, got["/b"].RequestBody, "internal/b/ports.BRequest")
	assertResponseSuffix(t, got["/b"].Responses, 200, "internal/b/ports.BResponse")
	if got["/a"].RequestBodyContentType != "application/json" || got["/b"].RequestBodyContentType != "application/json" {
		t.Fatalf("ShouldBindJSON routes should infer application/json content types: a=%q b=%q", got["/a"].RequestBodyContentType, got["/b"].RequestBodyContentType)
	}
}

func TestSamePackageQueryHelpersInferTypeAndRequiredness(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/queryhelpers

go 1.22

require (
	github.com/gin-gonic/gin v0.0.0
	github.com/google/uuid v0.0.0
)

replace github.com/gin-gonic/gin => ./ginstub
replace github.com/google/uuid => ./uuidstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	if err := os.Mkdir(filepath.Join(dir, "uuidstub"), 0o755); err != nil {
		t.Fatalf("mkdir uuidstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Query(string) string { return "" }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "uuidstub", "go.mod"), "module github.com/google/uuid\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "uuidstub", "uuid.go"), `package uuid

type UUID [16]byte

var Nil UUID

func Parse(string) (UUID, error) { return UUID{}, nil }
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package queryhelpers

import (
	"errors"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

type Server struct{ R *gin.Engine }
type Result struct { OK bool `+"`json:\"ok\"`"+` }

func (s Server) Register() {
	s.R.GET("/search", s.search)
}

func (s Server) search(c *gin.Context) {
	branchID, _ := optionalQueryUUID(c, "branchId", uuid.Nil)
	orgID, _ := requiredQueryUUID(c, "orgId")
	_, _ = branchID, orgID
	c.JSON(200, Result{})
}

func optionalQueryUUID(c *gin.Context, key string, fallback uuid.UUID) (uuid.UUID, error) {
	raw := strings.TrimSpace(c.Query(key))
	if raw == "" {
		return fallback, nil
	}
	return uuid.Parse(raw)
}

func requiredQueryUUID(c *gin.Context, key string) (uuid.UUID, error) {
	raw := strings.TrimSpace(c.Query(key))
	if raw == "" {
		return uuid.UUID{}, errors.New("missing")
	}
	return uuid.Parse(raw)
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load query helpers: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/queryhelpers", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	branch, ok := paramByName(cf.Params, "branchId")
	if !ok || branch.Required || branch.Schema.Type != facts.TypeWellKnown || branch.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("branchId should be optional uuid query param, got %+v", branch)
	}
	org, ok := paramByName(cf.Params, "orgId")
	if !ok || !org.Required || org.Schema.Type != facts.TypeWellKnown || org.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("orgId should be required uuid query param, got %+v", org)
	}
}

func TestGenericJSONBodyHelperInfersRequestBody(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/genericbody

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBindJSON(any) error { return nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package genericbody

import "github.com/gin-gonic/gin"

type Server struct{ R *gin.Engine }
type CreateRequest struct { Name string `+"`json:\"name\"`"+` }
type CreateResponse struct { ID string `+"`json:\"id\"`"+` }

func (s Server) Register() {
	s.R.POST("/create", s.create)
}

func (s Server) create(c *gin.Context) {
	input, err := parseRequest[CreateRequest](c)
	_, _ = input, err
	c.JSON(200, CreateResponse{})
}

func parseRequest[T any](c *gin.Context) (T, error) {
	var input T
	if err := c.ShouldBindJSON(&input); err != nil {
		return input, err
	}
	return input, nil
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load generic body: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/genericbody", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	assertBodySuffix(t, cf.RequestBody, "CreateRequest")
	if cf.RequestBodyContentType != "application/json" {
		t.Fatalf("generic JSON helper content type: want application/json got %q", cf.RequestBodyContentType)
	}
}

func TestGenericJSONBodyHelperRequiresTypeParamBind(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/genericbodyguard

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBindJSON(any) error { return nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package genericbodyguard

import "github.com/gin-gonic/gin"

type Server struct{ R *gin.Engine }
type Envelope struct { Token string `+"`json:\"token\"`"+` }
type CreateRequest struct { Name string `+"`json:\"name\"`"+` }
type CreateResponse struct { ID string `+"`json:\"id\"`"+` }

func (s Server) Register() {
	s.R.POST("/create", s.create)
}

func (s Server) create(c *gin.Context) {
	input, err := parseEnvelope[CreateRequest](c)
	_, _ = input, err
	c.JSON(200, CreateResponse{})
}

func parseEnvelope[T any](c *gin.Context) (T, error) {
	var zero T
	var envelope Envelope
	if err := c.ShouldBindJSON(&envelope); err != nil {
		return zero, err
	}
	return zero, nil
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load generic body guard: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/genericbodyguard", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	if cf.RequestBody != nil || cf.RequestBodyContentType != "" {
		t.Fatalf("generic helper binding a concrete envelope should not infer CreateRequest body, got body=%+v content_type=%q", cf.RequestBody, cf.RequestBodyContentType)
	}
}

func TestDelegatedSamePackageJSONBindHelperInfersRequestBody(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/delegatedbody

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) ShouldBindJSON(any) error { return nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package delegatedbody

import "github.com/gin-gonic/gin"

type HttpServer struct{ R *gin.Engine }
type CreateEventInput struct { Name string `+"`json:\"name\"`"+` }
type CreatedResponse struct { OK bool `+"`json:\"ok\"`"+` }
type ErrorResponse struct { Message string `+"`json:\"message\"`"+` }

func (h HttpServer) Register() {
	h.R.POST("/events", h.publishEvent)
}

func (h HttpServer) publishEvent(c *gin.Context) {
	input, err := h.bindEventInput(c)
	if err != nil {
		c.JSON(400, ErrorResponse{})
		return
	}
	_ = input
	c.JSON(201, CreatedResponse{})
}

func (h HttpServer) bindEventInput(c *gin.Context) (CreateEventInput, error) {
	var input CreateEventInput
	if err := c.ShouldBindJSON(&input); err != nil {
		return CreateEventInput{}, err
	}
	return input, nil
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load delegated body: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/delegatedbody", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	assertBodySuffix(t, cf.RequestBody, "CreateEventInput")
	if !cf.RequestBodyRequired || cf.RequestBodyContentType != "application/json" {
		t.Fatalf("delegated JSON bind should be required application/json, got required=%v content_type=%q", cf.RequestBodyRequired, cf.RequestBodyContentType)
	}
}

func TestHelperBasedQueryPatternsInferConcreteSchemasRequirednessAndDefaults(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/querypatterns

go 1.22

require (
	github.com/gin-gonic/gin v0.0.0
	github.com/google/uuid v0.0.0
)

replace github.com/gin-gonic/gin => ./ginstub
replace github.com/google/uuid => ./uuidstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	if err := os.Mkdir(filepath.Join(dir, "uuidstub"), 0o755); err != nil {
		t.Fatalf("mkdir uuidstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Query(string) string { return "" }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "uuidstub", "go.mod"), "module github.com/google/uuid\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "uuidstub", "uuid.go"), `package uuid

type UUID [16]byte

var Nil UUID

func Parse(string) (UUID, error) { return UUID{}, nil }
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package querypatterns

import (
	"errors"
	"strconv"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

type Server struct{ R *gin.Engine }
type Result struct { OK bool `+"`json:\"ok\"`"+` }

func (s Server) Register() {
	s.R.GET("/things", s.listThings)
}

func (s Server) listThings(c *gin.Context) {
	branchID, branchErr := optionalQueryUUID(c, "branchId")
	id, idErr := requiredQueryUUID(c, "id")
	pageSize, pageErr := optionalUintQuery(c, "pageSize", 20)
	startDate, timeErr := ParseTime(c.Query("startDate"))
	_, _, _, _, _, _, _, _ = branchID, branchErr, id, idErr, pageSize, pageErr, startDate, timeErr
	c.JSON(200, Result{})
}

func optionalQueryUUID(c *gin.Context, key string) (*uuid.UUID, error) {
	value := c.Query(key)
	if value == "" {
		return nil, nil
	}
	parsed, err := uuid.Parse(value)
	if err != nil {
		return nil, err
	}
	return &parsed, nil
}

func requiredQueryUUID(c *gin.Context, key string) (uuid.UUID, error) {
	value := c.Query(key)
	if value == "" {
		return uuid.Nil, errors.New("missing")
	}
	return uuid.Parse(value)
}

func optionalUintQuery(c *gin.Context, key string, defaultValue uint) (uint, error) {
	value := c.Query(key)
	if value == "" {
		return defaultValue, nil
	}
	parsed, err := strconv.ParseUint(value, 10, 32)
	if err != nil {
		return 0, err
	}
	return uint(parsed), nil
}

func ParseTime(value string) (*time.Time, error) {
	if value == "" {
		return nil, nil
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return nil, err
	}
	return &parsed, nil
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load query patterns: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/querypatterns", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	branch, ok := paramByName(cf.Params, "branchId")
	if !ok || branch.Required || branch.Schema.Type != facts.TypeWellKnown || branch.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("branchId should be optional uuid query param, got %+v", branch)
	}
	id, ok := paramByName(cf.Params, "id")
	if !ok || !id.Required || id.Schema.Type != facts.TypeWellKnown || id.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("id should be required uuid query param, got %+v", id)
	}
	pageSize, ok := paramByName(cf.Params, "pageSize")
	if !ok || pageSize.Required || primName(pageSize.Schema) != facts.PrimInt || pageSize.Default == nil || pageSize.Default.Type != "number" || pageSize.Default.Value != "20" {
		t.Fatalf("pageSize should be optional int query param with default 20, got %+v", pageSize)
	}
	startDate, ok := paramByName(cf.Params, "startDate")
	if !ok || startDate.Required || startDate.Schema.Type != facts.TypeWellKnown || startDate.Schema.Of != facts.WellKnownDateTime {
		t.Fatalf("startDate should be optional date_time query param, got %+v", startDate)
	}
	for _, item := range diags.Items() {
		if strings.Contains(item.Message, "untyped query param") {
			t.Fatalf("typed helper query params should not emit untyped diagnostics: %+v", item)
		}
	}
}

func TestGetRawDataWithJSONUsageSynthesizesFreeFormJSONBody(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/rawjsonbody

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) GetRawData() ([]byte, error) { return nil, nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package rawjsonbody

import (
	"encoding/json"

	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }
type Result struct { OK bool `+"`json:\"ok\"`"+` }

func (s Server) Register() {
	s.R.POST("/ingest", s.ingest)
}

func (s Server) ingest(c *gin.Context) {
	raw, _ := c.GetRawData()
	var payload map[string]any
	_ = json.Unmarshal(raw, &payload)
	c.JSON(200, Result{})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load raw json body: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/rawjsonbody", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	if cf.RequestBody == nil || cf.RequestBody.RefID != "__synthetic.IngestRawJSONRequest" {
		t.Fatalf("raw JSON body should synthesize free-form request schema, got %+v", cf.RequestBody)
	}
	if cf.RequestBodyContentType != "application/json" {
		t.Fatalf("raw JSON body content type: want application/json got %q", cf.RequestBodyContentType)
	}
	if len(cf.Schemas) != 1 || cf.Schemas[0].Body.Type != facts.TypeAny {
		t.Fatalf("raw JSON body schema should be Any, got %+v", cf.Schemas)
	}
}

func TestGetRawDataIgnoresUnrelatedJSONStringEvidence(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/rawjsonbodyguard

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) POST(string, HandlerFunc) {}
func (c *Context) GetRawData() ([]byte, error) { return nil, nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package rawjsonbodyguard

import (
	"strings"

	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }
type Result struct { OK bool `+"`json:\"ok\"`"+` }

func (s Server) Register() {
	s.R.POST("/ingest", s.ingest)
}

func (s Server) ingest(c *gin.Context) {
	raw, _ := c.GetRawData()
	_ = raw
	_ = strings.Contains("application/json", "json")
	c.JSON(200, Result{})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load raw json body guard: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/rawjsonbodyguard", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	if cf.RequestBody != nil || cf.RequestBodyContentType != "" || len(cf.Schemas) != 0 {
		t.Fatalf("unrelated JSON string evidence should not synthesize raw JSON body, got body=%+v content_type=%q schemas=%+v", cf.RequestBody, cf.RequestBodyContentType, cf.Schemas)
	}
}

func TestTypedSuccessResponseOutranksErrorishSameStatusBranch(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/successresponse

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package successresponse

import "github.com/gin-gonic/gin"

var fail bool

type Server struct{ R *gin.Engine }
type ErrorResponse struct { Message string `+"`json:\"message\"`"+` }
type SuccessResponse struct { ID string `+"`json:\"id\"`"+` }

func (s Server) Register() {
	s.R.GET("/thing", s.getThing)
}

func (s Server) getThing(c *gin.Context) {
	if fail {
		c.JSON(200, ErrorResponse{})
		return
	}
	c.JSON(200, SuccessResponse{})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load success response: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/successresponse", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	assertResponseSuffix(t, cf.Responses, 200, "SuccessResponse")
}

func TestSyntheticSuccessResponseOutranksErrorishSameStatusBranch(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/syntheticsuccessresponse

go 1.22

require github.com/gin-gonic/gin v0.0.0

replace github.com/gin-gonic/gin => ./ginstub
`)
	if err := os.Mkdir(filepath.Join(dir, "ginstub"), 0o755); err != nil {
		t.Fatalf("mkdir ginstub: %v", err)
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type H map[string]any
type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package syntheticsuccessresponse

import "github.com/gin-gonic/gin"

var fail bool

type Server struct{ R *gin.Engine }
type ErrorResponse struct { Message string `+"`json:\"message\"`"+` }

func (s Server) Register() {
	s.R.GET("/thing", s.getThing)
}

func (s Server) getThing(c *gin.Context) {
	if fail {
		c.JSON(200, ErrorResponse{})
		return
	}
	c.JSON(200, gin.H{"id": "ok"})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load synthetic success response: %v", err)
	}
	diags := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/syntheticsuccessresponse", diags)
	var cf handlers.CodeFacts
	for _, r := range routes.Recognize(res) {
		cf = analyzer.Analyze(r, diags)
	}
	assertResponseSuffix(t, cf.Responses, 200, "GetThing200Response")
}

func TestModuleOwnedMultiHopTypedParametersAndMultipartAreExtracted(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/typedrequests

go 1.22

require (
	github.com/gin-gonic/gin v0.0.0
	github.com/google/uuid v0.0.0
)

replace github.com/gin-gonic/gin => ./ginstub
replace github.com/google/uuid => ./uuidstub
`)
	for _, subdir := range []string{"ginstub", "uuidstub", "requesthelpers"} {
		if err := os.Mkdir(filepath.Join(dir, subdir), 0o755); err != nil {
			t.Fatalf("mkdir %s: %v", subdir, err)
		}
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

import (
	"mime/multipart"
	"net/http"
)

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct { Request *http.Request }

func (e *Engine) GET(string, HandlerFunc) {}
func (c *Context) Query(string) string { return "" }
func (c *Context) GetQuery(string) (string, bool) { return "", false }
func (c *Context) QueryArray(string) []string { return nil }
func (c *Context) GetQueryArray(string) ([]string, bool) { return nil, false }
func (c *Context) QueryMap(string) map[string]string { return nil }
func (c *Context) GetHeader(string) string { return "" }
func (c *Context) Cookie(string) (string, error) { return "", nil }
func (c *Context) GetPostForm(string) (string, bool) { return "", false }
func (c *Context) PostForm(string) string { return "" }
func (c *Context) FormFile(string) (*multipart.FileHeader, error) { return nil, nil }
func (c *Context) ShouldBindQuery(any) error { return nil }
func (c *Context) ShouldBindHeader(any) error { return nil }
func (c *Context) JSON(int, any) {}
`)
	mustWrite(t, filepath.Join(dir, "uuidstub", "go.mod"), "module github.com/google/uuid\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "uuidstub", "uuid.go"), `package uuid

type UUID [16]byte
func MustParse(string) UUID { return UUID{} }
`)
	mustWrite(t, filepath.Join(dir, "requesthelpers", "helpers.go"), `package requesthelpers

import (
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
)

func RequiredQueryUUID(c *gin.Context, name string) uuid.UUID {
	return uuid.MustParse(requiredQuery(c, name))
}

func requiredQuery(c *gin.Context, name string) string {
	return queryValue(c, name)
}

func queryValue(c *gin.Context, name string) string {
	return c.Query(name)
}

func MultipartParts(c *gin.Context, fileName, titleName string) {
	_, _ = c.FormFile(fileName)
	_ = c.PostForm(titleName)
}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package typedrequests

import (
	"time"

	"example.com/typedrequests/requesthelpers"
	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }

type QueryInput struct {
	Statuses []string  `+"`"+`form:"statuses" binding:"required"`+"`"+`
	Force    bool      `+"`"+`form:"force"`+"`"+`
	Limit    int       `+"`"+`form:"limit,default=25"`+"`"+`
	State    string    `+"`"+`form:"state" binding:"oneof=active paused"`+"`"+`
	Day      time.Time `+"`"+`form:"day" time_format:"2006-01-02"`+"`"+`
}

type HeaderInput struct {
	Signature string `+"`"+`header:"X-Webhook-Signature" binding:"required"`+"`"+`
	Retries   []int  `+"`"+`header:"X-Retry"`+"`"+`
}

type Response struct { OK bool `+"`"+`json:"ok"`+"`"+` }

func (s Server) Register() { s.R.GET("/search", s.search) }

func (s Server) search(c *gin.Context) {
	var query QueryInput
	var headers HeaderInput
	_ = c.ShouldBindQuery(&query)
	_ = c.ShouldBindHeader(&headers)
	_ = requesthelpers.RequiredQueryUUID(c, "branchId")
	_ = c.QueryArray("labels")
	_, _ = c.GetQueryArray("optionalLabels")
	_ = c.QueryMap("filters")
	_ = c.GetHeader("X-Direct")
	_ = c.Request.Header.Get("X-Request-Direct")
	_, _ = c.Cookie("session")
	_, _ = c.GetPostForm("note")
	requesthelpers.MultipartParts(c, "attachment", "title")
	c.JSON(200, Response{OK: true})
}
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load typed request fixture: %v", err)
	}
	diagnostics := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/typedrequests", diagnostics)
	var code handlers.CodeFacts
	for _, route := range routes.Recognize(res) {
		code = analyzer.Analyze(route, diagnostics)
	}

	branch, ok := paramByName(code.Params, "branchId")
	if !ok || !branch.Required || branch.Location != "query" || branch.Schema.Type != facts.TypeWellKnown || branch.Schema.Of != facts.WellKnownUUID {
		t.Fatalf("multi-hop cross-package UUID query must be exact, got %+v", branch)
	}
	if !strings.Contains(branch.Span.File, "requesthelpers") || branch.Span.StartLine == 0 {
		t.Fatalf("helper-derived parameter must preserve inner source span, got %+v", branch.Span)
	}
	statuses, ok := paramByName(code.Params, "statuses")
	if !ok || !statuses.Required || statuses.Schema.Type != facts.TypeArray || statuses.Style != "form" || statuses.Explode == nil || !*statuses.Explode {
		t.Fatalf("bound query array must preserve type/required/style/explode, got %+v", statuses)
	}
	force, _ := paramByName(code.Params, "force")
	if primName(force.Schema) != facts.PrimBool || force.Required {
		t.Fatalf("bound bool query mismatch: %+v", force)
	}
	limit, _ := paramByName(code.Params, "limit")
	if primName(limit.Schema) != facts.PrimInt || limit.Default == nil || limit.Default.Type != "number" || limit.Default.Value != "25" {
		t.Fatalf("bound integer/default mismatch: %+v", limit)
	}
	state, _ := paramByName(code.Params, "state")
	if state.Schema.Type != facts.TypeEnum {
		t.Fatalf("binding oneof must become an enum, got %+v", state.Schema)
	}
	day, _ := paramByName(code.Params, "day")
	if day.Schema.Type != facts.TypeWellKnown || day.Schema.Of != facts.WellKnownDate {
		t.Fatalf("time_format date must remain a date, got %+v", day.Schema)
	}
	signature, _ := paramByName(code.Params, "X-Webhook-Signature")
	if signature.Location != "header" || !signature.Required || primName(signature.Schema) != facts.PrimString {
		t.Fatalf("bound header mismatch: %+v", signature)
	}
	retries, _ := paramByName(code.Params, "X-Retry")
	if retries.Style != "simple" || retries.Explode == nil || *retries.Explode {
		t.Fatalf("header array serialization mismatch: %+v", retries)
	}
	filters, _ := paramByName(code.Params, "filters")
	if filters.Schema.Type != facts.TypeMap || filters.Style != "deepObject" || filters.Explode == nil || !*filters.Explode {
		t.Fatalf("QueryMap serialization mismatch: %+v", filters)
	}
	for _, headerName := range []string{"X-Direct", "X-Request-Direct"} {
		header, exists := paramByName(code.Params, headerName)
		if !exists || header.Location != "header" || !header.Required {
			t.Fatalf("missing direct header %s: %+v", headerName, code.Params)
		}
	}
	cookie, _ := paramByName(code.Params, "session")
	if cookie.Location != "cookie" || !cookie.Required {
		t.Fatalf("cookie mismatch: %+v", cookie)
	}

	if code.RequestBody == nil || code.RequestBodyContentType != "multipart/form-data" || len(code.Schemas) != 1 {
		t.Fatalf("helper-wrapped multipart body mismatch: body=%+v type=%q schemas=%+v", code.RequestBody, code.RequestBodyContentType, code.Schemas)
	}
	fields, ok := code.Schemas[0].Body.Of.([]facts.FieldFact)
	if !ok {
		t.Fatalf("multipart schema must be an object: %+v", code.Schemas[0].Body)
	}
	byName := map[string]facts.FieldFact{}
	for _, field := range fields {
		byName[field.JSONName] = field
	}
	if primName(byName["attachment"].Schema) != facts.PrimBytes || !byName["attachment"].Required || primName(byName["title"].Schema) != facts.PrimString || primName(byName["note"].Schema) != facts.PrimString {
		t.Fatalf("multipart fields mismatch: %+v", byName)
	}
	for _, item := range diagnostics.Items() {
		if item.Code == "request.parameter.unresolved" && (item.Subject == "branchId" || strings.Contains(item.Message, "RequiredQueryUUID")) {
			t.Fatalf("resolved multi-hop parameter must not emit incompleteness diagnostic: %+v", item)
		}
	}
}

func TestContextHelperCyclesAndExternalBoundariesAreDiagnosed(t *testing.T) {
	dir := t.TempDir()
	mustWrite(t, filepath.Join(dir, "go.mod"), `module example.com/helperdiagnostics

go 1.22

require (
	github.com/gin-gonic/gin v0.0.0
	example.net/external v0.0.0
)

replace github.com/gin-gonic/gin => ./ginstub
replace example.net/external => ./external
`)
	for _, subdir := range []string{"ginstub", "external"} {
		if err := os.Mkdir(filepath.Join(dir, subdir), 0o755); err != nil {
			t.Fatalf("mkdir %s: %v", subdir, err)
		}
	}
	mustWrite(t, filepath.Join(dir, "ginstub", "go.mod"), "module github.com/gin-gonic/gin\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "ginstub", "gin.go"), `package gin

type HandlerFunc func(*Context)
type Engine struct{}
type Context struct{}
func (e *Engine) GET(string, HandlerFunc) {}
`)
	mustWrite(t, filepath.Join(dir, "external", "go.mod"), "module example.net/external\n\ngo 1.22\n")
	mustWrite(t, filepath.Join(dir, "external", "external.go"), `package external

import "github.com/gin-gonic/gin"
func Read(*gin.Context) {}
`)
	mustWrite(t, filepath.Join(dir, "app.go"), `package helperdiagnostics

import (
	"example.net/external"
	"github.com/gin-gonic/gin"
)

type Server struct{ R *gin.Engine }
func (s Server) Register() { s.R.GET("/broken", s.broken) }
func (s Server) broken(c *gin.Context) { cycleA(c); external.Read(c) }
func cycleA(c *gin.Context) { cycleB(c) }
func cycleB(c *gin.Context) { cycleA(c) }
`)

	res, err := load.Load(dir)
	if err != nil {
		t.Fatalf("load helper diagnostic fixture: %v", err)
	}
	diagnostics := diag.New()
	analyzer := handlers.NewAnalyzer(res, "example.com/helperdiagnostics", diagnostics)
	for _, route := range routes.Recognize(res) {
		_ = analyzer.Analyze(route, diagnostics)
	}
	cycle, external := false, false
	for _, item := range diagnostics.Items() {
		if item.Code != "request.parameter.unresolved" || item.Operation != "GET /broken" {
			continue
		}
		cycle = cycle || strings.Contains(item.Message, "cycle detected")
		external = external || strings.Contains(item.Message, "external package example.net/external")
	}
	if !cycle || !external {
		t.Fatalf("cycle and external context boundaries must both be diagnosed: %+v", diagnostics.Items())
	}
}

// --- helpers -------------------------------------------------------------

type facts2Diag struct {
	msg, file string
	line      uint32
}

func mustWrite(t *testing.T, path string, contents string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}

func assertBinaryOctetStreamResponse(t *testing.T, responses []facts.ResponseFact) {
	t.Helper()
	assertBinaryResponse(t, responses, "application/octet-stream")
}

func assertBinaryResponse(t *testing.T, responses []facts.ResponseFact, contentType string) {
	t.Helper()
	if len(responses) != 1 {
		t.Fatalf("want exactly one response, got %+v", responses)
	}
	response := responses[0]
	if response.Status != 200 || response.BodyKind != "binary" || response.Body != nil {
		t.Fatalf("want 200 binary response with no body schema, got %+v", response)
	}
	if response.ContentType != contentType {
		t.Fatalf("want %s content type, got %+v", contentType, response)
	}
	if !equalStrings(response.ContentTypes, []string{contentType}) {
		t.Fatalf("want %s content_types, got %+v", contentType, response)
	}
}

func statuses(rs []facts.ResponseFact) []uint16 {
	out := make([]uint16, 0, len(rs))
	for _, r := range rs {
		out = append(out, r.Status)
	}
	// responses are appended in source order; sort for stable comparison.
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j] < out[j-1]; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}

func paramByName(ps []facts.ParamFact, name string) (facts.ParamFact, bool) {
	for _, p := range ps {
		if p.Name == name {
			return p, true
		}
	}
	return facts.ParamFact{}, false
}

func primName(ty facts.Type) string {
	if ty.Type != facts.TypePrimitive {
		return ""
	}
	if p, ok := ty.Of.(*facts.Prim); ok {
		return p.Prim
	}
	return ""
}

func assertBodySuffix(t *testing.T, body *facts.TypeRef, suffix string) {
	t.Helper()
	if body == nil || !strings.HasSuffix(body.RefID, suffix) {
		t.Fatalf("want body ref suffix %q, got %+v", suffix, body)
	}
}

func assertResponseSuffix(t *testing.T, responses []facts.ResponseFact, status uint16, suffix string) {
	t.Helper()
	for _, response := range responses {
		if response.Status != status {
			continue
		}
		if response.Body == nil || !strings.HasSuffix(response.Body.RefID, suffix) {
			t.Fatalf("response %d: want body suffix %q, got %+v", status, suffix, response.Body)
		}
		return
	}
	t.Fatalf("missing response %d in %+v", status, responses)
}

func equalUint16(a, b []uint16) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func sortedCopy(in []string) []string {
	out := append([]string(nil), in...)
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j] < out[j-1]; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}
