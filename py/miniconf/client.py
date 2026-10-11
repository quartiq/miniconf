"""Asynchronous Miniconf-over-MQTT client."""

from __future__ import annotations

import asyncio
import json
import uuid
from collections import defaultdict
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any, Self

from aiomqtt import Client, Message, MqttError
from paho.mqtt.packettypes import PacketTypes
from paho.mqtt.properties import Properties

from .common import (
    LOGGER,
    AliveManifest,
    _Deadline,
    _RetainedBurst,
    MiniconfException,
    alive_manifest,
    is_authoritative,
    json_dumps,
    message_expiry,
    mqtt_client,
    subtree_match,
    subscribe,
    unsubscribe,
    validate_path,
)
from .schema import Schema


@dataclass(frozen=True)
class SettingEvent:
    """One authoritative `/settings` publication."""

    path: str
    present: bool
    retained: bool = True
    value: Any = None
    rev: str | None = None


class _BaseClient:
    def __init__(self, client: Client, prefix: str):
        self.client = client
        self.prefix = prefix
        self.response_topic = f"{prefix}/response/{uuid.uuid4().hex}"
        self._inflight: dict[bytes, asyncio.Future[Message]] = {}
        self._watchers: dict[str, list[asyncio.Queue[Message]]] = defaultdict(list)
        self._subscription_lock = asyncio.Lock()
        self._ready = asyncio.Event()
        self._listener: asyncio.Task | None = None

    @classmethod
    @asynccontextmanager
    async def connect(
        cls, broker: str, prefix: str, **client_kwargs: Any
    ) -> AsyncIterator[Self]:
        async with mqtt_client(broker, **client_kwargs) as client:
            async with cls(client, prefix) as interface:
                yield interface

    def _listen_topics(self) -> tuple[str, ...]:
        return (self.response_topic,)

    async def __aenter__(self):
        if self._listener is not None:
            raise MqttError("Miniconf session cannot be reopened")
        self._listener = asyncio.create_task(self._listen())
        return self

    async def __aexit__(self, *_exc_info) -> None:
        await self.close()

    async def close(self) -> None:
        """End this session and release its subscriptions."""
        if self._listener is None:
            return
        self._listener.cancel()
        await asyncio.gather(self._listener, return_exceptions=True)
        async with self._subscription_lock:
            topics = list(dict.fromkeys((*self._listen_topics(), *self._watchers)))
            self._watchers.clear()
            await unsubscribe(self.client, topics)

    async def _listen(self):
        async def setup():
            await subscribe(self.client, *self._listen_topics())
            self._ready.set()

        async def receive():
            async for message in self.client.messages:
                self._dispatch(message)

        tasks = [asyncio.create_task(setup()), asyncio.create_task(receive())]
        try:
            await asyncio.gather(*tasks)
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)

    async def _wait(self, awaitable, deadline: _Deadline):
        """Wait within the operation budget, failing when the session ends."""
        task = asyncio.ensure_future(awaitable)
        try:
            if self._listener is None:
                raise MqttError("Miniconf session is not open")
            done, _ = await asyncio.wait(
                (self._listener, task),
                timeout=deadline.remaining(),
                return_when=asyncio.FIRST_COMPLETED,
            )
            if self._listener in done:
                if not self._listener.cancelled():
                    self._listener.result()
                raise MqttError("Miniconf session ended")
            if task in done:
                deadline.remaining()
                return task.result()
            raise TimeoutError("Miniconf operation timed out")
        finally:
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)

    def _dispatch(self, message: Message):
        topic = str(message.topic)
        LOGGER.debug("Received %s: %s", topic, message.payload)
        for topic_filter, queues in self._watchers.items():
            if message.topic.matches(topic_filter):
                for queue in queues:
                    queue.put_nowait(message)
        if topic == self.response_topic:
            correlation = getattr(message.properties, "CorrelationData", None)
            fut = self._inflight.pop(correlation, None)
            if fut is not None and not fut.done():
                fut.set_result(message)
        else:
            self._handle_message(message)

    def _handle_message(self, _message: Message) -> None:
        pass

    @asynccontextmanager
    async def _watch(self, topic_filter: str, deadline: _Deadline, *listen_topics: str):
        queue: asyncio.Queue[Message] = asyncio.Queue()
        topics = (topic_filter, *listen_topics)
        try:
            # Observe session-owned topics before setup can yield to the listener.
            for topic in listen_topics:
                self._watchers[topic].append(queue)
            await self._wait(self._ready.wait(), deadline)
            async with asyncio.timeout(deadline.remaining()):
                async with self._subscription_lock:
                    self._watchers[topic_filter].append(queue)
                    # Each reader needs retained replay, even for an existing filter.
                    await self._wait(subscribe(self.client, topic_filter), deadline)
            yield queue
        finally:
            async with self._subscription_lock:
                for topic in topics:
                    watchers = self._watchers.get(topic, [])
                    if queue in watchers:
                        watchers.remove(queue)
                        if not watchers:
                            del self._watchers[topic]
                            if topic not in self._listen_topics():
                                await unsubscribe(self.client, topic)

    def _setting_event(self, message: Message, root: str, schema: Schema | None = None):
        if not message.retain or not is_authoritative(message.properties):
            return None
        path = str(message.topic).removeprefix(f"{self.prefix}/settings")
        if not subtree_match(path, root):
            return None
        if schema is not None:
            try:
                node = schema.node(path)
            except MiniconfException:
                return None
            if node.kind != "leaf":
                return None
        rev = dict(getattr(message.properties, "UserProperty", ())).get("rev")
        if not message.payload:
            return SettingEvent(path, False, rev=rev)
        return SettingEvent(path, True, value=json.loads(message.payload), rev=rev)

    async def _snapshot(self, root, schema, deadline, rel_timeout, abs_timeout):
        start = asyncio.get_running_loop().time()
        retained: dict[str, Any] = {}
        async with self._watch(f"{self.prefix}/settings{root}/#", deadline) as queue:
            burst = _RetainedBurst(
                start,
                asyncio.get_running_loop().time(),
                rel_timeout,
                abs_timeout,
            )
            while (
                message := await self._wait(burst.receive(queue), deadline)
            ) is not None:
                event = self._setting_event(message, root, schema)
                if event is None:
                    continue
                if event.present:
                    retained[event.path] = event.value
                else:
                    retained.pop(event.path, None)
                burst.reset()
        return retained

    async def _publish_set(self, path, value, response, deadline):
        await self._wait(self._ready.wait(), deadline)
        props = Properties(PacketTypes.PUBLISH)
        props.PayloadFormatIndicator = 1
        props.MessageExpiryInterval = message_expiry(deadline.remaining())
        future = None
        try:
            if response:
                props.ResponseTopic = self.response_topic
                props.CorrelationData = uuid.uuid4().bytes
                future = asyncio.get_running_loop().create_future()
                self._inflight[props.CorrelationData] = future
            await self._wait(
                self.client.publish(
                    f"{self.prefix}/set{path}",
                    payload=json_dumps(value),
                    qos=1,
                    properties=props,
                ),
                deadline,
            )
            if future is not None:
                reply = await self._wait(future, deadline)
                try:
                    code = dict(getattr(reply.properties, "UserProperty", ()))["code"]
                except KeyError as exc:
                    raise MiniconfException(
                        "Protocol", "Missing response code"
                    ) from exc
                if code != "Ok":
                    raise MiniconfException(code, reply.payload.decode("utf-8"))
        finally:
            if future is not None:
                self._inflight.pop(props.CorrelationData, None)
                future.cancel()

    async def _get(self, path, deadline):
        async with self._watch(f"{self.prefix}/settings{path}", deadline) as queue:
            while True:
                message = await self._wait(queue.get(), deadline)
                if not message.retain or not is_authoritative(message.properties):
                    continue
                if not message.payload:
                    raise MiniconfException("NotFound", path)
                try:
                    return json.loads(message.payload)
                except json.JSONDecodeError as exc:
                    raise MiniconfException(
                        "Protocol", f"Invalid retained JSON for {path or '/'}"
                    ) from exc


class Miniconf(_BaseClient):
    """Schema-aware session; enter its context before issuing operations."""

    def __init__(self, client: Client, prefix: str):
        self.alive_topic = f"{prefix}/alive"
        self._schema: Schema | None = None
        self._schema_pages: dict[int, bytes] = {}
        self._schema_changed = asyncio.Event()
        self._replay_needed = False
        self._alive = b""
        self._alive_ready = asyncio.Event()
        super().__init__(client, prefix)

    def _listen_topics(self) -> tuple[str, ...]:
        return (*super()._listen_topics(), self.alive_topic, f"{self.prefix}/schema/#")

    def _handle_message(self, message: Message):
        if str(message.topic) == self.alive_topic and message.payload != self._alive:
            self._alive = message.payload
            self._schema = None
            self._schema_changed.set()
            if self._alive:
                self._alive_ready.set()
            else:
                self._alive_ready.clear()

        elif message.retain and message.topic.matches(f"{self.prefix}/schema/+"):
            try:
                page = int(str(message.topic).rsplit("/", 1)[1])
            except ValueError:
                return
            if page < 0:
                return
            if message.payload:
                self._schema_pages[page] = message.payload
            else:
                self._schema_pages.pop(page, None)
            self._schema_changed.set()

    async def _load_manifest(self, deadline) -> AliveManifest:
        await self._wait(self._ready.wait(), deadline)
        while not self._alive:
            await self._wait(self._alive_ready.wait(), deadline)
        return alive_manifest(json.loads(self._alive))

    async def set(
        self,
        path: str,
        value: Any,
        *,
        response: bool = True,
        timeout: float | None = None,
    ):
        """Set one schema-validated leaf through `set/#`."""
        deadline = _Deadline(timeout)
        schema = await self._load_schema(deadline)
        path = schema.path(path)
        if schema.node(path).kind != "leaf":
            raise MiniconfException("LeafRequired", path)
        await self._publish_set(path, value, response, deadline)

    async def get(self, path: str, *, timeout: float = 3.0):
        """Read one schema-validated retained authoritative leaf."""
        deadline = _Deadline(timeout)
        schema = await self._load_schema(deadline)
        path = schema.path(path)
        if schema.node(path).kind != "leaf":
            raise MiniconfException("LeafRequired", path)
        return await self._get(path, deadline)

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ):
        """Return a finite retained settings snapshot below one subtree."""
        deadline = _Deadline(timeout)
        schema = await self._load_schema(deadline)
        return await self._snapshot(
            schema.path(path), schema, deadline, rel_timeout, abs_timeout
        )

    async def watch(
        self, path: str = "", *, timeout: float = 3.0
    ) -> AsyncIterator[SettingEvent]:
        """Stream settings; timeout bounds setup.

        Opening another reader can replay retained values to existing watchers.
        Going offline or changing the alive manifest ends this watch.
        """
        deadline = _Deadline(timeout)
        schema = await self._load_schema(deadline)
        root = schema.path(path)
        manifest = alive_manifest(json.loads(self._alive or b"null"))
        async with self._watch(
            f"{self.prefix}/settings{root}/#", deadline, self.alive_topic
        ) as queue:
            while True:
                message = await self._wait(queue.get(), _Deadline(None))
                if str(message.topic) == self.alive_topic:
                    if not message.payload:
                        raise MiniconfException("Offline", "Device went offline")
                    if alive_manifest(json.loads(message.payload)) != manifest:
                        raise MiniconfException(
                            "Changed",
                            "Device publication manifest changed; reopen watch",
                        )
                    continue
                event = self._setting_event(message, root, schema)
                if event is not None:
                    yield event

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""
        return await self._load_schema(_Deadline(timeout))

    async def _load_schema(self, deadline):
        try:
            return await self._assemble_schema(deadline)
        except TimeoutError:
            # An externally bounded aiomqtt queue can drop retained replay. Let
            # the next operation recover instead of retaining an incomplete cache.
            self._replay_needed = True
            raise

    async def _assemble_schema(self, deadline):
        if self._replay_needed:
            await self._wait(self._ready.wait(), deadline)
            async with asyncio.timeout(deadline.remaining()):
                async with self._subscription_lock:
                    if self._replay_needed:
                        await self._wait(
                            subscribe(
                                self.client, self.alive_topic, f"{self.prefix}/schema/#"
                            ),
                            deadline,
                        )
                        self._replay_needed = False
        manifest = await self._load_manifest(deadline)
        while True:
            if alive_manifest(json.loads(self._alive or b"null")) != manifest:
                raise MiniconfException(
                    "Protocol", "Manifest changed while loading schema"
                )
            if self._schema is not None:
                return self._schema
            self._schema_changed.clear()
            pages = [self._schema_pages.get(page) for page in range(manifest.pages)]
            if all(page is not None for page in pages):
                # Pages can precede alive or overlap a restart. Only its advertised
                # generation is safe to assemble; revision is 32-bit FNV-1a.
                revision = 0x811C9DC5
                for page in pages:
                    for byte in page:
                        revision = ((revision ^ byte) * 0x01000193) & 0xFFFFFFFF
                if revision == manifest.schema_rev:
                    schema = Schema.from_defs(
                        [
                            json.loads(line)
                            for page in pages
                            for line in page.splitlines()
                            if line
                        ],
                        manifest.schema_rev,
                    )
                    deadline.remaining()
                    self._schema = schema
                    return schema
            try:
                await self._wait(self._schema_changed.wait(), deadline)
            except TimeoutError:
                alive_manifest(json.loads(self._alive or b"null"))
                raise


class RawMiniconf(_BaseClient):
    """Schema-less session; enter its context before issuing operations."""

    async def set(
        self,
        path: str,
        value: Any,
        *,
        response: bool = True,
        timeout: float | None = None,
    ):
        """Set one exact leaf; response=False still waits for the broker's PUBACK."""
        await self._publish_set(
            validate_path(path), value, response, _Deadline(timeout)
        )

    async def get(self, path: str, *, timeout: float = 3.0):
        """Read one exact retained authoritative leaf without schema tracking."""
        return await self._get(validate_path(path), _Deadline(timeout))

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ):
        """Return a finite retained settings snapshot below one exact subtree."""
        return await self._snapshot(
            validate_path(path), None, _Deadline(timeout), rel_timeout, abs_timeout
        )

    async def watch(
        self, path: str = "", *, timeout: float = 3.0
    ) -> AsyncIterator[SettingEvent]:
        """Stream settings after bounded setup; retained values may repeat."""
        root = validate_path(path)
        async with self._watch(
            f"{self.prefix}/settings{root}/#", _Deadline(timeout)
        ) as queue:
            while True:
                message = await self._wait(queue.get(), _Deadline(None))
                event = self._setting_event(message, root)
                if event is not None:
                    yield event
