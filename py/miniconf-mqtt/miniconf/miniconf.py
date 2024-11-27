#!/usr/bin/python
"""
Author: Vertigo Designs, Ryan Summers
        Robert Jördens

Description: Provides an API for controlling Miniconf devices over MQTT.
"""
import asyncio
import json
import logging
import uuid

from aiomqtt import Client, Message, MqttError
from paho.mqtt.properties import Properties, PacketTypes
import paho.mqtt

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5

LOGGER = logging.getLogger(__name__)


class MiniconfException(Exception):
    """Generic exceptions generated by Miniconf."""

    def __init__(self, code, message):
        self.code = code
        self.message = message

    def __repr__(self):
        return f"{self.code}: {self.message}"


class Miniconf:
    """An asynchronous API for controlling Miniconf devices using MQTT."""

    def __init__(self, client: Client, prefix: str):
        """Constructor.

        Args:
            client: A connected MQTT5 client.
            prefix: The MQTT toptic prefix of the device to control.
        """
        self.client = client
        self.prefix = prefix
        # A dispatcher is required since mqtt does not guarantee in-order processing
        # across topics (within a topic processing is mostly in-order).
        # Responses to requests on different topics may arrive out-of-order.
        self._inflight = {}
        self.response_topic = f"{prefix}/response"
        self.listener = asyncio.create_task(self._listen())
        self.subscribed = asyncio.Event()

    async def close(self):
        """Cancel the response listener and all in-flight requests"""
        self.listener.cancel()
        for fut in self._inflight.values():
            fut.cancel()
        try:
            await self.listener
        except asyncio.CancelledError:
            pass
        if len(self._inflight) > 0:
            await asyncio.wait(self._inflight.values())

    async def _listen(self):
        await self.client.subscribe(self.response_topic)
        LOGGER.info(f"Subscribed to {self.response_topic}")
        self.subscribed.set()
        try:
            async for message in self.client.messages:
                self._dispatch(message)
        except asyncio.CancelledError:
            pass
        except MqttError as e:
            LOGGER.debug(f"MQTT Error {e}", exc_info=True)
        finally:
            try:
                await self.client.unsubscribe(self.response_topic)
                self.subscribed.clear()
                LOGGER.info(f"Unsubscribed from {self.response_topic}")
            except MqttError as e:
                LOGGER.debug(f"MQTT Error {e}", exc_info=True)

    def _dispatch(self, message: Message):
        if message.topic.value != self.response_topic:
            LOGGER.warning(
                "Discarding message with unexpected topic: %s", message.topic.value
            )
            return

        try:
            properties = message.properties.json()
        except AttributeError:
            properties = {}
        # lazy formatting
        LOGGER.debug("Received %s: %s [%s]", message.topic, message.payload, properties)

        try:
            response_id = bytes.fromhex(properties["CorrelationData"])
        except KeyError:
            LOGGER.info("Discarding message without CorrelationData")
            return
        try:
            fut, ret = self._inflight[response_id]
        except KeyError:
            LOGGER.info(
                f"Discarding message with unexpected CorrelationData: {response_id}"
            )
            return

        try:
            code = dict(properties["UserProperty"])["code"]
        except KeyError:
            LOGGER.warning("Discarding message without response code user property")
            return

        response = message.payload.decode("utf-8")
        if code == "Continue":
            ret.append(response)
            return

        if code == "Ok":
            if response:
                ret.append(response)
            fut.set_result(ret)
        else:
            fut.set_exception(MiniconfException(code, response))
        del self._inflight[response_id]

    async def _do(self, topic: str, *, response: bool = True, **kwargs):
        await self.subscribed.wait()

        props = Properties(PacketTypes.PUBLISH)
        request_id = uuid.uuid1().bytes
        props.CorrelationData = request_id
        if response:
            fut = asyncio.get_event_loop().create_future()
            assert request_id not in self._inflight
            self._inflight[request_id] = fut, []
            props.ResponseTopic = self.response_topic

        LOGGER.info(f"Publishing {topic}: {kwargs['payload']}, [{props}]")
        await self.client.publish(
            topic,
            properties=props,
            **kwargs,
        )
        if response:
            return await fut

    async def set(self, path: str, value, retain=False):
        """Write the provided data to the specified path.

        Args:
            path: The path to set.
            value: The value to set.
            retain: Retain the the setting on the broker.
        """
        ret = await self._do(
            topic=f"{self.prefix}/settings{path}",
            payload=json.dumps(value, separators=(",", ":")),
            retain=retain,
        )
        if len(ret) != 1:
            raise MiniconfException("not a leaf", ret)
        return ret[0]

    async def list(self, root: str = ""):
        """Get a list of all the paths below a given root.

        Args:
            root: Path to the root node to list.
        """
        return await self._do(topic=f"{self.prefix}/settings{root}", payload="")

    async def dump(self, root: str = ""):
        """Dump all the paths at or below a given root into the settings namespace.

        Note that the target may be unable to respond to messages when a multipart
        operation (list or dump) is in progress.
        This method does not wait for completion.

        Args:
            root: Path to the root node to dump. Can be a leaf or an internal node.
        """
        await self._do(
            topic=f"{self.prefix}/settings{root}", payload="", response=False
        )

    async def get(self, path: str):
        """Get the specific value of a given path.

        Args:
            path: The path to get. Must be a leaf node.
        """
        ret = await self._do(topic=f"{self.prefix}/settings{path}", payload="")
        if len(ret) != 1:
            raise MiniconfException("not a leaf", ret)
        return ret[0]

    async def clear(self, path: str):
        """Clear retained value from a path.

        Args:
            path: The path to clear. Must be a leaf node.
        """
        ret = await self._do(f"{self.prefix}/settings{path}", payload="", retain=True)
        if len(ret) != 1:
            raise MiniconfException("not a leaf", ret)
        return ret[0]
