"""Docstring split tests.

SPLIT_CASES is the CANONICAL split table, mirrored verbatim from
``goextract/internal/docs/docs_test.go`` and ``tsextract/tests/docs.test.js``. All three
sidecars are held to byte-identical behavior and cannot drift. Adding a case here means
adding it in both of those files too.
"""

import unittest

from pyextract import docs

SPLIT_CASES = [
    ("empty", "", "", ""),
    ("whitespace only", "   \n\t \n ", "", ""),
    ("single sentence", "Returns widgets.", "Returns widgets.", ""),
    ("no terminator at all", "Returns widgets", "Returns widgets", ""),
    (
        "summary and description",
        "Returns widgets.\n\nResults are scoped to the caller org.",
        "Returns widgets.",
        "Results are scoped to the caller org.",
    ),
    (
        "summary wrapped across lines collapses to one line",
        "Returns widgets for\nthe caller organisation.\n\nMore detail.",
        "Returns widgets for the caller organisation.",
        "More detail.",
    ),
    (
        "second sentence with no blank line is description, not dropped",
        "Does one thing. Also does another.",
        "Does one thing.",
        "Also does another.",
    ),
    (
        # Documented edge of the initials guard, inherited from `go/doc`: a sentence
        # ending in a lone capital is indistinguishable from an initial, so it does not
        # split. Pinned here so the behavior is visible rather than surprising.
        "sentence ending in a lone capital does not split (initials-guard edge)",
        "Supports mode X. Also supports mode Y.",
        "Supports mode X. Also supports mode Y.",
        "",
    ),
    (
        "initials do not split",
        "Reviewed by A. Smith before release. Second sentence.",
        "Reviewed by A. Smith before release.",
        "Second sentence.",
    ),
    (
        "abbreviations do not split",
        "Serves the U.S. Army fleet. Then more.",
        "Serves the U.S. Army fleet.",
        "Then more.",
    ),
    (
        "question mark terminates",
        "Is the widget ready? It depends.",
        "Is the widget ready?",
        "It depends.",
    ),
    (
        "exclamation mark terminates",
        "Deletes everything! Use with care.",
        "Deletes everything!",
        "Use with care.",
    ),
    ("CRLF normalizes to LF", "First.\r\n\r\nSecond.", "First.", "Second."),
    (
        "description keeps its paragraph structure",
        "A summary.\n\nParagraph one.\n\nParagraph two.",
        "A summary.",
        "Paragraph one.\n\nParagraph two.",
    ),
    (
        "surrounding whitespace is trimmed",
        "\n\n  Returns widgets.\n\n  Detail.  \n\n",
        "Returns widgets.",
        "Detail.",
    ),
    (
        "terminator at end of text with no trailing space",
        "Only one sentence here.",
        "Only one sentence here.",
        "",
    ),
]


def _strip_space(text):
    return "".join(ch for ch in text if ch not in " \n\r\t")


class TestSplit(unittest.TestCase):
    def test_split(self):
        for name, text, summary, description in SPLIT_CASES:
            with self.subTest(name):
                got_summary, got_description = docs.split(text)
                self.assertEqual(got_summary, summary)
                self.assertEqual(got_description, description)

    def test_split_is_lossless(self):
        """The rule never silently drops text.

        Every non-space character of the input survives into summary+description. This is
        the property that separates a split from a parse — a parse may discard what it
        does not understand, and that is exactly how a comment convention turns into a
        dialect.
        """
        for name, text, _summary, _description in SPLIT_CASES:
            with self.subTest(name):
                got_summary, got_description = docs.split(text)
                self.assertEqual(
                    _strip_space(got_summary + got_description), _strip_space(text)
                )

    def test_none_is_handled(self):
        """A function with no docstring yields empty prose, never a guess (rule 3)."""
        self.assertEqual(docs.split(None), ("", ""))


if __name__ == "__main__":
    unittest.main()
