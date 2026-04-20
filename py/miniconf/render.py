"""Shared human-readable rendering for Miniconf schema and values."""

from __future__ import annotations

from typing import Any, Callable

from .common import json_dumps
from .schema import Schema, SchemaNode


def _segment(path: str) -> str:
    return path.rsplit("/", 1)[-1] if path else ""


def _format_scalar(value: Any, *, quote_strings: bool = False) -> str:
    if isinstance(value, str):
        return json_dumps(value) if quote_strings else value
    return json_dumps(value)


def _format_mapping(prefix: str, value: Any, *, quote_strings: bool = False) -> str:
    if not isinstance(value, dict):
        return f"{prefix} {_format_scalar(value, quote_strings=quote_strings)}"
    items = []
    for key, item in value.items():
        items.append(
            key
            if item is True
            else f"{key}={_format_scalar(item, quote_strings=quote_strings)}"
        )
    return prefix if not items else f"{prefix} {' '.join(items)}"


def _annotations(
    node: SchemaNode,
    *,
    compressed_homogeneous: bool = False,
) -> list[str]:
    tags = []
    if compressed_homogeneous or node.kind == "homogeneous":
        tags.append("homogeneous")
    elif node.kind == "numbered":
        tags.append("numbered")
    sem = node.schema.get("sem")
    if sem is not None:
        tags.append(_format_mapping("sem", sem))
    if node.edge is not None:
        tags.append(_format_mapping("edge", node.edge, quote_strings=True))
    if node.node is not None:
        tags.append(_format_mapping("node", node.node, quote_strings=True))
    return [f"[{tag}]" for tag in tags]


def format_schema_label(
    node: SchemaNode,
    *,
    name: str | None = None,
    compressed_homogeneous: bool = False,
) -> str:
    label = name if name is not None else (_segment(node.path) if node.path else "")
    tags = _annotations(node, compressed_homogeneous=compressed_homogeneous)
    return " ".join([label, *tags]).strip()


def format_value_label(
    node: SchemaNode,
    *,
    name: str | None = None,
    present: bool = False,
    value: Any = None,
) -> str:
    label = name if name is not None else (_segment(node.path) if node.path else "")
    if node.kind == "leaf":
        return f"{label} = {json_dumps(value) if present else '<absent>'}"
    return label


def _tree_lines(
    root_line: str | None,
    children: list[tuple[str, Callable[[str], list[str]]]],
) -> list[str]:
    lines = [] if root_line is None else [root_line]
    for index, (label, descend) in enumerate(children):
        last = index + 1 == len(children)
        branch = "└─ " if last else "├─ "
        lines.append(f"{branch}{label}")
        child_prefix = "   " if last else "│  "
        for line in descend(child_prefix):
            lines.append(f"{child_prefix}{line}")
    return lines


def render_schema_tree(schema: Schema, root: str = "") -> str:
    root = schema.path(root)

    def visit(path: str, *, compress: bool) -> list[str]:
        node = schema.node(path)
        if compress and node.kind == "homogeneous":
            children = schema.children(path)
            if children:
                count = node.schema["internal"]["len"]
                child = children[0]

                return _tree_lines(
                    format_schema_label(node),
                    [
                        (
                            format_schema_label(
                                child,
                                name=f"0..{count}",
                            ),
                            lambda _prefix: visit(child.path, compress=False)[1:],
                        )
                    ],
                )

        return _tree_lines(
            format_schema_label(node),
            [
                (
                    format_schema_label(child),
                    lambda _prefix, path=child.path: visit(path, compress=True)[1:],
                )
                for child in schema.children(path)
            ],
        )

    if not root:
        lines = []
        children = schema.children("")
        for index, child in enumerate(children):
            last = index + 1 == len(children)
            branch = "└─ " if last else "├─ "
            child_lines = visit(child.path, compress=True)
            lines.append(f"{branch}{child_lines[0]}")
            prefix = "   " if last else "│  "
            for line in child_lines[1:]:
                lines.append(f"{prefix}{line}")
        return "\n".join(lines)
    return "\n".join(visit(root, compress=True))


def render_value_tree(schema: Schema, values: dict[str, Any], root: str = "") -> str:
    root = schema.path(root)

    def visit(path: str) -> list[str]:
        node = schema.node(path)
        line = format_value_label(
            node,
            present=path in values,
            value=values.get(path),
        )
        return _tree_lines(
            line,
            [
                (
                    format_value_label(
                        child,
                        present=child.path in values,
                        value=values.get(child.path),
                    ),
                    lambda _prefix, path=child.path: visit(path)[1:],
                )
                for child in schema.children(path)
            ],
        )

    if not root:
        lines = []
        children = schema.children("")
        for index, child in enumerate(children):
            last = index + 1 == len(children)
            branch = "└─ " if last else "├─ "
            child_lines = visit(child.path)
            lines.append(f"{branch}{child_lines[0]}")
            prefix = "   " if last else "│  "
            for line in child_lines[1:]:
                lines.append(f"{prefix}{line}")
        return "\n".join(lines)
    return "\n".join(visit(root))
