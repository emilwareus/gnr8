"""Tests for ``pyextract.load`` — static discovery + ``ast.parse`` of a target tree.

Asserts the loader parses a 2-module fixture-shaped tree WITHOUT importing it, that a
``SyntaxError`` becomes a diagnostic (not an abort), and that dotted-module ids are
derived deterministically.
"""

import ast
import os
import tempfile
import unittest

from pyextract import load
from pyextract.diagnostics import Diagnostics


class _Tree:
    """A temporary on-disk Python tree for a single test."""

    def __init__(self, files):
        self._dir = tempfile.TemporaryDirectory()
        self.root = os.path.realpath(self._dir.name)
        for rel, content in files.items():
            abs_path = os.path.join(self.root, rel)
            os.makedirs(os.path.dirname(abs_path), exist_ok=True)
            with open(abs_path, "w", encoding="utf-8") as handle:
                handle.write(content)

    def close(self):
        self._dir.cleanup()


class LoadTests(unittest.TestCase):
    def test_parses_two_module_tree_statically(self):
        tree = _Tree(
            {
                "app/__init__.py": "",
                "app/models.py": "class Book:\n    pass\n",
                "app/main.py": "from app.models import Book\n",
            }
        )
        self.addCleanup(tree.close)
        diags = Diagnostics()

        modules = load.load(tree.root, diags)

        dotted = [m.dotted for m in modules]
        # __init__.py collapses to its package name "app"; sorted dotted order.
        self.assertEqual(dotted, ["app", "app.main", "app.models"])
        # Each module carries a real ast.Module (parsed, NOT executed).
        for module in modules:
            self.assertIsInstance(module.tree, ast.Module)
            self.assertTrue(os.path.isabs(module.abs_path))
        self.assertEqual(diags.items(), [])

    def test_syntax_error_becomes_diagnostic_not_abort(self):
        tree = _Tree(
            {
                "good.py": "x = 1\n",
                "bad.py": "def broken(:\n",  # invalid syntax
            }
        )
        self.addCleanup(tree.close)
        diags = Diagnostics()

        modules = load.load(tree.root, diags)

        # The good module still loads; the bad one is skipped with a diagnostic.
        self.assertEqual([m.dotted for m in modules], ["good"])
        items = diags.items()
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0]["severity"], "WARN")
        self.assertIn("could not parse", items[0]["message"])
        self.assertTrue(items[0]["file"].endswith("bad.py"))
        self.assertGreaterEqual(items[0]["line"], 1)

    def test_target_syntax_newer_than_runtime_parses(self):
        # Target fixtures may use 3.10+ spellings (`X | Y`, `list[T]`). ast.parse on
        # 3.9 recognizes the syntax of these annotations without executing them.
        tree = _Tree(
            {
                "mod.py": (
                    "from __future__ import annotations\n"
                    "def f(x: list[int], y: int | None) -> None: ...\n"
                ),
            }
        )
        self.addCleanup(tree.close)
        diags = Diagnostics()

        modules = load.load(tree.root, diags)

        self.assertEqual([m.dotted for m in modules], ["mod"])
        self.assertEqual(diags.items(), [])

    def test_dotted_module_derivation(self):
        self.assertEqual(load._dotted_module("app/models.py"), "app.models")
        self.assertEqual(load._dotted_module("app/__init__.py"), "app")
        self.assertEqual(load._dotted_module("__init__.py"), "")
        self.assertEqual(load._dotted_module("top.py"), "top")


if __name__ == "__main__":
    unittest.main()
