"""Discover alive Miniconf prefixes
"""

import asyncio
import json
import logging

from typing import List, Union

from aiomqtt import Client
import paho.mqtt
MQTTv5 = paho.mqtt.enums.MQTTProtocolVersion.MQTTv5



async def discover(
    client: Union[str, Client],
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
    if isinstance(client, str):
        async with Client(client, protocol=MQTTv5) as mqtt_client:
            return await do_discovery(mqtt_client, prefix, rel_timeout, abs_timeout)
    else:
        return await do_discovery(client, prefix, rel_timeout, abs_timeout)


async def do_discovery(
        client: Client, prefix: str, rel_timeout: float, abs_timeout: float
    ) -> List[str]:
    """ Do the discovery operation. Refer to `discover` doc strings for parameters. """
    discovered = []
    suffix = "/alive"

    t_start = asyncio.get_running_loop().time()
    await client.subscribe(f"{prefix}{suffix}")
    t_subscribe = asyncio.get_running_loop().time() - t_start

    async def listen():
        async for message in client.messages:
            logging.info(f"Got message from {message.topic}: {message.payload}")
            if json.loads(message.payload):
                peer = message.topic.value[: -len(suffix)]
                logging.info(f"Adding {peer} to discovered list")
                discovered.append(peer)
            else:
                logging.info(f"Ignoring {message.topic}")

    listen_task = asyncio.create_task(listen())
    try:
        await asyncio.wait_for(listen_task, timeout=rel_timeout * t_subscribe + abs_timeout)
    except asyncio.TimeoutError:
        listen_task.cancel()
    logging.info(f"Discovery complete: {discovered}")

    await client.unsubscribe(f"{prefix}{suffix}")
    return discovered
