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
import warnings

from gmqtt import Client as MqttClient

LOGGER = logging.getLogger(__name__)

class MiniconfException(Exception):
    """ Generic exceptions generated by Miniconf. """


class Miniconf:
    """An asynchronous API for controlling Miniconf devices using MQTT."""

    @classmethod
    async def create(cls, prefix, broker):
        """Create a connection to the broker and a Miniconf device using it."""
        client = MqttClient(client_id='')
        await client.connect(broker)
        miniconf = cls(client, prefix)
        await miniconf.subscriptions_complete()
        return miniconf


    def __init__(self, client, prefix):
        """Constructor.

        Args:
            client: A connected MQTT5 client.
            prefix: The MQTT toptic prefix of the device to control.
        """
        self.client = client
        self.prefix = prefix
        self.inflight = {}
        self.client.on_message = self._handle_response
        self.client.on_subscribe = self._handle_subscription
        self.response_topic = f'{prefix}/response'
        response_mid = self.client.subscribe(f'{prefix}/response')
        settings_mid = self.client.subscribe(f'{self.prefix}/settings/#', no_local=True)
        self._pending_subscriptions = {
            response_mid: asyncio.get_running_loop().create_future(),
            settings_mid: asyncio.get_running_loop().create_future(),
        }


    async def subscriptions_complete(self):
        """ Wait for all pending subscriptions to complete. """
        for subscription in list(self._pending_subscriptions.values()):
            await subscription


    def _handle_subscription(self, _client, mid, _qos, _props):
        LOGGER.info("Handling subscription for %s", mid)
        if mid not in self._pending_subscriptions:
            LOGGER.warning("MID: %s, unexpected subscription", mid)
            return

        self._pending_subscriptions[mid].set_result(True)
        del self._pending_subscriptions[mid]


    def _handle_response(self, _client, topic, payload, _qos, properties):
        """Callback function for when messages are received over MQTT.

        Args:
            _client: The MQTT client.
            topic: The topic that the message was received on.
            payload: The payload of the message.
            _qos: The quality-of-service level of the received packet
            properties: A dictionary of properties associated with the message.
        """
        # Extract request_id corrleation data from the properties
        try:
            request_id = properties['correlation_data'][0]
        except KeyError:
            LOGGER.info("Discarding message without CD")
            return

        try:
            handler = self.inflight[request_id]
        except KeyError:
            LOGGER.info("Discarding message with unexpected CD: %s", request_id)
            return

        # When receiving data not on the specific response topic, the data is some partial result of
        # another response. Append it to the data collected for the request.
        if topic != self.response_topic:
            # Payloads for path values are JSON formatted.
            response = json.loads(payload)

            # Handle get subscription data.
            handler[0].append(response)
            return

        # Payloads for generic responses are UTF8
        response = payload.decode('utf-8')

        try:
            response_prop = next(prop for prop in properties['user_property'] if prop[0] == 'code')
            code = response_prop[1]
        except (KeyError, StopIteration):
            LOGGER.warning("Discarding message without response code user property")
            return

        # Otherwise, a request has completed with a result. Check the result code and handle it
        # appropriately.
        if code == 'Continue':
            handler[0].append(response)
            return

        if code == 'Ok':
            handler[1].set_result(handler[0])
        else:
            handler[1].set_exception(MiniconfException(
                f'Received code: {code}, Message: {response}'))

        del self.inflight[request_id]


    async def command(self, *args, **kwargs):
        """ Refer to `set` for more information. """
        warnings.warn("The `command` API function is deprecated in favor of `set`",
                      DeprecationWarning)
        return self.set(*args, **kwargs)


    def set(self, path, value, retain=False):
        """Write the provided data to the specified path.

        Args:
            path: The path to write the message to.
            value: The value to write to the path.
            retain: Retain the MQTT message changing the setting
                by the broker.
        """
        topic = f'{self.prefix}/settings/{path}'

        fut = asyncio.get_running_loop().create_future()

        # Assign unique correlation data for response dispatch
        request_id = uuid.uuid1().hex.encode()
        assert request_id not in self.inflight
        self.inflight[request_id] = ([], fut)

        payload = json.dumps(value, separators=(",", ":"))
        LOGGER.info('Sending "%s" to "%s" with CD: %s', value, topic, request_id)

        self.client.publish(
            topic, payload=payload, qos=0, retain=retain,
            response_topic=self.response_topic,
            correlation_data=request_id)

        return fut


    def list_paths(self):
        """ Get a list of all the paths available on the device. """
        fut = asyncio.get_running_loop().create_future()

        request_id = uuid.uuid1().hex.encode()
        assert request_id not in self.inflight
        self.inflight[request_id] = ([], fut)

        self.client.publish(f'{self.prefix}/list', payload='',
                            correlation_data=request_id,
                            response_topic=self.response_topic)
        return fut


    async def get(self, path):
        """ Get the specific value of a given path. """
        fut = asyncio.get_running_loop().create_future()

        # Assign unique correlation data for response dispatch
        request_id = uuid.uuid1().hex.encode()
        assert request_id not in self.inflight
        self.inflight[request_id] = ([], fut)

        self.client.publish(
            f'{self.prefix}/settings/{path}', payload='', qos=0,
            response_topic=self.response_topic,
            correlation_data=request_id)

        result = await fut

        return result[0]
