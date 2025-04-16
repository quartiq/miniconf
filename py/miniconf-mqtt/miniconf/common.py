"""Common code for miniconf.async_ and miniconf.sync"""

# pylint: disable=R0801,C0415,W1203,R0903,W0707

from typing import Dict, Any, Tuple
import logging

import paho.mqtt

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5

LOGGER = logging.getLogger("miniconf")


class MiniconfException(Exception):
    """Miniconf Error"""

    def __init__(self, code, message):
        self.code = code
        self.message = message

    def __repr__(self):
        return f"{self.code}: {self.message}"


def one(devices: Dict[str, Any]) -> Tuple[str, Any]:
    """Return the prefix for the unique alive Miniconf device.

    See `discover()` for arguments.
    """
    try:
        (device,) = devices.items()
    except ValueError:
        raise MiniconfException(
            "Discover", f"No unique Miniconf device (found `{devices}`)."
        )
    LOGGER.info("Found device: %s", device)
    return device


class _Path:
    def __init__(self):
        self.current = ""

    def normalize(self, path):
        """Return an absolute normalized path and update current absolute reference."""
        if path.startswith("/") or not path:
            self.current = path[: path.rfind("/")]
        else:
            path = f"{self.current}/{path}"
        assert path.startswith("/") or not path
        return path


def _cli():
    import argparse

    parser = argparse.ArgumentParser(
        description="Miniconf command line interface.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Examples (with a target at prefix 'app/id' and id discovery):
%(prog)s -d app/+ /path       # GET
%(prog)s -d app/+ /path=value # SET
%(prog)s -d app/+ /path=      # CLEAR
%(prog)s -d app/+ /path?      # LIST-GET
%(prog)s -d app/+ /path!      # DUMP
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
    return parser
