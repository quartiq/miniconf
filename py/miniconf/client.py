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

from .common import (
    LOGGER,
    AliveManifest,
    _RetainedBurst,
    MiniconfException,
    alive_manifest,
    is_authoritative,
    is_retained,
    json_dumps,
    message_expiry,
    subtree_match,
    validate_path,
)
from ._mqtt import Client, Message, MQTTError, topic_matches_sub
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
        self._startup = asyncio.create_task(self._subscribe())
        self._listener = asyncio.create_task(self._listen())

    @classmethod
    @asynccontextmanager
    async def connect(
        cls, broker: str, prefix: str, **client_kwargs: Any
    ) -> AsyncIterator[Self]:
        async with Client(broker, **client_kwargs) as client:
            async with cls(client, prefix) as interface:
                yield interface

    def _listen_topics(self) -> tuple[str, ...]:
        return (self.response_topic,)

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_exc_info) -> None:
        await self.close()

    async def close(self) -> None:
        """Cancel the response listener and all in-flight requests."""
        self._listener.cancel()
        self._startup.cancel()
        for fut in self._inflight.values():
            fut.cancel()
        await asyncio.gather(self._listener, self._startup, return_exceptions=True)
        for topic in self._listen_topics():
            try:
                await self.client.unsubscribe(topic)
            except (MQTTError, TimeoutError):
                LOGGER.debug("MQTT unsubscribe error", exc_info=True)

    async def _subscribe(self):
        for topic in self._listen_topics():
            await self.client.subscribe(topic, retain_as_published=True)

    async def _listen(self):
        await self._startup
        async for message in self.client.messages:
            self._dispatch(message)

    def _dispatch(self, message: Message):
        topic = message.topic
        properties = message.properties
        LOGGER.debug("Received %s: %s [%s]", topic, message.payload, properties)

        for topic_filter, queues in tuple(self._watchers.items()):
            if topic_matches_sub(topic_filter, topic):
                for queue in tuple(queues):
                    queue.put_nowait(message)

        if topic == self.response_topic:
            fut = self._inflight.pop(properties.get("correlation_data"), None)
            if fut is not None and not fut.done():
                fut.set_result(message)
            return
        self._handle_message(message)

    def _handle_message(self, _message: Message) -> None:
        pass

    @asynccontextmanager
    async def _watch(
        self,
        topic_filter: str,
    ) -> AsyncIterator[asyncio.Queue[Message]]:
        await asyncio.shield(self._startup)
        queue: asyncio.Queue[Message] = asyncio.Queue()
        try:
            async with self._subscription_lock:
                self._watchers[topic_filter].append(queue)
                # Every reader needs its own retained replay, including shared filters.
                await self.client.subscribe(topic_filter, retain_as_published=True)
            yield queue
        finally:
            async with self._subscription_lock:
                watchers = self._watchers.get(topic_filter, [])
                if queue in watchers:
                    watchers.remove(queue)
                    if not watchers:
                        del self._watchers[topic_filter]
                        await self.client.unsubscribe(topic_filter)

    def _setting_event(
        self,
        message: Message,
        root: str,
        schema: Schema | None = None,
    ) -> SettingEvent | None:
        properties = message.properties
        if not is_retained(message) or not is_authoritative(properties):
            return None
        topic = message.topic
        if not topic.startswith(f"{self.prefix}/settings"):
            return None
        path = topic.removeprefix(f"{self.prefix}/settings")
        if path and not path.startswith("/"):
            return None
        if not subtree_match(path, root):
            return None
        if schema is not None:
            try:
                node = schema.node(path)
            except MiniconfException:
                LOGGER.debug("Ignoring setting outside the schema: %s", path)
                return None
            if node.kind != "leaf":
                LOGGER.debug("Ignoring setting for non-leaf schema path: %s", path)
                return None
        rev = dict(properties.get("user_property", ())).get("rev")
        if not message.payload:
            return SettingEvent(path, False, rev=rev)
        return SettingEvent(path, True, value=json.loads(message.payload), rev=rev)

    async def _snapshot(self, root, schema, *, timeout, rel_timeout, abs_timeout):
        start = asyncio.get_running_loop().time()
        retained: dict[str, Any] = {}
        async with self._watch(f"{self.prefix}/settings{root}/#") as queue:
            burst = _RetainedBurst(
                start,
                asyncio.get_running_loop().time(),
                timeout,
                rel_timeout,
                abs_timeout,
            )
            while (message := await burst.receive(queue)) is not None:
                event = self._setting_event(message, root, schema)
                if event is None:
                    continue
                if event.present:
                    retained[event.path] = event.value
                else:
                    retained.pop(event.path, None)
                burst.reset()
        return retained

    async def _publish_set(
        self, path: str, payload: str, *, response: bool, timeout: float | None = None
    ):
        props: dict[str, Any] = {
            "payload_format_id": 1,
            "message_expiry_interval": message_expiry(timeout),
        }
        fut = None
        topic = f"{self.prefix}/set{path}"
        try:
            async with asyncio.timeout(timeout):
                if response:
                    await asyncio.shield(self._startup)
                    props["response_topic"] = self.response_topic
                    cd = uuid.uuid4().bytes
                    props["correlation_data"] = cd
                    fut = asyncio.get_running_loop().create_future()
                    self._inflight[cd] = fut
                LOGGER.debug("Publishing %s: %s [%s]", topic, payload, props)
                await self.client.publish(
                    topic, payload=payload, qos=1, properties=props
                )
                if fut is not None:
                    reply = await fut
                    try:
                        code = dict(reply.properties["user_property"])["code"]
                    except KeyError as exc:
                        raise MiniconfException(
                            "Protocol", "Missing response code"
                        ) from exc
                    if code != "Ok":
                        raise MiniconfException(code, reply.payload.decode("utf-8"))
        finally:
            if fut is not None:
                self._inflight.pop(cd, None)
                fut.cancel()

    async def _get(self, path: str, *, timeout: float):
        async with self._watch(f"{self.prefix}/settings{path}") as queue:
            end = asyncio.get_running_loop().time() + timeout
            while True:
                remaining = end - asyncio.get_running_loop().time()
                if remaining <= 0:
                    raise TimeoutError(
                        f"Timed out waiting for retained setting {path or '/'}"
                    )
                message = await asyncio.wait_for(queue.get(), remaining)
                if not is_retained(message) or not is_authoritative(message.properties):
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
    """Long-lived Miniconf session with retained schema cache.

    The client keeps `/alive` subscribed to notice new device epochs and schema revisions. Retained
    `settings/#` publications without `auth` are treated as non-authoritative and ignored.
    """

    def __init__(self, client: Client, prefix: str):
        self.alive_topic = f"{prefix}/alive"
        self._schema: Schema | None = None
        self._alive = b""
        self._alive_ready = asyncio.Event()
        super().__init__(client, prefix)

    def _listen_topics(self) -> tuple[str, ...]:
        return (*super()._listen_topics(), self.alive_topic)

    def _handle_message(self, message: Message):
        if message.topic == self.alive_topic and message.payload != self._alive:
            self._alive = message.payload
            self._schema = None
            if self._alive:
                self._alive_ready.set()
            else:
                self._alive_ready.clear()

    async def _load_manifest(self, *, timeout: float) -> AliveManifest:
        async with asyncio.timeout(timeout):
            await asyncio.shield(self._startup)
            while not self._alive:
                await self._alive_ready.wait()
        return alive_manifest(json.loads(self._alive))

    async def set(
        self,
        path: str,
        value: Any,
        *,
        response: bool = True,
        timeout: float | None = None,
    ):
        """Set one leaf through `set/#`."""
        schema = await self.schema(timeout=timeout or 3.0)
        path = schema.path(path)
        if schema.node(path).kind != "leaf":
            raise MiniconfException("LeafRequired", path)
        await self._publish_set(
            path, json_dumps(value), response=response, timeout=timeout
        )

    async def get(self, path: str, *, timeout: float = 3.0):
        """Read one schema-validated retained authoritative leaf."""

        schema = await self.schema(timeout=timeout)
        path = schema.path(path)
        if schema.node(path).kind != "leaf":
            raise MiniconfException("LeafRequired", path)
        return await self._get(path, timeout=timeout)

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ) -> dict[str, Any]:
        """Return a finite retained settings snapshot below one subtree."""

        schema = await self.schema(timeout=timeout)
        return await self._snapshot(
            schema.path(path),
            schema,
            timeout=timeout,
            rel_timeout=rel_timeout,
            abs_timeout=abs_timeout,
        )

    async def watch(
        self, path: str = "", *, timeout: float = 3.0
    ) -> AsyncIterator[SettingEvent]:
        """Yield authoritative settings updates below one subtree without waiting for quiescence.

        Opening another reader can replay retained values to existing watchers.
        """

        root = (await self.schema(timeout=timeout)).path(path)
        async with self._watch(f"{self.prefix}/settings{root}/#") as queue:
            while True:
                message = await queue.get()
                schema = await self.schema(timeout=timeout)
                schema.path(root)
                event = self._setting_event(message, root, schema)
                if event is not None:
                    yield event

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""

        manifest = await self._load_manifest(timeout=timeout)
        if self._schema is not None:
            return self._schema
        schema_rev = manifest.schema_rev
        pages = manifest.pages

        defs: list[list[dict[str, Any]] | None] = [None] * pages
        async with self._watch(f"{self.prefix}/schema/#") as queue:
            deadline = asyncio.get_running_loop().time() + timeout
            while any(page is None for page in defs):
                remaining = deadline - asyncio.get_running_loop().time()
                if remaining <= 0:
                    raise TimeoutError("Timed out waiting for schema pages")
                try:
                    message = await asyncio.wait_for(queue.get(), remaining)
                except TimeoutError:
                    alive_manifest(json.loads(self._alive or b"null"))
                    raise
                if not is_retained(message):
                    continue
                suffix = message.topic.removeprefix(f"{self.prefix}/schema/")
                try:
                    page = int(suffix)
                except ValueError:
                    continue
                if page < 0 or page >= pages:
                    continue
                lines = message.payload.decode("utf-8").splitlines()
                defs[page] = [json.loads(line) for line in lines if line]

        if alive_manifest(json.loads(self._alive or b"null")) != manifest:
            raise MiniconfException("Protocol", "Manifest changed while loading schema")
        self._schema = Schema.from_defs(
            [record for page in defs for record in page or ()], schema_rev
        )
        return self._schema


class RawMiniconf(_BaseClient):
    """Schema-less Miniconf client for exact-path GET and SET operations."""

    async def set(
        self,
        path: str,
        value: Any,
        *,
        response: bool = True,
        timeout: float | None = None,
    ):
        """Set one exact leaf path through `set/#` without schema lookup."""
        await self._publish_set(
            validate_path(path),
            json_dumps(value),
            response=response,
            timeout=timeout,
        )

    async def get(self, path: str, *, timeout: float = 3.0):
        """Read one exact retained authoritative leaf without schema tracking."""
        return await self._get(validate_path(path), timeout=timeout)

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ) -> dict[str, Any]:
        """Return a finite retained settings snapshot below one exact subtree."""

        return await self._snapshot(
            validate_path(path),
            None,
            timeout=timeout,
            rel_timeout=rel_timeout,
            abs_timeout=abs_timeout,
        )

    async def watch(self, path: str = "") -> AsyncIterator[SettingEvent]:
        """Yield authoritative settings updates below one exact subtree.

        Opening another reader can replay retained values to existing watchers.
        """

        root = validate_path(path)
        async with self._watch(f"{self.prefix}/settings{root}/#") as queue:
            while True:
                message = await queue.get()
                event = self._setting_event(message, root)
                if event is not None:
                    yield event
