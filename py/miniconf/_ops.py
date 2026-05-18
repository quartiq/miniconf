"""CLI/support operations for discovery and retained-topic pruning."""

from __future__ import annotations

import asyncio
import json
from typing import TYPE_CHECKING, Any

from aiomqtt import Client, Message

from .common import (
    LOGGER,
    BurstState,
    MiniconfException,
    is_authoritative,
    is_retained,
    quiet_window,
    retained_options,
    settings_topics,
    subtree_match,
)

if TYPE_CHECKING:
    from .client import Miniconf


def _properties(message: Message) -> dict[str, Any]:
    try:
        return message.properties.json()
    except AttributeError:
        return {}


async def discover(
    client: Client,
    prefix: str,
    timeout: float = 0.1,
    rel_timeout: float = 3.0,
) -> dict[str, Any]:
    """Return discovered devices keyed by prefix."""

    discovered: dict[str, Any] = {}
    suffix = "/alive"
    topic = f"{prefix}{suffix}"

    start = asyncio.get_running_loop().time()
    await client.subscribe(topic, options=retained_options())
    quiet = quiet_window(
        start,
        asyncio.get_running_loop().time(),
        rel_timeout,
        timeout,
    )

    async def listen():
        deadline = asyncio.get_running_loop().time() + quiet
        while True:
            now = asyncio.get_running_loop().time()
            if now >= deadline:
                return
            try:
                message = await asyncio.wait_for(
                    client.messages.__anext__(), deadline - now
                )
            except (asyncio.TimeoutError, StopAsyncIteration):
                return
            if not is_retained(message):
                continue
            peer = message.topic.value.removesuffix(suffix)
            try:
                discovered[peer] = json.loads(message.payload)
            except json.JSONDecodeError:
                LOGGER.info("Ignoring %s not/invalid alive", peer)
            deadline = asyncio.get_running_loop().time() + quiet

    try:
        await listen()
    finally:
        await client.unsubscribe(topic)
    return discovered


async def _manifest(interface: Miniconf, *, timeout: float = 3.0) -> dict[str, Any]:
    if interface._manifest is not None:
        return interface._manifest
    async with interface._watch(
        f"{interface.prefix}/alive", retained_options()
    ) as queue:
        end = asyncio.get_running_loop().time() + timeout
        while True:
            remaining = end - asyncio.get_running_loop().time()
            if remaining <= 0:
                raise TimeoutError("Timed out waiting for live manifest")
            message = await asyncio.wait_for(queue.get(), remaining)
            if not message.payload:
                continue
            if not is_retained(message):
                continue
            interface._note_manifest_payload(message.payload)
            if interface._manifest is not None:
                return interface._manifest


async def _collect_retained_settings(
    interface: Miniconf,
    path: str,
    *,
    timeout: float,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> dict[str, Any]:
    root = (await interface.schema(timeout=timeout)).path(path)
    start = asyncio.get_running_loop().time()
    retained: dict[str, Any] = {}
    (topic_filter,) = settings_topics(interface.prefix, root)
    async with interface._watch(topic_filter, retained_options()) as queue:
        now = asyncio.get_running_loop().time()
        burst = BurstState.from_roundtrip(start, now, rel_timeout, abs_timeout)
        end = now + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if now >= burst.deadline:
                return retained
            if now >= end:
                return retained
            try:
                message = await asyncio.wait_for(
                    queue.get(),
                    min(burst.deadline, end) - now,
                )
            except TimeoutError:
                continue
            if not is_retained(message):
                continue
            topic = message.topic.value
            if not topic.startswith(f"{interface.prefix}/settings"):
                continue
            if not is_authoritative(_properties(message)):
                continue
            settings_path = topic.removeprefix(f"{interface.prefix}/settings")
            if not subtree_match(settings_path, root):
                continue
            if not message.payload:
                retained.pop(settings_path, None)
            else:
                retained[settings_path] = json.loads(message.payload)
            burst.reset(asyncio.get_running_loop().time())


async def _collect_retained_topics(
    interface: Miniconf,
    topic_filter: str,
    *,
    timeout: float,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> list[str]:
    start = asyncio.get_running_loop().time()
    seen: set[str] = set()
    async with interface._watch(topic_filter, retained_options()) as queue:
        now = asyncio.get_running_loop().time()
        burst = BurstState.from_roundtrip(start, now, rel_timeout, abs_timeout)
        end = now + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if now >= burst.deadline:
                return sorted(seen)
            if now >= end:
                return sorted(seen)
            try:
                message = await asyncio.wait_for(
                    queue.get(),
                    min(burst.deadline, end) - now,
                )
            except TimeoutError:
                continue
            if not is_retained(message):
                continue
            if message.payload:
                seen.add(message.topic.value)
            burst.reset(asyncio.get_running_loop().time())


async def _prune_schema(
    interface: Miniconf,
    *,
    timeout: float = 3.0,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> list[int]:
    """Clear retained schema pages above the current manifest page count."""

    manifest = await _manifest(interface, timeout=timeout)
    pages = int(manifest["pages"])
    seen: set[int] = set()
    start = asyncio.get_running_loop().time()
    async with interface._watch(
        f"{interface.prefix}/schema/#", retained_options()
    ) as queue:
        now = asyncio.get_running_loop().time()
        burst = BurstState.from_roundtrip(start, now, rel_timeout, abs_timeout)
        end = now + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if now >= burst.deadline:
                break
            if now >= end:
                raise TimeoutError("Timed out waiting for schema pages")
            try:
                message = await asyncio.wait_for(
                    queue.get(), min(burst.deadline, end) - now
                )
            except TimeoutError:
                break
            if not is_retained(message):
                continue
            suffix = message.topic.value.removeprefix(f"{interface.prefix}/schema/")
            try:
                seen.add(int(suffix))
            except ValueError:
                continue
            burst.reset(asyncio.get_running_loop().time())

    stale = sorted(page for page in seen if page >= pages)
    for page in stale:
        await interface.client.publish(
            f"{interface.prefix}/schema/{page}",
            payload=b"",
            qos=1,
            retain=True,
        )
    return stale


async def _prune_settings(
    interface: Miniconf, path: str = "", *, timeout: float = 3.0
) -> list[str]:
    """Clear retained settings below `path` that are not present in the current schema."""

    schema = await interface.schema(timeout=timeout)
    path = schema.path(path)
    retained = await _collect_retained_settings(interface, path, timeout=timeout)
    stale = []
    for cache_path in retained:
        try:
            schema.path(cache_path)
        except MiniconfException:
            stale.append(cache_path)
    stale.sort()
    for cache_path in stale:
        await interface.client.publish(
            f"{interface.prefix}/settings{cache_path}",
            payload=b"",
            qos=1,
            retain=True,
        )
        interface._settings.pop(cache_path, None)
    return stale


async def prune(
    interface: Miniconf, path: str = "", *, timeout: float = 3.0
) -> tuple[list[int], list[str]]:
    """Clear stale retained schema pages and retained settings."""

    return (
        await _prune_schema(interface, timeout=timeout),
        await _prune_settings(interface, path, timeout=timeout),
    )


async def force_prune(interface: Miniconf, *, timeout: float = 3.0) -> list[str]:
    """Clear all retained MM2 topics below the current prefix."""

    topics = await _collect_retained_topics(
        interface, f"{interface.prefix}/#", timeout=timeout
    )
    for topic in topics:
        await interface.client.publish(topic, payload=b"", qos=1, retain=True)
    interface._schema = None
    interface._manifest = None
    interface._settings.clear()
    return [topic.removeprefix(f"{interface.prefix}/") for topic in topics]
