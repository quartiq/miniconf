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
