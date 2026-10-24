"""Discover Markdown explanations and resolve their schema targets."""

import difflib
import re
import tomllib
from dataclasses import dataclass
from itertools import combinations
from pathlib import Path

from markdown_it import MarkdownIt

from .schema import Inventory

PRECEDENCE = ("id", "typename", "path")


@dataclass
class Document:
    source: Path
    scope: str
    selector: str
    value: str
    body: str
    line: int

    @property
    def topic(self):
        return "topic-" + f"{self.scope}:{self.selector}:{self.value}".encode().hex()

    @property
    def priority(self):
        return (
            PRECEDENCE.index(self.selector),
            self.selector == "path" and "+" in self.value.split("/"),
        )

    def matches(self, entry):
        if self.selector != "path":
            return entry.addresses(self.scope).get(self.selector) == self.value
        pattern, path = self.value.split("/"), entry.path.split("/")
        return len(pattern) == len(path) and all(
            a == "+" or a == b for a, b in zip(pattern, path)
        )


class Documents:
    def __init__(self, *roots: Path):
        self.documents = []
        sources = set()
        for root in roots:
            if not root.is_dir():
                raise ValueError(f"Not a documentation directory: {root}")
            sources.update(source.resolve() for source in root.rglob("*.md"))
        for source in sorted(sources):
            text = source.read_text()
            lines = text.splitlines(keepends=True)
            markers = [
                token
                for token in MarkdownIt("commonmark").parse(text)
                if token.type == "fence"
                and token.level == 0
                and token.info.strip() == "miniconf"
            ]
            if markers and "".join(lines[: markers[0].map[0]]).strip():
                raise ValueError(
                    f"{source}: put prose after the first miniconf selector"
                )
            for index, token in enumerate(markers):
                start, end = token.map
                if end <= start + 1 or not re.fullmatch(
                    re.escape(token.markup[0]) + "{" + str(len(token.markup)) + ",}",
                    lines[end - 1].strip(),
                ):
                    raise ValueError(f"{source}:{start + 1}: unclosed miniconf fence")
                stop = (
                    markers[index + 1].map[0]
                    if index + 1 < len(markers)
                    else len(lines)
                )
                body = "".join(lines[end:stop]).strip()
                self._add(source, token.content, body, start + 1)

    def _add(self, source, header, body, line):
        try:
            meta = tomllib.loads(header)
            scope = meta.pop("scope")
            if scope not in ("node", "edge"):
                raise ValueError("scope must be 'node' or 'edge'")
            if len(meta) != 1 or next(iter(meta)) not in PRECEDENCE:
                raise ValueError(f"expected one selector: {', '.join(PRECEDENCE)}")
            selector, value = next(iter(meta.items()))
            if not isinstance(value, str):
                raise ValueError("selector must be a string")
            if selector == "path" and value and not value.startswith("/"):
                raise ValueError("paths must start with '/' (or be empty for root)")
            if selector == "path" and (
                "#" in value or any("+" in s and s != "+" for s in value.split("/"))
            ):
                raise ValueError("use '+' as a whole path segment; '#' is unsupported")
        except (ValueError, KeyError) as error:
            raise ValueError(f"{source}:{line}: {error}") from error
        self.documents.append(Document(source, scope, selector, value, body, line))

    def attach(self, inventories: dict[str, Inventory]):
        attachments = {app: {} for app in inventories}
        matched = set()
        paths = {doc.value for doc in self.documents if doc.selector == "path"}
        # Partial indices in different array dimensions can overlap away from
        # either representative path. Resolve their intersections as well, then
        # apply ordinary precedence (an exact selector may resolve the overlap).
        for first, second in combinations(sorted(paths), 2):
            a, b = first.split("/"), second.split("/")
            if len(a) == len(b) and all(x == y or "+" in (x, y) for x, y in zip(a, b)):
                paths.add("/".join(y if x == "+" else x for x, y in zip(a, b)))
        for app, inventory in inventories.items():
            entries = dict(inventory.entries)
            for path in sorted(paths):
                entries.update((entry.path, entry) for entry in inventory.select(path))
            for entry in entries.values():
                for scope in ("node", "edge"):
                    candidates = [
                        (index, doc)
                        for index, doc in enumerate(self.documents)
                        if doc.scope == scope and doc.matches(entry)
                    ]
                    if not candidates:
                        continue
                    matched.update(index for index, _ in candidates)
                    rank = min(doc.priority for _, doc in candidates)
                    winners = [doc for _, doc in candidates if doc.priority == rank]
                    if len(winners) != 1:
                        names = ", ".join(f"{doc.source}:{doc.line}" for doc in winners)
                        raise ValueError(
                            f"Ambiguous documentation in {app} at {entry.path or '(root)'}: {names}"
                        )
                    attachments[app][entry.path, scope] = winners[0]
        problems = []
        for index, doc in enumerate(self.documents):
            if index in matched:
                continue
            choices = [
                entry.path
                if doc.selector == "path"
                else entry.addresses(doc.scope).get(doc.selector, "")
                for inventory in inventories.values()
                for entry in inventory.entries.values()
            ]
            similar = difflib.get_close_matches(doc.value, choices, n=3)
            hint = f"; nearby: {', '.join(similar)}" if similar else ""
            problems.append(
                f"{doc.source}:{doc.line}: unmatched {doc.selector}={doc.value!r}{hint}"
            )
        return attachments, problems


def changes(
    before: Inventory, after: Inventory, docs: Documents | None, app: str
) -> list[str]:
    """Compare exports; source changes are review leads, not prose rewrites."""
    result = []
    for path in dict.fromkeys([*before.entries, *after.entries]):
        old, new = before.entries.get(path), after.entries.get(path)
        if old is None:
            result.append(f"Added {path or '(root)'}")
        elif new is None:
            result.append(f"Removed {path or '(root)'}")
        else:
            a, b = before.local_context(old), after.local_context(new)
            if a != b:
                fields = [
                    key
                    for key in dict.fromkeys([*a["schema"], *b["schema"]])
                    if a["schema"].get(key) != b["schema"].get(key)
                ]
                if a["defaults"] != b["defaults"]:
                    fields.append("default")
                if a["nullable"] != b["nullable"]:
                    fields.append("null value allowed")
                result.append(f"Changed {path or '(root)'}: {', '.join(fields)}")
    if docs:
        old_links, _ = docs.attach({app: before})
        new_links, _ = docs.attach({app: after})
        for doc in docs.documents:
            old = [
                before.context(before.select(path)[0], scope)
                for (path, scope), attached in old_links[app].items()
                if attached == doc
            ]
            new = [
                after.context(after.select(path)[0], scope)
                for (path, scope), attached in new_links[app].items()
                if attached == doc
            ]
            if old and not new:
                candidates = (
                    after.select(doc.value)
                    if doc.selector == "path"
                    else after.entries.values()
                )
                if not any(doc.matches(entry) for entry in candidates):
                    result.append(
                        f"{doc.source}:{doc.line}: unmatched {doc.selector}={doc.value!r}"
                    )
                    continue
            # Shared node prose describes distinct contexts, not how many uses
            # happen to select each context. Path and edge prose keep use order.
            changed = (
                any(context not in new for context in old)
                or any(context not in old for context in new)
                if doc.scope == "node" and doc.selector != "path"
                else old != new
            )
            if changed:
                result.append(
                    f"Review {doc.source}:{doc.line} ({doc.scope} {doc.selector}={doc.value!r})"
                )
    return result
