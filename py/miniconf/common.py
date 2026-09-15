"""Common code for the Miniconf MQTT clients."""

from dataclasses import dataclass
import asyncio
from typing import Any
import json
import logging

PROTOCOL_VERSION = 1

LOGGER = logging.getLogger("miniconf")
# Expire transient set requests. Retained alive/schema/settings publications are storage.
TRANSIENT_EXPIRY_S = 30
RETAIN_SEND_ON_SUBSCRIBE = 0
SubscriptionKey = tuple[int, bool, bool, int]
DEFAULT_SUBSCRIPTION: SubscriptionKey = (1, False, False, RETAIN_SEND_ON_SUBSCRIBE)
RETAINED_SUBSCRIPTION: SubscriptionKey = (1, False, True, RETAIN_SEND_ON_SUBSCRIBE)


def message_expiry(timeout: float | None) -> int:
    if timeout is None:
        return TRANSIENT_EXPIRY_S
    return max(1, int(timeout + 0.999))


@dataclass(frozen=True)
class AliveManifest:
    proto: int
    epoch: int
    schema_rev: int
    pages: int


def alive_manifest(value: Any) -> AliveManifest:
    if not isinstance(value, dict):
        raise MiniconfException("Protocol", "Invalid alive manifest")
    if value.get("proto") != PROTOCOL_VERSION:
        raise MiniconfException("Protocol", "Unsupported alive manifest")
    epoch = value.get("epoch")
    schema_rev = value.get("schema_rev")
    pages = value.get("pages")
    if (
        not isinstance(epoch, int)
        or not isinstance(schema_rev, int)
        or not isinstance(pages, int)
    ):
        raise MiniconfException("Protocol", "Invalid alive manifest")
    return AliveManifest(PROTOCOL_VERSION, epoch, schema_rev, pages)


def is_retained(message) -> bool:
    return bool(getattr(message, "retain", False))


def user_property_values(properties: dict, name: str) -> list[str]:
    return [value for key, value in properties.get("user_property", ()) if key == name]


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


def quiet_window(
    start: float, now: float, rel_timeout: float, abs_timeout: float
) -> float:
    """Quiescence delay from a measured subscribe round trip."""

    return abs_timeout + rel_timeout * (now - start)


class _RetainedBurst:
    """Receive until quiescence; fail if the collection deadline comes first.

    Call `reset()` only after accepting a publication.
    """

    def __init__(self, start, now, timeout, rel_timeout, abs_timeout):
        self.delay = quiet_window(start, now, rel_timeout, abs_timeout)
        self.deadline = now + self.delay
        self.end = now + timeout

    def reset(self):
        self.deadline = asyncio.get_running_loop().time() + self.delay

    async def receive(self, queue):
        while True:
            remaining = min(self.deadline, self.end) - asyncio.get_running_loop().time()
            if remaining <= 0:
                if self.end < self.deadline:
                    raise TimeoutError(
                        "Timed out waiting for retained traffic quiescence"
                    )
                return None
            try:
                return await asyncio.wait_for(queue.get(), remaining)
            except TimeoutError:
                continue


class MiniconfException(Exception):
    """Miniconf Error"""

    def __init__(self, code, message):
        self.code = code
        self.message = message

    def __repr__(self):
        return f"{self.code}: {self.message}"
