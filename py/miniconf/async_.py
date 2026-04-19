"""Asynchronous MM2 Miniconf-over-MQTT client."""

from __future__ import annotations

import asyncio
import json
import uuid
from collections import defaultdict
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any

import paho.mqtt.client as mqtt
from aiomqtt import Client, Message, MqttError
from paho.mqtt.properties import PacketTypes, Properties

from . import ops
from .common import (
    LOGGER,
    BurstState,
    MiniconfException,
    json_dumps,
    settings_topics,
    subtree_match,
)
from .schema import Schema

discover = ops.discover
dump = ops.dump
force_prune = ops.force_prune
prune = ops.prune
read = ops.read


def _properties(message: Message) -> dict[str, Any]:
    try:
        return message.properties.json()
    except AttributeError:
        return {}


def _response_code(properties: dict[str, Any]) -> str:
    try:
        return dict(properties["UserProperty"])["code"]
    except KeyError as exc:
        raise MiniconfException("Protocol", "Missing response code") from exc


def _response_cd(properties: dict[str, Any]) -> bytes | None:
    try:
        return bytes.fromhex(properties["CorrelationData"])
    except KeyError:
        return None


def _response_rev(properties: dict[str, Any]) -> int | None:
    try:
        return int(dict(properties["UserProperty"])["rev"])
    except KeyError:
        return None


@dataclass
class _TrackState:
    refs: int
    rel_timeout: float
    abs_timeout: float
    burst: BurstState


class MiniconfClient:
    """Long-lived MM2 Miniconf session with schema and settings caches."""

    def __init__(self, client: Client, prefix: str):
        self.client = client
        self.prefix = prefix
        self.response_topic = f"{prefix}/response"
        self._inflight: dict[bytes, asyncio.Future[None]] = {}
        self._watchers: dict[str, list[asyncio.Queue[Message]]] = defaultdict(list)
        self._subscriptions: dict[str, int] = {}
        self._schema: Schema | None = None
        self._schema_rev: int | None = None
        self._settings: dict[str, Any] = {}
        self._tracking: dict[str, _TrackState] = {}
        self.listener = asyncio.create_task(self._listen())
        self.subscribed = asyncio.Event()

    async def close(self):
        """Cancel the response listener and all in-flight requests."""
        self.listener.cancel()
        for fut in self._inflight.values():
            fut.cancel()
        try:
            await self.listener
        except asyncio.CancelledError:
            pass
        if self._inflight:
            await asyncio.wait(self._inflight.values())

    async def _listen(self):
        await self.client.subscribe(self.response_topic)
        LOGGER.debug("Subscribed to %s", self.response_topic)
        self.subscribed.set()
        try:
            async for message in self.client.messages:
                self._dispatch(message)
        except asyncio.CancelledError:
            pass
        except MqttError:
            LOGGER.debug("MQTT error", exc_info=True)
        finally:
            self.subscribed.clear()
            try:
                await self.client.unsubscribe(self.response_topic)
            except MqttError:
                LOGGER.debug("MQTT unsubscribe error", exc_info=True)

    def _dispatch(self, message: Message):
        topic = message.topic.value
        properties = _properties(message)
        LOGGER.debug("Received %s: %s [%s]", topic, message.payload, properties)

        if topic == self.response_topic:
            cd = _response_cd(properties)
            if cd is None:
                LOGGER.debug("Discarding response without CorrelationData")
            else:
                fut = self._inflight.pop(cd, None)
                if fut is None:
                    LOGGER.debug("Discarding unexpected CorrelationData: %s", cd.hex())
                elif fut.done():
                    LOGGER.debug("Discarding late response: %s", cd.hex())
                else:
                    code = _response_code(properties)
                    if code == "Ok":
                        fut.set_result(None)
                    else:
                        fut.set_exception(
                            MiniconfException(code, message.payload.decode("utf-8"))
                        )

        for topic_filter, queues in tuple(self._watchers.items()):
            if mqtt.topic_matches_sub(topic_filter, topic):
                for queue in tuple(queues):
                    queue.put_nowait(message)

        if not topic.startswith(f"{self.prefix}/settings"):
            return
        if _response_rev(properties) is None:
            return
        path = topic.removeprefix(f"{self.prefix}/settings")
        if not message.payload:
            self._settings.pop(path, None)
        else:
            self._settings[path] = json.loads(message.payload)
        now = asyncio.get_running_loop().time()
        for root, state in self._tracking.items():
            if subtree_match(path, root):
                state.burst.note(now, state.rel_timeout, state.abs_timeout)

    async def _subscribe(self, topic_filter: str):
        if not self._subscriptions.get(topic_filter):
            await self.client.subscribe(topic_filter)
            LOGGER.debug("Subscribed to %s", topic_filter)
        self._subscriptions[topic_filter] = self._subscriptions.get(topic_filter, 0) + 1

    async def _unsubscribe(self, topic_filter: str):
        remaining = self._subscriptions[topic_filter] - 1
        if remaining:
            self._subscriptions[topic_filter] = remaining
            return
        del self._subscriptions[topic_filter]
        await self.client.unsubscribe(topic_filter)
        LOGGER.debug("Unsubscribed from %s", topic_filter)

    @asynccontextmanager
    async def _watch(self, topic_filter: str) -> AsyncIterator[asyncio.Queue[Message]]:
        queue: asyncio.Queue[Message] = asyncio.Queue()
        await self._subscribe(topic_filter)
        self._watchers[topic_filter].append(queue)
        try:
            yield queue
        finally:
            watchers = self._watchers[topic_filter]
            watchers.remove(queue)
            if not watchers:
                del self._watchers[topic_filter]
            await self._unsubscribe(topic_filter)

    async def _publish_set(
        self, path: str, payload: str, *, response: bool, timeout: float | None = None
    ):
        props = Properties(PacketTypes.PUBLISH)
        fut = None
        if response:
            await self.subscribed.wait()
            props.ResponseTopic = self.response_topic
            cd = uuid.uuid4().bytes
            props.CorrelationData = cd
            fut = asyncio.get_running_loop().create_future()
            assert cd not in self._inflight
            self._inflight[cd] = fut

        topic = f"{self.prefix}/set{path}"
        LOGGER.debug("Publishing %s: %s [%s]", topic, payload, props)
        await self.client.publish(topic, payload=payload, properties=props)

        if fut is not None:
            try:
                await asyncio.wait_for(fut, timeout)
            finally:
                self._inflight.pop(cd, None)

    async def set(
        self,
        path: str,
        value: Any,
        *,
        response: bool = True,
        timeout: float | None = None,
    ):
        """Set one leaf through `set/#`."""
        path = (await self.schema(timeout=timeout or 3.0)).path(path)
        await self._publish_set(
            path, json_dumps(value), response=response, timeout=timeout
        )

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""

        manifest = await ops._manifest(self, timeout=timeout)
        schema_rev = int(manifest["schema_rev"])
        if self._schema is not None and self._schema_rev == schema_rev:
            return self._schema

        pages = int(manifest["pages"])
        defs: dict[int, list[dict[str, Any]]] = {}
        async with self._watch(f"{self.prefix}/schema/#") as queue:
            deadline = asyncio.get_running_loop().time() + timeout
            while len(defs) < pages:
                remaining = deadline - asyncio.get_running_loop().time()
                if remaining <= 0:
                    raise TimeoutError("Timed out waiting for schema pages")
                message = await asyncio.wait_for(queue.get(), remaining)
                suffix = message.topic.value.removeprefix(f"{self.prefix}/schema/")
                try:
                    page = int(suffix)
                except ValueError:
                    continue
                if page < 0 or page >= pages:
                    continue
                lines = message.payload.decode("utf-8").splitlines()
                defs[page] = [json.loads(line) for line in lines if line]

        assembled = [defs[index] for index in range(pages)]
        self._schema = Schema.from_defs(
            [record for page in assembled for record in page], schema_rev
        )
        self._schema_rev = schema_rev
        return self._schema

    async def watch(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 4.0,
        abs_timeout: float = 0.05,
    ):
        """Subscribe to retained authoritative settings below `path` and wait for quiescence."""
        # 50 ms keeps the common local-broker case fast when retained packets are already queued.
        # The relative term stretches the quiet window when packets arrive more slowly or with
        # larger gaps, so one burst does not terminate early on slower links.

        root = (await self.schema(timeout=timeout)).path(path)
        state = self._tracking.get(root)
        if state is None:
            now = asyncio.get_running_loop().time()
            state = _TrackState(
                refs=0,
                rel_timeout=rel_timeout,
                abs_timeout=abs_timeout,
                burst=BurstState(now, now + abs_timeout),
            )
            self._tracking[root] = state
            for topic_filter in settings_topics(self.prefix, root):
                await self._subscribe(topic_filter)
        state.refs += 1
        await self._await_tracking(root, timeout)

    async def unwatch(self, path: str = ""):
        """Undo one `watch()` reference."""

        root = (await self.schema()).path(path)
        state = self._tracking[root]
        state.refs -= 1
        if state.refs:
            return
        del self._tracking[root]
        for topic_filter in settings_topics(self.prefix, root):
            await self._unsubscribe(topic_filter)

    async def _await_tracking(self, path: str, timeout: float):
        state = self._tracking[path]
        end = asyncio.get_running_loop().time() + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if now >= state.burst.deadline:
                return
            if now >= end:
                raise TimeoutError(
                    f"Timed out waiting for retained settings under {path or '/'}"
                )
            await asyncio.sleep(min(0.01, state.burst.deadline - now, end - now))

    def _covering(self, path: str) -> str | None:
        return min(
            (root for root in self._tracking if subtree_match(path, root)),
            key=len,
            default=None,
        )

    async def cached(self, path: str, *, timeout: float = 3.0):
        """Read one cached retained value without changing subscriptions."""

        covering = self._covering(path)
        if covering is None:
            raise MiniconfException("Untracked", path)
        await self._await_tracking(covering, timeout)
        try:
            return self._settings[path]
        except KeyError as exc:
            raise MiniconfException("NotFound", path) from exc

    async def snapshot(self, path: str = "", *, timeout: float = 3.0) -> dict[str, Any]:
        """Return cached retained authoritative values below one subtree."""

        root = (await self.schema(timeout=timeout)).path(path)
        started = False
        if root not in self._tracking:
            await self.watch(root, timeout=timeout)
            started = True
        try:
            return {
                cache_path: value
                for cache_path, value in self._settings.items()
                if subtree_match(cache_path, root)
            }
        finally:
            if started:
                await self.unwatch(root)
