"""Tests for ``pyextract.symtab`` — the owned, static, cross-module symbol table.

Asserts ``resolve`` finds a local class, follows a cross-module ``from x import Y``
statically, resolves an alias, and returns the distinct ``UNRESOLVABLE`` sentinel
(NOT a guessed string) for a foreign name.
"""

import ast
import unittest

from pyextract.symtab import UNRESOLVABLE, SymbolTable


class _FakeModule:
    """A load.Module stand-in built from a source string."""

    def __init__(self, dotted, source):
        self.dotted = dotted
        self.tree = ast.parse(source)
        self.abs_path = "/virtual/{}.py".format(dotted.replace(".", "/"))


def _table(modules):
    return SymbolTable(modules)


class SymbolTableTests(unittest.TestCase):
    def test_resolves_local_class(self):
        table = _table([_FakeModule("app.models", "class Book:\n    pass\n")])

        res = table.resolve("Book", "app.models")

        self.assertIsNot(res, UNRESOLVABLE)
        self.assertEqual(res.kind, "class")
        self.assertEqual(res.qualified_id, "app.models.Book")
        self.assertEqual(res.module, "app.models")
        self.assertIsInstance(res.node, ast.ClassDef)

    def test_follows_cross_module_import(self):
        table = _table(
            [
                _FakeModule("app.models", "class Book:\n    pass\n"),
                _FakeModule("app.main", "from app.models import Book\n"),
            ]
        )

        res = table.resolve("Book", "app.main")

        self.assertIsNot(res, UNRESOLVABLE)
        self.assertEqual(res.qualified_id, "app.models.Book")
        self.assertEqual(res.module, "app.models")

    def test_follows_import_alias(self):
        table = _table(
            [
                _FakeModule("app.models", "class Book:\n    pass\n"),
                _FakeModule("app.main", "from app.models import Book as B\n"),
            ]
        )

        res = table.resolve("B", "app.main")

        self.assertIsNot(res, UNRESOLVABLE)
        self.assertEqual(res.qualified_id, "app.models.Book")

    def test_resolves_alias_assignment(self):
        table = _table(
            [_FakeModule("app.models", "SortOrder = Literal['asc', 'desc']\n")]
        )

        res = table.resolve("SortOrder", "app.models")

        self.assertIsNot(res, UNRESOLVABLE)
        self.assertEqual(res.kind, "alias")
        self.assertEqual(res.qualified_id, "app.models.SortOrder")
        self.assertIsInstance(res.node, ast.Subscript)

    def test_foreign_name_is_unresolvable_sentinel(self):
        table = _table([_FakeModule("app.models", "class Book:\n    pass\n")])

        res = table.resolve("BaseModel", "app.models")

        # A distinct sentinel — NOT a guessed string and NOT None-by-coincidence.
        self.assertIs(res, UNRESOLVABLE)

    def test_unknown_module_is_unresolvable(self):
        table = _table([_FakeModule("app.models", "class Book:\n    pass\n")])

        self.assertIs(table.resolve("Book", "no.such.module"), UNRESOLVABLE)

    def test_cyclic_alias_import_is_unresolvable(self):
        table = _table(
            [
                _FakeModule("a", "from b import X\n"),
                _FakeModule("b", "from a import X\n"),
            ]
        )

        self.assertIs(table.resolve("X", "a"), UNRESOLVABLE)


if __name__ == "__main__":
    unittest.main()
