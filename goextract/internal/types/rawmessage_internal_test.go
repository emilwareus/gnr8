package types

import (
	gotypes "go/types"
	"testing"

	"github.com/gnr8/goextract/internal/facts"
)

func TestRawMessageLowersToAny(t *testing.T) {
	pkg := gotypes.NewPackage(jsonPkgPath, "json")
	obj := gotypes.NewTypeName(0, pkg, "RawMessage", nil)
	named := gotypes.NewNamed(obj, gotypes.NewSlice(gotypes.Typ[gotypes.Byte]), nil)

	got := mapNamed(named, mapCtx{})
	if got.Type != facts.TypeAny {
		t.Fatalf("RawMessage: want any, got %+v", got)
	}
}
