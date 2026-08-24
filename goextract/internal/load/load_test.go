package load_test

import (
	"testing"

	"golang.org/x/tools/go/packages"

	"github.com/gnr8/goextract/internal/load"
)

// The loader already classifies every per-package error by the stage that produced it.
// Which stage failed decides what the reader has to fix — a "list" error is the go
// command failing to describe the package at all (module, build, or toolchain
// resolution), while "parse" and "type" are the package's own source — so the name must
// be carried through rather than flattened into one undifferentiated "load error".
func TestErrorKindNamesTheLoaderStage(t *testing.T) {
	for _, tc := range []struct {
		name string
		kind packages.ErrorKind
		want string
	}{
		{"list", packages.ListError, "list"},
		{"parse", packages.ParseError, "parse"},
		{"type", packages.TypeError, "type"},
		{"unknown", packages.UnknownError, "unknown"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := load.ErrorKind(tc.kind); got != tc.want {
				t.Fatalf("ErrorKind(%v) = %q, want %q", tc.kind, got, tc.want)
			}
		})
	}
}

// An ErrorKind the loader adds later must still render as a name, never as an empty
// string that would produce the message "go/packages  error: ...".
func TestErrorKindNamesAnUnrecognizedStage(t *testing.T) {
	if got := load.ErrorKind(packages.ErrorKind(99)); got != "unknown" {
		t.Fatalf("ErrorKind(99) = %q, want %q", got, "unknown")
	}
}

// A module that loads cleanly reports no errors, so nothing reaches the diagnostic path.
// This is the control for the kind mapping above: the goextract module itself is a real,
// well-formed Go module compiled by the same toolchain that runs the test.
func TestLoadReportsNoErrorsForAWellFormedModule(t *testing.T) {
	res, err := load.Load("..", "./...")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if len(res.Errors) != 0 {
		t.Fatalf("expected no load errors, got %+v", res.Errors)
	}
}
