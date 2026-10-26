"""Compose independently parsed Markdown fragments into a settings reference."""

import json
import re
from pathlib import Path

from markdown_it import MarkdownIt

from .layout import default_owners, sections
from .prose import Markdown, code, headings, link
from .schema import Inventory, default_rows

INTEGER_FORMAT = re.compile(r"(u?)int(8|16|32|64|128)")


def anchor(path: str) -> str:
    return "setting-" + path.encode().hex()


def metadata(entry, scope, omit=()):
    other = entry.metadata("edge" if scope == "node" else "node")
    return [
        (scope + " " if key in other else "", code(key), " = ", code(value))
        for key, value in entry.metadata(scope).items()
        if key != "doc" and key not in omit
    ]


def value_type(schema):
    """Describe a leaf's serialized value, never its children in the tree."""
    if isinstance(schema, bool):
        return "JSON value" if schema else "no allowed value"
    if "const" in schema:
        return json.dumps(schema["const"])
    if "enum" in schema:
        return " or ".join(json.dumps(value) for value in schema["enum"])
    for union in ("oneOf", "anyOf"):
        if union in schema:
            return " or ".join(value_type(branch) for branch in schema[union])
    if "prefixItems" in schema:
        return (
            "tuple ("
            + ", ".join(value_type(item) for item in schema["prefixItems"])
            + ")"
        )
    if isinstance(schema.get("items"), dict):
        length = schema.get("minItems")
        size = (
            str(length)
            if length is not None and length == schema.get("maxItems")
            else "variable length"
        )
        item = value_type(schema["items"])
        if any(key in schema["items"] for key in ("oneOf", "anyOf", "enum")):
            item = f"({item})"
        return f"array ({size}) of {item}"
    if "properties" in schema:
        return (
            "object {\n"
            + "\n".join(
                f"  {name or json.dumps(name)}"
                + (" (optional)" if name not in schema.get("required", []) else "")
                + ": "
                + value_type(child).replace("\n", "\n  ")
                for name, child in schema["properties"].items()
            )
            + "\n}"
        )
    if isinstance(schema.get("additionalProperties"), dict):
        return "map of string keys to " + value_type(schema["additionalProperties"])
    representation = schema.get("format", "")
    match = INTEGER_FORMAT.fullmatch(representation)
    if match:
        return f"{match[2]}-bit {'unsigned' if match[1] else 'signed'} integer"
    if representation in ("float", "double"):
        return f"{32 if representation == 'float' else 64}-bit floating-point number"
    kind = schema.get("type", "JSON value")
    return " or ".join(kind) if isinstance(kind, list) else kind


def render(
    inventory: Inventory, attachments=None, page=Path("index.md"), *, page_link=None
) -> str:
    """Compose a reference; page_link optionally maps external Markdown destinations."""
    attachments = attachments or {}
    targets = {}
    parser = MarkdownIt("commonmark")
    documents = {doc.topic: doc for doc in attachments.values()}
    for doc in sorted(documents.values(), key=lambda doc: (doc.source, doc.line)):
        targets.setdefault((doc.source, ""), doc.topic)
        for _, slug in headings(parser.parse(doc.body)):
            key = (doc.source, slug)
            if key in targets:
                raise ValueError(
                    f"{doc.source}:{doc.line}: duplicate explanation heading {slug!r}"
                )
            targets[key] = doc.topic + "-" + slug
    output = Markdown(page, targets, page_link=page_link)
    name = inventory.document.get("title")
    output.heading(1, f"{name + ' settings' if name else 'Settings'} reference")
    emitted = set()
    section = sections(inventory, attachments)
    owners = default_owners(inventory, section)
    concrete = {
        path: inventory.select(path)[0]
        for path, _ in attachments
        if path not in inventory.entries
    }

    def title(entry):
        return entry.metadata("node").get("typename") or entry.path or "Root"

    def explanation(entry, scope, level):
        text = entry.metadata(scope).get("doc")
        if text and entry.template is None:
            output.prose(text, page, anchor(entry.path) + scope, level)
        doc = attachments.get((entry.path, scope))
        if doc:
            if doc.topic in emitted:
                output.paragraph(link("Further explanation", "#" + doc.topic), ".")
            else:
                emitted.add(doc.topic)
                output.anchor(doc.topic)
                output.prose(doc.body, doc.source, doc.topic, level)

    def defaults(entries):
        values = {}
        for entry in entries:
            if entry.path not in owners:
                continue
            for path, value in entry.defaults.items():
                text = json.dumps(value, ensure_ascii=False)
                values.setdefault(text, (entry, value, []))[2].append(path)
        for text, (entry, value, paths) in values.items():
            rows = list(default_rows(inventory, entry, value))
            label = ["Default"]
            if len(values) > 1:
                label += [
                    " at ",
                    *[
                        (", " if i else "", code(path) if path else "Root")
                        for i, path in enumerate(paths)
                    ],
                ]
            if len(rows) == 1 and not rows[0][0] and rows[0][3]:
                output.paragraph(label, ": ", rows[0][3], ".")
            elif entry.kind == "leaf":
                if len(text) > 88:
                    output.paragraph(label, ":")
                    output.fence(
                        json.dumps(value, indent=2, ensure_ascii=False), "json"
                    )
                else:
                    output.paragraph(label, ": ", code(text), ".")
            else:
                if len(values) > 1:
                    output.paragraph(label, ":")
                output.table(
                    ["Setting", "Default"],
                    [
                        [
                            link(
                                code(path) if path else "This node",
                                "#" + anchor(target.path),
                            ),
                            state or code(json.dumps(default, ensure_ascii=False)),
                        ]
                        for path, target, default, state in rows
                    ],
                )

    def node(entry, level, standalone=False):
        summary = []
        if entry.kind == "homogeneous":
            summary.append(f"{entry.schema['maxItems']} items")
        if entry.kind == "numbered":
            summary.append(f"{len(entry.schema['prefixItems'])} children")
        if entry.schema.get("tree-oneof"):
            summary.append("mutually exclusive children")
        if entry.schema.get("tree-maybe-absent"):
            summary.append("may be absent")
        if entry.nullable and (not standalone or not entry.path):
            summary.append("null value allowed")
        if entry.kind == "leaf":
            value = value_type(entry.schema)
            if "enum" in entry.schema:
                summary.append(
                    [
                        (", " if i else "", code(json.dumps(item)))
                        for i, item in enumerate(entry.schema["enum"])
                    ],
                )
            elif "\n" not in value and len(value) <= 88:
                structured = any(
                    key in entry.schema
                    for key in (
                        "items",
                        "prefixItems",
                        "properties",
                        "additionalProperties",
                    )
                )
                summary.append(("leaf: " if structured else "", code(value)))
        summary += metadata(entry, "node", ("typename",) if standalone else ())
        if not standalone or not entry.path:
            summary += metadata(entry, "edge")
        if summary:
            output.paragraph(
                [(" · " if i else "", part) for i, part in enumerate(summary)]
            )
        if (
            entry.kind == "leaf"
            and "enum" not in entry.schema
            and ("\n" in value or len(value) > 88)
        ):
            output.paragraph("Leaf value:")
            output.fence(value, "text")
        if not standalone or not entry.path:
            explanation(entry, "edge", level)
        explanation(entry, "node", level)
        if entry.schema.get("description"):
            output.prose(
                entry.schema["description"],
                page,
                anchor(entry.path) + "description",
                level,
            )
        if entry.kind == "leaf":
            integer = INTEGER_FORMAT.fullmatch(entry.schema.get("format", ""))
            bounds = {}
            if integer:
                signed = not integer[1]
                bits = int(integer[2]) - signed
                bounds = {
                    "minimum": -(1 << bits) if signed else 0,
                    "maximum": (1 << bits) - 1,
                }
            for key, label in (
                ("minimum", "Minimum"),
                ("maximum", "Maximum"),
                ("exclusiveMinimum", "Greater than"),
                ("exclusiveMaximum", "Less than"),
                ("minLength", "Minimum string length"),
                ("maxLength", "Maximum string length"),
                ("pattern", "String pattern"),
            ):
                if key in entry.schema and entry.schema[key] != bounds.get(key):
                    output.paragraph(label, ": ", code(entry.schema[key]), ".")

    pending = [section[""]]
    visited = set()

    def fields(entry, instances, level):
        for child in inventory.children(entry.path):
            suffix = child.path[len(entry.path) :]
            uses = [inventory.entries[instance.path + suffix] for instance in instances]
            group = section.get(child.path)
            if group:
                pending.append(group)
            for use in uses:
                output.anchor("edge-" + use.path.encode().hex())
                if not group:
                    output.anchor(anchor(use.path))
            # Beyond h6, show ancestry relative to the preceding h5.
            name = "/".join(child.path.split("/")[-max(1, level - 5) :])
            output.heading(min(level, 6), code(name or '""'))
            if group:
                output.paragraph(
                    "Type: ",
                    link(code(title(group[0])), "#" + anchor(group[0].path)),
                    " · null value allowed" if child.nullable else "",
                    [(" · ", part) for part in metadata(child, "edge")],
                    ".",
                )
                explanation(child, "edge", min(level + 1, 6))
            else:
                node(child, min(level + 1, 6))
            defaults(uses)
            use_paths = {use.path for use in uses}
            for instance in concrete.values():
                if instance.template not in use_paths:
                    continue
                for scope in ("node", "edge"):
                    doc = attachments.get((instance.path, scope))
                    if doc and doc != attachments.get((instance.template, scope)):
                        output.paragraph("At ", code(instance.path), ":")
                        explanation(instance, scope, min(level + 1, 6))
            if not group:
                fields(child, uses, level + 1)

    while pending:
        instances = pending.pop(0)
        entry = instances[0]
        if entry.path in visited:
            continue
        visited.add(entry.path)
        for instance in instances:
            output.anchor(anchor(instance.path))
        output.heading(2, code(title(entry)))
        if entry.path:
            output.paragraph(
                "Used by: ",
                [
                    (
                        ", " if i else "",
                        link(
                            code(instance.path), "#edge-" + instance.path.encode().hex()
                        ),
                    )
                    for i, instance in enumerate(instances)
                ],
                ".",
            )
        node(entry, 3, standalone=True)
        if not entry.path:
            defaults(instances)
        fields(entry, instances, 3)
    return output.render()
