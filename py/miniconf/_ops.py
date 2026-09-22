"""CLI/support operations for discovery and retained-topic pruning."""

from __future__ import annotations

import asyncio
import json
from typing import TYPE_CHECKING, Any

from .common import (
    LOGGER,
    _RetainedBurst,
    MiniconfException,
    _Deadline,
    alive_manifest,
    quiet_window,
    subscribe,
)
from aiomqtt import Client, MqttError

if TYPE_CHECKING:
    from .client import Miniconf


async def discover(
    client: Client,
    prefix: str,
    timeout: float = 3.0,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> dict[str, Any]:
    """Discover devices within one deadline, completing after retained quiescence."""
    discovered: dict[str, Any] = {}
    topic = f"{prefix}/alive"
    budget = _Deadline(timeout)
    start = asyncio.get_running_loop().time()
    try:
        remaining = budget.remaining()
        await asyncio.wait_for(subscribe(client, topic), remaining)
        quiet = quiet_window(
            start, asyncio.get_running_loop().time(), rel_timeout, abs_timeout
        )
        deadline = asyncio.get_running_loop().time() + quiet
        while True:
            remaining = min(
                deadline - asyncio.get_running_loop().time(), budget.remaining()
            )
            if remaining <= 0:
                break
            try:
                message = await asyncio.wait_for(anext(client.messages), remaining)
            except TimeoutError:
                budget.remaining()  # Only quiet-window expiry completes discovery.
                break
            if not message.retain or not message.topic.matches(topic):
                continue
            peer = str(message.topic).removesuffix("/alive")
            if not message.payload:
                discovered.pop(peer, None)
                continue
            try:
                manifest = alive_manifest(json.loads(message.payload))
            except (json.JSONDecodeError, MiniconfException):
                LOGGER.info("Ignoring %s not/invalid alive", peer)
                continue
            discovered[peer] = manifest
            deadline = asyncio.get_running_loop().time() + quiet
    finally:
        try:
            await client.unsubscribe(topic, timeout=1.0)
        except (MqttError, TimeoutError):
            LOGGER.debug("MQTT unsubscribe error", exc_info=True)
    return discovered


async def _collect_retained_topics(
    interface: Miniconf,
    topic_filter: str,
    *,
    deadline: _Deadline,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> list[str]:
    start = asyncio.get_running_loop().time()
    seen: set[str] = set()
    async with interface._watch(topic_filter, deadline) as queue:
        now = asyncio.get_running_loop().time()
        burst = _RetainedBurst(start, now, rel_timeout, abs_timeout)
        while (
            message := await interface._wait(burst.receive(queue), deadline)
        ) is not None:
            if not message.retain:
                continue
            if message.payload:
                seen.add(str(message.topic))
            burst.reset()
    return sorted(seen)


async def _prune_schema(
    interface: Miniconf,
    *,
    deadline: _Deadline,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> list[int]:
    """Clear retained schema pages above the current manifest page count."""

    manifest = await interface._load_manifest(deadline)
    pages = manifest.pages
    seen: set[int] = set()
    start = asyncio.get_running_loop().time()
    async with interface._watch(f"{interface.prefix}/schema/#", deadline) as queue:
        now = asyncio.get_running_loop().time()
        burst = _RetainedBurst(start, now, rel_timeout, abs_timeout)
        while (
            message := await interface._wait(burst.receive(queue), deadline)
        ) is not None:
            if not message.retain:
                continue
            suffix = str(message.topic).removeprefix(f"{interface.prefix}/schema/")
            try:
                seen.add(int(suffix))
            except ValueError:
                continue
            burst.reset()

    stale = sorted(page for page in seen if page >= pages)
    for page in stale:
        await interface._wait(
            interface.client.publish(
                f"{interface.prefix}/schema/{page}",
                payload=b"",
                qos=1,
                retain=True,
            ),
            deadline,
        )
    return stale


async def _prune_settings(
    interface: Miniconf, path: str, deadline: _Deadline
) -> list[str]:
    """Clear retained settings below `path` that are not present in the current schema."""

    schema = await interface._load_schema(deadline)
    path = schema.path(path)
    topics = await _collect_retained_topics(
        interface, f"{interface.prefix}/settings{path}/#", deadline=deadline
    )
    stale = []
    prefix = f"{interface.prefix}/settings"
    for topic in topics:
        cache_path = topic.removeprefix(prefix)
        try:
            node = schema.node(cache_path)
        except MiniconfException:
            stale.append(cache_path)
        else:
            if node.kind != "leaf":
                stale.append(cache_path)
    stale.sort()
    for cache_path in stale:
        await interface._wait(
            interface.client.publish(
                f"{interface.prefix}/settings{cache_path}",
                payload=b"",
                qos=1,
                retain=True,
            ),
            deadline,
        )
    return stale


async def prune(
    interface: Miniconf, path: str = "", *, timeout: float = 3.0
) -> tuple[list[int], list[str]]:
    """Clear stale retained schema pages and retained settings."""

    deadline = _Deadline(timeout)
    return (
        await _prune_schema(interface, deadline=deadline),
        await _prune_settings(interface, path, deadline),
    )


async def force_prune(interface: Miniconf, *, timeout: float = 3.0) -> list[str]:
    """Clear all retained topics under the current prefix."""

    deadline = _Deadline(timeout)
    topics = await _collect_retained_topics(
        interface, f"{interface.prefix}/#", deadline=deadline
    )
    for topic in topics:
        await interface._wait(
            interface.client.publish(topic, payload=b"", qos=1, retain=True), deadline
        )
    interface._schema = None
    interface._alive = b""
    interface._alive_ready.clear()
    return [topic.removeprefix(f"{interface.prefix}/") for topic in topics]
