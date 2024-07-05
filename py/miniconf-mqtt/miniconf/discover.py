"""Discover alive Miniconf prefixes
"""

import asyncio
import json
import logging

from typing import List

from aiomqtt import Client
import paho.mqtt

MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5


async def discover(
    client: Client,
    prefix: str,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> List[str]:
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
        A list of discovered client prefixes that match the provided filter.
    """
    discovered = []
    suffix = "/alive"
    topic = f"{prefix}{suffix}"

    t_start = asyncio.get_running_loop().time()
    await client.subscribe(topic)
    t_subscribe = asyncio.get_running_loop().time() - t_start

    async def listen():
        async for message in client.messages:
            logging.debug(f"Got message from {message.topic}: {message.payload}")
            peer = message.topic.value.removesuffix(suffix)
            if json.loads(message.payload) == 1:
                logging.info(f"Discovered {peer} alive")
                discovered.append(peer)
            else:
                logging.info(f"Ignoring {peer} not alive")

    try:
        await asyncio.wait_for(
            listen(), timeout=rel_timeout * t_subscribe + abs_timeout
        )
    except asyncio.TimeoutError:
        pass

    await client.unsubscribe(topic)
    return discovered
