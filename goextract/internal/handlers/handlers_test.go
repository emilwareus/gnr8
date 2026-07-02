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
