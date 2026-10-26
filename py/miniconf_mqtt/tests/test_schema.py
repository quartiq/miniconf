"""Broker-free schema, rendering, and CLI path contracts."""

import json
import subprocess
import sys
from pathlib import Path
from unittest import TestCase

from miniconf_mqtt.cli import _normalize_command_path
from miniconf_mqtt.errors import MiniconfException
from miniconf_mqtt.render import render_schema_tree, render_value_tree
from miniconf_mqtt.schema import Indices, Schema


class SchemaTests(TestCase):
    def test_offline_imports(self):
        subprocess.run(
            [
                sys.executable,
                "-c",
                """
import sys
sys.modules['aiomqtt'] = None
sys.modules['paho'] = None
from miniconf_mqtt.schema import Schema
from miniconf_mqtt.render import render_schema_tree
assert render_schema_tree(Schema([{}], 1))
""",
            ],
            check=True,
        )

    def test_rust_export(self):
        fixture = Path(__file__).resolve().parents[3] / "miniconf/tests/common.ndjson"
        schema = Schema(
            [json.loads(line) for line in fixture.read_text().splitlines()], 1
        )
        nodes = list(schema.walk())
        assert len(nodes) > 1
        for node in nodes:
            assert schema.node(schema.indices(node.path)) == node

    def test_invalid_definitions(self):
        for defs in (
            [],
            [None],
            [{"i": None}],
            [{"i": {"k": "n", "c": {"bad": 1}}}, {}],
            [{"i": {"k": "d", "c": [0]}}],
            [{}, {"i": {"k": "n", "c": []}}],
            [{}, {"i": {"k": "d", "c": {}}}],
            [{}, {"i": {"k": "unknown", "c": 0}}],
            *(
                [{}, {"i": {"k": "h", "c": ref, "l": 1}}]
                for ref in (-1, 2, 0.0, False, {}, {"r": -1})
            ),
            *(
                [{}, {"i": {"k": "h", "c": 0, "l": length}}]
                for length in (-1, 1.0, True, None)
            ),
        ):
            with self.subTest(defs=defs), self.assertRaises(MiniconfException) as error:
                Schema(defs, 1)
            self.assertEqual(error.exception.code, "Protocol")

        # Validation must not enumerate a homogeneous node's concrete paths.
        schema = Schema([{}, {"i": {"k": "h", "c": 0, "l": 10**12}}], 1)
        self.assertEqual(schema.node("/999999999999").kind, "leaf")

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

    def test_compact_subtree_remaps_references(self):
        schema = Schema(
            [
                {},
                {"s": {"ty": "bool"}},
                {"i": {"k": "h", "l": 2, "c": 1}},
                {"i": {"k": "n", "c": {"unused": 0, "values": 2}}},
            ],
            1,
        )
        assert schema.compact("/values") == {
            "path": "/values",
            "rev": 1,
            "defs": [
                {"s": {"ty": "bool"}},
                {"i": {"k": "h", "l": 2, "c": 0}},
            ],
        }

    def test_render_metadata(self):
        compressed_sem = render_schema_tree(
            Schema(
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
            "└─ array_tree [homogeneous len=2]",
            "   └─ {index} [leaf] [sem ty=i32]",
        ], compressed_sem
        quoted_meta = render_schema_tree(
            Schema(
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
        schema = Schema(
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
            "(root) [numbered len=2]",
            "├─ 0 [homogeneous len=3]",
            "│  └─ {index} [homogeneous len=2]",
            "│     └─ {index} [leaf] [sem ty=u16]",
            "└─ 1 [leaf] [sem ty=bool]",
        ]

    def test_empty_names(self):
        empty_name_schema = Schema(
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
        schema = Schema([{"s": {"ty": "i32"}}], 1)
        assert render_schema_tree(schema) == "(root) [leaf] [sem ty=i32]"
        assert render_value_tree(schema, {"": 42}) == "(root) = 42"
        assert render_value_tree(schema, {}) == "(root) = <absent>"

    def test_array_indices(self):
        for internal in ({"k": "d", "c": [0, 0]}, {"k": "h", "c": 0, "l": 2}):
            schema = Schema([{}, {"i": internal}], 1)
            assert schema.path("/0") == "/0"
            assert schema.path("/1") == "/1"
            for keys in ("/-1", "/2", "/00", "/+0", "/ 0", Indices((-1,))):
                with self.assertRaises(MiniconfException):
                    schema.path(keys)
        named = Schema([{}, {"i": {"k": "n", "c": {"00": 0}}}], 1)
        assert named.path("/00") == "/00"
