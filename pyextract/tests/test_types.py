"""Tests for ``pyextract.types`` (annotation -> neutral Type) + ``pyextract.schemas``.

These assert the BYTE-EXACT neutral dicts the facts contract (facts.rs) requires:
primitive int/float widths, the four-axis optional/nullable matrix, named-ref vs
inline-Literal enum, source-order unions, sorted enum members, and the rule-3
"unresolvable -> (None, diagnostic), never any" behavior.
"""

import ast
import unittest

from pyextract import types
from pyextract.diagnostics import Diagnostics
from pyextract.schemas import build_schemas
from pyextract.symtab import SymbolTable


class _FakeModule:
    def __init__(self, dotted, source):
        self.dotted = dotted
        self.tree = ast.parse(source)
        self.abs_path = "/virtual/{}.py".format(dotted.replace(".", "/"))


def _ann(expr):
    """Parse a single annotation expression string into its ast node."""
    return ast.parse(expr, mode="eval").body


def _map(expr, module="m", modules=None):
    """Map an annotation expression in a single-module table, returning (type, diags)."""
    src_modules = modules if modules is not None else [_FakeModule(module, "x = 1\n")]
    table = SymbolTable(src_modules)
    diags = Diagnostics()
    result = types.map_annotation(_ann(expr), module, table, diags)
    return result, diags


class PrimitiveTests(unittest.TestCase):
    def test_str(self):
        t, _ = _map("str")
        self.assertEqual(t, {"type": "primitive", "of": {"prim": "string"}})

    def test_bool(self):
        t, _ = _map("bool")
        self.assertEqual(t, {"type": "primitive", "of": {"prim": "bool"}})

    def test_int_is_int64_signed(self):
        t, _ = _map("int")
        self.assertEqual(
            t, {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}}
        )

    def test_float_is_float64(self):
        t, _ = _map("float")
        self.assertEqual(
            t, {"type": "primitive", "of": {"prim": "float", "bits": 64}}
        )


class ContainerTests(unittest.TestCase):
    def test_list_lowercase(self):
        t, _ = _map("list[str]")
        self.assertEqual(
            t, {"type": "array", "of": {"type": "primitive", "of": {"prim": "string"}}}
        )

    def test_list_uppercase(self):
        t, _ = _map("List[int]")
        self.assertEqual(
            t,
            {
                "type": "array",
                "of": {
                    "type": "primitive",
                    "of": {"prim": "int", "bits": 64, "signed": True},
                },
            },
        )

    def test_dict_is_map(self):
        t, _ = _map("dict[str, int]")
        self.assertEqual(
            t,
            {
                "type": "map",
                "of": {
                    "key": {"type": "primitive", "of": {"prim": "string"}},
                    "value": {
                        "type": "primitive",
                        "of": {"prim": "int", "bits": 64, "signed": True},
                    },
                },
            },
        )


class EnumAndUnionTests(unittest.TestCase):
    def test_literal_is_inline_sorted_enum(self):
        t, _ = _map("Literal['desc', 'asc']")
        self.assertEqual(t, {"type": "enum", "of": ["asc", "desc"]})

    def test_union_keeps_source_order(self):
        t, _ = _map("Union[int, float]")
        self.assertEqual(
            t,
            {
                "type": "union",
                "of": [
                    {
                        "type": "primitive",
                        "of": {"prim": "int", "bits": 64, "signed": True},
                    },
                    {"type": "primitive", "of": {"prim": "float", "bits": 64}},
                ],
            },
        )

    def test_pep604_union_keeps_source_order(self):
        t, _ = _map("int | float")
        self.assertEqual(
            t,
            {
                "type": "union",
                "of": [
                    {
                        "type": "primitive",
                        "of": {"prim": "int", "bits": 64, "signed": True},
                    },
                    {"type": "primitive", "of": {"prim": "float", "bits": 64}},
                ],
            },
        )


class OptionalNullableTests(unittest.TestCase):
    def test_optional_unwraps_and_signals_nullable(self):
        node = _ann("Optional[str]")
        table = SymbolTable([_FakeModule("m", "x = 1\n")])
        diags = Diagnostics()
        inner, nullable = types.map_field_annotation(node, "m", table, diags)
        self.assertTrue(nullable)
        self.assertEqual(inner, {"type": "primitive", "of": {"prim": "string"}})

    def test_pep604_none_unwraps_and_signals_nullable(self):
        node = _ann("int | None")
        table = SymbolTable([_FakeModule("m", "x = 1\n")])
        diags = Diagnostics()
        inner, nullable = types.map_field_annotation(node, "m", table, diags)
        self.assertTrue(nullable)
        self.assertEqual(
            inner,
            {"type": "primitive", "of": {"prim": "int", "bits": 64, "signed": True}},
        )

    def test_non_optional_is_not_nullable(self):
        node = _ann("str")
        table = SymbolTable([_FakeModule("m", "x = 1\n")])
        diags = Diagnostics()
        inner, nullable = types.map_field_annotation(node, "m", table, diags)
        self.assertFalse(nullable)
        self.assertEqual(inner, {"type": "primitive", "of": {"prim": "string"}})

    def test_optional_union_nullable_with_union_inner(self):
        node = _ann("Optional[Union[int, float]]")
        table = SymbolTable([_FakeModule("m", "x = 1\n")])
        diags = Diagnostics()
        inner, nullable = types.map_field_annotation(node, "m", table, diags)
        self.assertTrue(nullable)
        self.assertEqual(inner["type"], "union")
        self.assertEqual(len(inner["of"]), 2)


class NamedRefTests(unittest.TestCase):
    def test_model_class_becomes_named_ref(self):
        modules = [
            _FakeModule(
                "app.models",
                "from pydantic import BaseModel\nclass Author(BaseModel):\n    name: str\n",
            )
        ]
        t, diags = _map("Author", module="app.models", modules=modules)
        self.assertEqual(t, {"type": "named", "of": "app.models.Author"})
        self.assertEqual(diags.items(), [])

    def test_enum_class_becomes_named_ref(self):
        modules = [
            _FakeModule(
                "app.models",
                "import enum\nclass BookFormat(str, enum.Enum):\n    A = 'a'\n",
            )
        ]
        t, diags = _map("BookFormat", module="app.models", modules=modules)
        self.assertEqual(t, {"type": "named", "of": "app.models.BookFormat"})

    def test_unresolvable_yields_none_and_diagnostic(self):
        modules = [_FakeModule("app.models", "class Author:\n    name: str\n")]
        t, diags = _map("Mystery", module="app.models", modules=modules)
        # rule 3: None (omit the fact) + a diagnostic, NEVER a {"type":"any"} default.
        self.assertIsNone(t)
        items = diags.items()
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0]["severity"], "WARN")
        self.assertNotEqual(t, {"type": "any", "of": {}})


class SchemaBuilderTests(unittest.TestCase):
    """End-to-end schema build over the four-axis matrix and named refs."""

    SRC = (
        "from __future__ import annotations\n"
        "import enum\n"
        "from dataclasses import dataclass\n"
        "from typing import Literal, Optional, Union\n"
        "from pydantic import BaseModel\n"
        "\n"
        "class BookFormat(str, enum.Enum):\n"
        "    PAPERBACK = 'paperback'\n"
        "    HARDCOVER = 'hardcover'\n"
        "\n"
        "SortOrder = Literal['asc', 'desc']\n"
        "\n"
        "class BookFilters(BaseModel):\n"
        "    genre: str\n"
        "    in_stock: bool = True\n"
        "    published: Optional[int]\n"
        "    sort: Optional[SortOrder] = 'asc'\n"
        "\n"
        "@dataclass\n"
        "class CreatedMessage:\n"
        "    message: str\n"
        "    id: int\n"
        "\n"
        "class OutOfStock(BaseModel):\n"
        "    reason: str\n"
        "\n"
        "BookOrError = Union[BookFilters, OutOfStock]\n"
    )

    def _schemas(self):
        modules = [_FakeModule("app.models", self.SRC)]
        table = SymbolTable(modules)
        diags = Diagnostics()
        schemas = build_schemas(modules, table, diags)
        return {s["id"]: s for s in schemas}, diags

    def test_enum_schema_sorted_members(self):
        schemas, _ = self._schemas()
        self.assertEqual(
            schemas["app.models.BookFormat"]["body"],
            {"type": "enum", "of": ["hardcover", "paperback"]},
        )

    def test_four_axis_matrix(self):
        schemas, _ = self._schemas()
        fields = {
            f["json_name"]: f
            for f in schemas["app.models.BookFilters"]["body"]["of"]
        }
        # genre: neither
        self.assertEqual(
            (fields["genre"]["optional"], fields["genre"]["nullable"]), (False, False)
        )
        self.assertTrue(fields["genre"]["required"])
        # in_stock: optional only
        self.assertEqual(
            (fields["in_stock"]["optional"], fields["in_stock"]["nullable"]),
            (True, False),
        )
        self.assertFalse(fields["in_stock"]["required"])
        # published: nullable only
        self.assertEqual(
            (fields["published"]["optional"], fields["published"]["nullable"]),
            (False, True),
        )
        self.assertTrue(fields["published"]["required"])
        # sort: both
        self.assertEqual(
            (fields["sort"]["optional"], fields["sort"]["nullable"]), (True, True)
        )
        self.assertFalse(fields["sort"]["required"])

    def test_inline_literal_alias_field_is_inline_enum(self):
        schemas, _ = self._schemas()
        fields = {
            f["json_name"]: f
            for f in schemas["app.models.BookFilters"]["body"]["of"]
        }
        self.assertEqual(
            fields["sort"]["schema"], {"type": "enum", "of": ["asc", "desc"]}
        )

    def test_dataclass_is_object(self):
        schemas, _ = self._schemas()
        body = schemas["app.models.CreatedMessage"]["body"]
        self.assertEqual(body["type"], "object")
        names = sorted(f["json_name"] for f in body["of"])
        self.assertEqual(names, ["id", "message"])

    def test_top_level_union_alias_is_union_schema(self):
        schemas, _ = self._schemas()
        body = schemas["app.models.BookOrError"]["body"]
        self.assertEqual(body["type"], "union")
        self.assertEqual(
            body["of"],
            [
                {"type": "named", "of": "app.models.BookFilters"},
                {"type": "named", "of": "app.models.OutOfStock"},
            ],
        )

    def test_field_keys_exact(self):
        schemas, _ = self._schemas()
        f = schemas["app.models.CreatedMessage"]["body"]["of"][0]
        self.assertEqual(
            set(f.keys()),
            {
                "json_name",
                "required",
                "optional",
                "nullable",
                "schema",
                "description",
                "example",
            },
        )
        self.assertIsNone(f["description"])
        self.assertIsNone(f["example"])


if __name__ == "__main__":
    unittest.main()
