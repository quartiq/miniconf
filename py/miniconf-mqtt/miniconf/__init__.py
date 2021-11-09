#!/usr/bin/python3
""" Root Miniconf module file. """
import asyncio
import logging
import time

from typing import List

from gmqtt import Client as MqttClient

from .miniconf import Miniconf
from .version import __version__

async def get_devices(broker: str,
                      prefix_filter: str = None,
                      discovery_timeout: float = 0.1) -> List[str]:
    """ Get a list of available Miniconf devices.

    Args:
        * `broker` - The broker to search for clients on.
        * `prefix_filter` - An MQTT-specific topic filter for device prefixes.
        * `discovery_timeout` - The duration to search for clients in seconds.

    Returns:
        A list of discovered client prefixes that match the provided filter.
    """
    discovered_devices = []

    suffix = '/connected'

    def handle_message(_client, topic, payload, _qos, _properties):
        if not topic.endswith(suffix):
            return

        logging.debug('Got message from %s: %s', topic, payload)
        if payload == b"1":
            discovered_devices.append(topic[:-len(suffix)])


    client = MqttClient(client_id='')
    client.on_message = handle_message

    await client.connect(broker)

    if prefix_filter is None:
        prefix_filter = '#'

    client.subscribe(prefix_filter)

    await asyncio.sleep(discovery_timeout)

    return discovered_devices
