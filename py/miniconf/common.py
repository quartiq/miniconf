"""Common code for the MM2 Python clients."""

from dataclasses import dataclass
import json
import logging

import paho.mqtt

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5

LOGGER = logging.getLogger("miniconf")


def json_dumps(value):
    """Like json.dumps but without whitespace in separators"""
    return json.dumps(value, separators=(",", ":"))


def validate_path(path: str) -> str:
    """Validate one Miniconf slash-separated path."""
    if not path:
        return path
    if path[0] != "/":
        raise MiniconfException("Path", "Path must be empty or start with '/'")
    return path


def subtree_match(path: str, root: str) -> bool:
    """Whether `path` is equal to or below `root`."""
    root = validate_path(root)
    return not root or path == root or path.startswith(f"{root}/")


def settings_topics(prefix: str, path: str) -> tuple[str, ...]:
    """MQTT topic filters needed to track one MM2 subtree."""
    root = validate_path(path)
    if not root:
        return (f"{prefix}/settings/#",)
    return (f"{prefix}/settings{root}/#",)


@dataclass
class BurstState:
    """Simple retained-burst settle heuristic."""

    start: float
    deadline: float
    count: int = 0
    last: float | None = None

    def note(self, now: float, rel_timeout: float, abs_timeout: float):
        self.count += 1
        # The fixed floor keeps local retained bursts fast when messages are already buffered.
        # The relative term stretches the quiet window when retained packets arrive more slowly.
        if self.last is None:
            gap = abs_timeout
        else:
            gap = max(abs_timeout, rel_timeout * ((now - self.start) / self.count))
        self.last = now
        self.deadline = now + gap


class MiniconfException(Exception):
    """Miniconf Error"""

    def __init__(self, code, message):
        self.code = code
        self.message = message

    def __repr__(self):
        return f"{self.code}: {self.message}"


def _cli():
    import argparse

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
