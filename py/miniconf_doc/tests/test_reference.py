import copy
import json
import subprocess
import sys
from html import unescape
from pathlib import Path

import pytest
from markdown_it import MarkdownIt

from miniconf_doc.documents import Documents, changes
from miniconf_doc.links import Page, check
from miniconf_doc.prose import Markdown
from miniconf_doc.render import render
from miniconf_doc.schema import Inventory


@pytest.fixture
def schema():
    return {
        "tree-kind": "named",
        "title": "Example",
        "type": "object",
        "properties": {
            "channels": {
                "tree-kind": "homogeneous",
                "type": "array",
                "minItems": 2,
                "maxItems": 2,
                "items": {"$ref": "#/$defs/Channel"},
            }
        },
        "$defs": {
            "Channel": {
                "tree-kind": "named",
                "type": "object",
                "tree-node-meta": {"typename": "Channel", "doc": "Channel docs."},
                "properties": {
                    "gain": {
                        "type": "number",
                        "tree-kind": "leaf",
                        "tree-edge-meta": {
                            "doc": "Gain docs.",
                            "unit": "V",
                            "id": "gain",
                            "ty": "voltage",
                        },
                        "tree-node-meta": {"doc": "Number docs."},
                    },
                    "coefficients": {
                        "type": "array",
                        "items": {"type": "number"},
                        "tree-kind": "leaf",
                    },
                },
            }
        },
        "default": {
            "channels": [
                {"gain": 1, "coefficients": [1, 2]},
                {"gain": 2, "coefficients": [3, 4]},
            ]
        },
    }


def document(
    root,
    selector="typename",
    value="Channel.gain",
    name="gain.md",
    scope="edge",
    body="# Explanation\n\nLong form.",
):
    (root / name).write_text(
        f'```miniconf\nscope = "{scope}"\n{selector} = "{value}"\n```\n\n{body}\n'
    )
    return Documents(root)


def test_rust_export_contract():
    # Export checked by miniconf/tests/json_schema.rs::consumer_contract_fixture.
    export = Path(__file__).resolve().parents[3] / "miniconf/tests/common.json"
    inv = Inventory(json.loads(export.read_text()))
    docs = Documents(Path(__file__).parents[1] / "examples/docs")
    links, problems = docs.attach({"common": inv})
    assert not problems
    text = render(inv, links["common"])
    page = Page(MarkdownIt().enable("table").render(text))
    assert page.links
    assert all(link[1:] in page.ids for link in page.links if link.startswith("#"))


def test_inventory_and_complete_reference(schema):
    inv = Inventory(schema)
    assert list(inv.entries) == [
        "",
        "/channels",
        "/channels/{index}",
        "/channels/{index}/gain",
        "/channels/{index}/coefficients",
    ]
    assert inv.entries["/channels/{index}/gain"].defaults == {
        "/channels/0/gain": 1,
        "/channels/1/gain": 2,
    }
    text = render(inv)
    for value in (
        "Channel docs.",
        "Gain docs.",
        "Number docs.",
        "unit",
        "V",
        "/channels/1",
    ):
        assert value in text
    assert "unavailable" not in text


def test_definition_renumbering_does_not_trigger_review(schema):
    before = Inventory(copy.deepcopy(schema))
    schema["$defs"]["renumbered"] = schema["$defs"].pop("Channel")
    schema["properties"]["channels"]["items"]["$ref"] = "#/$defs/renumbered"
    assert changes(before, Inventory(schema), None, "example") == []


def test_generated_markdown_preserves_literal_content(tmp_path):
    from miniconf_doc.prose import code

    output = Markdown(tmp_path / "index.md", {})
    output.heading(1, "[Title] *literal*")
    output.paragraph(code(" gain "), " ", code("`offset`"))
    output.table(["Setting", "Default"], [[code("a|b"), code("[1, 2]")]])
    tokens = MarkdownIt().enable("table").parse(output.render())
    contents = [
        child.content
        for token in tokens
        for child in (token.children or [])
        if child.type == "code_inline"
    ]
    assert contents == [" gain ", "`offset`", "a|b", "[1, 2]"]
    assert tokens[1].children[0].content == "[Title] *literal*"


def test_absent_and_inaccessible_array_elements_are_states():
    schema = {
        "tree-kind": "homogeneous",
        "maxItems": 2,
        "items": {"tree-kind": "leaf", "type": "number", "tree-maybe-absent": True},
        "default": ["__tree-absent__", "__tree-access__"],
    }
    text = render(Inventory(schema))
    assert "absent" in text and "access denied" in text
    assert "__tree-" not in text


@pytest.mark.parametrize("winner", range(4))
def test_precedence(tmp_path, schema, winner):
    choices = [
        ("id", "gain"),
        ("typename", "Channel.gain"),
        ("path", "/channels/{index}/gain"),
        ("path", "/channels/+/gain"),
    ]
    for kind, value in choices[winner:]:
        document(tmp_path, kind, value, str(choices.index((kind, value))) + ".md")
    docs = document(tmp_path, "typename", "Channel", "channel.md", "node")
    links, problems = docs.attach({"example": Inventory(schema)})
    assert not problems
    assert (
        links["example"]["/channels/{index}/gain", "edge"].source.name
        == str(winner) + ".md"
    )
    assert links["example"]["/channels/{index}", "node"].source.name == "channel.md"


def test_ambiguity_and_typo(tmp_path, schema):
    document(tmp_path, "path", "/channels/+/gain")
    docs = document(tmp_path, "path", "/+/{index}/gain", "other.md")
    with pytest.raises(ValueError, match="Ambiguous"):
        docs.attach({"example": Inventory(schema)})
    (tmp_path / "other.md").unlink()
    docs = document(tmp_path, value="Channel.gaim")
    _, errors = docs.attach({"example": Inventory(schema)})
    assert "Channel.gain" in errors[0]


def test_review_scope_and_move(tmp_path, schema):
    docs = document(tmp_path)
    before = Inventory(schema)
    moved = copy.deepcopy(schema)
    moved["properties"]["outputs"] = moved["properties"].pop("channels")
    moved["default"]["outputs"] = moved["default"].pop("channels")
    assert not any(
        line.startswith("Review")
        for line in changes(before, Inventory(moved), docs, "example")
    )
    moved["$defs"]["Channel"]["properties"]["gain"]["tree-edge-meta"]["unit"] = "A"
    assert f"Review {tmp_path / 'gain.md'}:1 (edge typename='Channel.gain')" in changes(
        before, Inventory(moved), docs, "example"
    )
    schema["$defs"]["Channel"]["properties"]["coefficients"]["maxItems"] = 3
    assert (
        f"Review {tmp_path / 'gain.md'}:1 (edge typename='Channel.gain')"
        not in changes(before, Inventory(schema), docs, "example")
    )
    docs = document(tmp_path, value="Channel", name="channel.md", scope="node")
    assert f"Review {tmp_path / 'channel.md'}:1 (node typename='Channel')" in changes(
        before, Inventory(schema), docs, "example"
    )


def test_shared_prose_reviews_contexts_not_occurrence_counts(tmp_path, schema):
    docs = document(tmp_path, value="Channel", scope="node")
    before = Inventory(copy.deepcopy(schema))
    schema["properties"]["monitor"] = {"$ref": "#/$defs/Channel"}
    after = Inventory(copy.deepcopy(schema))
    for old, new in [(before, after), (after, before)]:
        assert not any(
            line.startswith("Review") for line in changes(old, new, docs, "app")
        )
    # One use now supplies a genuinely different context with the same typename.
    schema["$defs"]["Monitor"] = copy.deepcopy(schema["$defs"]["Channel"])
    schema["$defs"]["Monitor"]["properties"]["gain"]["tree-edge-meta"]["unit"] = "A"
    schema["properties"]["monitor"]["$ref"] = "#/$defs/Monitor"
    altered = Inventory(schema)
    assert (
        sum(line.startswith("Review") for line in changes(after, altered, docs, "app"))
        == 1
    )
    # The same prose pool remains independent across applications.
    assert not any(
        line.startswith("Review") for line in changes(before, before, docs, "other")
    )


def test_diff_reports_newly_unmatched_selectors(tmp_path, schema):
    docs = document(tmp_path, value="Channel", scope="node")
    before = Inventory(copy.deepcopy(schema))
    schema["$defs"]["Channel"]["tree-node-meta"]["typename"] = "Output"
    result = changes(before, Inventory(schema), docs, "app")
    assert any("unmatched typename='Channel'" in line for line in result)
    assert not any(line.startswith("Review") for line in result)
    # A shared-pool explanation absent in both revisions is quiet.
    assert changes(Inventory(schema), Inventory(schema), docs, "other") == []


def test_nullable_arrays_refs_and_atomic_values(schema):
    channel = schema["properties"]["channels"]
    channel["items"] = {"anyOf": [channel["items"], {"const": None}]}
    schema["properties"]["channels"] = {"anyOf": [channel, {"const": None}]}
    inv = Inventory(schema)
    assert inv.entries["/channels/{index}"].metadata("node")["doc"] == "Channel docs."
    assert inv.entries["/channels/{index}/gain"].owner == "Channel.gain"
    assert "/channels/{index}/coefficients/{index}" not in inv.entries


def test_reference_labels_headings_and_relative_links(tmp_path):
    page = tmp_path / "generated" / "index.md"
    source = tmp_path / "shared" / "doc.md"
    prose = Markdown(page, {(source, ""): "topic-0", (source, "title"): "one-title"})
    prose.prose(
        "# Title\n\n[guide][g] [self](#title) [book](../usage.md)\n\n[g]: https://first.example",
        source,
        "one",
        3,
    )
    prose.prose("[guide][g]\n\n[g]: https://second.example", source, "two", 3)
    prose.prose("[shared](../shared/doc.md)", page, "three", 3)
    text = prose.render()
    assert "[guide](https://first.example)" in text
    assert "[guide](https://second.example)" in text
    assert '<a id="one-title"></a>' in text
    assert "### Title" in text
    assert "[self](#one-title)" in text
    assert "[book](../usage.md)" in text
    assert "[shared](#topic-0)" in text

    html = Markdown(page, {}, page_link=lambda target: target.with_suffix(".html"))
    html.prose("[book](../usage.md?q=1#start) ![image](plot.svg)", source, "one", 3)
    assert "[book](../usage.html?q=1#start)" in html.render()
    assert "![image](../shared/plot.svg)" in html.render()


def test_cli_preview_orphan_check_and_diff(tmp_path, schema):
    document(tmp_path, value="Channel.missing")
    source = tmp_path / "schema.json"
    source.write_text(json.dumps(schema))

    def run(operation, *extra):
        return subprocess.run(
            [
                sys.executable,
                "-m",
                "miniconf_doc",
                operation,
                str(source),
                *extra,
                str(tmp_path),
            ],
            text=True,
            capture_output=True,
        )

    assert run("build").returncode == 0
    assert run("check").returncode == 1
    assert run("diff", str(source)).stdout == "No schema changes.\n"


def test_recursive_schema_rejected():
    with pytest.raises(ValueError, match="Recursive"):
        Inventory(
            {
                "$ref": "#/$defs/node",
                "$defs": {
                    "node": {
                        "tree-kind": "named",
                        "properties": {"next": {"$ref": "#/$defs/node"}},
                    }
                },
            }
        )


@pytest.mark.parametrize("value", [7, "text", None, [1, 2], {"gain": 3}])
def test_atomic_root_values(value):
    inv = Inventory({"tree-kind": "leaf", "const": value, "default": value})
    assert list(inv.entries) == [""]
    assert json.dumps(value) in render(inv)


def test_documented_definition_shares_without_typename(tmp_path):
    inv = Inventory(
        {
            "tree-kind": "named",
            "properties": {
                "a": {"$ref": "#/$defs/config"},
                "b": {"$ref": "#/$defs/config"},
            },
            "$defs": {
                "config": {
                    "tree-kind": "named",
                    "tree-node-meta": {"doc": "Shared explanation."},
                    "properties": {"gain": {"tree-kind": "leaf", "type": "integer"}},
                }
            },
        }
    )
    docs = document(tmp_path, "path", "/a/gain", body="Application detail")
    attached, problems = docs.attach({"app": inv})
    assert not problems
    assert inv.entries["/a/gain"].kind == "leaf"
    assert attached["app"]["/a/gain", "edge"].body == "Application detail"
    assert changes(inv, inv, docs, "app") == []
    # Without use-specific prose, the reusable definition is explained once.
    text = render(inv)
    assert text.count("Shared explanation.") == 1
    assert "Used by:" in text


def test_empty_names_optional_fields_and_metadata_links():
    schema = {
        "tree-kind": "named",
        "properties": {
            "": {
                "tree-kind": "leaf",
                "type": "object",
                "properties": {
                    "gain": {"type": "integer"},
                    "enabled": {"type": "boolean"},
                },
                "required": ["gain"],
                "tree-node-meta": {"doc": "# Details\n\nSee [details](#details)."},
            }
        },
    }
    text = render(Inventory(schema))
    assert '### `""`' in text
    assert "gain: integer" in text
    assert "enabled (optional): boolean" in text
    page = Page(MarkdownIt().render(text))
    assert all(link[1:] in page.ids for link in page.links if link.startswith("#"))
    schema["properties"][""]["tree-node-meta"]["doc"] = "See [missing](#missing)."
    with pytest.raises(ValueError, match="unknown explanation heading"):
        render(Inventory(schema))


def test_default_review_keeps_array_instance_identity(tmp_path, schema):
    schema["properties"]["channels"].update(minItems=3, maxItems=3)
    schema["default"]["channels"].append(
        copy.deepcopy(schema["default"]["channels"][0])
    )
    before = Inventory(schema)
    schema["default"]["channels"][2]["gain"] = 2
    docs = document(tmp_path)
    assert f"Review {tmp_path / 'gain.md'}:1 (edge typename='Channel.gain')" in changes(
        before, Inventory(schema), docs, "example"
    )


def test_ignore_ordinary_book_pages(tmp_path):
    document(tmp_path)
    (tmp_path / "ordinary.md").write_text("# Book page\n")
    assert len(Documents(tmp_path).documents) == 1


def test_miniconf_kinds_values_and_absence():
    inv = Inventory(
        {
            "tree-kind": "named",
            "properties": {
                "channels": {
                    "tree-kind": "homogeneous",
                    "type": "array",
                    "items": {"type": "boolean", "tree-kind": "leaf"},
                    "minItems": 2,
                    "maxItems": 2,
                },
                "tuple": {
                    "tree-kind": "numbered",
                    "prefixItems": [{"tree-kind": "leaf", "type": "string"}],
                },
                "coefficients": {
                    "tree-kind": "leaf",
                    "type": "array",
                    "items": {"type": "number", "format": "float"},
                    "minItems": 5,
                    "maxItems": 5,
                },
                "variant": {
                    "tree-kind": "named",
                    "tree-oneof": True,
                    "properties": {
                        "A": {"tree-kind": "leaf", "type": "integer"},
                        "B": {"tree-kind": "leaf", "type": "integer"},
                    },
                },
                "optional": {
                    "tree-kind": "leaf",
                    "type": "integer",
                    "tree-maybe-absent": True,
                },
                "nullable": {
                    "anyOf": [
                        {"tree-kind": "leaf", "type": "integer"},
                        {"type": "null"},
                    ],
                },
            },
        }
    )
    assert inv.entries["/channels"].kind == "homogeneous"
    assert inv.entries["/tuple"].kind == "numbered"
    assert inv.entries["/coefficients"].kind == "leaf"
    assert inv.entries["/variant"].kind == "named"
    assert not inv.entries["/optional"].nullable
    assert inv.entries["/nullable"].nullable
    text = render(inv)
    assert "2 items" in text
    assert "\nnamed\n" not in text
    assert "leaf: `boolean`" not in text
    assert "leaf: `array (5)" in text
    assert "array (5) of 32-bit floating-point number" in text
    assert "mutually exclusive children" in text
    assert "may be absent" in text
    for keyword in ("minItems", "maxItems", "required", "format", "tree-nullable"):
        assert keyword not in text


def test_internal_default_presence_changes_are_reported():
    schema = {
        "anyOf": [
            {"tree-kind": "named", "type": "object", "properties": {}},
            {"const": None},
        ],
        "default": None,
    }
    before = Inventory(schema)
    schema["default"] = {}
    assert changes(before, Inventory(schema), None, "example") == [
        "Changed (root): default"
    ]


def test_default_presence_stops_at_unavailable_parent(schema):
    schema["default"]["channels"] = [
        {"gain": 1},
        "__tree-access__",
    ]
    inv = Inventory(schema)
    assert inv.entries["/channels/{index}/coefficients"].defaults == {
        "/channels/0/coefficients": "__tree-absent__"
    }
    text = render(inv)
    assert "access denied" in text and "absent" in text
    assert "__tree-" not in text
    del schema["default"]
    unknown = Inventory(schema)
    assert all(not entry.defaults for entry in unknown.entries.values())
    assert "Default" not in render(unknown)


def test_nullable_use_shares_definition_and_reviews_only_edge(tmp_path):
    schema = {
        "tree-kind": "named",
        "properties": {
            "first": {"$ref": "#/$defs/Config"},
            "second": {"$ref": "#/$defs/Config"},
        },
        "$defs": {
            "Config": {
                "tree-kind": "named",
                "tree-node-meta": {"typename": "Config", "doc": "Shared semantics."},
                "properties": {},
            }
        },
    }
    docs = document(tmp_path, value="Config", scope="node")
    docs = document(tmp_path, selector="path", value="/second", name="use.md")
    before = Inventory(schema)
    schema["properties"]["second"] = {
        "anyOf": [{"$ref": "#/$defs/Config"}, {"type": "null"}]
    }
    after = Inventory(schema)
    attached, _ = docs.attach({"test": after})
    text = render(after, attached["test"])
    assert text.count("Shared semantics.") == 1
    assert text.count("Type: [`Config`]") == 2
    assert text.count("null value allowed") == 1
    assert "null value allowed" not in text.split("## `Config`")[1]
    reviews = [
        line
        for line in changes(before, after, docs, "test")
        if line.startswith("Review")
    ]
    assert len(reviews) == 1 and "use.md" in reviews[0]


def test_unique_heading_ids_and_stable_topics(tmp_path, schema):
    page = tmp_path / "index.md"
    prose = Markdown(page, {})
    prose.prose("# A\n\n# A-1\n\n# A", page, "topic", 3)
    text = prose.render()
    assert not Page(MarkdownIt().render(text)).duplicates
    docs = document(tmp_path)
    before = render(
        Inventory(schema), docs.attach({"example": Inventory(schema)})[0]["example"]
    )
    docs = document(tmp_path, value="Channel", name="channel.md", scope="node")
    after = render(
        Inventory(schema), docs.attach({"example": Inventory(schema)})[0]["example"]
    )
    before_ids = {
        name
        for name in Page(MarkdownIt().render(before)).ids
        if name.startswith("topic-")
    }
    assert before_ids and before_ids <= Page(MarkdownIt().render(after)).ids


def test_rendered_link_validation(tmp_path):
    settings = tmp_path / "settings"
    settings.mkdir()
    page = settings / "index.html"
    page.write_text(
        '<h1 id="a">A</h1><h2 id="a">Again</h2><a href="missing.html">Missing</a><a href="#b">Anchor</a>'
    )
    errors = check(tmp_path)
    assert len(errors) == 3
    assert any("duplicate anchor" in error for error in errors)
    assert any("missing target" in error for error in errors)
    assert any("missing anchor" in error for error in errors)
    page.write_text('<h1 id="a">A</h1><a href="#a">OK</a>')
    assert not check(tmp_path)


def test_shared_matching_across_apps_and_orphans(tmp_path, schema):
    docs = document(tmp_path, value="Channel", scope="node")
    first = Inventory(schema)
    second_schema = copy.deepcopy(schema)
    second_schema["default"]["channels"][0]["gain"] = 99
    second = Inventory(second_schema)
    unrelated = Inventory({"tree-kind": "named", "type": "object", "properties": {}})
    links, errors = docs.attach(
        {"first": first, "second": second, "unrelated": unrelated}
    )
    assert not errors
    assert links["unrelated"] == {}
    assert (
        links["first"]["/channels/{index}", "node"]
        is links["second"]["/channels/{index}", "node"]
    )
    assert "99" in render(second, links["second"])
    assert "99" not in render(first, links["first"])
    assert len(docs.attach({"unrelated": unrelated})[1]) == 1
    local_dir = tmp_path / "local"
    local_dir.mkdir()
    local = document(local_dir, body="Only the second app")
    links, errors = docs.attach(
        {"first": first, "second": second}, local={"second": local.documents}
    )
    assert not errors
    assert ("/channels/{index}/gain", "edge") not in links["first"]
    assert (
        links["second"]["/channels/{index}/gain", "edge"].body == "Only the second app"
    )
    _, errors = docs.attach(
        {"first": first, "unrelated": unrelated},
        local={"unrelated": local.documents},
    )
    assert len(errors) == 1
    conflicting = document(local_dir, value="Channel", scope="node")
    with pytest.raises(ValueError, match="Ambiguous"):
        docs.attach({"first": first}, local={"first": conflicting.documents})


def test_explicit_directories_select_prose(tmp_path, schema):
    shared = tmp_path / "common"
    shared.mkdir()
    document(shared, value="Channel.gain", body="Shared edge")
    for name in ("first", "shared", "apps"):
        local = tmp_path / name / "nested"
        local.mkdir(parents=True)
        document(local, value="Channel", scope="node", body=name)
        docs = Documents(shared, local, shared)
        links, errors = docs.attach({"arbitrary": Inventory(schema)})
        assert not errors
        assert links["arbitrary"]["/channels/{index}", "node"].body == name
        assert len(docs.documents) == 2
    with pytest.raises(ValueError, match="Ambiguous"):
        Documents(shared, tmp_path / "first", tmp_path / "apps").attach(
            {"a": Inventory(schema)}
        )


def test_multiple_explanations_links_and_review(tmp_path, schema):
    source = tmp_path / "channel.md"
    source.write_text("""```miniconf
scope = "node"
typename = "Channel"
```
# Channel operation

[Gain](#gain-units) and [external][ref].

[ref]: https://node.example

````markdown
```miniconf
scope = "edge"
path = "/example-only"
```
````

```miniconf
scope = "edge"
typename = "Channel.gain"
```
# Gain units

[Operation](channel.md#channel-operation) and [external][ref].

[ref]: https://edge.example
""")
    docs = Documents(tmp_path)
    assert len(docs.documents) == 2
    inv = Inventory(schema)
    links, errors = docs.attach({"example": inv})
    assert not errors
    page = tmp_path / "settings/index.html"
    page.parent.mkdir()
    text = render(inv, links["example"], page)
    assert "https://node.example" in text and "https://edge.example" in text
    assert "### Channel operation" in text
    page.write_text(MarkdownIt("commonmark").render(text))
    assert not check(tmp_path)
    assert len({doc.topic for doc in docs.documents}) == 2
    changed = copy.deepcopy(schema)
    changed["$defs"]["Channel"]["properties"]["gain"]["tree-edge-meta"]["unit"] = "A"
    reviews = [
        line
        for line in changes(inv, Inventory(changed), docs, "example")
        if line.startswith("Review")
    ]
    assert len(reviews) == 2
    assert any("channel.md:1 (node" in line for line in reviews)
    assert any("(edge typename='Channel.gain')" in line for line in reviews)


@pytest.mark.parametrize(
    "text",
    [
        '```miniconf\nscope = "node"\ntypename = "Channel"',
        '# Lost introduction\n\n```miniconf\nscope = "node"\ntypename = "Channel"\n```',
    ],
)
def test_malformed_selector_sections(tmp_path, text):
    (tmp_path / "bad.md").write_text(text)
    with pytest.raises(ValueError):
        Documents(tmp_path)


def test_shared_nodes_keep_edges_defaults_and_path_specific_prose(tmp_path, schema):
    schema["properties"] = {
        name: {"$ref": "#/$defs/Channel", "tree-edge-meta": {"doc": doc}}
        for name, doc in [("output", "Main output."), ("monitor", "Monitor output.")]
    }
    schema["default"] = dict(zip(("output", "monitor"), schema["default"]["channels"]))
    inventory = Inventory(schema)
    document(
        tmp_path,
        value="Channel",
        scope="node",
        name="channel.md",
        body="Shared advice.",
    )
    docs = document(tmp_path, body="Gain advice.")
    links, problems = docs.attach({"schema": inventory})
    assert not problems
    text = render(inventory, links["schema"])
    assert text.count("Channel docs.") == 1
    assert text.count("Shared advice.") == 1
    assert text.count("Gain docs.") == 1
    assert text.count("Gain advice.") == 1
    assert "Main output." in text and "Monitor output." in text
    assert "/output" in text and "/monitor" in text
    output, monitor = text.split("### `monitor`", 1)
    assert "`1`" in output and "`2`" in monitor
    assert text.index("# Example") < text.index("## `Channel`")
    assert not Page(MarkdownIt().enable("table").render(text)).duplicates
    # An explanation selected at only one instance must not leak to another.
    document(
        tmp_path,
        "path",
        "/monitor/gain",
        scope="node",
        name="monitor.md",
        body="Monitor-only advice.",
    )
    docs = Documents(tmp_path)
    links, _ = docs.attach({"schema": inventory})
    text = render(inventory, links["schema"])
    assert text.count("Channel docs.") == 2
    assert text.count("Monitor-only advice.") == 1
    # A bare typename is a selector, not sufficient evidence to merge definitions.
    schema["$defs"]["OtherChannel"] = copy.deepcopy(schema["$defs"]["Channel"])
    schema["$defs"]["OtherChannel"]["properties"]["gain"]["maximum"] = 7
    schema["properties"]["monitor"]["$ref"] = "#/$defs/OtherChannel"
    text = render(Inventory(schema))
    assert text.count("Channel docs.") == 2
    assert "Maximum:" in text


def test_cli_accepts_schema_then_explicit_directories(tmp_path, schema):
    source = tmp_path / "arbitrary-name.json"
    source.write_text(json.dumps(schema))
    shared, local = tmp_path / "common", tmp_path / "local"
    shared.mkdir()
    local.mkdir()
    document(shared, value="Channel", scope="node", body="Shared node")
    document(local, body="Local edge")
    output = tmp_path / "reference.md"
    run = subprocess.run(
        [
            sys.executable,
            "-m",
            "miniconf_doc",
            "build",
            "--output",
            str(output),
            str(source),
            str(shared),
            str(local),
        ],
        text=True,
        capture_output=True,
    )
    assert run.returncode == 0, run.stderr
    assert "Shared node" in output.read_text() and "Local edge" in output.read_text()


def test_mqtt_paths_match_concrete_instances_and_preserve_root(tmp_path, schema):
    document(tmp_path, "path", "/channels/+/gain", name="all.md", body="All channels")
    docs = document(tmp_path, "path", "/channels/0/gain", body="First channel")
    inv = Inventory(schema)
    attached, problems = docs.attach({"test": inv})
    assert not problems
    assert attached["test"]["/channels/0/gain", "edge"].body == "First channel"
    assert attached["test"]["/channels/{index}/gain", "edge"].body == "All channels"
    text = render(inv, attached["test"])
    assert text.count("First channel") == 1
    assert text.count("Gain docs.") == 1
    assert "At `/channels/0/gain`:" in text
    assert '<a id="setting-"></a>' in text
    assert not inv.select("/channels/2/gain")
    assert not inv.select("/channels/00/gain")
    docs = document(tmp_path, "path", "/channels/2/gain", name="bad.md")
    assert any("unmatched" in problem for problem in docs.attach({"test": inv})[1])


def test_definition_identity_and_order_are_preserved():
    leaf = {"tree-kind": "leaf", "type": "integer"}
    definition = {
        "tree-kind": "named",
        "tree-node-meta": {"typename": "Config"},
        "properties": {"a": leaf, "b": leaf},
    }
    schema = {
        "tree-kind": "named",
        "properties": {
            "first": {"$ref": "#/$defs/A"},
            "second": {"$ref": "#/$defs/B"},
            "another_a": {"$ref": "#/$defs/A"},
            "another_b": {"$ref": "#/$defs/B"},
        },
        "$defs": {"A": definition, "B": copy.deepcopy(definition)},
    }
    inv = Inventory(schema)
    assert render(inv).count("## `Config`") == 2
    assert inv.entries["/first"].definition == "#/$defs/A"
    schema["$defs"]["A"]["properties"] = {"b": leaf, "a": leaf}
    assert "Changed /first: child order" in changes(
        inv, Inventory(schema), None, "test"
    )


def test_concrete_prose_under_second_use_of_shared_definition(tmp_path, schema):
    schema["properties"]["monitors"] = copy.deepcopy(schema["properties"]["channels"])
    docs = document(tmp_path, "path", "/monitors/0/gain", body="Monitor calibration.")
    inv = Inventory(schema)
    attached, problems = docs.attach({"test": inv})
    assert not problems
    # Resolving documentation must not leave hidden state on the inventory.
    text = render(Inventory(schema), attached["test"])
    assert text.count("Channel docs.") == 1
    assert text.count("Monitor calibration.") == 1
    assert "At `/monitors/0/gain`:" in text


def test_inlining_rules_preserve_values_metadata_and_documented_leaves(tmp_path):
    schema = {
        "tree-kind": "named",
        "tree-edge-meta": {"unit": "V"},
        "properties": {
            "mode": {"$ref": "#/$defs/Mode"},
            "other": {"$ref": "#/$defs/Mode"},
            "anonymous": {"tree-kind": "named", "properties": {}},
            "named": {
                "tree-kind": "named",
                "properties": {},
                "tree-node-meta": {"typename": "Config"},
            },
        },
        "$defs": {"Mode": {"tree-kind": "leaf", "enum": ["On", "Off"]}},
    }
    inv = Inventory(schema)
    text = render(inv)
    assert text.startswith("# Settings reference\n")
    assert "`unit` = `V`" in text
    assert text.count('`"On"`, `"Off"`') == 2
    # Documentation supplies a reason to give a reusable leaf its own section.
    schema["$defs"]["Mode"]["tree-node-meta"] = {
        "typename": "Mode",
        "doc": "Switches at the next frame.",
    }
    inv = Inventory(schema)
    text = render(inv)
    assert text.count("## `Mode`") == 1
    assert text.count("Switches at the next frame.") == 1
    assert text.count('`"On"`, `"Off"`') == 1
    # A selected explanation works the same way without schema doc metadata.
    del schema["$defs"]["Mode"]["tree-node-meta"]["doc"]
    docs = document(tmp_path, value="Mode", scope="node")
    inv = Inventory(schema)
    attached, _ = docs.attach({"test": inv})
    text = render(inv, attached["test"])
    assert text.count("## `Mode`") == 1
    assert text.count("Long form.") == 1


def test_reference_identity_survives_nullable_use():
    schema = {
        "tree-kind": "named",
        "properties": {
            "value": {"anyOf": [{"$ref": "#/$defs/Value"}, {"type": "null"}]}
        },
        "$defs": {"Value": {"tree-kind": "leaf", "type": "integer"}},
    }
    entry = Inventory(schema).entries["/value"]
    assert entry.definition == "#/$defs/Value"
    assert entry.nullable and entry.kind == "leaf"
    assert entry.schema["type"] == "integer"


def test_shared_parent_child_is_single_use_and_backlinks_survive_inlining():
    from miniconf_doc.render import anchor

    schema = {
        "tree-kind": "named",
        "properties": {
            "left": {"$ref": "#/$defs/Parent"},
            "right": {"$ref": "#/$defs/Parent"},
        },
        "$defs": {
            "Parent": {
                "tree-kind": "named",
                "tree-node-meta": {"typename": "Parent"},
                "properties": {"child": {"$ref": "#/$defs/Child"}},
            },
            "Child": {
                "tree-kind": "named",
                "tree-node-meta": {"typename": "Child"},
                "properties": {"value": {"tree-kind": "leaf", "type": "integer"}},
            },
        },
    }
    inv = Inventory(schema)
    text = render(inv)
    assert "## `Parent`" in text and "## `Child`" not in text
    page = Page(MarkdownIt().render(text))
    assert not page.duplicates
    assert {"#edge-2f6c656674", "#edge-2f7269676874"} <= set(page.links)
    assert all(link[1:] in page.ids for link in page.links if link.startswith("#"))
    del schema["properties"]["right"]
    inline = render(Inventory(schema))
    assert "## `Parent`" not in inline and "Used by:" not in inline
    for target in (anchor("/left"), "edge-" + "/left".encode().hex()):
        assert target in Page(MarkdownIt().render(inline)).ids
    # Independent use of the child justifies a section of its own.
    schema["properties"]["outside"] = {"$ref": "#/$defs/Child"}
    assert "## `Child`" in render(Inventory(schema))


def test_deep_inline_headings_and_defaults(schema):
    nested = {"tree-kind": "leaf", "type": "integer"}
    for name in reversed("abcdef"):
        nested = {"tree-kind": "named", "properties": {name: nested}}
    text = render(Inventory(nested))
    assert "###### `d`" in text
    assert "###### `d/e`" in text
    assert "###### `d/e/f`" in text
    # Only differing values need concrete instance labels.
    text = render(Inventory(schema))
    assert "Default at `/channels/0/gain`: `1`." in text
    schema["default"]["channels"][1]["gain"] = 1
    text = render(Inventory(schema))
    assert "Default: `1`." in text
    assert "Default at `/channels/0/gain`" not in text


@pytest.mark.parametrize(
    "format,minimum,maximum",
    [
        ("uint8", 0, 255),
        ("int8", -128, 127),
        ("uint32", 0, 2**32 - 1),
        ("int64", -(2**63), 2**63 - 1),
    ],
)
def test_integer_bounds_only_show_additional_constraints(format, minimum, maximum):
    schema = {
        "tree-kind": "leaf",
        "type": "integer",
        "format": format,
        "minimum": minimum,
        "maximum": maximum,
    }
    text = render(Inventory(schema))
    assert "Minimum:" not in text and "Maximum:" not in text
    schema.update(minimum=minimum + 1, maximum=maximum - 1)
    text = render(Inventory(schema))
    assert f"Minimum: `{minimum + 1}`." in text
    assert f"Maximum: `{maximum - 1}`." in text
    schema["exclusiveMinimum"] = minimum
    assert f"Greater than: `{minimum}`." in render(Inventory(schema))


def test_defaults_belong_to_uses_and_include_presence(tmp_path):
    schema = {
        "tree-kind": "named",
        "properties": {
            "first": {"$ref": "#/$defs/Config"},
            "second": {"$ref": "#/$defs/Config"},
            "optional": {
                "tree-kind": "named",
                "properties": {},
                "tree-maybe-absent": True,
            },
            "nullable": {
                "anyOf": [{"tree-kind": "named", "properties": {}}, {"const": None}]
            },
        },
        "$defs": {
            "Config": {
                "tree-kind": "named",
                "tree-node-meta": {"typename": "Config"},
                "properties": {"gain": {"type": "number", "tree-kind": "leaf"}},
            }
        },
        "default": {"first": {"gain": 1}, "second": {"gain": 2}, "nullable": None},
    }
    inv = Inventory(schema)
    text = render(inv)
    definition = text.index("## `Config`")
    assert text.index("`1`") < definition
    assert text.index("`2`") < definition
    assert "Default" not in text[definition:]
    assert "null subtree" in text and "absent" in text
    assert "Type: [`Config`]" in text
    assert all(
        "→" not in line and "](" not in line
        for line in text.splitlines()
        if line.startswith("#")
    )
    docs = document(tmp_path, value="Config", scope="node")
    schema["default"]["first"]["gain"] = 3
    assert not any(
        line.startswith("Review")
        for line in changes(inv, Inventory(schema), docs, "test")
    )


def test_default_tables_follow_miniconf_boundaries_and_link_to_definitions():
    from miniconf_doc.schema import default_rows

    number = {"tree-kind": "leaf", "type": "number"}
    config = {
        "tree-kind": "named",
        "tree-node-meta": {"typename": "Config"},
        "properties": {
            "clamp": {
                "tree-kind": "leaf",
                "type": "array",
                "items": {"type": "number"},
            },
            "channels": {
                "tree-kind": "homogeneous",
                "items": number,
                "minItems": 2,
                "maxItems": 2,
            },
            "pair": {"tree-kind": "numbered", "prefixItems": [number, number]},
            "object": {"tree-kind": "leaf", "type": "object"},
            "optional": {
                "tree-kind": "named",
                "properties": {},
                "tree-maybe-absent": True,
            },
            "nullable": {
                "anyOf": [{"tree-kind": "named", "properties": {}}, {"const": None}]
            },
            "empty": {"tree-kind": "named", "properties": {}},
        },
    }
    value = {
        "clamp": [1, 2],
        "channels": [1, 2],
        "pair": [1, 2],
        "object": {"message": "a|b"},
        "nullable": None,
        "empty": {},
    }
    schema = {
        "tree-kind": "named",
        "properties": {
            "first": {"$ref": "#/$defs/Config"},
            "second": {"$ref": "#/$defs/Config"},
        },
        "$defs": {"Config": config},
        "default": {"first": value, "second": value},
    }
    inv = Inventory(schema)
    rows = list(default_rows(inv, inv.entries["/first"], value))
    assert [path for path, _, _, _ in rows] == [
        "clamp",
        "channels/0",
        "channels/1",
        "pair/0",
        "pair/1",
        "object",
        "optional",
        "nullable",
        "empty",
    ]
    assert rows[0][2] == [1, 2]
    assert [row[3] for row in rows[-3:]] == [
        "absent",
        "null subtree",
        "empty subtree",
    ]
    text = render(inv)
    assert "<th>Setting</th>" in MarkdownIt().enable("table").render(text)
    assert "`clamp/0`" not in text and "`object/message`" not in text
    html = MarkdownIt().enable("table").render(text)
    assert '<code>{"message": "a|b"}</code>' in unescape(html)
    page = Page(html)
    assert not page.duplicates
    assert all(link[1:] in page.ids for link in page.links if link.startswith("#"))


def test_variant_defaults_include_selection_and_stop_at_atomic_value():
    from miniconf_doc.schema import default_rows

    inv = Inventory(
        {
            "tree-kind": "named",
            "tree-oneof": True,
            "properties": {
                "Pid": {
                    "tree-kind": "named",
                    "properties": {"clamp": {"tree-kind": "leaf"}},
                },
                "Off": {"tree-kind": "named", "properties": {}},
            },
        }
    )
    rows = list(default_rows(inv, inv.entries[""], {"Pid": {"clamp": [1, 2]}}))
    assert [(path, state or value) for path, _, value, state in rows] == [
        ("Pid", "selected variant"),
        ("Pid/clamp", [1, 2]),
    ]
    rows = list(default_rows(inv, inv.entries[""], {"Off": {}}))
    assert [(path, state or value) for path, _, value, state in rows] == [
        ("Off", "selected variant")
    ]


def test_markdown_prose_preserves_gfm_and_independent_references(tmp_path):
    text = "# Title\n\n- **bold** and ~~old~~\n\n| Name | Value |\n|---|---|\n| A | 1 |\n\n```python\nx = 1\n```\n"
    output = Markdown(tmp_path / "out.md", {})
    output.prose(text, tmp_path / "source.md", "topic", 3)
    result = output.render()
    assert "### Title" in result
    assert "**bold**" in result and "~~old~~" in result
    assert "| Name" in result and "```python" in result
    assert "<p>" not in result and "<table>" not in result
    html = MarkdownIt("commonmark").enable(["table", "strikethrough"]).render(result)
    assert "<h3>Title</h3>" in html and "<table>" in html and "<s>old</s>" in html


def test_intersecting_partial_array_patterns_respect_precedence(tmp_path):
    schema = {
        "tree-kind": "homogeneous",
        "items": {
            "tree-kind": "homogeneous",
            "items": {
                "tree-kind": "named",
                "properties": {"gain": {"tree-kind": "leaf"}},
            },
            "maxItems": 1000000,
        },
        "maxItems": 1000000,
    }
    inv = Inventory(schema)
    document(tmp_path, "path", "/0/+/gain", name="row.md", body="Row advice")
    docs = document(
        tmp_path, "path", "/+/0/gain", name="column.md", body="Column advice"
    )
    with pytest.raises(ValueError, match="at /0/0/gain"):
        docs.attach({"matrix": inv})
    docs = document(
        tmp_path,
        "path",
        "/0/0/gain",
        name="intersection.md",
        body="Intersection advice",
    )
    attached, problems = docs.attach({"matrix": inv})
    assert not problems
    assert len(inv.entries) == 4
    assert attached["matrix"]["/0/0/gain", "edge"].body == "Intersection advice"
    text = render(inv, attached["matrix"])
    for body in ("Row advice", "Column advice", "Intersection advice"):
        assert text.count(body) == 1


def test_heading_links_use_visible_text(tmp_path, schema):
    docs = document(
        tmp_path,
        value="Channel",
        scope="node",
        body=("# [Gain](https://example.org) and `offset`\n\n[Jump](#gain-and-offset)"),
    )
    inv = Inventory(schema)
    attached, _ = docs.attach({"example": inv})
    text = render(inv, attached["example"])
    page = Page(MarkdownIt().enable("table").render(text))
    assert any(target.endswith("-gain-and-offset") for target in page.ids)
    assert all(link[1:] in page.ids for link in page.links if link.startswith("#"))


def test_long_literal_representation_is_not_rewritten():
    value = "first or second " * 8
    text = render(Inventory({"tree-kind": "leaf", "const": value}))
    assert json.dumps(value) in text
