"""Unit-level regression tests for ``pyextract.schemas`` edge cases the hand-tuned
fixtures do not exercise.

Currently: an ``enum.Enum`` subclass with no string-valued members (int / ``auto()`` /
tuple values) must diagnose + omit the schema, never emit an invalid empty
``{"type": "enum", "of": []}`` (rule 3 — no guess; mirrors the empty-``Literal`` guard
in ``pyextract.types``).
"""

import ast
import unittest

from pyextract import schemas
from pyextract.diagnostics import Diagnostics
from pyextract.symtab import SymbolTable


class _FakeModule:
    def __init__(self, dotted, source):
        self.dotted = dotted
        self.tree = ast.parse(source)
        self.abs_path = "/virtual/{}.py".format(dotted.replace(".", "/"))


def _class(source):
    for stmt in ast.parse(source).body:
        if isinstance(stmt, ast.ClassDef):
            return stmt
    raise AssertionError("no class def in source")


class EmptyEnumOmissionTests(unittest.TestCase):
    def _build(self, source):
        module = _FakeModule("app.models", source)
        table = SymbolTable([module])
        diags = Diagnostics()
        schema = schemas._build_class_schema(
            _class(source), "app.models", module.abs_path, table, diags
        )
        return schema, diags

    def test_int_enum_omitted_with_diagnostic(self):
        # An int-valued enum has no string members → omit + diagnose, never `of: []`.
        schema, diags = self._build(
            "import enum\n"
            "class Priority(enum.Enum):\n"
            "    LOW = 1\n"
            "    HIGH = 2\n"
        )
        self.assertIsNone(schema)
        self.assertTrue(diags.items(), "expected a diagnostic for the empty enum")

    def test_auto_enum_omitted_with_diagnostic(self):
        schema, diags = self._build(
            "import enum\n"
            "class Kind(enum.Enum):\n"
            "    A = enum.auto()\n"
            "    B = enum.auto()\n"
        )
        self.assertIsNone(schema)
        self.assertTrue(diags.items())

    def test_string_enum_still_emitted(self):
        # Control: a string enum is unaffected (sorted string members).
        schema, _diags = self._build(
            "import enum\n"
            "class Fmt(enum.Enum):\n"
            "    HARD = 'hardcover'\n"
            "    SOFT = 'paperback'\n"
        )
        self.assertIsNotNone(schema)
        self.assertEqual(schema["body"], {"type": "enum", "of": ["hardcover", "paperback"]})


if __name__ == "__main__":
    unittest.main()
