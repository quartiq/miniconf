"""Async CLI frontend for the Miniconf MQTT client."""

from __future__ import annotations

import asyncio
import argparse
import json
import logging
import os
import sys

from aiomqtt import Client

from .client import Miniconf, RawMiniconf
from .common import LOGGER, MQTTv5, MiniconfException, json_dumps, validate_path
from ._ops import discover, force_prune, prune
from .render import render_schema_tree, render_value_tree


def _parent_path(path: str) -> str:
    """Return the containing path for one absolute Miniconf path."""

    if not path:
        return ""
    parent = path.rsplit("/", 1)[0]
    return parent if parent else ""


def _normalize_command_path(
    path: str, base: str, *, subtree: bool = True
) -> tuple[str, str]:
    """Normalize one CLI path.

    Relative paths are resolved against the current base without changing it. Absolute paths update
    the base: subtree commands anchor at the path itself, exact leaf commands anchor at the parent.
    The empty path stays the tree root.
    """

    if not path:
        return "", base
    if path[0] == "/":
        path = validate_path(path)
        return path, path if subtree else _parent_path(path)
    return validate_path(f"{base}/{path}"), base


def main() -> None:
    asyncio.run(_main())


async def _main() -> None:
    if sys.platform.lower() == "win32" or os.name.lower() == "nt":
        from asyncio import WindowsSelectorEventLoopPolicy, set_event_loop_policy

        set_event_loop_policy(WindowsSelectorEventLoopPolicy())

    args = _cli().parse_args()
    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    async with Client(args.broker, protocol=MQTTv5) as client:
        prefix = await _resolve_prefix(client, args.prefix, args.discover)
        if args.raw and (args.prune or args.force_prune):
            raise MiniconfException(
                "RawMode", "--prune and --force-prune require tracked mode"
            )
        interface = (
            RawMiniconf(client, prefix) if args.raw else Miniconf(client, prefix)
        )
        async with interface:
            if args.force_prune:
                for topic in await force_prune(interface, timeout=args.timeout):
                    print(topic)
            for path in args.prune:
                pages, stale = await prune(interface, path, timeout=args.timeout)
                for page in pages:
                    print(f"schema/{page}")
                for stale in stale:
                    print(stale)
            await _handle_commands(
                interface,
                args.commands,
                args.fire_and_forget,
                args.timeout,
                raw=args.raw,
            )


def _cli() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Miniconf MQTT command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples (with a target at prefix 'app/id' and id discovery):
        %(prog)s -d app/+ /path       # exact leaf read
        %(prog)s -d app/+ /path=value # SET with explicit ACK/NACK
        %(prog)s -n -d app/+ /path=value # fire-and-forget SET
        %(prog)s -d app/+ /path?      # show human-readable schema below PATH
        %(prog)s -d app/+ /path??     # show machine-readable compact defs below PATH
        %(prog)s -d app/+ /path!      # show human-readable values below PATH
        %(prog)s -d app/+ /path!!     # dump raw /path=value values below PATH
        %(prog)s --raw app/id /path   # exact retained GET without schema tracking
        %(prog)s --raw -d app/+ /path=value # discover one prefix, then exact SET
""",
    )
    parser.add_argument(
        "-v", "--verbose", action="count", default=0, help="Increase logging verbosity"
    )
    parser.add_argument(
        "--broker", "-b", default="mqtt", type=str, help="The MQTT broker address"
    )
    parser.add_argument(
        "--discover", "-d", action="store_true", help="Detect device prefix"
    )
    parser.add_argument(
        "--raw",
        action="store_true",
        help="Use exact-path GET/SET only, without schema or tracked retained-state caching",
    )
    parser.add_argument(
        "--fire-and-forget",
        "-n",
        action="store_true",
        help="Do not request an explicit ACK/NACK for SET",
    )
    parser.add_argument(
        "--timeout",
        "-t",
        default=3.0,
        type=float,
        help="Timeout in seconds for explicit replies, exact reads, or subtree snapshots",
    )
    parser.add_argument(
        "--prune",
        action="append",
        default=[],
        metavar="PATH",
        help="Clear stale retained schema pages and retained settings below PATH",
    )
    parser.add_argument(
        "--force-prune",
        action="store_true",
        help="Clear all retained Miniconf MQTT topics below the resolved prefix",
    )
    parser.add_argument(
        "prefix",
        type=str,
        help="The MQTT topic prefix of the target or a prefix filter for discovery",
    )
    parser.add_argument(
        "commands",
        metavar="CMD",
        nargs="*",
        help="Path to exact leaf read ('PATH') or path and JSON encoded value to set "
        "('PATH=VALUE') or path to show schema ('PATH?' or 'PATH??') or path "
        "to show or dump retained subtree values ('PATH!' or 'PATH!!'). "
        "Use sufficient shell quoting/escaping. "
        "Absolute PATHs are empty or start with a '/'. "
        "All other PATHs are relative to the current base. "
        "Absolute subtree commands set the base to PATH; absolute leaf reads and SETs "
        "set it to PATH's parent.",
    )
    return parser


async def _resolve_prefix(client: Client, prefix: str, discover_enabled: bool) -> str:
    if not discover_enabled:
        return prefix
    devices = await discover(client, prefix)
    try:
        ((prefix, alive),) = devices.items()
    except ValueError as exc:
        raise MiniconfException(
            "Discover", f"No unique Miniconf device (found `{devices}`)."
        ) from exc
    LOGGER.info("Found device: %s", (prefix, alive))
    return prefix


async def _handle_commands(
    interface,
    commands: list[str],
    fire_and_forget: bool,
    timeout: float,
    *,
    raw: bool,
) -> None:
    base = ""

    def normalize(path: str) -> str:
        nonlocal base
        path, base = _normalize_command_path(path, base)
        return path

    for arg in commands:
        try:
            if raw and arg.endswith(("??", "?", "!!", "!")):
                raise MiniconfException("RawMode", f"{arg} requires tracked mode")
            if arg.endswith("??"):
                path = normalize(arg.removesuffix("??"))
                schema = await interface.schema(timeout=timeout)
                for definition in schema.compact(path)["defs"]:
                    print(json_dumps(definition))
            elif arg.endswith("?"):
                path = normalize(arg.removesuffix("?"))
                print(render_schema_tree(await interface.schema(timeout=timeout), path))
            elif arg.endswith("!!"):
                path = normalize(arg.removesuffix("!!"))
                async with interface.track(path, timeout=timeout) as tracked:
                    for dump_path, value in sorted(tracked.snapshot().items()):
                        print(f"{dump_path}={json_dumps(value)}")
            elif arg.endswith("!"):
                path = normalize(arg.removesuffix("!"))
                schema = await interface.schema(timeout=timeout)
                async with interface.track(path, timeout=timeout) as tracked:
                    print(render_value_tree(schema, tracked.snapshot(), tracked.root))
            elif "=" in arg:
                path, value = arg.split("=", 1)
                path, base = _normalize_command_path(path, base, subtree=False)
                await interface.set(
                    path,
                    json.loads(value),
                    response=not fire_and_forget,
                    timeout=timeout,
                )
                print(f"{path}={value}")
            else:
                path, base = _normalize_command_path(arg, base, subtree=False)
                value = await interface.get(path, timeout=timeout)
                print(f"{path}={json_dumps(value)}")
        except (MiniconfException, TimeoutError, json.JSONDecodeError) as err:
            print(f"{arg}: {err!r}")
            sys.exit(1)
