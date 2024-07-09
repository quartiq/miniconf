"""Miniconf command line interfae
"""

import asyncio
import argparse
import logging
import json
import sys
import os

from .miniconf import Miniconf, MiniconfException, Client, MQTTv5
from .discover import discover

if sys.platform.lower() == "win32" or os.name.lower() == "nt":
    from asyncio import set_event_loop_policy, WindowsSelectorEventLoopPolicy

    set_event_loop_policy(WindowsSelectorEventLoopPolicy())


def main():
    """Main program entry point."""
    parser = argparse.ArgumentParser(
        description="Miniconf command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples:
%(prog)s test/id '/stream="192.0.2.16:9293"'
%(prog)s -d test/+ '/afe/0'       # GET
%(prog)s -d test/+ '/afe/0="G10"' # SET
%(prog)s -d test/+ '/afe/0='      # CLEAR
%(prog)s -d test/+ '/afe?' '?'    # LIST-GET
%(prog)s -d test/+ '/afe!'        # DUMP
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
        help="Retain the settings that are being set",
    )
    parser.add_argument(
        "--discover", "-d", action="store_true", help="Detect and list device prefixes"
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
        "('PATH=VALUE') or path to clear ('PATH=') or path to list (`PATH?`) or "
        "path to dump (`PATH!`). "
        "Use sufficient shell escaping.",
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
                devices = await discover(client, args.prefix)
                if len(devices) != 1:
                    raise MiniconfException(
                        "Discover", f"No unique Miniconf device (found `{devices}`)."
                    )
                prefix = devices.pop()
                logging.info("Found device prefix: %s", prefix)
            else:
                prefix = args.prefix

            interface = Miniconf(client, prefix)

            for arg in args.commands:
                if arg.endswith("?"):
                    path = arg.removesuffix("?")
                    assert path.startswith("/") or not path
                    for p in await interface.list(path):
                        try:
                            value = await interface.get(p)
                            print(f"List `{p}` = `{value}`")
                        except MiniconfException as err:
                            print(f"List `{p}`: {repr(err)}")
                elif arg.endswith("!"):
                    path = arg.removesuffix("!")
                    assert path.startswith("/") or not path
                    await interface.dump(path)
                    print(f"Dumped `{path}` into namespace")
                elif "=" in arg:
                    path, value = arg.split("=", 1)
                    assert path.startswith("/") or not path
                    if not value:
                        await interface.clear(path)
                        print(f"Cleared retained `{path}`")
                    else:
                        await interface.set(path, json.loads(value), args.retain)
                        print(f"Set `{path}` = `{value}`")
                else:
                    path = arg
                    assert path.startswith("/") or not path
                    value = await interface.get(path)
                    print(f"Get `{path}` = `{value}`")

    asyncio.run(run())


if __name__ == "__main__":
    main()
