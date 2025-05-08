"""
Asynchronous Miniconf-over-MQTT utilities
"""

# pylint: disable=R0801,C0415,W1203,R0903,W0707

import asyncio
import json
import uuid
from typing import Dict, Any

from paho.mqtt.properties import Properties, PacketTypes
from aiomqtt import Client, Message, MqttError

from .common import MiniconfException, LOGGER, json_dumps


class Miniconf:
    """Miniconf over MQTT (asynchronous)"""

    def __init__(self, client: Client, prefix: str):
        """
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
        for fut, _ret in self._inflight.values():
            fut.cancel()
        try:
            await self.listener
        except asyncio.CancelledError:
            pass
        if len(self._inflight) > 0:
            await asyncio.wait(self._inflight.values())

    async def _listen(self):
        await self.client.subscribe(self.response_topic)
        LOGGER.debug(f"Subscribed to {self.response_topic}")
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
                LOGGER.debug(f"Unsubscribed from {self.response_topic}")
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
            cd = bytes.fromhex(properties["CorrelationData"])
        except KeyError:
            LOGGER.info("Discarding message without CorrelationData")
            return
        try:
            fut, ret = self._inflight[cd]
        except KeyError:
            LOGGER.info(f"Discarding message with unexpected CorrelationData: {cd}")
            return

        try:
            code = dict(properties["UserProperty"])["code"]
        except KeyError:
            LOGGER.warning("Discarding message without response code user property")
            return

        resp = message.payload.decode("utf-8")
        if code == "Continue":
            ret.append(resp)
        elif code == "Ok":
            if resp:
                ret.append(resp)
            fut.set_result(ret)
            del self._inflight[cd]
        else:
            fut.set_exception(MiniconfException(code, resp))
            del self._inflight[cd]

    async def _do(self, path: str, *, response=1, **kwargs):
        response = int(response)
        props = Properties(PacketTypes.PUBLISH)
        if response:
            await self.subscribed.wait()
            props.ResponseTopic = self.response_topic
            cd = uuid.uuid1().bytes
            props.CorrelationData = cd
            fut = asyncio.get_event_loop().create_future()
            assert cd not in self._inflight
            self._inflight[cd] = fut, []

        topic = f"{self.prefix}/settings{path}"
        LOGGER.debug("Publishing %s: %s, [%s]", topic, kwargs.get("payload"), props)
        await self.client.publish(
            topic,
            properties=props,
            **kwargs,
        )
        if response:
            ret = await fut
            if response == 1:
                if len(ret) != 1:
                    raise MiniconfException("Not a leaf", ret)
                return ret[0]
            assert ret
            return ret
        return None

    async def set(self, path: str, value: Any, retain=False, response=True, **kwargs):
        """Write the provided data to the specified path.

        Args:
            path: The path to set.
            value: The value to set.
            retain: Retain the the setting on the broker.
            response: Request and await the result of the operation.
        """
        return await self._do(
            path,
            payload=json_dumps(value),
            response=response,
            retain=retain,
            **kwargs,
        )

    async def list(self, path: str = "", **kwargs):
        """Get a list of all the paths below a given root.

        Args:
            path: Path to the root node to list. Can be a leaf or an internal node.
        """
        return await self._do(path, response=2, **kwargs)

    async def dump(self, path: str = "", **kwargs):
        """Dump all the paths at or below a given root into the settings namespace.

        Note that the target may be unable to respond to messages when a multipart
        operation (list or dump) is in progress.
        This method does not wait for a response or completion or indicate an error.

        Args:
            path: Path to the root node to dump. Can be a leaf or an internal node.
        """
        await self._do(path, response=0, **kwargs)

    async def get(self, path: str, **kwargs):
        """Get the specific value of a given path.

        Args:
            path: The path to get. Must be a leaf node.
        """
        return json.loads(await self._do(path, **kwargs))

    async def clear(self, path: str, response=True, **kwargs):
        """Clear retained value from a path.

        This does not change (`set()`) or reset/clear the value on the device.

        Args:
            path: The path to clear. Must be a leaf node.
            response: Obtain and await the result of the operation.
        """
        return json.loads(
            await self._do(
                path,
                retain=True,
                response=response,
                **kwargs,
            )
        )


async def discover(
    client: Client,
    prefix: str,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> Dict[str, Any]:
    """Get a list of available Miniconf devices.

    Args:
        * `client` - The MQTT client to search for clients on. Connected to a broker
        * `prefix` - An MQTT-specific topic filter for device prefixes. Note that this will
          be appended to with the default status topic name `/alive`.
        * `rel_timeout` - The duration to search for clients in units of the time it takes
          to ack the subscribe to the alive topic.
        * `abs_timeout` - Additional absolute duration to wait for client discovery
          in seconds.

    Returns:
        A dictionary of discovered client prefixes and metadata payload.
    """
    discovered = {}
    suffix = "/alive"
    topic = f"{prefix}{suffix}"

    async def listen():
        async for message in client.messages:
            peer = message.topic.value.removesuffix(suffix)
            try:
                payload = json.loads(message.payload)
            except json.JSONDecodeError:
                LOGGER.info(f"Ignoring {peer} not/invalid alive")
            else:
                LOGGER.debug(f"Discovered {peer} alive")
                discovered[peer] = payload

    t_start = asyncio.get_running_loop().time()
    await client.subscribe(topic)
    t_subscribe = asyncio.get_running_loop().time() - t_start

    try:
        await asyncio.wait_for(
            listen(), timeout=rel_timeout * t_subscribe + abs_timeout
        )
    except asyncio.TimeoutError:
        pass
    finally:
        await client.unsubscribe(topic)
    return discovered


async def _main():
    import sys
    import os
    import logging
    from .common import _cli, MQTTv5, one

    if sys.platform.lower() == "win32" or os.name.lower() == "nt":
        from asyncio import set_event_loop_policy, WindowsSelectorEventLoopPolicy

        set_event_loop_policy(WindowsSelectorEventLoopPolicy())

    args = _cli().parse_args()

    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    async with Client(args.broker, protocol=MQTTv5) as client:
        if args.discover:
            prefix, _alive = one(await discover(client, args.prefix))
        else:
            prefix = args.prefix

        interface = Miniconf(client, prefix)

        try:
            await _handle_commands(interface, args.commands, args.retain)
        finally:
            await interface.close()


async def _handle_commands(interface, commands, retain):
    import sys
    from .common import _Path

    current = _Path()
    for arg in commands:
        try:
            if arg.endswith("?"):
                path = current.normalize(arg.removesuffix("?"))
                paths = await interface.list(path)
                # Note: There is no way for the CLI tool to reliably
                # distinguish a one-element leaf get responce from a
                # one-element inner list response without looking at
                # the payload.
                # The only way is to note that a JSON payload of a
                # get can not start with the / that a list response
                # starts with.
                if len(paths) == 1 and not paths[0].startswith("/"):
                    value = paths[0]
                    print(f"{path}={value}")
                    continue
                for p in paths:
                    try:
                        value = json_dumps(await interface.get(p))
                        print(f"{p}={value}")
                    except MiniconfException as err:
                        print(f"{p}: {err!r}")
            elif arg.endswith("!"):
                path = current.normalize(arg.removesuffix("!"))
                await interface.dump(path)
                print(f"DUMP {path}")
            elif "=" in arg:
                path, value = arg.split("=", 1)
                path = current.normalize(path)
                if not value:
                    value = json_dumps(await interface.clear(path))
                    print(f"CLEAR {path}={value}")
                else:
                    await interface.set(path, json.loads(value), retain)
                    print(f"{path}={value}")
            else:
                path = current.normalize(arg)
                value = json_dumps(await interface.get(path))
                print(f"{path}={value}")
        except MiniconfException as err:
            print(f"{arg}: {err!r}")
            sys.exit(1)


if __name__ == "__main__":
    asyncio.run(_main())
