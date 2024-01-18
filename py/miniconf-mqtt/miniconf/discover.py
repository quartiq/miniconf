"""Discover alive Miniconf prefixes
"""

import asyncio
import json
import logging

from typing import List, Union

from gmqtt import Client


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
        * `abs_timeout` - Additional absolute duration to wair for client discovery
          in seconds.

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

    if isinstance(client, str):
        client_ = Client(client_id="")
        await client_.connect(client)
        client = client_
    client.on_subscribe = on_subscribe
    client.on_message = on_message

    fut = asyncio.get_running_loop().create_future()
    t_start = asyncio.get_running_loop().time()
    sub[client.subscribe(f"{prefix}{suffix}")] = fut
    await fut
    t_rtt = asyncio.get_running_loop().time() - t_start
    await asyncio.sleep(rel_timeout * t_rtt + abs_timeout)

    client.unsubscribe(f"{prefix}{suffix}")
    client.on_subscribe = lambda *_a, **_k: None
    client.on_message = lambda *_a, **_k: None
    return discovered
