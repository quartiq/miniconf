#!/usr/bin/python3
"""
Author: Ryan Summers, Robert Jördens

Description: Command-line utility to program run-time settings utilize Miniconf.
"""

import asyncio
import argparse
import logging
import json

from .miniconf import Miniconf
from . import discover


def main():
    """ Main program entry point. """
    parser = argparse.ArgumentParser(
        description='Miniconf command line interface.',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''Examples:
%(prog)s dt/sinara/dual-iir/00-11-22-33-aa-bb stream_target=\
'{"ip": [192, 168, 0, 1], "port": 1000}'
''')
    parser.add_argument('-v', '--verbose', action='count', default=0,
                        help='Increase logging verbosity')
    parser.add_argument('--broker', '-b', default='mqtt', type=str,
                        help='The MQTT broker address')
    parser.add_argument('--no-retain', '-n', default=False,
                        action='store_true',
                        help='Do not retain the affected settings')
    parser.add_argument('prefix', type=str,
                        help='The MQTT topic prefix of the target (or a prefix filter in the case '
                             'of discovery)')
    parser.add_argument('settings', metavar="PATH=VALUE", nargs='+',
                        help='JSON encoded values for settings path keys.')
    parser.add_argument('--discover', '-d', action='store_true',
                        help='Detect and list device prefixes')

    args = parser.parse_args()

    logging.basicConfig(
        format='%(asctime)s [%(levelname)s] %(name)s: %(message)s',
        level=logging.WARN - 10*args.verbose)

    loop = asyncio.get_event_loop()

    # If a discovery was requested, try to find a device.
    prefix = args.prefix

    if args.discover:
        devices = loop.run_until_complete(discover(args.broker, args.prefix))

        if not devices:
            raise Exception('No Miniconf devices found. Please specify a --prefix')

        assert len(devices) == 1, \
            'Multiple miniconf devices found ({devices}}). Please specify a more specific --prefix'

        logging.info('Automatically using detected device prefix: %s', devices[0])
        prefix = devices[0]

    async def configure_settings():
        interface = await Miniconf.create(prefix, args.broker)
        for setting in args.settings:
            path, value = setting.split("=", 1)
            await interface.command(path, json.loads(value), not args.no_retain)
            print(f'{path}: OK')

    loop.run_until_complete(configure_settings())


if __name__ == '__main__':
    main()
