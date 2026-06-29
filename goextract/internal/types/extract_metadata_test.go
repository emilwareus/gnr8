package types

import (
	"reflect"
	"strings"
	"testing"

	"github.com/gnr8/goextract/internal/diag"
	"github.com/gnr8/goextract/internal/facts"
)

func TestFieldMetaFromTagsParsesConstraintsDefaultsAndExtensions(t *testing.T) {
	tag := reflect.StructTag(`json:"name" binding:"required,min=3,max=80,oneof=alpha beta" default:"alpha" placeholder:"Goal name" render:"textarea" x-gnr8-widget:"compact"`)
	diags := diag.New()

	meta := fieldMetaFromTags(
		"CreateGoalInput",
		"Name",
		tag,
		string(tag),
		facts.PrimitiveType(facts.StringPrim()),
		"dto.go",
		10,
		diags,
	)

	if meta == nil || meta.Constraints == nil {
		t.Fatalf("expected metadata constraints, got %#v", meta)
	}
	if meta.Constraints.MinLength == nil || *meta.Constraints.MinLength != 3 {
		t.Fatalf("minLength not parsed: %#v", meta.Constraints.MinLength)
	}
	if meta.Constraints.MaxLength == nil || *meta.Constraints.MaxLength != 80 {
		t.Fatalf("maxLength not parsed: %#v", meta.Constraints.MaxLength)
	}
	if got := meta.Constraints.EnumValues; len(got) != 2 || got[0] != "alpha" || got[1] != "beta" {
		t.Fatalf("oneof values not parsed in source order: %#v", got)
	}
	if meta.Default == nil || meta.Default.Type != "string" || meta.Default.Value != "alpha" {
		t.Fatalf("default literal not parsed: %#v", meta.Default)
	}
	if len(meta.Extensions) != 3 {
		t.Fatalf("expected 3 extensions, got %#v", meta.Extensions)
	}
	if got := meta.Extensions[0].Name; got != "x-gnr8-placeholder" {
		t.Fatalf("extensions should be sorted by name, first=%q", got)
	}
	if len(diags.Items()) != 0 {
		t.Fatalf("unexpected diagnostics: %#v", diags.Items())
	}
}

func TestFieldMetaFromTagsParsesNumericBindingsAndUnsupportedDiagnostics(t *testing.T) {
	tag := reflect.StructTag(`json:"windowDays" binding:"gte=1,lte=365,uuid" default:"30"`)
	diags := diag.New()

	meta := fieldMetaFromTags(
		"GoalAnalyticsQuery",
		"WindowDays",
		tag,
		string(tag),
		facts.PrimitiveType(facts.IntPrim(64, true)),
		"dto.go",
		11,
		diags,
	)

	if meta == nil || meta.Constraints == nil {
		t.Fatalf("expected metadata constraints, got %#v", meta)
	}
	if meta.Constraints.Minimum == nil || *meta.Constraints.Minimum != "1" {
		t.Fatalf("minimum not parsed: %#v", meta.Constraints.Minimum)
	}
	if meta.Constraints.Maximum == nil || *meta.Constraints.Maximum != "365" {
		t.Fatalf("maximum not parsed: %#v", meta.Constraints.Maximum)
	}
	if meta.Default == nil || meta.Default.Type != "number" || meta.Default.Value != "30" {
		t.Fatalf("numeric default not parsed: %#v", meta.Default)
	}
	if !hasMetadataDiag(diags.Items(), "unsupported binding tag", "uuid") {
		t.Fatalf("expected unsupported binding diagnostic, got %#v", diags.Items())
	}
}

func TestBindingHasRequiredRequiresExactToken(t *testing.T) {
	if !bindingHasRequired("omitempty,required") {
		t.Fatal("expected exact required token to mark field required")
	}
	if bindingHasRequired("required_without=Name") {
		t.Fatal("required_without must not mark the field strictly required")
	}
	if bindingHasRequired("notrequired") {
		t.Fatal("substring matches must not mark the field required")
	}
}

func TestValidateHasRequiredRequiresExactToken(t *testing.T) {
	if !validateHasRequired("required,email") {
		t.Fatal("expected validate required token to mark field required")
	}
	if validateHasRequired("required_without=Name") {
		t.Fatal("required_without must not mark the field strictly required")
	}
	if validateHasRequired("notrequired") {
		t.Fatal("substring matches must not mark the field required")
	}
}

func hasMetadataDiag(diags []facts.DiagnosticFact, rule string, token string) bool {
	for _, d := range diags {
		if strings.Contains(d.Message, rule) && strings.Contains(d.Message, token) {
			return true
		}
	}
	return false
}
