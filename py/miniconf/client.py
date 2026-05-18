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
from paho.mqtt.subscribeoptions import SubscribeOptions

from . import _ops
from .common import (
    LOGGER,
    MM2_PROTO,
    BurstState,
    MiniconfException,
    is_authoritative,
    is_retained,
    json_dumps,
    message_expiry,
    retained_options,
    settings_topics,
    subscription_key,
    subtree_match,
    validate_path,
)
from .schema import Schema


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


@dataclass
class _TrackState:
    burst: BurstState
    timeout: float
    rel_timeout: float
    abs_timeout: float
    reload_task: asyncio.Task[None] | None = None


class _BaseClient:
    def __init__(self, client: Client, prefix: str):
        self.client = client
        self.prefix = prefix
        self.response_topic = f"{prefix}/response/{uuid.uuid4().hex}"
        self._inflight: dict[bytes, asyncio.Future[None]] = {}
        self._watchers: dict[str, list[asyncio.Queue[Message]]] = defaultdict(list)
        self._subscriptions: dict[str, tuple[int, tuple[int, bool, bool, int]]] = {}
        self._listener = asyncio.create_task(self._listen())
        self._subscribed = asyncio.Event()

    def _listen_topics(self) -> tuple[str, ...]:
        return (self.response_topic,)

    def _listen_options(self, _topic: str) -> SubscribeOptions | None:
        return None

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
                await self._subscribe(topic, self._listen_options(topic))
                topics.append(topic)
            self._subscribed.set()
            async for message in self.client.messages:
                self._dispatch(message)
        except asyncio.CancelledError:
            pass
        except MqttError:
            LOGGER.debug("MQTT error", exc_info=True)
        finally:
            self._subscribed.clear()
            for topic in reversed(topics):
                try:
                    await self._unsubscribe(topic)
                except MqttError:
                    LOGGER.debug("MQTT unsubscribe error", exc_info=True)

    def _dispatch(self, message: Message):
        topic = message.topic.value
        properties = _properties(message)
        LOGGER.debug("Received %s: %s [%s]", topic, message.payload, properties)

        for topic_filter, queues in tuple(self._watchers.items()):
            if mqtt.topic_matches_sub(topic_filter, topic):
                for queue in tuple(queues):
                    queue.put_nowait(message)

        if topic == self.response_topic:
            self._handle_response(properties, message.payload)
            return
        self._handle_message(message, topic, properties)

    def _handle_response(self, properties: dict[str, Any], payload: bytes):
        cd = _response_cd(properties)
        if cd is None:
            LOGGER.debug("Discarding response without CorrelationData")
            return
        fut = self._inflight.pop(cd, None)
        if fut is None:
            LOGGER.debug("Discarding unexpected CorrelationData: %s", cd.hex())
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
        self, topic_filter: str, options: SubscribeOptions | None = None
    ):
        key = subscription_key(options)
        existing = self._subscriptions.get(topic_filter)
        if existing is None:
            if options is None:
                await self.client.subscribe(topic_filter, qos=1)
            else:
                await self.client.subscribe(topic_filter, options=options)
            LOGGER.debug("Subscribed to %s", topic_filter)
            self._subscriptions[topic_filter] = (1, key)
            return
        count, existing_key = existing
        if existing_key != key:
            raise MiniconfException("Subscription", topic_filter)
        self._subscriptions[topic_filter] = (count + 1, key)

    async def _unsubscribe(self, topic_filter: str):
        count, key = self._subscriptions[topic_filter]
        remaining = count - 1
        if remaining:
            self._subscriptions[topic_filter] = (remaining, key)
            return
        del self._subscriptions[topic_filter]
        await self.client.unsubscribe(topic_filter)
        LOGGER.debug("Unsubscribed from %s", topic_filter)

    @asynccontextmanager
    async def _watch(
        self, topic_filter: str, options: SubscribeOptions | None = None
    ) -> AsyncIterator[asyncio.Queue[Message]]:
        queue: asyncio.Queue[Message] = asyncio.Queue()
        watchers = self._watchers[topic_filter]
        watchers.append(queue)
        try:
            await self._subscribe(topic_filter, options)
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

    async def _publish_set(
        self, path: str, payload: str, *, response: bool, timeout: float | None = None
    ):
        props = Properties(PacketTypes.PUBLISH)
        props.PayloadFormatIndicator = 1
        props.MessageExpiryInterval = message_expiry(timeout)
        fut = None
        if response:
            await self._subscribed.wait()
            props.ResponseTopic = self.response_topic
            cd = uuid.uuid4().bytes
            props.CorrelationData = cd
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
    async with watch(topic_filter, retained_options()) as queue:
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


class TrackedSubtree:
    """Scoped retained-settings subtree tracker."""

    def __init__(
        self,
        client: Miniconf,
        path: str,
        *,
        timeout: float,
        rel_timeout: float,
        abs_timeout: float,
    ):
        self._client = client
        self._path = path
        self._timeout = timeout
        self._rel_timeout = rel_timeout
        self._abs_timeout = abs_timeout
        self._root: str | None = None

    @property
    def root(self) -> str:
        if self._root is None:
            raise MiniconfException("Closed", "Tracked subtree is closed")
        return self._root

    async def __aenter__(self) -> TrackedSubtree:
        self._root = await self._client._open_track(
            self._path,
            timeout=self._timeout,
            rel_timeout=self._rel_timeout,
            abs_timeout=self._abs_timeout,
        )
        return self

    async def __aexit__(self, *_exc_info) -> None:
        await self.close()

    async def close(self) -> None:
        if self._root is None:
            return
        await self._client._close_track(self._root)
        self._root = None

    @property
    def ready(self) -> bool:
        """Whether the tracked cache is currently usable."""

        if self._root is None:
            return False
        task = self._client._tracking_reload()
        if task is None:
            return True
        if not task.done() or task.cancelled():
            return False
        return task.exception() is None

    async def wait_ready(self, timeout: float | None = None) -> None:
        """Wait until any tracked-cache reload triggered by `/alive` is complete."""

        if self._root is None:
            raise MiniconfException("Closed", "Tracked subtree is closed")
        await self._client._wait_tracking_reload(timeout)

    def _resolve(self, path: str, *, leaf: bool) -> str:
        root = self.root
        if not path:
            full = root
        else:
            full = validate_path(path)
        full = self._client._schema.path(full)
        if not subtree_match(full, root):
            raise MiniconfException("Untracked", full)
        if leaf and self._client._schema.node(full).kind != "leaf":
            raise MiniconfException("LeafRequired", full)
        return full

    def cached(self, path: str = ""):
        """Read one cached tracked leaf."""

        path = self._resolve(path, leaf=True)
        self._client._check_tracking_reload()
        try:
            return self._client._settings[path]
        except KeyError as exc:
            raise MiniconfException("NotFound", path) from exc

    def snapshot(self, path: str = "") -> dict[str, Any]:
        """Return cached retained values below one tracked subtree."""

        root = self._resolve(path, leaf=False)
        self._client._check_tracking_reload()
        return {
            cache_path: value
            for cache_path, value in self._client._settings.items()
            if subtree_match(cache_path, root)
        }


class Miniconf(_BaseClient):
    """Long-lived MM2 Miniconf session with schema and settings caches.

    The client keeps `/alive` subscribed to notice new device epochs and schema revisions. Retained
    `settings/#` publications without `auth` are treated as non-authoritative and ignored. One
    explicit tracked settings subtree may be active at a time through `track()`.
    """

    def __init__(self, client: Client, prefix: str):
        self.alive_topic = f"{prefix}/alive"
        self._schema: Schema | None = None
        self._manifest: dict[str, Any] | None = None
        self._settings: dict[str, Any] = {}
        self._tracked_root: str | None = None
        self._track_state: _TrackState | None = None
        super().__init__(client, prefix)

    def _listen_topics(self) -> tuple[str, ...]:
        return self.response_topic, self.alive_topic

    def _listen_options(self, topic: str) -> SubscribeOptions | None:
        if topic == self.alive_topic:
            return retained_options()
        return None

    async def close(self) -> None:
        if self._tracked_root is not None:
            await self._close_track(self._tracked_root)
        await super().close()

    def _handle_message(self, message: Message, topic: str, properties: dict[str, Any]):
        if topic == self.alive_topic:
            self._note_manifest_payload(message.payload)
            return

        if self._tracked_root is None or not topic.startswith(
            f"{self.prefix}/settings"
        ):
            return
        if not is_authoritative(properties) or not is_retained(message):
            return
        path = topic.removeprefix(f"{self.prefix}/settings")
        if not subtree_match(path, self._tracked_root):
            return
        if not message.payload:
            self._settings.pop(path, None)
        else:
            self._settings[path] = json.loads(message.payload)
        now = asyncio.get_running_loop().time()
        assert self._track_state is not None
        self._track_state.burst.reset(now)

    def _note_manifest(self, manifest: dict[str, Any]):
        if not isinstance(manifest, dict):
            LOGGER.debug("Ignoring invalid alive manifest: %r", manifest)
            return
        if manifest.get("proto") != MM2_PROTO:
            LOGGER.debug("Ignoring unsupported alive manifest: %r", manifest)
            return

        prev = self._manifest
        self._manifest = manifest
        if prev is None:
            prev_epoch = prev_schema_rev = None
        else:
            prev_epoch = prev.get("epoch")
            prev_schema_rev = prev.get("schema_rev")
        epoch = manifest.get("epoch")
        schema_rev = manifest.get("schema_rev")
        if prev_epoch != epoch or prev_schema_rev != schema_rev:
            self._start_tracking_reload()
        if prev_schema_rev != schema_rev:
            self._schema = None

    def _note_device_gone(self):
        self._manifest = None
        self._schema = None
        self._settings.clear()
        self._cancel_tracking_reload()

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

    async def schema(self, *, timeout: float = 3.0) -> Schema:
        """Load and cache the retained paged schema."""

        if self._schema is not None:
            return self._schema
        manifest = await _ops._manifest(self, timeout=timeout)
        schema_rev = int(manifest["schema_rev"])
        pages = int(manifest["pages"])

        defs: list[list[dict[str, Any]] | None] = [None] * pages
        async with self._watch(f"{self.prefix}/schema/#", retained_options()) as queue:
            deadline = asyncio.get_running_loop().time() + timeout
            while any(page is None for page in defs):
                remaining = deadline - asyncio.get_running_loop().time()
                if remaining <= 0:
                    raise TimeoutError("Timed out waiting for schema pages")
                message = await asyncio.wait_for(queue.get(), remaining)
                if not is_retained(message):
                    continue
                suffix = message.topic.value.removeprefix(f"{self.prefix}/schema/")
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

    def track(
        self,
        path: str = "",
        *,
        timeout: float = 3.0,
        rel_timeout: float = 3.0,
        abs_timeout: float = 0.1,
    ) -> TrackedSubtree:
        """Return a scoped tracker for one retained settings subtree."""

        return TrackedSubtree(
            self,
            path,
            timeout=timeout,
            rel_timeout=rel_timeout,
            abs_timeout=abs_timeout,
        )

    async def _open_track(
        self,
        path: str,
        *,
        timeout: float,
        rel_timeout: float,
        abs_timeout: float,
    ) -> str:
        # Match the embedded retained-load phase: measure the subscribe round trip once, then wait
        # for quiescence until the last retained publication plus abs_timeout + rel_timeout * rtt.
        if self._tracked_root is not None:
            raise MiniconfException("Tracked", self._tracked_root)
        root = (await self.schema(timeout=timeout)).path(path)
        start = asyncio.get_running_loop().time()
        self._tracked_root = root
        self._track_state = _TrackState(
            burst=BurstState.from_roundtrip(start, start, rel_timeout, abs_timeout),
            timeout=timeout,
            rel_timeout=rel_timeout,
            abs_timeout=abs_timeout,
        )
        self._settings.clear()
        try:
            for topic_filter in settings_topics(self.prefix, root):
                await self._subscribe(topic_filter, retained_options())
            now = asyncio.get_running_loop().time()
            assert self._track_state is not None
            self._track_state.burst.set_roundtrip(start, now, rel_timeout, abs_timeout)
            await self._wait_tracking(timeout)
        except Exception:
            for topic_filter in settings_topics(self.prefix, root):
                await self._unsubscribe(topic_filter)
            self._tracked_root = None
            self._track_state = None
            self._settings.clear()
            raise
        return root

    async def _close_track(self, root: str):
        if self._tracked_root != root:
            raise MiniconfException("Tracked", root)
        self._cancel_tracking_reload()
        for topic_filter in settings_topics(self.prefix, root):
            await self._unsubscribe(topic_filter)
        self._tracked_root = None
        self._track_state = None
        self._settings.clear()

    def _start_tracking_reload(self) -> None:
        state = self._track_state
        if self._tracked_root is None or state is None:
            self._settings.clear()
            return
        if state.reload_task is not None and not state.reload_task.done():
            return
        self._settings.clear()
        state.reload_task = asyncio.create_task(self._reload_tracking())

    def _cancel_tracking_reload(self) -> None:
        state = self._track_state
        if state is None or state.reload_task is None:
            return
        state.reload_task.cancel()
        state.reload_task = None

    def _check_tracking_reload(self) -> None:
        task = self._tracking_reload()
        if task is None:
            return
        if task.done():
            self._finish_tracking_reload(task)
            return
        raise MiniconfException("Reloading", self._tracked_root or "/")

    def _tracking_reload(self) -> asyncio.Task[None] | None:
        state = self._track_state
        if state is None or state.reload_task is None:
            return None
        return state.reload_task

    def _finish_tracking_reload(self, task: asyncio.Task[None]) -> None:
        try:
            task.result()
        finally:
            state = self._track_state
            if state is not None and state.reload_task is task:
                state.reload_task = None

    async def _wait_tracking_reload(self, timeout: float | None) -> None:
        task = self._tracking_reload()
        if task is None:
            return
        await asyncio.wait_for(asyncio.shield(task), timeout)
        self._finish_tracking_reload(task)

    async def _reload_tracking(self) -> None:
        state = self._track_state
        root = self._tracked_root
        if state is None or root is None:
            return
        if self._schema is None:
            root = (await self.schema(timeout=state.timeout)).path(root)
            self._tracked_root = root
        start = asyncio.get_running_loop().time()
        for topic_filter in settings_topics(self.prefix, root):
            # Force a retained replay for the already-open tracked subscription without changing
            # the local refcount: a new alive generation means the cache must be rebuilt from the
            # authoritative retained mirror.
            await self.client.subscribe(topic_filter, options=retained_options())
        state.burst.reset(asyncio.get_running_loop().time())
        state.burst.set_roundtrip(
            start,
            asyncio.get_running_loop().time(),
            rel_timeout=state.rel_timeout,
            abs_timeout=state.abs_timeout,
        )
        await self._wait_tracking(state.timeout)

    async def _wait_tracking(self, timeout: float):
        state = self._track_state
        assert state is not None
        end = asyncio.get_running_loop().time() + timeout
        while True:
            now = asyncio.get_running_loop().time()
            if now >= state.burst.deadline:
                return
            if now >= end:
                raise TimeoutError(
                    f"Timed out waiting for retained settings under {self._tracked_root or '/'}"
                )
            await asyncio.sleep(min(0.01, state.burst.deadline - now, end - now))


class RawMiniconf(_BaseClient):
    """Schema-less MM2 client for exact-path GET and SET operations."""

    def __init__(self, client: Client, prefix: str):
        super().__init__(client, prefix)

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
