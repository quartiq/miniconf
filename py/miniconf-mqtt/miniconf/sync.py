#!/usr/bin/python
import json
import logging
import uuid
import threading
import time
from typing import Dict, List, Any
import enum

import paho.mqtt
from paho.mqtt.properties import Properties, PacketTypes
from paho.mqtt.client import Client, MQTTMessage

from .async_ import MiniconfException

LOGGER = logging.getLogger(__name__)


class Miniconf:
    """An synchronous API for controlling Miniconf devices using MQTT."""

    def __init__(self, client: Client, prefix: str):
        self.client = client
        self.prefix = prefix
        self.response_topic = f"{prefix}/response"
        self._subscribe()

    def _subscribe(self):
        cond = threading.Event()
        self.client.on_subscribe = (
            lambda client, userdata, mid, reason, prop: cond.set()
        )
        self.client.subscribe(self.response_topic)
        cond.wait()
        self.client.on_subscribe = None
        LOGGER.info(f"Subscribed to {self.response_topic}")

    def close(self):
        cond = threading.Event()
        self.client.on_unsubscribe = (
            lambda client, userdata, mid, reason, prop: cond.set()
        )
        self.client.unsubscribe(self.response_topic)
        cond.wait()
        self.client.on_unsubscribe = None
        LOGGER.info(f"Unsubscribed from {self.response_topic}")

    def _do(self, topic: str, *, response=1, timeout=None, **kwargs):
        response = int(response)

        props = Properties(PacketTypes.PUBLISH)
        ret = []
        event = threading.Event()

        if response:
            props.CorrelationData = uuid.uuid1().bytes
            props.ResponseTopic = self.response_topic

            def on_message(client, userdata, message):
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
                LOGGER.debug(
                    "Received %s: %s [%s]", message.topic, message.payload, properties
                )

                try:
                    cd = bytes.fromhex(properties["CorrelationData"])
                except KeyError:
                    LOGGER.info("Discarding message without CorrelationData")
                    return
                if cd != props.CorrelationData:
                    LOGGER.info(
                        f"Discarding message with unexpected CorrelationData: {cd}"
                    )
                    return

                try:
                    code = dict(properties["UserProperty"])["code"]
                except KeyError:
                    LOGGER.warning(
                        "Discarding message without response code user property"
                    )
                    return

                resp = message.payload.decode("utf-8")
                if code == "Continue":
                    ret.append(resp)
                elif code == "Ok":
                    if resp:
                        ret.append(resp)
                    event.set()
                else:
                    ret[:] = [MiniconfException(code, resp)]
                    event.set()

            self.client.on_message = on_message

        LOGGER.info(f"Publishing {topic}: {kwargs.get('payload')}, [{props}]")
        pub = self.client.publish(topic, properties=props, **kwargs)

        if response:
            event.wait(timeout)
            self.client.on_message = None
            if len(ret) == 1 and isinstance(ret[0], MiniconfException):
                raise ret[0]
            elif response == 1:
                if len(ret) != 1:
                    raise MiniconfException("Not a leaf", ret)
                return ret[0]
            else:
                assert ret
                return ret
        # else:
        #    pub.wait_for_publish(timeout)

    def set(self, path: str, value, retain=False, response=True, **kwargs):
        """Write the provided data to the specified path.

        Args:
            path: The path to set.
            value: The value to set.
            retain: Retain the the setting on the broker.
        """
        return self._do(
            topic=f"{self.prefix}/settings{path}",
            payload=json.dumps(value, separators=(",", ":")),
            response=response,
            retain=retain,
            **kwargs,
        )

    def list(self, root: str = "", **kwargs):
        """Get a list of all the paths below a given root.

        Args:
            root: Path to the root node to list.
        """
        return self._do(topic=f"{self.prefix}/settings{root}", response=2, **kwargs)

    def dump(self, root: str = "", **kwargs):
        """Dump all the paths at or below a given root into the settings namespace.

        Note that the target may be unable to respond to messages when a multipart
        operation (list or dump) is in progress.
        This method does not wait for completion.

        Args:
            root: Path to the root node to dump. Can be a leaf or an internal node.
        """
        return self._do(topic=f"{self.prefix}/settings{root}", response=0, **kwargs)

    def get(self, path: str, **kwargs):
        """Get the specific value of a given path.

        Args:
            path: The path to get. Must be a leaf node.
        """
        return self._do(topic=f"{self.prefix}/settings{path}", **kwargs)

    def clear(self, path: str, response=True, **kwargs):
        """Clear retained value from a path.

        Args:
            path: The path to clear. Must be a leaf node.
        """
        return self._do(
            f"{self.prefix}/settings{path}",
            retain=True,
            response=response,
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

    discovered = {}

    def on_message(client, userdata, message):
        logging.debug(f"Got message from {message.topic}: {message.payload}")
        peer = message.topic.removesuffix(suffix)
        try:
            payload = json.loads(message.payload)
        except json.JSONDecodeError:
            logging.info(f"Ignoring {peer} not/invalid alive")
        else:
            logging.info(f"Discovered {peer} alive")
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
    import sys
    from .async_ import _cli, Path, MQTTv5, one

    args = _cli().parse_args()

    logging.basicConfig(
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        level=logging.WARN - 10 * args.verbose,
    )

    client = Client(paho.mqtt.enums.CallbackAPIVersion.VERSION2, protocol=MQTTv5)
    client.connect(args.broker)
    client.enable_logger()
    client.loop_start()

    if args.discover:
        prefix, _alive = one(discover(client, args.prefix))
    else:
        prefix = args.prefix

    interface = Miniconf(client, prefix)

    try:
        current = Path()
        for arg in args.commands:
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
                        print(f"{path}={paths[0]}")
                        continue
                    for p in paths:
                        try:
                            value = interface.get(p)
                            print(f"{p}={value}")
                        except MiniconfException as err:
                            print(f"{p}: {repr(err)}")
                elif arg.endswith("!"):
                    path = current.normalize(arg.removesuffix("!"))
                    interface.dump(path)
                    print(f"DUMP '{path}'")
                elif "=" in arg:
                    path, value = arg.split("=", 1)
                    path = current.normalize(path)
                    if not value:
                        interface.clear(path)
                        print(f"CLEAR '{path}'")
                    else:
                        interface.set(path, json.loads(value), args.retain)
                        print(f"{path}={value}")
                else:
                    path = current.normalize(arg)
                    value = interface.get(path)
                    print(f"{path}={value}")
            except MiniconfException as err:
                print(f"{arg}: {repr(err)}")
                sys.exit(1)
    finally:
        interface.close()
        client.disconnect()
        client.loop_stop()


if __name__ == "__main__":
    _main()
