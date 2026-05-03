"""One-shot discovery, read, dump, and prune operations."""

from __future__ import annotations

import asyncio
import json
from contextlib import AsyncExitStack
from typing import TYPE_CHECKING, Any

from aiomqtt import Client, Message

from .common import LOGGER, BurstState, quiet_window, settings_topics, subtree_match

if TYPE_CHECKING:
    from .async_ import MiniconfClient


def _user_properties(message: Message) -> dict[str, str]:
    try:
        return dict(message.properties.json()["UserProperty"])
    except (AttributeError, KeyError):
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
    await client.subscribe(topic)
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


async def _manifest(
    interface: MiniconfClient, *, timeout: float = 3.0
) -> dict[str, Any]:
    if interface._manifest is not None:
        return interface._manifest
    async with interface._watch(f"{interface.prefix}/alive") as queue:
        end = asyncio.get_running_loop().time() + timeout
        while True:
            remaining = end - asyncio.get_running_loop().time()
            if remaining <= 0:
                raise TimeoutError("Timed out waiting for live manifest")
            message = await asyncio.wait_for(queue.get(), remaining)
            if not message.payload:
                continue
            interface._note_manifest_payload(message.payload)
            if interface._manifest is not None:
                return interface._manifest


async def read(interface: MiniconfClient, path: str, *, timeout: float = 3.0):
    """One-shot exact read."""

    return await interface.get(path, timeout=timeout)


async def _collect_retained_settings(
    interface: MiniconfClient,
    path: str,
    *,
    timeout: float,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> dict[str, Any]:
    root = (await interface.schema(timeout=timeout)).path(path)
    start = asyncio.get_running_loop().time()
    retained: dict[str, Any] = {}
    seen_any = False
    async with AsyncExitStack() as stack:
        queues = [
            await stack.enter_async_context(interface._watch(topic_filter))
            for topic_filter in settings_topics(interface.prefix, root)
        ]
        now = asyncio.get_running_loop().time()
        burst = BurstState.from_roundtrip(start, now, rel_timeout, abs_timeout)
        end = now + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if seen_any and now >= burst.deadline:
                return retained
            if now >= end:
                return retained
            tasks = [asyncio.create_task(queue.get()) for queue in queues]
            done: set[asyncio.Task[Message]]
            pending: set[asyncio.Task[Message]]
            try:
                done, pending = await asyncio.wait(
                    tasks,
                    timeout=(min(burst.deadline, end) - now)
                    if seen_any
                    else (end - now),
                    return_when=asyncio.FIRST_COMPLETED,
                )
            finally:
                for task in pending:
                    task.cancel()
            if not done:
                continue
            for task in done:
                message = task.result()
                topic = message.topic.value
                if not topic.startswith(f"{interface.prefix}/settings"):
                    continue
                props = _user_properties(message)
                if "rev" not in props:
                    continue
                settings_path = topic.removeprefix(f"{interface.prefix}/settings")
                if not subtree_match(settings_path, root):
                    continue
                if not message.payload:
                    retained.pop(settings_path, None)
                else:
                    retained[settings_path] = json.loads(message.payload)
                seen_any = True
                burst.note(
                    asyncio.get_running_loop().time(),
                    rel_timeout,
                    abs_timeout,
                )


async def _collect_retained_topics(
    interface: MiniconfClient,
    topic_filter: str,
    *,
    timeout: float,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> list[str]:
    start = asyncio.get_running_loop().time()
    seen: set[str] = set()
    seen_any = False
    async with interface._watch(topic_filter) as queue:
        now = asyncio.get_running_loop().time()
        burst = BurstState.from_roundtrip(start, now, rel_timeout, abs_timeout)
        end = now + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if seen_any and now >= burst.deadline:
                return sorted(seen)
            if now >= end:
                return sorted(seen)
            try:
                message = await asyncio.wait_for(
                    queue.get(),
                    (min(burst.deadline, end) - now) if seen_any else (end - now),
                )
            except TimeoutError:
                continue
            if message.payload:
                seen.add(message.topic.value)
            seen_any = True
            burst.note(
                asyncio.get_running_loop().time(),
                rel_timeout,
                abs_timeout,
            )


async def dump(
    interface: MiniconfClient, path: str = "", *, timeout: float = 3.0
) -> dict[str, Any]:
    """One-shot retained subtree dump without using the tracked cache."""

    return await _collect_retained_settings(interface, path, timeout=timeout)


async def _prune_schema(
    interface: MiniconfClient, *, timeout: float = 3.0
) -> list[int]:
    """Clear retained schema pages above the current manifest page count."""

    manifest = await _manifest(interface, timeout=timeout)
    pages = int(manifest["pages"])
    quiet = 0.1
    seen: set[int] = set()
    async with interface._watch(f"{interface.prefix}/schema/#") as queue:
        end = asyncio.get_running_loop().time() + timeout
        deadline = min(end, asyncio.get_running_loop().time() + quiet)
        while True:
            now = asyncio.get_running_loop().time()
            if now >= deadline:
                break
            if now >= end:
                raise TimeoutError("Timed out waiting for schema pages")
            try:
                message = await asyncio.wait_for(queue.get(), min(deadline, end) - now)
            except TimeoutError:
                break
            suffix = message.topic.value.removeprefix(f"{interface.prefix}/schema/")
            try:
                seen.add(int(suffix))
            except ValueError:
                continue
            deadline = min(end, asyncio.get_running_loop().time() + quiet)

    stale = sorted(page for page in seen if page >= pages)
    for page in stale:
        await interface.client.publish(
            f"{interface.prefix}/schema/{page}",
            payload=b"",
            retain=True,
        )
    return stale


async def _prune_settings(
    interface: MiniconfClient, path: str = "", *, timeout: float = 3.0
) -> list[str]:
    """Clear retained settings below `path` that are not present in the current schema."""

    schema = await interface.schema(timeout=timeout)
    path = schema.path(path)
    retained = await _collect_retained_settings(interface, path, timeout=timeout)
    stale = sorted(
        cache_path for cache_path in retained if not schema.contains(cache_path)
    )
    for cache_path in stale:
        await interface.client.publish(
            f"{interface.prefix}/settings{cache_path}",
            payload=b"",
            retain=True,
        )
        interface._settings.pop(cache_path, None)
    return stale


async def prune(
    interface: MiniconfClient, path: str = "", *, timeout: float = 3.0
) -> tuple[list[int], list[str]]:
    """Clear stale retained schema pages and retained settings."""

    return (
        await _prune_schema(interface, timeout=timeout),
        await _prune_settings(interface, path, timeout=timeout),
    )


async def force_prune(interface: MiniconfClient, *, timeout: float = 3.0) -> list[str]:
    """Clear all retained MM2 topics below the current prefix."""

    topics = await _collect_retained_topics(
        interface, f"{interface.prefix}/#", timeout=timeout
    )
    for topic in topics:
        await interface.client.publish(topic, payload=b"", retain=True)
    interface._schema = None
    interface._manifest = None
    interface._settings.clear()
    return [topic.removeprefix(f"{interface.prefix}/") for topic in topics]
