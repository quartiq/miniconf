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
from urllib.parse import urlsplit

from aiomqtt import Client, Message, MqttError, ProtocolVersion
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
    subtree_match,
    subscribe,
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
        address = urlsplit(f"//{broker}")
        client_kwargs.setdefault("port", address.port or 1883)
        async with Client(
            address.hostname, protocol=ProtocolVersion.V5, **client_kwargs
        ) as client:
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
            topics = [*self._listen_topics(), *self._watchers]
            self._watchers.clear()
            await self._unsubscribe(topics)

    async def _unsubscribe(self, topics):
        # Cleanup has its own bounded allowance and must not mask the operation's error.
        try:
            await self.client.unsubscribe(topics, timeout=1.0)
        except (MqttError, TimeoutError):
            LOGGER.debug("MQTT unsubscribe error", exc_info=True)

    async def _listen(self):
        for topic in self._listen_topics():
            await subscribe(self.client, topic)
        self._ready.set()
        async for message in self.client.messages:
            self._dispatch(message)

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
    async def _watch(self, topic_filter: str, deadline: _Deadline):
        queue: asyncio.Queue[Message] = asyncio.Queue()
        try:
            await self._wait(self._ready.wait(), deadline)
            async with asyncio.timeout(deadline.remaining()):
                async with self._subscription_lock:
                    self._watchers[topic_filter].append(queue)
                    # Each reader needs retained replay, even for an existing filter.
                    await self._wait(subscribe(self.client, topic_filter), deadline)
            yield queue
        finally:
            async with self._subscription_lock:
                watchers = self._watchers.get(topic_filter, [])
                if queue in watchers:
                    watchers.remove(queue)
                    if not watchers:
                        del self._watchers[topic_filter]
                        await self._unsubscribe(topic_filter)

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
        self._alive = b""
        self._alive_ready = asyncio.Event()
        super().__init__(client, prefix)

    def _listen_topics(self) -> tuple[str, ...]:
        return (*super()._listen_topics(), self.alive_topic)

    def _handle_message(self, message: Message):
        if str(message.topic) == self.alive_topic and message.payload != self._alive:
            self._alive = message.payload
            self._schema = None
            if self._alive:
                self._alive_ready.set()
            else:
                self._alive_ready.clear()

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
        """Stream settings; timeout bounds setup and subsequent schema reloads.

        Opening another reader can replay retained values to existing watchers.
        """
        deadline = _Deadline(timeout)
        root = (await self._load_schema(deadline)).path(path)
        async with self._watch(f"{self.prefix}/settings{root}/#", deadline) as queue:
            while True:
                message = await self._wait(queue.get(), _Deadline(None))
                schema = await self.schema(timeout=timeout)
                schema.path(root)
                event = self._setting_event(message, root, schema)
                if event is not None:
                    yield event

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""
        return await self._load_schema(_Deadline(timeout))

    async def _load_schema(self, deadline):
        manifest = await self._load_manifest(deadline)
        if self._schema is not None:
            return self._schema
        defs: list[list[dict[str, Any]] | None] = [None] * manifest.pages
        async with self._watch(f"{self.prefix}/schema/#", deadline) as queue:
            while any(page is None for page in defs):
                try:
                    message = await self._wait(queue.get(), deadline)
                except TimeoutError:
                    alive_manifest(json.loads(self._alive or b"null"))
                    raise
                if not message.retain:
                    continue
                suffix = str(message.topic).removeprefix(f"{self.prefix}/schema/")
                try:
                    page = int(suffix)
                except ValueError:
                    continue
                if 0 <= page < manifest.pages:
                    defs[page] = [
                        json.loads(line)
                        for line in message.payload.splitlines()
                        if line
                    ]
        if alive_manifest(json.loads(self._alive or b"null")) != manifest:
            raise MiniconfException("Protocol", "Manifest changed while loading schema")
        self._schema = Schema.from_defs(
            [record for page in defs for record in page or ()], manifest.schema_rev
        )
        return self._schema


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
