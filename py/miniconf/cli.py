"""Async CLI frontend for the MM2 Python client."""

from __future__ import annotations

import asyncio
import argparse
import json
import logging
import os
import sys

from aiomqtt import Client

from .async_ import MiniconfClient, discover, dump, force_prune, prune, read
from .common import LOGGER, MQTTv5, MiniconfException, json_dumps, validate_path
from .render import render_schema_tree, render_value_tree


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
        interface = MiniconfClient(client, prefix)
        try:
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
                interface, args.commands, args.fire_and_forget, args.timeout
            )
        finally:
            await interface.close()


def _cli() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Miniconf MM2 command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples (with a target at prefix 'app/id' and id discovery):
%(prog)s -d app/+ /path       # cached read from retained settings mirror
%(prog)s -d app/+ /path=value # SET with explicit ACK/NACK
%(prog)s -n -d app/+ /path=value # fire-and-forget SET
%(prog)s -d app/+ /path?      # show human-readable schema below PATH
%(prog)s -d app/+ /path??     # show machine-readable compact defs below PATH
%(prog)s -d app/+ /path!      # show human-readable values below PATH
%(prog)s -d app/+ /path!!     # dump raw /path=value values below PATH
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
        help="Timeout in seconds for explicit replies or cached reads",
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
        help="Clear all retained MM2 topics below the resolved prefix",
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
        help="Path to cached-read ('PATH') or path and JSON encoded value to set "
        "('PATH=VALUE') or path to show schema ('PATH?' or 'PATH??') or path "
        "to dump retained settings ('PATH!' or 'PATH!!'). "
        "Use sufficient shell quoting/escaping. "
        "Absolute PATHs are empty or start with a '/'. "
        "All other PATHs are relative to the last absolute PATH.",
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
    interface: MiniconfClient,
    commands: list[str],
    fire_and_forget: bool,
    timeout: float,
) -> None:
    current = ""

    def normalize(path: str) -> str:
        nonlocal current
        if path and path[0] != "/":
            path = f"{current}/{path}"
        path = validate_path(path)
        current = path[: path.rfind("/")]
        return path

    for arg in commands:
        try:
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
                for dump_path, value in sorted(
                    (await dump(interface, path, timeout=timeout)).items()
                ):
                    print(f"{dump_path}={json_dumps(value)}")
            elif arg.endswith("!"):
                path = normalize(arg.removesuffix("!"))
                schema = await interface.schema(timeout=timeout)
                print(
                    render_value_tree(
                        schema,
                        await interface.snapshot(path, timeout=timeout),
                        path,
                    )
                )
            elif "=" in arg:
                path, value = arg.split("=", 1)
                path = normalize(path)
                await interface.set(
                    path,
                    json.loads(value),
                    response=not fire_and_forget,
                    timeout=timeout,
                )
                print(f"{path}={value}")
            else:
                path = normalize(arg)
                print(
                    f"{path}={json_dumps(await read(interface, path, timeout=timeout))}"
                )
        except (MiniconfException, TimeoutError, json.JSONDecodeError) as err:
            print(f"{arg}: {err!r}")
            sys.exit(1)
