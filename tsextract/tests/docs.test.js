"use strict";

// JSDoc split tests.
//
// SPLIT_CASES is the CANONICAL split table, mirrored verbatim from
// `goextract/internal/docs/docs_test.go` and `pyextract/tests/test_docs.py`. All three
// sidecars are held to byte-identical behavior and cannot drift. Adding a case here
// means adding it in both of those files too.

const assert = require("node:assert");
const test = require("node:test");

const docs = require("../docs");

const SPLIT_CASES = [
  ["empty", "", "", ""],
  ["whitespace only", "   \n\t \n ", "", ""],
  ["single sentence", "Returns widgets.", "Returns widgets.", ""],
  ["no terminator at all", "Returns widgets", "Returns widgets", ""],
  [
    "summary and description",
    "Returns widgets.\n\nResults are scoped to the caller org.",
    "Returns widgets.",
    "Results are scoped to the caller org.",
  ],
  [
    "summary wrapped across lines collapses to one line",
    "Returns widgets for\nthe caller organisation.\n\nMore detail.",
    "Returns widgets for the caller organisation.",
    "More detail.",
  ],
  [
    "second sentence with no blank line is description, not dropped",
    "Does one thing. Also does another.",
    "Does one thing.",
    "Also does another.",
  ],
  [
    // Documented edge of the initials guard, inherited from `go/doc`: a sentence ending
    // in a lone capital is indistinguishable from an initial, so it does not split.
    // Pinned here so the behavior is visible rather than surprising.
    "sentence ending in a lone capital does not split (initials-guard edge)",
    "Supports mode X. Also supports mode Y.",
    "Supports mode X. Also supports mode Y.",
    "",
  ],
  [
    "initials do not split",
    "Reviewed by A. Smith before release. Second sentence.",
    "Reviewed by A. Smith before release.",
    "Second sentence.",
  ],
  [
    "abbreviations do not split",
    "Serves the U.S. Army fleet. Then more.",
    "Serves the U.S. Army fleet.",
    "Then more.",
  ],
  [
    "question mark terminates",
    "Is the widget ready? It depends.",
    "Is the widget ready?",
    "It depends.",
  ],
  [
    "exclamation mark terminates",
    "Deletes everything! Use with care.",
    "Deletes everything!",
    "Use with care.",
  ],
  ["CRLF normalizes to LF", "First.\r\n\r\nSecond.", "First.", "Second."],
  [
    "description keeps its paragraph structure",
    "A summary.\n\nParagraph one.\n\nParagraph two.",
    "A summary.",
    "Paragraph one.\n\nParagraph two.",
  ],
  [
    "surrounding whitespace is trimmed",
    "\n\n  Returns widgets.\n\n  Detail.  \n\n",
    "Returns widgets.",
    "Detail.",
  ],
  [
    "terminator at end of text with no trailing space",
    "Only one sentence here.",
    "Only one sentence here.",
    "",
  ],
];

const stripSpace = (text) => text.replace(/[ \n\r\t]/g, "");

test("split matches the canonical table", () => {
  for (const [name, text, summary, description] of SPLIT_CASES) {
    const got = docs.split(text);
    assert.strictEqual(got.summary, summary, `summary: ${name}`);
    assert.strictEqual(got.description, description, `description: ${name}`);
  }
});

// The rule never silently drops text: every non-space character of the input survives
// into summary+description. This is the property that separates a split from a parse — a
// parse may discard what it does not understand, and that is exactly how a comment
// convention turns into a dialect.
test("split is lossless", () => {
  for (const [name, text] of SPLIT_CASES) {
    const got = docs.split(text);
    assert.strictEqual(
      stripSpace(got.summary + got.description),
      stripSpace(text),
      `lossless: ${name}`
    );
  }
});

// A method with no JSDoc yields empty prose, never a guess (rule 3).
test("absent documentation yields empty prose", () => {
  for (const absent of [undefined, null, ""]) {
    assert.deepStrictEqual(docs.split(absent), { summary: "", description: "" });
  }
});
