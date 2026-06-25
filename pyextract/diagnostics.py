"""The diagnostics accumulator — the Python twin of ``goextract/internal/diag``.

A diagnostic is emitted (rule 3) whenever a fact cannot be derived from a single
deterministic code source: an unresolvable/foreign type name, an untyped read, etc.
The fact is then OMITTED — never guessed.

A ``DiagnosticFact`` carries EXACTLY ``severity, message, file, line`` — ``line`` is
a single 1-based int, NOT a span (facts.rs ``DiagnosticFact``). Severity is ``WARN``
for every diagnostic this sidecar emits.
"""


class Diagnostics:
    """Accumulates WARN diagnostics as plain dicts in the neutral facts shape."""

    def __init__(self):
        self._items = []

    def warn(self, message, file, line):
        """Record a WARN diagnostic.

        Args:
            message: the human-readable rule + identity.
            file: the source file (canonical absolute path; the host relativizes).
            line: the 1-based line number (a single int, never a span).
        """
        self._items.append(
            {
                "severity": "WARN",
                "message": message,
                "file": file,
                "line": int(line),
            }
        )

    def items(self):
        """Return the accumulated diagnostic dicts (host re-sorts the final slice)."""
        return list(self._items)
