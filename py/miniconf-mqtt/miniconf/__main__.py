"""Miniconf command line interfae
"""

import asyncio
import argparse
import logging
import json
import sys
import os

from .miniconf import Miniconf, MiniconfException, Client, MQTTv5
from .discover import discover_one

if sys.platform.lower() == "win32" or os.name.lower() == "nt":
    from asyncio import set_event_loop_policy, WindowsSelectorEventLoopPolicy

    set_event_loop_policy(WindowsSelectorEventLoopPolicy())


class Path:
    def __init__(self):
        self.current = ""

    def normalize(self, path):
        if path.startswith("/") or not path:
            self.current = path[: path.rfind("/")]
        else:
            path = f"{self.current}/{path}"
        return path


def main():
    """Main program entry point."""
    parser = argparse.ArgumentParser(
        description="Miniconf command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples (with a target at prefix 'app/id' and device-discovery):
%(prog)s -d app/+ '/path'       # GET
%(prog)s -d app/+ '/path=value' # SET
%(prog)s -d app/+ '/path='      # CLEAR
%(prog)s -d app/+ '/path?'      # LIST-GET
%(prog)s -d app/+ '/path!'      # DUMP
""",
    )
    parser.add_argument(
        "-v", "--verbose", action="count", default=0, help="Increase logging verbosity"
    )
    parser.add_argument(
        "--broker", "-b", default="mqtt", type=str, help="The MQTT broker address"
    )
    parser.add_argument(
        "--retain",
        "-r",
        default=False,
        action="store_true",
        help="Retain the settings that are being set on the broker",
    )
    parser.add_argument(
        "--discover", "-d", action="store_true", help="Detect device prefix"
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
        help="Path to get ('PATH') or path and JSON encoded value to set "
        "('PATH=VALUE') or path to clear ('PATH=') or path to list ('PATH?') or "
        "path to dump ('PATH!'). "
        "Use sufficient shell quoting/escaping. "
        "Absolute PATHs are empty or start with a '/'. "
        "All other PATHs are relative to the last absolute PATH.",
    )
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    async def run():
        async with Client(
            args.broker, protocol=MQTTv5, logger=logging.getLogger("aiomqtt-client")
        ) as client:
            if args.discover:
                prefix, _alive = await discover_one(client, args.prefix)
            else:
                prefix = args.prefix

            interface = Miniconf(client, prefix)

            current = Path()
            for arg in args.commands:
                try:
                    if arg.endswith("?"):
                        path = current.normalize(arg.removesuffix("?"))
                        paths = await interface.list(path)
                        # Note: There is no way for the CLI tool to reliably
                        # distinguish a one-element leaf get responce from a
                        # one-element inner list response without looking at
                        # the payload.
                        # The only way is to note that a JSON payload of a
                        # get can not start with the / that a list response
                        # starts with.
                        if len(paths) == 1 and not paths[0].startswith("/"):
                            print(f"{path}={paths[0]}")
                            continue
                        for p in paths:
                            value = await interface.get(p)
                            print(f"{p}={value}")
                    elif arg.endswith("!"):
                        path = current.normalize(arg.removesuffix("!"))
                        await interface.dump(path)
                        print(f"DUMP '{path}'")
                    elif "=" in arg:
                        path, value = arg.split("=", 1)
                        path = current.normalize(path)
                        if not value:
                            await interface.clear(path)
                            print(f"CLEAR '{path}'")
                        else:
                            await interface.set(path, json.loads(value), args.retain)
                            print(f"{path}={value}")
                    else:
                        path = current.normalize(arg)
                        assert path.startswith("/") or not path
                        value = await interface.get(path)
                        print(f"{path}={value}")
                except MiniconfException as err:
                    print(f"{arg}: {repr(err)}")

    asyncio.run(run())


if __name__ == "__main__":
    main()
