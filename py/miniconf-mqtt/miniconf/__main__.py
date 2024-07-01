"""Miniconf command line interfae
"""

import asyncio
import argparse
import logging
import json

from aiomqtt import Client
import paho.mqtt

from .miniconf import Miniconf, MiniconfException
from .discover import discover

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5

def main():
    """Main program entry point."""
    parser = argparse.ArgumentParser(
        description="Miniconf command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples:
%(prog)s -d dt/sinara/dual-iir/+ 'stream_target={"ip":[192, 168, 0, 1],"port":1000}'
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
        help="Retain the affected settings",
    )
    parser.add_argument(
        "--discover", "-d", action="store_true", help="Detect and list device prefixes"
    )
    parser.add_argument(
        "--list",
        "-l",
        action="store_true",
        help="List all active settings after modification",
    )
    parser.add_argument(
        "prefix",
        type=str,
        help="The MQTT topic prefix of the target (or a prefix filter in the case "
        "of discovery)",
    )
    parser.add_argument(
        "paths",
        metavar="PATH/PATH=VALUE",
        nargs="*",
        help="Path to get or path and JSON encoded value to set.",
    )
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    loop = asyncio.get_event_loop()

    # If a discovery was requested, try to find a device.

    async def run():
        async with Client(args.broker,
                          protocol=MQTTv5,
                          logger=logging.getLogger(__name__)) as client:
            if args.discover:
                devices = await discover(client, args.prefix)
                if len(devices) != 1:
                    raise MiniconfException(
                        f"No unique Miniconf device (found `{devices}`). "
                        "Please specify a `--prefix`"
                    )
                prefix = devices.pop()
                logging.info("Found device prefix: %s", prefix)
            else:
                prefix = args.prefix

            interface = Miniconf(client, prefix)

            for arg in args.paths:
                try:
                    path, value = arg.split("=", 1)
                except ValueError:
                    value = await interface.get(arg)
                    print(f"{arg} = {value}")
                else:
                    print(f'Setting {arg} to {value}')
                    await interface.set(path, json.loads(value), args.retain)
                    print(f"Set {path}: OK")

            if args.list:
                for path in await interface.list_paths():
                    value = await interface.get(path)
                    print(f"{path} = {value}")

    loop.run_until_complete(run())


if __name__ == "__main__":
    main()
