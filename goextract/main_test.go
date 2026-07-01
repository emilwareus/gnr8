package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/gnr8/goextract/internal/facts"
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

	if err := run(dir, nil, tmp); err != nil {
		t.Fatalf("run goextract: %v", err)
	}
	if _, err := tmp.Seek(0, 0); err != nil {
		t.Fatalf("rewind facts file: %v", err)
	}
	var doc facts.GoFacts
	if err := json.NewDecoder(tmp).Decode(&doc); err != nil {
		t.Fatalf("decode facts: %v", err)
	}

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
