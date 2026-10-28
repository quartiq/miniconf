"""Build a Miniconf settings reference from a schema and Markdown explanations."""

import argparse
import json
import sys
from pathlib import Path

from .documents import Documents, changes
from .render import render
from .schema import Inventory


def main():
    parser = argparse.ArgumentParser(prog="miniconf-doc", description=__doc__)
    commands = parser.add_subparsers(dest="operation", required=True)
    for operation in ("build", "check", "diff"):
        command = commands.add_parser(operation)
        if operation == "build":
            command.add_argument(
                "-o", "--output", type=Path, help="Markdown output (default: stdout)"
            )
            command.add_argument(
                "--html-links",
                action="store_true",
                help="link to .html versions of Markdown pages",
            )
        command.add_argument("schema", type=Path, help="Miniconf JSON Schema export")
        if operation == "diff":
            command.add_argument("new_schema", type=Path, help="new schema export")
        command.add_argument(
            "docs",
            nargs="*",
            type=Path,
            help="directories containing Markdown explanations (searched recursively)",
        )
    args = parser.parse_args()
    try:
        inventory = Inventory(json.loads(args.schema.read_text()))
        docs = Documents(*args.docs)
        if args.operation == "diff":
            after = Inventory(json.loads(args.new_schema.read_text()))
            result = changes(inventory, after, docs, "schema")
            print("\n".join(result) or "No schema changes.")
            return
        attachments, problems = docs.attach({"schema": inventory})
        for problem in problems:
            print(problem, file=sys.stderr)
        if problems and args.operation == "check":
            parser.exit(1)
        page = getattr(args, "output", None) or Path("index.md")
        page_link = (
            (lambda target: target.with_suffix(".html"))
            if getattr(args, "html_links", False)
            else None
        )
        markdown = render(inventory, attachments["schema"], page, page_link=page_link)
        if args.operation == "build":
            if args.output:
                args.output.write_text(markdown)
            else:
                sys.stdout.write(markdown)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"miniconf-doc: {error}\n")


if __name__ == "__main__":
    main()
