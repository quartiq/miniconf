"""Compose Markdown fragments while preserving headings and resolving links."""

import json
import os
import re
from urllib.parse import unquote, urlsplit, urlunsplit

from markdown_it import MarkdownIt
from markdown_it.token import Token
from mdformat import plugins
from mdformat.renderer import MDRenderer


def headings(tokens):
    used = set()
    for index, token in enumerate(tokens):
        if token.type != "heading_open":
            continue
        title = "".join(
            child.content
            for child in tokens[index + 1].children
            if child.type in ("text", "code_inline", "image")
        )
        slug = re.sub(r"[^\w -]", "", title.lower()).replace(" ", "-")
        candidate, count = slug, 0
        while candidate in used:
            count += 1
            candidate = f"{slug}-{count}"
        used.add(candidate)
        yield token, candidate


def code(value):
    return Token(
        "code_inline",
        "code",
        0,
        content=value if isinstance(value, str) else json.dumps(value),
    )


def inline(parts):
    return [
        token
        for part in parts
        for token in (
            inline(part)
            if isinstance(part, (list, tuple))
            else [
                part
                if isinstance(part, Token)
                else Token("text", "", 0, content=str(part))
            ]
        )
    ]


def link(label, target):
    return [
        Token("link_open", "a", 1, attrs={"href": target}),
        *inline([label]),
        Token("link_close", "a", -1),
    ]


def element(kind, tag, children, **attributes):
    return [
        Token(kind + "_open", tag, 1, **attributes),
        *children,
        Token(kind + "_close", tag, -1),
    ]


class Markdown:
    def __init__(self, page, targets, *, page_link=None):
        self.page = page.resolve()
        self.targets = targets
        self.page_link = page_link
        self.parser = MarkdownIt("commonmark", renderer_cls=MDRenderer)
        plugin = plugins.PARSER_EXTENSIONS["gfm"]
        self.parser.options["parser_extension"] = [plugin]
        self.parser.options["mdformat"] = {"wrap": 88}
        plugin.update_mdit(self.parser)
        self.tokens = []

    def paragraph(self, *parts):
        self.tokens.extend(
            element("paragraph", "p", [Token("inline", "", 0, children=inline(parts))])
        )

    def heading(self, level, *parts):
        self.tokens.extend(
            element(
                "heading",
                f"h{level}",
                [Token("inline", "", 0, children=inline(parts))],
                markup="#" * level,
            )
        )

    def anchor(self, name):
        self.tokens.append(Token("html_block", "", 0, content=f'<a id="{name}"></a>\n'))

    def fence(self, content, language):
        self.tokens.append(
            Token("fence", "code", 0, content=content + "\n", info=language)
        )

    def table(self, headers, rows):
        sections = []
        for tag, cells in [("thead", [headers]), ("tbody", rows)]:
            body = []
            for row in cells:
                columns = []
                for cell in row:
                    kind = "th" if tag == "thead" else "td"
                    columns.extend(
                        element(
                            kind,
                            kind,
                            [Token("inline", "", 0, children=inline([cell]))],
                        )
                    )
                body.extend(element("tr", "tr", columns))
            sections.extend(element(tag, tag, body))
        self.tokens.extend(element("table", "table", sections))

    def prose(self, text, source, prefix, level):
        # Parse each fragment with its own reference-label environment.
        tokens = self.parser.parse(text)
        ids = {id(token): prefix + "-" + slug for token, slug in headings(tokens)}
        anchored = []
        for token in tokens:
            if id(token) in ids:
                marker = Token("html_block", "", 0)
                marker.content = f'<a id="{ids[id(token)]}"></a>\n'
                anchored.append(marker)
            if token.type in ("heading_open", "heading_close"):
                token.tag = f"h{min(6, int(token.tag[1:]) + level - 1)}"
                token.markup = "#" * int(token.tag[1:])
            anchored.append(token)

        def links(tokens):
            for token in tokens:
                for attr in ("href", "src"):
                    value = token.attrGet(attr)
                    if value is None:
                        continue
                    url = urlsplit(value)
                    if url.scheme or url.netloc or url.path.startswith("/"):
                        continue
                    target = (
                        (source.parent / unquote(url.path)).resolve()
                        if url.path
                        else source
                    )
                    key = (target, unquote(url.fragment))
                    if key in self.targets:
                        token.attrSet(attr, "#" + self.targets[key])
                    elif (target, "") in self.targets:
                        raise ValueError(
                            f"{source}: unknown explanation heading in {value!r}"
                        )
                    elif not url.path:
                        if url.fragment:
                            target_id = prefix + "-" + unquote(url.fragment)
                            if target_id not in ids.values():
                                raise ValueError(
                                    f"{source}: unknown explanation heading in {value!r}"
                                )
                            token.attrSet(attr, "#" + target_id)
                    else:
                        if target.suffix == ".md" and self.page_link is not None:
                            target = self.page_link(target)
                        path = os.path.relpath(target, self.page.parent)
                        token.attrSet(
                            attr, urlunsplit(("", "", path, url.query, url.fragment))
                        )
                links(token.children or [])

        links(tokens)
        self.tokens.extend(anchored)

    def render(self):
        return self.parser.renderer.render(self.tokens, self.parser.options, {})
