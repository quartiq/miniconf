"""Small gmqtt transport used by the Miniconf protocol client."""

from __future__ import annotations

import asyncio
import uuid
from dataclasses import dataclass
from typing import Any

from gmqtt import Client as GmqttClient
from gmqtt import Subscription as GmqttSubscription
from gmqtt.mqtt.constants import MQTTv50
from gmqtt.mqtt.handler import MQTTError

MQTTv5 = MQTTv50


@dataclass(frozen=True)
class Message:
    topic: str
    payload: bytes
    retain: bool
    properties: dict[str, Any]


class _Messages:
    def __init__(self):
        self._queue: asyncio.Queue[Message] = asyncio.Queue()

    def __aiter__(self):
        return self

    async def __anext__(self) -> Message:
        return await self._queue.get()

    def put(self, message: Message) -> None:
        self._queue.put_nowait(message)


def _normalize_properties(properties: dict[str, Any]) -> dict[str, Any]:
    # gmqtt groups properties into lists; these two properties are repeatable.
    return {
        key: value[0]
        if isinstance(value, list)
        and key not in ("user_property", "subscription_identifier")
        else value
        for key, value in properties.items()
    }


def topic_matches_sub(topic_filter: str, topic: str) -> bool:
    filter_parts = topic_filter.split("/")
    topic_parts = topic.split("/")
    for index, part in enumerate(filter_parts):
        if part == "#":
            return index == len(filter_parts) - 1
        if index >= len(topic_parts):
            return False
        if part != "+" and part != topic_parts[index]:
            return False
    return len(topic_parts) == len(filter_parts)


def _split_host_port(broker: str) -> tuple[str, int]:
    if ":" not in broker:
        return broker, 1883
    host, port = broker.rsplit(":", 1)
    try:
        return host, int(port)
    except ValueError:
        return broker, 1883


class Client:
    def __init__(
        self,
        broker: str,
        *,
        protocol: int = MQTTv5,
        client_id: str | None = None,
        keepalive: int = 60,
    ):
        self.broker = broker
        self.protocol = protocol
        self.client_id = client_id or f"miniconf-py-{uuid.uuid4().hex}"
        self.keepalive = keepalive
        self.messages = _Messages()
        self._client: GmqttClient | None = None
        self._subacks: dict[int, asyncio.Future[tuple[int, ...]]] = {}
        self._unsubacks: dict[int, asyncio.Future[tuple[int, ...]]] = {}

    async def __aenter__(self) -> Client:
        host, port = _split_host_port(self.broker)
        client = GmqttClient(self.client_id, clean_session=True)
        client.on_message = self._on_message
        client.on_subscribe = self._on_subscribe
        client.on_unsubscribe = self._on_unsubscribe
        await client.connect(
            host, port, keepalive=self.keepalive, version=self.protocol
        )
        self._client = client
        return self

    async def __aexit__(self, *_exc_info) -> None:
        await self.disconnect()

    async def disconnect(self) -> None:
        if self._client is None:
            return
        client = self._client
        self._client = None
        await client.disconnect()

    async def subscribe(
        self,
        topic_filter: str,
        *,
        qos: int = 1,
        no_local: bool = False,
        retain_as_published: bool = False,
        retain_handling: int = 0,
    ) -> None:
        client = self._require_client()
        mid = client.subscribe(
            GmqttSubscription(
                topic_filter,
                qos=qos,
                no_local=no_local,
                retain_as_published=retain_as_published,
                retain_handling_options=retain_handling,
            )
        )
        if mid is not None:
            fut = asyncio.get_running_loop().create_future()
            self._subacks[mid] = fut
            try:
                reasons = await asyncio.wait_for(fut, 3.0)
            finally:
                self._subacks.pop(mid, None)
            if not reasons or reasons[0] >= 128:
                raise MQTTError(f"SUBACK failed for {topic_filter}: {reasons}")

    async def unsubscribe(self, topic_filter: str) -> None:
        client = self._require_client()
        mid = client.unsubscribe(topic_filter)
        if mid is not None:
            fut = asyncio.get_running_loop().create_future()
            self._unsubacks[mid] = fut
            try:
                reasons = await asyncio.wait_for(fut, 3.0)
            finally:
                self._unsubacks.pop(mid, None)
            if reasons and reasons[0] >= 128:
                raise MQTTError(f"UNSUBACK failed for {topic_filter}: {reasons}")

    async def publish(
        self,
        topic: str,
        *,
        payload: str | bytes,
        qos: int = 1,
        retain: bool = False,
        properties: dict[str, Any] | None = None,
    ) -> None:
        self._require_client().publish(
            topic,
            payload,
            qos=qos,
            retain=retain,
            **(properties or {}),
        )
        await asyncio.sleep(0)

    def _require_client(self) -> GmqttClient:
        if self._client is None:
            raise MQTTError("MQTT client is not connected")
        return self._client

    def _on_message(
        self,
        _client: GmqttClient,
        topic: str,
        payload: bytes,
        _qos: int,
        properties: dict[str, Any],
    ) -> None:
        self.messages.put(
            Message(
                topic,
                payload,
                retain=bool(properties.get("retain")),
                properties=_normalize_properties(properties),
            )
        )

    def _on_subscribe(
        self,
        _client: GmqttClient,
        mid: int,
        reasons: tuple[int, ...],
        _properties: dict[str, Any],
    ) -> None:
        if (fut := self._subacks.pop(mid, None)) is not None and not fut.done():
            fut.set_result(reasons)

    def _on_unsubscribe(
        self,
        _client: GmqttClient,
        mid: int,
        reasons: tuple[int, ...],
    ) -> None:
        if (fut := self._unsubacks.pop(mid, None)) is not None and not fut.done():
            fut.set_result(reasons)
