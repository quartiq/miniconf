"""Check local links and anchors in the rendered settings reference."""

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


class Page(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.ids = set()
        self.duplicates = set()
        self.links = []
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        identifier = attrs.get("id", attrs.get("name") if tag == "a" else None)
        if identifier is not None:
            if identifier in self.ids:
                self.duplicates.add(identifier)
            self.ids.add(identifier)
        for attr in ("href", "src"):
            if attr in attrs:
                self.links.append(attrs[attr])


def check(root: Path):
    pages = {}
    problems = []
    for source in sorted(root.rglob("*.html")):
        source = source.resolve()
        page = pages.setdefault(source, Page(source.read_text()))
        problems.extend(
            f"{source}: duplicate anchor {name}" for name in sorted(page.duplicates)
        )
        for link in page.links:
            url = urlsplit(link)
            if url.scheme or url.netloc or url.path.startswith("/"):
                continue
            target = (
                (source.parent / unquote(url.path)).resolve() if url.path else source
            )
            if not target.is_file():
                problems.append(f"{source}: missing target {link}")
            elif url.fragment and target.suffix == ".html":
                if target not in pages:
                    pages[target] = Page(target.read_text())
                if unquote(url.fragment) not in pages[target].ids:
                    problems.append(f"{source}: missing anchor {link}")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "directory", type=Path, help="directory of rendered reference HTML"
    )
    args = parser.parse_args()
    if not args.directory.is_dir():
        parser.error("expected an HTML directory")
    problems = check(args.directory)
    if problems:
        parser.exit(1, "\n".join(problems) + "\n")


if __name__ == "__main__":
    main()
