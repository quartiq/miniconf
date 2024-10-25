"""Discover alive Miniconf prefixes
"""

import asyncio
import json
import logging

from typing import Dict, Any

from aiomqtt import Client

from .miniconf import MiniconfException


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

    t_start = asyncio.get_running_loop().time()
    await client.subscribe(topic)
    t_subscribe = asyncio.get_running_loop().time() - t_start

    async def listen():
        async for message in client.messages:
            logging.debug(f"Got message from {message.topic}: {message.payload}")
            peer = message.topic.value.removesuffix(suffix)
            try:
                payload = json.loads(message.payload)
            except json.JSONDecodeError:
                logging.info(f"Ignoring {peer} not/invalid alive")
            else:
                logging.info(f"Discovered {peer} alive")
                discovered[peer] = payload

    try:
        await asyncio.wait_for(
            listen(), timeout=rel_timeout * t_subscribe + abs_timeout
        )
    except asyncio.TimeoutError:
        pass
    finally:
        await client.unsubscribe(topic)
    return discovered


async def discover_one(
    client: Client,
    prefix: str,
    rel_timeout: float = 3.0,
    abs_timeout: float = 0.1,
) -> (str, Any):
    """Return the prefix for the unique alive Miniconf device.

    See `discover()` for arguments.
    """
    devices = await discover(client, prefix, rel_timeout, abs_timeout)
    try:
        (device,) = devices.items()
    except ValueError as exc:
        raise MiniconfException(
            "Discover", f"No unique Miniconf device (found `{devices}`)."
        ) from exc
    logging.info("Found device: %s", device)
    return device
