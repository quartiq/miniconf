"""
Synchronous Miniconf-over-MQTT utilities
"""

# pylint: disable=R0801,C0415,W1203,R0903,W0707

import json
import uuid
import threading
import time
from typing import Dict, Any

import paho.mqtt
from paho.mqtt.properties import Properties, PacketTypes
from paho.mqtt.client import Client, MQTTMessage

from .common import MiniconfException, LOGGER, json_dumps


class Miniconf:
    """Miniconf over MQTT (synchronous)"""

    def __init__(self, client: Client, prefix: str):
        """
        Args:
            client: A connected MQTT5 client.
            prefix: The MQTT toptic prefix of the device to control.
        """
        self.client = client
        self.prefix = prefix
        self._inflight = {}
        self.response_topic = f"{prefix}/response"
        self._subscribe()

    def _subscribe(self):
        cond = threading.Event()
        self.client.on_subscribe = (
            lambda _client, _userdata, _mid, _reason, _prop: cond.set()
        )
        try:
            self.client.subscribe(self.response_topic)
            self.client.on_message = self._dispatch
            cond.wait()
        finally:
            self.client.on_subscribe = None
        LOGGER.debug(f"Subscribed to {self.response_topic}")

    def close(self):
        """Unsubscribe from the response topic"""
        cond = threading.Event()
        self.client.on_unsubscribe = (
            lambda _client, _userdata, _mid, _reason, _prop: cond.set()
        )
        try:
            self.client.unsubscribe(self.response_topic)
            self.client.on_message = None
            cond.wait()
        finally:
            self.client.on_unsubscribe = None
        LOGGER.debug(f"Unsubscribed from {self.response_topic}")

    def _dispatch(self, _client, _userdara, message: MQTTMessage):
        if message.topic != self.response_topic:
            LOGGER.warning(
                "Discarding message with unexpected topic: %s", message.topic
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
            event, ret = self._inflight[cd]
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
            event.set()
            del self._inflight[cd]
        else:
            ret[:] = [MiniconfException(code, resp)]
            event.set()
            del self._inflight[cd]

    def _do(self, path: str, *, response=1, timeout=None, **kwargs):
        response = int(response)

        props = Properties(PacketTypes.PUBLISH)

        if response:
            event = threading.Event()
            ret = []
            cd = uuid.uuid1().bytes
            props.ResponseTopic = self.response_topic
            props.CorrelationData = cd
            assert cd not in self._inflight
            self._inflight[cd] = event, ret

        topic = f"{self.prefix}/settings{path}"
        LOGGER.debug("Publishing %s: %s, [%s]", topic, kwargs.get("payload"), props)
        _pub = self.client.publish(topic, properties=props, **kwargs)

        if response:
            event.wait(timeout)
            if len(ret) == 1 and isinstance(ret[0], MiniconfException):
                raise ret[0]
            if response == 1:
                if len(ret) != 1:
                    raise MiniconfException("Not a leaf", ret)
                return ret[0]
            assert ret
            return ret
        # pub.wait_for_publish(timeout)
        return None

    def set(self, path: str, value, retain=False, response=True, **kwargs):
        """Write the provided data to the specified path.

        Args:
            path: The path to set.
            value: The value to set.
            retain: Retain the the setting on the broker.
            response: Request and await the result of the operation.
        """
        return self._do(
            path,
            payload=json_dumps(value),
            response=response,
            retain=retain,
            **kwargs,
        )

    def list(self, path: str = "", **kwargs):
        """Get a list of all the paths below a given root.

        Args:
            path: Path to the root node to list.
        """
        return self._do(path, response=2, **kwargs)

    def dump(self, path: str = "", **kwargs):
        """Dump all the paths at or below a given root into the settings namespace.

        Note that the target may be unable to respond to messages when a multipart
        operation (list or dump) is in progress.
        This method does not wait for completion.

        Args:
            path: Path to the root node to dump. Can be a leaf or an internal node.
        """
        return self._do(path, response=0, **kwargs)

    def get(self, path: str, **kwargs):
        """Get the specific value of a given path.

        Args:
            path: The path to get. Must be a leaf node.
        """
        return json.loads(self._do(path, **kwargs))

    def clear(self, path: str, response=True, **kwargs):
        """Clear retained value from a path.

        This does not change (`set()`) or reset/clear the value on the device.

        Args:
            path: The path to clear. Must be a leaf node.
            response: Obtain and await the result of the operation.
        """
        return json.loads(
            self._do(
                path,
                retain=True,
                response=response,
                **kwargs,
            )
        )


def discover(
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

    def on_message(_client, _userdata, message):
        LOGGER.debug(f"Got message from {message.topic}: {message.payload}")
        peer = message.topic.removesuffix(suffix)
        try:
            payload = json.loads(message.payload)
        except json.JSONDecodeError:
            LOGGER.info(f"Ignoring {peer} not/invalid alive")
        else:
            LOGGER.debug(f"Discovered {peer} alive")
            discovered[peer] = payload

    client.on_message = on_message

    t_start = time.monotonic()
    cond = threading.Event()
    client.on_subscribe = lambda client, userdata, mid, reason, prop: cond.set()
    client.subscribe(topic)
    cond.wait()
    client.on_subscribe = None
    t_subscribe = time.monotonic() - t_start

    time.sleep(rel_timeout * t_subscribe + abs_timeout)
    client.unsubscribe(topic)
    client.on_message = None
    return discovered


def _main():
    import logging
    from .common import _cli, MQTTv5, one

    args = _cli().parse_args()

    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    client = Client(paho.mqtt.enums.CallbackAPIVersion.VERSION2, protocol=MQTTv5)
    client.connect(args.broker)
    client.loop_start()

    if args.discover:
        prefix, _alive = one(discover(client, args.prefix))
    else:
        prefix = args.prefix

    interface = Miniconf(client, prefix)

    try:
        _handle_commands(interface, args.commands, args.retain)
    finally:
        interface.close()
        client.disconnect()
        client.loop_stop()


def _handle_commands(interface, commands, retain):
    import sys
    from .common import _Path

    current = _Path()
    for arg in commands:
        try:
            if arg.endswith("?"):
                path = current.normalize(arg.removesuffix("?"))
                paths = interface.list(path)
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
                        value = json_dumps(interface.get(p))
                        print(f"{p}={value}")
                    except MiniconfException as err:
                        print(f"{p}: {err!r}")
            elif arg.endswith("!"):
                path = current.normalize(arg.removesuffix("!"))
                interface.dump(path)
                print(f"DUMP {path}")
            elif "=" in arg:
                path, value = arg.split("=", 1)
                path = current.normalize(path)
                if not value:
                    value = json_dumps(interface.clear(path))
                    print(f"CLEAR {path}={value}")
                else:
                    interface.set(path, json.loads(value), retain)
                    print(f"{path}={value}")
            else:
                path = current.normalize(arg)
                value = json_dumps(interface.get(path))
                print(f"{path}={value!r}")
        except MiniconfException as err:
            print(f"{arg}: {err!r}")
            sys.exit(1)


if __name__ == "__main__":
    _main()
