"""Async CLI frontend for the Miniconf MQTT client."""

from __future__ import annotations

import asyncio
import argparse
import json
import logging
import math
import os
import sys
from contextlib import AsyncExitStack

from aiomqtt import MqttError

from .client import Miniconf, RawMiniconf
from .mqtt import json_dumps, mqtt_client
from .errors import MiniconfException
from .paths import validate_path
from .operations import discover_one, force_prune, prune
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
    if sys.platform.lower() == "win32" or os.name.lower() == "nt":
        from asyncio import WindowsSelectorEventLoopPolicy, set_event_loop_policy

        set_event_loop_policy(WindowsSelectorEventLoopPolicy())
    try:
        asyncio.run(_main())
    except* KeyboardInterrupt:
        pass
    except* (
        MiniconfException,
        MqttError,
        TimeoutError,
        json.JSONDecodeError,
    ) as errors:
        _print_errors(errors)
        sys.exit(1)


def _print_errors(error: BaseException) -> None:
    if isinstance(error, BaseExceptionGroup):
        for child in error.exceptions:
            _print_errors(child)
    else:
        message = repr(error) if isinstance(error, MiniconfException) else str(error)
        print(f"error: {message or type(error).__name__}", file=sys.stderr)


async def _main() -> None:

    parser = _cli()
    args = parser.parse_args()
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be a finite positive number")
    if args.raw and (args.prune or args.force_prune):
        parser.error("--prune and --force-prune require schema support; omit --raw")
    if args.raw and any(
        "=" not in command and command.endswith(("?", "!")) for command in args.commands
    ):
        parser.error("schema and subtree commands require schema support; omit --raw")
    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    async with mqtt_client(args.broker) as client:
        prefix = args.prefix
        if args.discover:
            prefix = await discover_one(client, prefix, timeout=args.timeout)
        interface = (
            RawMiniconf(client, prefix) if args.raw else Miniconf(client, prefix)
        )
        async with interface, AsyncExitStack() as stack:
            events = None
            if args.subscribe:
                events = await stack.enter_async_context(
                    interface.subscribe(timeout=args.timeout)
                )
            async with asyncio.TaskGroup() as tasks:
                if events is not None:
                    tasks.create_task(_print_settings(events))
                if args.force_prune:
                    await force_prune(interface, timeout=args.timeout)
                for path in args.prune:
                    await prune(interface, path, timeout=args.timeout)
                await _handle_commands(
                    interface,
                    args.commands,
                    args.fire_and_forget,
                    args.timeout,
                    raw=args.raw,
                )


async def _print_settings(events) -> None:
    async for event in events:
        value = json_dumps(event.value) if event.present else "<unavailable>"
        print(f"{event.path}={value}", flush=True)


def _cli() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="miniconf-mqtt",
        description="Miniconf MQTT command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples (with a target at prefix 'app/id' and id discovery):
        %(prog)s -d 'app/+' /path     # exact leaf read from one matching device
        %(prog)s -d 'app/+' /path=42  # SET; silent on success
        %(prog)s -n app/id /path=42   # SET without waiting for a device reply
        %(prog)s --subscribe app/id /path=42 # SET and stream settings until Ctrl-C
        %(prog)s app/id '/path?'     # show human-readable schema below PATH
        %(prog)s app/id '/path??'    # show machine-readable compact defs below PATH
        %(prog)s app/id '/path!'     # show human-readable values below PATH
        %(prog)s app/id '/path!!'    # dump raw /path=value values below PATH
        %(prog)s app/id '?'           # show schema from the root (empty path)
        %(prog)s app/id '/?'          # show schema below the empty-name child
        %(prog)s --raw app/id /path   # exact retained GET without schema tracking
        %(prog)s --raw -d 'app/+' /path=42 # discover one prefix, then exact SET
""",
    )
    parser.add_argument(
        "-v", "--verbose", action="count", default=0, help="Increase logging verbosity"
    )
    parser.add_argument(
        "--broker", "-b", default="mqtt", type=str, help="The MQTT broker address"
    )
    parser.add_argument(
        "--discover",
        "-d",
        action="store_true",
        help="Select exactly one matching device prefix",
    )
    parser.add_argument(
        "--raw",
        action="store_true",
        help="Read, write, and subscribe without loading the device schema",
    )
    parser.add_argument(
        "--fire-and-forget",
        "-n",
        action="store_true",
        help="Do not request an explicit ACK/NACK for SET",
    )
    parser.add_argument(
        "--subscribe",
        action="store_true",
        help="Subscribe to live settings before commands and keep printing updates until Ctrl-C",
    )
    parser.add_argument(
        "--timeout",
        "-t",
        default=3.0,
        type=float,
        help="Timeout in seconds per discovery or operation (default: 3)",
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
        help="Clear all retained topics under the resolved prefix",
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
        "The root PATH is empty; '/' addresses its empty-name child. "
        "All other PATHs are relative to the current base. "
        "Absolute subtree commands set the base to PATH; absolute leaf reads and SETs "
        "set it to PATH's parent.",
    )
    return parser


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
        if "=" in arg:
            path, value = arg.split("=", 1)
            path, base = _normalize_command_path(path, base, subtree=False)
            try:
                value = json.loads(value)
            except json.JSONDecodeError as error:
                raise MiniconfException(
                    "JSON",
                    f"{path!r}: {error.msg} (line {error.lineno}, column {error.colno})",
                ) from error
            await interface.set(
                path,
                value,
                response=not fire_and_forget,
                timeout=timeout,
            )
        elif raw and arg.endswith(("?", "!")):
            raise MiniconfException("RawMode", f"{arg} requires schema support")
        elif arg.endswith("??"):
            path = normalize(arg.removesuffix("??"))
            schema = await interface.schema(timeout=timeout)
            for definition in schema.compact(path)["defs"]:
                print(json_dumps(definition))
        elif arg.endswith("?"):
            path = normalize(arg.removesuffix("?"))
            print(render_schema_tree(await interface.schema(timeout=timeout), path))
        elif arg.endswith("!!"):
            path = normalize(arg.removesuffix("!!"))
            for dump_path, value in sorted(
                (await interface.snapshot(path, timeout=timeout)).items()
            ):
                print(f"{dump_path}={json_dumps(value)}")
        elif arg.endswith("!"):
            path = normalize(arg.removesuffix("!"))
            schema = await interface.schema(timeout=timeout)
            root = schema.path(path)
            snapshot = await interface.snapshot(root, timeout=timeout)
            print(render_value_tree(schema, snapshot, root))
        else:
            path, base = _normalize_command_path(arg, base, subtree=False)
            value = await interface.get(path, timeout=timeout)
            print(f"{path}={json_dumps(value)}")
