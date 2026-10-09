"""Broker-free schema, rendering, and CLI path contracts."""

import json
from pathlib import Path
from unittest import TestCase

from miniconf.cli import _normalize_command_path
from miniconf.common import MiniconfException
from miniconf.render import render_schema_tree, render_value_tree
from miniconf.schema import Indices, Schema


def fixture_schema():
    fixture = Path(__file__).resolve().parents[2] / "fixtures/compact-schema.ndjson"
    return Schema.from_defs(
        [json.loads(line) for line in fixture.read_text().splitlines()], 1
    )


class SchemaTests(TestCase):
    def test_paths(self):
        assert _normalize_command_path("", "/items/0") == ("", "/items/0")
        assert _normalize_command_path("/", "") == ("/", "/")
        assert _normalize_command_path("value", "/") == ("//value", "/")
        assert _normalize_command_path("/items/0/group", "") == (
            "/items/0/group",
            "/items/0/group",
        )
        assert _normalize_command_path("a", "/items/0/group") == (
            "/items/0/group/a",
            "/items/0/group",
        )
        assert _normalize_command_path("/items/0/group/a", "", subtree=False) == (
            "/items/0/group/a",
            "/items/0/group",
        )
        assert _normalize_command_path("b", "/items/0/group") == (
            "/items/0/group/b",
            "/items/0/group",
        )

    def test_schema(self):
        schema_fixture = fixture_schema()
        assert [node.path for node in schema_fixture.walk()] == [
            "",
            "/value",
            "/nested",
            "/nested/leaf",
        ]
        assert schema_fixture.node().kind == "named"
        assert schema_fixture.node("/nested").kind == "named"
        assert schema_fixture.node("/value").kind == "leaf"
        assert schema_fixture.node("/value").edge == {"role": "selector"}
        assert schema_fixture.node("/nested").edge is None
        assert schema_fixture.compact("/nested") == {
            "path": "/nested",
            "rev": 1,
            "defs": [
                {},
                {"i": {"k": "n", "c": {"leaf": 0}}},
            ],
        }

    def test_render_metadata(self):
        compressed_sem = render_schema_tree(
            Schema.from_defs(
                [
                    {"s": {"ty": "i32"}},
                    {"i": {"k": "h", "l": 2, "c": 0}},
                    {"i": {"k": "n", "c": {"array_tree": 1}}},
                ],
                1,
            )
        ).splitlines()
        assert compressed_sem == [
            "(root) [named]",
            "└─ array_tree [homogeneous]",
            "   └─ 0..2 [leaf] [sem ty=i32]",
        ], compressed_sem
        quoted_meta = render_schema_tree(
            Schema.from_defs(
                [
                    {"m": {"typename": "InnerType"}},
                    {
                        "i": {
                            "k": "n",
                            "c": {"node": {"r": 0, "m": {"doc": "Outer doc"}}},
                        }
                    },
                ],
                1,
            )
        ).splitlines()
        assert quoted_meta == [
            "(root) [named]",
            '└─ node [leaf] [edge doc="Outer doc"] [node typename="InnerType"]',
        ], quoted_meta

    def test_nested_homogeneous_and_numbered(self):
        schema = Schema.from_defs(
            [
                {"s": {"ty": "u16"}},
                {"s": {"ty": "bool"}},
                {"i": {"k": "h", "l": 2, "c": 0}},
                {"i": {"k": "h", "l": 3, "c": 2}},
                {"i": {"k": "d", "c": [3, 1]}},
            ],
            1,
        )
        assert render_schema_tree(schema).splitlines() == [
            "(root) [numbered]",
            "├─ 0 [homogeneous]",
            "│  └─ 0..3 [homogeneous]",
            "│     └─ 0..2 [leaf] [sem ty=u16]",
            "└─ 1 [leaf] [sem ty=bool]",
        ]

    def test_empty_names(self):
        empty_name_schema = Schema.from_defs(
            [
                {"s": {"ty": "i32"}},
                {"i": {"k": "n", "c": {"value": 0}}},
                {"i": {"k": "n", "c": {"": 1, "value": 0}}},
            ],
            1,
        )
        assert empty_name_schema.path("") == ""
        assert empty_name_schema.path("/") == "/"
        assert empty_name_schema.path("//value") == "//value"
        assert render_schema_tree(empty_name_schema, "/").splitlines() == [
            "/ [named]",
            "└─ value [leaf] [sem ty=i32]",
        ]
        assert render_schema_tree(empty_name_schema).splitlines() == [
            "(root) [named]",
            '├─ "" [named]',
            "│  └─ value [leaf] [sem ty=i32]",
            "└─ value [leaf] [sem ty=i32]",
        ]
        empty_values = {"//value": 1, "/value": 2}
        assert render_value_tree(empty_name_schema, empty_values).splitlines() == [
            '├─ ""',
            "│  └─ value = 1",
            "└─ value = 2",
        ]

    def test_root_leaf(self):
        schema = Schema.from_defs([{"s": {"ty": "i32"}}], 1)
        assert render_schema_tree(schema) == "(root) [leaf] [sem ty=i32]"
        assert render_value_tree(schema, {"": 42}) == "(root) = 42"
        assert render_value_tree(schema, {}) == "(root) = <absent>"

    def test_array_indices(self):
        for internal in ({"k": "d", "c": [0, 0]}, {"k": "h", "c": 0, "l": 2}):
            schema = Schema.from_defs([{}, {"i": internal}], 1)
            assert schema.path("/0") == "/0"
            assert schema.path("/1") == "/1"
            for keys in ("/-1", "/2", "/00", "/+0", "/ 0", Indices((-1,))):
                with self.assertRaises(MiniconfException):
                    schema.path(keys)
        named = Schema.from_defs([{}, {"i": {"k": "n", "c": {"00": 0}}}], 1)
        assert named.path("/00") == "/00"
