#!/usr/bin/python3
""" Root Miniconf module file. """
import asyncio
import json
import logging
import time

from typing import Set

from gmqtt import Client as MqttClient

from .miniconf import Miniconf
from .version import __version__

async def discover(
        broker: str,
        prefix_filter: str,
        discovery_timeout: float = 0.1,
    ) -> Set[str]:
    """ Get a list of available Miniconf devices.

    Args:
        * `broker` - The broker to search for clients on.
        * `prefix_filter` - An MQTT-specific topic filter for device prefixes. Note that this will
          be appended to with the default status topic name `/alive`.
        * `discovery_timeout` - The duration to search for clients in seconds.

    Returns:
        A set of discovered client prefixes that match the provided filter.
    """
    discovered_devices = set()

    suffix = '/alive'

    def handle_message(_client, topic, payload, _qos, _properties):
        logging.debug('Got message from %s: %s', topic, payload)

        if json.loads(payload):
            discovered_devices.add(topic[:-len(suffix)])

    client = MqttClient(client_id='')
    client.on_message = handle_message

    await client.connect(broker)

    client.subscribe(f'{prefix_filter}{suffix}')

    await asyncio.sleep(discovery_timeout)

    return discovered_devices
