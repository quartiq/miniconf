import asyncio
import json
import logging
import time

from typing import Set

from gmqtt import Client as MqttClient


async def discover(broker: str, prefix: str, rel_timeout: float = 3) -> Set[str]:
    """Get a list of available Miniconf devices.

    Args:
        * `broker` - The broker to search for clients on.
        * `prefix` - An MQTT-specific topic filter for device prefixes. Note that this will
          be appended to with the default status topic name `/alive`.
        * `rel_timeout` - The duration to search for clients in units of the time it takes
          to ack the subscribe to the alive topic.

    Returns:
        A list of discovered client prefixes that match the provided filter.
    """
    suffix = "/alive"

    sub = {}

    def on_subscribe(_client, mid, _qos, _props):
        sub[mid].set_result(True)

    discovered = []

    def on_message(_client, topic, payload, _qos, _properties):
        logging.debug("Got message from %s: %s", topic, payload)
        if json.loads(payload):
            discovered.append(topic[: -len(suffix)])

    client = MqttClient(client_id="")
    client.on_subscribe = on_subscribe
    client.on_message = on_message
    await client.connect(broker)

    fut = asyncio.get_running_loop().create_future()
    t0 = asyncio.get_running_loop().time()
    sub[client.subscribe(f"{prefix}{suffix}")] = fut
    await fut
    dt = asyncio.get_running_loop().time() - t0
    await asyncio.sleep(rel_timeout * dt)
    client.unsubscribe(f"{prefix}{suffix}")
    return discovered
