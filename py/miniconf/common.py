"""Common code for the MM2 Python clients."""

from dataclasses import dataclass
import json
import logging

import paho.mqtt
from paho.mqtt.subscribeoptions import SubscribeOptions

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5
MM2_PROTO = 1

LOGGER = logging.getLogger("miniconf")
# Expire transient set requests. Retained alive/schema/settings publications are storage.
TRANSIENT_EXPIRY_S = 30


def message_expiry(timeout: float | None) -> int:
    if timeout is None:
        return TRANSIENT_EXPIRY_S
    return max(1, int(timeout + 0.999))


def retained_options() -> SubscribeOptions:
    return SubscribeOptions(
        qos=1,
        retainAsPublished=True,
        retainHandling=SubscribeOptions.RETAIN_SEND_ON_SUBSCRIBE,
    )


def subscription_key(options: SubscribeOptions | None) -> tuple[int, bool, bool, int]:
    if options is None:
        options = SubscribeOptions(qos=1)
    return (
        options.QoS,
        options.noLocal,
        options.retainAsPublished,
        options.retainHandling,
    )


def is_retained(message) -> bool:
    return bool(getattr(message, "retain", False))


def user_property_values(properties: dict, name: str) -> list[str]:
    return [value for key, value in properties.get("UserProperty", ()) if key == name]


def is_authoritative(properties: dict) -> bool:
    return user_property_values(properties, "auth") == [""]


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


def quiet_window(
    start: float, now: float, rel_timeout: float, abs_timeout: float
) -> float:
    """Quiescence delay from a measured subscribe round trip."""

    return abs_timeout + rel_timeout * (now - start)


@dataclass
class BurstState:
    """Retained-burst quiescence timer."""

    delay: float
    deadline: float
    last: float
    count: int = 0

    @classmethod
    def from_roundtrip(
        cls, start: float, now: float, rel_timeout: float, abs_timeout: float
    ) -> "BurstState":
        delay = quiet_window(start, now, rel_timeout, abs_timeout)
        return cls(delay, now + delay, now)

    def set_roundtrip(
        self, start: float, now: float, rel_timeout: float, abs_timeout: float
    ):
        self.delay = quiet_window(start, now, rel_timeout, abs_timeout)
        self.deadline = self.last + self.delay

    def reset(self, now: float):
        self.count += 1
        self.last = now
        self.deadline = now + self.delay


class MiniconfException(Exception):
    """Miniconf Error"""

    def __init__(self, code, message):
        self.code = code
        self.message = message

    def __repr__(self):
        return f"{self.code}: {self.message}"
