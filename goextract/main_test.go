package main

import (
	"encoding/json"
	"go/token"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
	"github.com/gnr8/goextract/internal/handlers"
	"github.com/gnr8/goextract/internal/load"
	"github.com/gnr8/goextract/internal/routes"
)

func TestGinContractRegressionFacts(t *testing.T) {
	dir, err := filepath.Abs("../fixtures/gin-contract-regression")
	if err != nil {
		t.Fatalf("fixture path: %v", err)
	}
	tmp, err := os.CreateTemp("", "gnr8-gin-contract-*.json")
	if err != nil {
		t.Fatalf("temp facts file: %v", err)
	}
	defer os.Remove(tmp.Name())
	defer tmp.Close()

	if err := run(dir, packageScopes{}, tmp); err != nil {
		t.Fatalf("run goextract: %v", err)
	}
	if _, err := tmp.Seek(0, 0); err != nil {
		t.Fatalf("rewind facts file: %v", err)
	}
	var doc facts.GoFacts
	if err := json.NewDecoder(tmp).Decode(&doc); err != nil {
		t.Fatalf("decode facts: %v", err)
	}
	if len(doc.Diagnostics) != 0 {
		t.Fatalf("gin contract fixture should not emit diagnostics, got %+v", doc.Diagnostics)
	}

	login := routeByHandler(t, doc, "login")
	assertResponseBodyRef(t, login, 200, "LoginResponse")

	getChild := routeByHandler(t, doc, "getChild")
	assertPathParam(t, getChild, "itemId")
	assertPathParam(t, getChild, "childId")

	update := routeByHandler(t, doc, "updateItem")
	if update.Method != "PATCH" {
		t.Fatalf("updateItem method: want PATCH got %s", update.Method)
	}

	assertBodylessStatus(t, routeByHandler(t, doc, "logout"), 204)
	assertBodylessStatus(t, routeByHandler(t, doc, "deleteItem"), 204)

	list := routeByHandler(t, doc, "listSavedViews")
	if list.Responses[0].Body == nil || list.Responses[0].Body.RefID != "__synthetic.ListSavedViews200Response" {
		t.Fatalf("listSavedViews response should use synthetic array schema, got %+v", list.Responses)
	}
	if schemaByID(t, doc, "__synthetic.ListSavedViews200Response").Body.Type != facts.TypeArray {
		t.Fatalf("listSavedViews synthetic schema should be array")
	}

	job := routeByHandler(t, doc, "createJob")
	if job.Responses[0].Body == nil || job.Responses[0].Body.RefID == "github.com/gin-gonic/gin.H" {
		t.Fatalf("createJob must not reference gin.H, got %+v", job.Responses)
	}
	if schemaByID(t, doc, "__synthetic.CreateJob202Response").Body.Type != facts.TypeObject {
		t.Fatalf("createJob synthetic schema should be object")
	}

	download := routeByHandler(t, doc, "downloadFile")
	if download.Responses[0].BodyKind != "binary" || download.Responses[0].ContentType != "application/octet-stream" {
		t.Fatalf("downloadFile should be binary octet-stream, got %+v", download.Responses)
	}
	stream := routeByHandler(t, doc, "streamFile")
	if stream.Responses[0].BodyKind != "binary" || stream.Responses[0].ContentType != "application/pdf" {
		t.Fatalf("streamFile should be binary application/pdf, got %+v", stream.Responses)
	}
	reader := routeByHandler(t, doc, "readFile")
	if reader.Responses[0].BodyKind != "binary" || reader.Responses[0].ContentType != "application/pdf" {
		t.Fatalf("readFile should be binary application/pdf, got %+v", reader.Responses)
	}
	events := routeByHandler(t, doc, "itemEvents")
	if events.Responses[0].BodyKind != "sse" || events.Responses[0].ContentType != "text/event-stream" {
		t.Fatalf("itemEvents should be SSE text/event-stream, got %+v", events.Responses)
	}
	rawStream := routeByHandler(t, doc, "rawStream")
	if len(rawStream.Responses) != 0 {
		t.Fatalf("rawStream should not be classified as SSE, got %+v", rawStream.Responses)
	}

	search := routeByHandler(t, doc, "searchItems")
	assertQueryParam(t, search, "q", true, `{"type":"primitive","of":{"prim":"string"}}`, "")
	assertQueryParam(t, search, "limit", false, `{"type":"primitive","of":{"prim":"int","bits":64,"signed":true}}`, "")
	assertQueryParam(t, search, "trimmedLimit", false, `{"type":"primitive","of":{"prim":"int","bits":64,"signed":true}}`, "")
	assertQueryParam(t, search, "wrappedLimit", false, `{"type":"primitive","of":{"prim":"int","bits":64,"signed":true}}`, "")
	assertQueryParamDefault(t, search, "sort", false, `{"type":"primitive","of":{"prim":"string"}}`, "string", "asc")
	assertQueryParamDefault(t, search, "cursor", false, `{"type":"primitive","of":{"prim":"string"}}`, "string", "first")
	assertQueryParam(t, search, "token", false, `{"type":"primitive","of":{"prim":"string"}}`, "")

	attendance := routeByHandler(t, doc, "attendance")
	assertQueryParam(t, attendance, "startDate", true, `{"type":"well_known","of":"date_time"}`, "")
	assertQueryParam(t, attendance, "days", false, `{"type":"primitive","of":{"prim":"int","bits":64,"signed":true}}`, "5")

	markRead := routeByHandler(t, doc, "markRead")
	if markRead.RequestBody == nil || markRead.RequestBodyRequired {
		t.Fatalf("markRead should have an optional request body, got body=%+v required=%v", markRead.RequestBody, markRead.RequestBodyRequired)
	}
	headerRead := routeByHandler(t, doc, "headerRead")
	if headerRead.RequestBody == nil || headerRead.RequestBodyRequired {
		t.Fatalf("headerRead should have an optional request body from direct Content-Length header guard, got body=%+v required=%v", headerRead.RequestBody, headerRead.RequestBodyRequired)
	}
	combinedHeaderRead := routeByHandler(t, doc, "combinedHeaderRead")
	if combinedHeaderRead.RequestBody == nil || !combinedHeaderRead.RequestBodyRequired {
		t.Fatalf("combinedHeaderRead should keep its request body required because another header can trigger binding, got body=%+v required=%v", combinedHeaderRead.RequestBody, combinedHeaderRead.RequestBodyRequired)
	}
	forceRead := routeByHandler(t, doc, "forceRead")
	if forceRead.RequestBody == nil || !forceRead.RequestBodyRequired {
		t.Fatalf("forceRead should keep its request body required because the OR guard can bind without a body, got body=%+v required=%v", forceRead.RequestBody, forceRead.RequestBodyRequired)
	}
	mixedRead := routeByHandler(t, doc, "mixedRead")
	if mixedRead.RequestBody == nil || !mixedRead.RequestBodyRequired {
		t.Fatalf("mixedRead should keep its request body required because not all binds are guarded, got body=%+v required=%v", mixedRead.RequestBody, mixedRead.RequestBodyRequired)
	}
	unrelatedLengthRead := routeByHandler(t, doc, "unrelatedLengthRead")
	if unrelatedLengthRead.RequestBody == nil || !unrelatedLengthRead.RequestBodyRequired {
		t.Fatalf("unrelatedLengthRead should keep its request body required because unrelated ContentLength is not a Gin request body guard, got body=%+v required=%v", unrelatedLengthRead.RequestBody, unrelatedLengthRead.RequestBodyRequired)
	}
}

func TestBuildRoutesKeepsRequiredBodyDefaultForMissingHandler(t *testing.T) {
	analyzer := handlers.NewAnalyzer(&load.Result{Fset: token.NewFileSet()}, "", diag.New())
	routeFacts, _ := buildRoutes(
		analyzer,
		[]routes.Route{
			{
				Method:  "GET",
				Path:    "/missing",
				Handler: "missingHandler",
				Span: facts.SourceSpan{
					File:      "routes.go",
					StartLine: 1,
					EndLine:   1,
				},
			},
		},
		diag.New(),
	)
	if len(routeFacts) != 1 {
		t.Fatalf("expected one route fact, got %d", len(routeFacts))
	}
	if !routeFacts[0].RequestBodyRequired {
		t.Fatalf("missing handler should preserve request_body_required default true, got %+v", routeFacts[0])
	}
}

func routeByHandler(t *testing.T, doc facts.GoFacts, handler string) facts.RouteFact {
	t.Helper()
	for _, route := range doc.Routes {
		if route.Handler == handler {
			return route
		}
	}
	t.Fatalf("missing route for handler %s", handler)
	return facts.RouteFact{}
}

func schemaByID(t *testing.T, doc facts.GoFacts, id string) facts.SchemaFact {
	t.Helper()
	for _, schema := range doc.Schemas {
		if schema.ID == id {
			return schema
		}
	}
	t.Fatalf("missing schema %s", id)
	return facts.SchemaFact{}
}

func assertPathParam(t *testing.T, route facts.RouteFact, name string) {
	t.Helper()
	for _, param := range route.Params {
		if param.Location == "path" && param.Name == name && param.Required {
			return
		}
	}
	t.Fatalf("%s should have required path param %s, got %+v", route.Handler, name, route.Params)
}

func assertBodylessStatus(t *testing.T, route facts.RouteFact, status uint16) {
	t.Helper()
	if len(route.Responses) != 1 || route.Responses[0].Status != status || route.Responses[0].Body != nil {
		t.Fatalf("%s should have bodyless status %d, got %+v", route.Handler, status, route.Responses)
	}
}

func assertResponseBodyRef(t *testing.T, route facts.RouteFact, status uint16, refID string) {
	t.Helper()
	for _, response := range route.Responses {
		if response.Status != status {
			continue
		}
		if response.Body == nil || response.Body.RefID != refID {
			t.Fatalf("%s response %d should reference %s, got %+v", route.Handler, status, refID, response)
		}
		return
	}
	t.Fatalf("%s should have response %d, got %+v", route.Handler, status, route.Responses)
}

func assertQueryParam(t *testing.T, route facts.RouteFact, name string, required bool, schemaJSON string, defaultNumber string) {
	t.Helper()
	if defaultNumber == "" {
		assertQueryParamDefault(t, route, name, required, schemaJSON, "", nil)
		return
	}
	assertQueryParamDefault(t, route, name, required, schemaJSON, "number", defaultNumber)
}

func assertQueryParamDefault(t *testing.T, route facts.RouteFact, name string, required bool, schemaJSON string, defaultType string, defaultValue any) {
	t.Helper()
	for _, param := range route.Params {
		if param.Location != "query" || param.Name != name {
			continue
		}
		gotSchema, err := json.Marshal(param.Schema)
		if err != nil {
			t.Fatalf("%s query %s schema marshal: %v", route.Handler, name, err)
		}
		if param.Required != required || !jsonEqual(t, gotSchema, []byte(schemaJSON)) {
			t.Fatalf("%s query %s: want required=%v schema=%s, got required=%v schema=%s", route.Handler, name, required, schemaJSON, param.Required, gotSchema)
		}
		if defaultType == "" {
			if param.Default != nil {
				t.Fatalf("%s query %s should not have default, got %+v", route.Handler, name, param.Default)
			}
			return
		}
		if param.Default == nil || param.Default.Type != defaultType || param.Default.Value != defaultValue {
			t.Fatalf("%s query %s default: want %s %v, got %+v", route.Handler, name, defaultType, defaultValue, param.Default)
		}
		return
	}
	t.Fatalf("%s missing query param %s, got %+v", route.Handler, name, route.Params)
}

func jsonEqual(t *testing.T, left, right []byte) bool {
	t.Helper()
	var l, r any
	if err := json.Unmarshal(left, &l); err != nil {
		t.Fatalf("unmarshal left json: %v", err)
	}
	if err := json.Unmarshal(right, &r); err != nil {
		t.Fatalf("unmarshal right json: %v", err)
	}
	return reflect.DeepEqual(l, r)
}
