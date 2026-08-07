package docs_test

import (
	"testing"

	"github.com/gnr8/goextract/internal/docs"
)

// splitCases is the CANONICAL split table. It is mirrored verbatim in
// `pyextract/tests/test_docs.py` and `tsextract/tests/docs.test.js`, so all three
// sidecars are held to byte-identical behavior and cannot drift. Adding a case here
// means adding it in both of those files too.
var splitCases = []struct {
	name        string
	text        string
	summary     string
	description string
}{
	{"empty", "", "", ""},
	{"whitespace only", "   \n\t \n ", "", ""},
	{"single sentence", "Returns widgets.", "Returns widgets.", ""},
	{"no terminator at all", "Returns widgets", "Returns widgets", ""},
	{
		"summary and description",
		"Returns widgets.\n\nResults are scoped to the caller org.",
		"Returns widgets.",
		"Results are scoped to the caller org.",
	},
	{
		"summary wrapped across lines collapses to one line",
		"Returns widgets for\nthe caller organisation.\n\nMore detail.",
		"Returns widgets for the caller organisation.",
		"More detail.",
	},
	{
		"second sentence with no blank line is description, not dropped",
		"Does one thing. Also does another.",
		"Does one thing.",
		"Also does another.",
	},
	{
		// Documented edge of the initials guard, inherited from `go/doc`: a sentence
		// ending in a lone capital is indistinguishable from an initial, so it does
		// not split. Pinned here so the behavior is visible rather than surprising.
		// Accepting it is the right trade — "Reviewed by A. Smith" is far more common
		// in real doc comments than a sentence ending in a bare capital letter.
		"sentence ending in a lone capital does not split (initials-guard edge)",
		"Supports mode X. Also supports mode Y.",
		"Supports mode X. Also supports mode Y.",
		"",
	},
	{
		"initials do not split",
		"Reviewed by A. Smith before release. Second sentence.",
		"Reviewed by A. Smith before release.",
		"Second sentence.",
	},
	{
		"abbreviations do not split",
		"Serves the U.S. Army fleet. Then more.",
		"Serves the U.S. Army fleet.",
		"Then more.",
	},
	{
		"question mark terminates",
		"Is the widget ready? It depends.",
		"Is the widget ready?",
		"It depends.",
	},
	{
		"exclamation mark terminates",
		"Deletes everything! Use with care.",
		"Deletes everything!",
		"Use with care.",
	},
	{
		"CRLF normalizes to LF",
		"First.\r\n\r\nSecond.",
		"First.",
		"Second.",
	},
	{
		"description keeps its paragraph structure",
		"A summary.\n\nParagraph one.\n\nParagraph two.",
		"A summary.",
		"Paragraph one.\n\nParagraph two.",
	},
	{
		"surrounding whitespace is trimmed",
		"\n\n  Returns widgets.\n\n  Detail.  \n\n",
		"Returns widgets.",
		"Detail.",
	},
	{
		"terminator at end of text with no trailing space",
		"Only one sentence here.",
		"Only one sentence here.",
		"",
	},
}

func TestSplit(t *testing.T) {
	for _, tc := range splitCases {
		t.Run(tc.name, func(t *testing.T) {
			summary, description := docs.Split(tc.text)
			if summary != tc.summary {
				t.Errorf("summary:\n got  %q\n want %q", summary, tc.summary)
			}
			if description != tc.description {
				t.Errorf("description:\n got  %q\n want %q", description, tc.description)
			}
		})
	}
}

// TestSplitIsLossless proves the rule never silently drops text: every non-space rune
// of the input survives into summary+description. This is the property that separates
// a split from a parse — a parse may discard what it does not understand, and that is
// exactly how a comment convention turns into a dialect.
func TestSplitIsLossless(t *testing.T) {
	for _, tc := range splitCases {
		t.Run(tc.name, func(t *testing.T) {
			summary, description := docs.Split(tc.text)
			got := stripSpace(summary + description)
			want := stripSpace(tc.text)
			if got != want {
				t.Errorf("text lost in split:\n got  %q\n want %q", got, want)
			}
		})
	}
}

func stripSpace(s string) string {
	out := make([]rune, 0, len(s))
	for _, r := range s {
		if r != ' ' && r != '\n' && r != '\r' && r != '\t' {
			out = append(out, r)
		}
	}
	return string(out)
}

func TestStripSymbolName(t *testing.T) {
	cases := []struct {
		name    string
		summary string
		symbol  string
		want    string
	}{
		{
			"go convention is stripped and capitalized",
			"listWidgets returns widgets for the caller org.",
			"listWidgets",
			"Returns widgets for the caller org.",
		},
		{
			"already-capitalized remainder is left alone",
			"listWidgets HTTPS health probe.",
			"listWidgets",
			"HTTPS health probe.",
		},
		{
			"summary not starting with the symbol is unchanged",
			"Returns widgets.",
			"listWidgets",
			"Returns widgets.",
		},
		{
			"bare symbol name with nothing after is unchanged",
			"listWidgets",
			"listWidgets",
			"listWidgets",
		},
		{
			"a symbol that is only a prefix of the first word is not stripped",
			"listWidgetsAndThings returns things.",
			"listWidgets",
			"listWidgetsAndThings returns things.",
		},
		{"empty symbol is a no-op", "Returns widgets.", "", "Returns widgets."},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := docs.StripSymbolName(tc.summary, tc.symbol); got != tc.want {
				t.Errorf("\n got  %q\n want %q", got, tc.want)
			}
		})
	}
}
