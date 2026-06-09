"""Asynchronous Miniconf-over-MQTT client."""

from __future__ import annotations

import asyncio
import json
import uuid
from collections import defaultdict
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any

from . import _ops
from .common import (
    DEFAULT_SUBSCRIPTION,
    LOGGER,
    RETAINED_SUBSCRIPTION,
    AliveManifest,
    SubscriptionKey,
    BurstState,
    MiniconfException,
    alive_manifest,
    is_authoritative,
    is_retained,
    json_dumps,
    message_expiry,
    settings_topics,
    subtree_match,
    validate_path,
)
from ._mqtt import Client, Message, MQTTError, topic_matches_sub
from .schema import Schema


def _properties(message: Message) -> dict[str, Any]:
    return message.properties


def _response_code(properties: dict[str, Any]) -> str:
    try:
        return dict(properties["user_property"])["code"]
    except KeyError as exc:
        raise MiniconfException("Protocol", "Missing response code") from exc


def _response_cd(properties: dict[str, Any]) -> bytes | None:
    return properties.get("correlation_data")


@dataclass(frozen=True)
class SettingEvent:
    """One authoritative `/settings` publication."""

    path: str
    present: bool
    retained: bool = True
    value: Any = None
    rev: str | None = None


def _setting_event(message: Message, prefix: str, root: str) -> SettingEvent | None:
    properties = _properties(message)
    if not is_retained(message) or not is_authoritative(properties):
        return None
    topic = message.topic
    if not topic.startswith(f"{prefix}/settings"):
        return None
    path = topic.removeprefix(f"{prefix}/settings")
    if path and not path.startswith("/"):
        return None
    if not subtree_match(path, root):
        return None
    rev = _user_property(properties, "rev")
    if not message.payload:
        return SettingEvent(path, False, rev=rev)
    return SettingEvent(path, True, value=json.loads(message.payload), rev=rev)


def _user_property(properties: dict[str, Any], name: str) -> str | None:
    try:
        return dict(properties.get("user_property", ())).get(name)
    except (TypeError, ValueError):
        return None


class _BaseClient:
    def __init__(self, client: Client, prefix: str):
        self.client = client
        self.prefix = prefix
        self.response_topic = f"{prefix}/response/{uuid.uuid4().hex}"
        self._inflight: dict[bytes, asyncio.Future[None]] = {}
        self._watchers: dict[str, list[asyncio.Queue[Message]]] = defaultdict(list)
        self._subscriptions: dict[str, tuple[int, SubscriptionKey]] = {}
        self._listener = asyncio.create_task(self._listen())
        self._subscribed = asyncio.Event()

    def _listen_topics(self) -> tuple[str, ...]:
        return (self.response_topic,)

    def _listen_subscription(self, _topic: str) -> SubscriptionKey:
        return DEFAULT_SUBSCRIPTION

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_exc_info) -> None:
        await self.close()

    async def close(self) -> None:
        """Cancel the response listener and all in-flight requests."""
        self._listener.cancel()
        for fut in self._inflight.values():
            fut.cancel()
        try:
            await self._listener
        except asyncio.CancelledError:
            pass
        if self._inflight:
            await asyncio.wait(self._inflight.values())

    async def _listen(self):
        topics: list[str] = []
        try:
            for topic in self._listen_topics():
                await self._subscribe(topic, self._listen_subscription(topic))
                topics.append(topic)
            self._subscribed.set()
            async for message in self.client.messages:
                self._dispatch(message)
        except asyncio.CancelledError:
            pass
        except MQTTError:
            LOGGER.debug("MQTT error", exc_info=True)
        finally:
            self._subscribed.clear()
            for topic in reversed(topics):
                try:
                    await self._unsubscribe(topic, missing_ok=True)
                except MQTTError:
                    LOGGER.debug("MQTT unsubscribe error", exc_info=True)

    def _dispatch(self, message: Message):
        topic = message.topic
        properties = _properties(message)
        LOGGER.debug("Received %s: %s [%s]", topic, message.payload, properties)

        for topic_filter, queues in tuple(self._watchers.items()):
            if topic_matches_sub(topic_filter, topic):
                for queue in tuple(queues):
                    queue.put_nowait(message)

        if topic == self.response_topic:
            self._handle_response(properties, message.payload)
            return
        self._handle_message(message, topic, properties)

    def _handle_response(self, properties: dict[str, Any], payload: bytes):
        cd = _response_cd(properties)
        if cd is None:
            LOGGER.debug("Discarding response without correlation_data")
            return
        fut = self._inflight.pop(cd, None)
        if fut is None:
            LOGGER.debug("Discarding unexpected correlation_data: %s", cd.hex())
            return
        if fut.done():
            LOGGER.debug("Discarding late response: %s", cd.hex())
            return
        code = _response_code(properties)
        if code == "Ok":
            fut.set_result(None)
            return
        fut.set_exception(MiniconfException(code, payload.decode("utf-8")))

    def _handle_message(
        self, _message: Message, _topic: str, _properties: dict[str, Any]
    ) -> None:
        pass

    async def _subscribe(
        self,
        topic_filter: str,
        subscription: SubscriptionKey = DEFAULT_SUBSCRIPTION,
    ):
        existing = self._subscriptions.get(topic_filter)
        if existing is None:
            qos, no_local, retain_as_published, retain_handling = subscription
            await self.client.subscribe(
                topic_filter,
                qos=qos,
                no_local=no_local,
                retain_as_published=retain_as_published,
                retain_handling=retain_handling,
            )
            LOGGER.debug("Subscribed to %s", topic_filter)
            self._subscriptions[topic_filter] = (1, subscription)
            return
        count, existing_key = existing
        if existing_key != subscription:
            raise MiniconfException("Subscription", topic_filter)
        self._subscriptions[topic_filter] = (count + 1, subscription)

    async def _unsubscribe(self, topic_filter: str, *, missing_ok: bool = False):
        existing = self._subscriptions.get(topic_filter)
        if existing is None:
            if missing_ok:
                return
            raise KeyError(topic_filter)
        count, key = existing
        remaining = count - 1
        if remaining:
            self._subscriptions[topic_filter] = (remaining, key)
            return
        del self._subscriptions[topic_filter]
        await self.client.unsubscribe(topic_filter)
        LOGGER.debug("Unsubscribed from %s", topic_filter)

    @asynccontextmanager
    async def _watch(
        self,
        topic_filter: str,
        subscription: SubscriptionKey = DEFAULT_SUBSCRIPTION,
    ) -> AsyncIterator[asyncio.Queue[Message]]:
        queue: asyncio.Queue[Message] = asyncio.Queue()
        watchers = self._watchers[topic_filter]
        watchers.append(queue)
        try:
            await self._subscribe(topic_filter, subscription)
        except Exception:
            watchers.remove(queue)
            if not watchers:
                del self._watchers[topic_filter]
            raise
        try:
            yield queue
        finally:
            watchers.remove(queue)
            if not watchers:
                del self._watchers[topic_filter]
            await self._unsubscribe(topic_filter)

    async def _watch_settings(self, root: str) -> AsyncIterator[SettingEvent]:
        async with self._settings_queue(root) as queue:
            while True:
                message = await queue.get()
                event = self._setting_event(message, root)
                if event is not None:
                    yield event

    def _setting_event(self, message: Message, root: str) -> SettingEvent | None:
        return _setting_event(message, self.prefix, root)

    @asynccontextmanager
    async def _settings_queue(self, root: str) -> AsyncIterator[asyncio.Queue[Message]]:
        (topic_filter,) = settings_topics(self.prefix, root)
        async with self._watch(topic_filter, RETAINED_SUBSCRIPTION) as queue:
            yield queue

    async def _publish_set(
        self, path: str, payload: str, *, response: bool, timeout: float | None = None
    ):
        props: dict[str, Any] = {
            "payload_format_id": 1,
            "message_expiry_interval": message_expiry(timeout),
        }
        fut = None
        if response:
            await self._subscribed.wait()
            props["response_topic"] = self.response_topic
            cd = uuid.uuid4().bytes
            props["correlation_data"] = cd
            fut = asyncio.get_running_loop().create_future()
            assert cd not in self._inflight
            self._inflight[cd] = fut

        topic = f"{self.prefix}/set{path}"
        LOGGER.debug("Publishing %s: %s [%s]", topic, payload, props)
        await self.client.publish(topic, payload=payload, qos=1, properties=props)

        if fut is not None:
            try:
                await asyncio.wait_for(fut, timeout)
            finally:
                self._inflight.pop(cd, None)


async def _read_retained_json(
    watch,
    topic_filter: str,
    path: str,
    *,
    timeout: float,
):
    async with watch(topic_filter, RETAINED_SUBSCRIPTION) as queue:
        end = asyncio.get_running_loop().time() + timeout
        while True:
            remaining = end - asyncio.get_running_loop().time()
            if remaining <= 0:
                raise TimeoutError(
                    f"Timed out waiting for retained setting {path or '/'}"
                )
            message = await asyncio.wait_for(queue.get(), remaining)
            if not is_retained(message):
                continue
            if not is_authoritative(_properties(message)):
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
        self._manifest: AliveManifest | None = None
        super().__init__(client, prefix)

    @classmethod
    @asynccontextmanager
    async def connect(
        cls, broker: str, prefix: str, **client_kwargs: Any
    ) -> AsyncIterator[Miniconf]:
        async with Client(broker, **client_kwargs) as client:
            async with cls(client, prefix) as interface:
                yield interface

    def _listen_topics(self) -> tuple[str, ...]:
        return self.response_topic, self.alive_topic

    def _listen_subscription(self, topic: str) -> SubscriptionKey:
        if topic == self.alive_topic:
            return RETAINED_SUBSCRIPTION
        return DEFAULT_SUBSCRIPTION

    def _handle_message(self, message: Message, topic: str, properties: dict[str, Any]):
        if topic == self.alive_topic:
            self._note_manifest_payload(message.payload)

    def _note_manifest(self, manifest: Any):
        prev = self._manifest
        try:
            next_manifest = alive_manifest(manifest)
        except MiniconfException:
            LOGGER.debug("Ignoring invalid alive manifest: %r", manifest)
            return
        self._manifest = next_manifest
        if prev is None:
            prev_schema_rev = None
        else:
            prev_schema_rev = prev.schema_rev
        if prev_schema_rev != next_manifest.schema_rev:
            self._schema = None

    def _note_device_gone(self):
        self._manifest = None
        self._schema = None

    def _note_manifest_payload(self, payload: bytes):
        if not payload:
            self._note_device_gone()
            return
        try:
            manifest = json.loads(payload)
        except json.JSONDecodeError:
            LOGGER.debug("Ignoring invalid alive payload: %r", payload)
            return
        self._note_manifest(manifest)

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
        return await _read_retained_json(
            self._watch,
            f"{self.prefix}/settings{path}",
            path,
            timeout=timeout,
        )

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ) -> dict[str, Any]:
        """Return a finite retained settings snapshot below one subtree."""

        return await _ops._collect_retained_settings(
            self,
            path,
            timeout=timeout,
            rel_timeout=rel_timeout,
            abs_timeout=abs_timeout,
        )

    async def watch(
        self, path: str = "", *, timeout: float = 3.0
    ) -> AsyncIterator[SettingEvent]:
        """Yield authoritative settings updates below one subtree without waiting for quiescence."""

        root = (await self.schema(timeout=timeout)).path(path)
        async for event in self._watch_settings(root):
            yield event

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""

        if self._schema is not None:
            return self._schema
        manifest = await _ops._manifest(self, timeout=timeout)
        schema_rev = manifest.schema_rev
        pages = manifest.pages

        defs: list[list[dict[str, Any]] | None] = [None] * pages
        async with self._watch(
            f"{self.prefix}/schema/#", RETAINED_SUBSCRIPTION
        ) as queue:
            deadline = asyncio.get_running_loop().time() + timeout
            while any(page is None for page in defs):
                remaining = deadline - asyncio.get_running_loop().time()
                if remaining <= 0:
                    raise TimeoutError("Timed out waiting for schema pages")
                message = await asyncio.wait_for(queue.get(), remaining)
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

        self._schema = Schema.from_defs(
            [record for page in defs for record in page or ()], schema_rev
        )
        return self._schema


class RawMiniconf(_BaseClient):
    """Schema-less Miniconf client for exact-path GET and SET operations."""

    def __init__(self, client: Client, prefix: str):
        super().__init__(client, prefix)

    @classmethod
    @asynccontextmanager
    async def connect(
        cls, broker: str, prefix: str, **client_kwargs: Any
    ) -> AsyncIterator[RawMiniconf]:
        async with Client(broker, **client_kwargs) as client:
            async with cls(client, prefix) as interface:
                yield interface

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
        path = validate_path(path)
        return await _read_retained_json(
            self._watch,
            f"{self.prefix}/settings{path}",
            path,
            timeout=timeout,
        )

    async def snapshot(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ) -> dict[str, Any]:
        """Return a finite retained settings snapshot below one exact subtree."""

        root = validate_path(path)
        start = asyncio.get_running_loop().time()
        retained: dict[str, Any] = {}
        async with self._settings_queue(root) as queue:
            burst = BurstState.from_roundtrip(
                start,
                asyncio.get_running_loop().time(),
                rel_timeout,
                abs_timeout,
            )
            end = asyncio.get_running_loop().time() + timeout
            while True:
                now = asyncio.get_running_loop().time()
                if now >= burst.deadline or now >= end:
                    return retained
                try:
                    message = await asyncio.wait_for(
                        queue.get(), min(burst.deadline, end) - now
                    )
                except TimeoutError:
                    continue
                event = self._setting_event(message, root)
                if event is None:
                    continue
                if not event.present:
                    retained.pop(event.path, None)
                else:
                    retained[event.path] = event.value
                burst.reset(asyncio.get_running_loop().time())

    async def watch(self, path: str = "") -> AsyncIterator[SettingEvent]:
        """Yield authoritative settings updates below one exact subtree."""

        root = validate_path(path)
        async for event in self._watch_settings(root):
            yield event
