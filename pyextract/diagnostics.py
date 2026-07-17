"""The diagnostics accumulator — the Python twin of ``goextract/internal/diag``.

A diagnostic is emitted (rule 3) whenever a fact cannot be derived from a single
deterministic code source: an unresolvable/foreign type name, an untyped read, etc.
The fact is then OMITTED — never guessed.

A ``DiagnosticFact`` carries a stable code/category, optional operation/schema subjects, and an
inclusive source span. Severity is ``WARN`` for every diagnostic this sidecar emits.
"""


class Diagnostics:
    """Accumulates WARN diagnostics as plain dicts in the neutral facts shape."""

    def __init__(self):
        self._items = []

    def warn(
        self,
        message,
        file,
        line,
        code="source.unresolved",
        category="source",
        operation=None,
        schema=None,
        subject=None,
    ):
        """Record a WARN diagnostic.

        Args:
            message: the human-readable rule + identity.
            file: the source file (canonical absolute path; the host relativizes).
            line: the 1-based line number (a single int, never a span).
        """
        self._items.append(
            {
                "code": code,
                "severity": "WARN",
                "category": category,
                "message": message,
                "file": file,
                "line": int(line),
                "end_line": int(line),
                **({"operation": operation} if operation else {}),
                **({"schema": schema} if schema else {}),
                **({"subject": subject} if subject else {}),
            }
        )

    def items(self):
        """Return the accumulated diagnostic dicts (host re-sorts the final slice)."""
        return list(self._items)
