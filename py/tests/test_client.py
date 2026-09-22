"""Session, deadline, and retained-stream contracts without a broker."""

import asyncio
import json
import time
from pathlib import Path
from unittest import IsolatedAsyncioTestCase
from unittest.mock import AsyncMock

from aiomqtt import Message, MqttError
from paho.mqtt.packettypes import PacketTypes
from paho.mqtt.properties import Properties
from paho.mqtt.reasoncodes import ReasonCode

from miniconf.client import Miniconf, RawMiniconf
from miniconf.common import MiniconfException
from miniconf._ops import discover


def message(topic, payload=b"1", **properties):
    props = Properties(PacketTypes.PUBLISH)
    for name, value in properties.items():
        setattr(props, name, value)
    return Message(topic, payload, 1, True, 0, props)


class Transport:
    def __init__(self):
        self.incoming = asyncio.Queue()
        self.subscriptions = asyncio.Queue()
        self.messages = self.stream()
        self.subscribe = AsyncMock(side_effect=self.subscribed)
        self.unsubscribe = AsyncMock()
        self.publish = AsyncMock()

    async def subscribed(self, topic, **_kwargs):
        self.subscriptions.put_nowait(topic)
        return (1,)

    async def stream(self):
        while True:
            item = await self.incoming.get()
            if isinstance(item, Exception):
                raise item
            yield item

    async def wait_subscription(self, topic):
        async with asyncio.timeout(1):
            while await self.subscriptions.get() != topic:
                pass


class ClientTests(IsolatedAsyncioTestCase):
    async def test_context_lifetime(self):
        transport = Transport()
        client = RawMiniconf(transport, "test")
        transport.subscribe.assert_not_awaited()
        with self.assertRaises(MqttError):
            await client.get("/value")
        async with client:
            await client.set("/value", 1, response=False)
            with self.assertRaises(MqttError):
                await client.__aenter__()
        for operation in (
            client.get("/value"),
            client.set("/value", 2, response=False),
        ):
            with self.assertRaises(MqttError):
                await operation
        transport.publish.assert_awaited_once()
        with self.assertRaises(MqttError):
            await client.__aenter__()

    async def test_startup_failure(self):
        transport = Transport()
        transport.subscribe.side_effect = [
            (1,),
            [ReasonCode(PacketTypes.SUBACK, "Not authorized")],
        ]
        async with Miniconf(transport, "test") as client:
            with self.assertRaisesRegex(MqttError, "Not authorized"):
                await asyncio.wait_for(client.set("/value", 1), 1)
        transport.unsubscribe.assert_awaited_once_with(
            [client.response_topic, client.alive_topic], timeout=1.0
        )

    async def test_transient_subscription_denial(self):
        transport = Transport()
        transport.subscribe.side_effect = [
            (1,),
            [ReasonCode(PacketTypes.SUBACK, "Not authorized")],
        ]
        async with RawMiniconf(transport, "test") as client:
            with self.assertRaisesRegex(MqttError, "Not authorized"):
                await client.get("/value")

    async def test_discovery_subscription_denial(self):
        transport = Transport()
        transport.subscribe.side_effect = None
        transport.subscribe.return_value = [
            ReasonCode(PacketTypes.SUBACK, "Not authorized")
        ]
        with self.assertRaisesRegex(MqttError, "Not authorized"):
            await discover(transport, "test/+")

    async def test_discovery_deadline_and_quiescence(self):
        transport = Transport()
        transport.incoming.put_nowait(
            message(
                "test/device/alive", b'{"proto":1,"epoch":1,"schema_rev":1,"pages":1}'
            )
        )
        self.assertEqual(
            list(await discover(transport, "test/+", abs_timeout=0.01)), ["test/device"]
        )
        with self.assertRaises(TimeoutError):
            await discover(Transport(), "test/+", timeout=0.01, abs_timeout=1)

    async def test_immediate_close(self):
        transport = Transport()
        async with RawMiniconf(transport, "test"):
            pass
        transport.subscribe.assert_not_awaited()

    async def test_disconnect_fails_requests_and_readers(self):
        transport = Transport()
        async with RawMiniconf(transport, "test") as client:
            watch = client.watch()
            tasks = [
                asyncio.create_task(operation)
                for operation in (
                    client.set("/value", 1),
                    client.get("/value"),
                    anext(watch),
                )
            ]
            await transport.wait_subscription("test/settings/#")
            transport.incoming.put_nowait(MqttError("connection lost"))
            for task in tasks:
                with self.assertRaisesRegex(MqttError, "connection lost"):
                    await asyncio.wait_for(task, 1)
            await watch.aclose()

    async def test_close_wakes_watch(self):
        transport = Transport()
        async with RawMiniconf(transport, "test") as client:
            watch = client.watch()
            pending = asyncio.create_task(anext(watch))
            await transport.wait_subscription("test/settings/#")
            await client.close()
            with self.assertRaises(MqttError):
                await asyncio.wait_for(pending, 1)
            await watch.aclose()

    async def test_timeout_does_not_cancel_shared_startup(self):
        transport = Transport()
        suback = asyncio.Event()

        async def subscribe(*_args, **_kwargs):
            await suback.wait()
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with RawMiniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.set("/value", 1, timeout=0.01)
            suback.set()
            await client.set("/value", 2, response=False, timeout=1)

    async def test_get_deadline_includes_suback(self):
        transport = Transport()

        async def subscribe(topic, **_kwargs):
            if topic.endswith("/value"):
                await asyncio.Future()
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with RawMiniconf(transport, "test") as client:
            # Distinguish the operation's deadline from the test's hang guard.
            async def read():
                with self.assertRaises(TimeoutError):
                    await client.get("/value", timeout=0.01)
                return "operation deadline"

            self.assertEqual(await asyncio.wait_for(read(), 1), "operation deadline")

    async def test_set_waits_for_puback_without_device_response(self):
        transport = Transport()
        puback = asyncio.Event()
        publishing = asyncio.Event()

        async def publish(*_args, **_kwargs):
            publishing.set()
            await puback.wait()

        transport.publish.side_effect = publish
        async with RawMiniconf(transport, "test") as client:
            pending = asyncio.create_task(client.set("/value", 1, response=False))
            await publishing.wait()
            self.assertFalse(pending.done())
            puback.set()
            await asyncio.wait_for(pending, 1)

    async def test_late_completion_does_not_beat_deadline(self):
        transport = Transport()

        async def publish(*_args, **_kwargs):
            time.sleep(0.03)  # Both completion and timeout become runnable together.

        transport.publish.side_effect = publish
        async with RawMiniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.set("/value", 1, response=False, timeout=0.01)

    async def test_malformed_reply_only_fails_its_request(self):
        transport = Transport()

        async def publish(_topic, *, properties, **_kwargs):
            transport.incoming.put_nowait(
                message(
                    properties.ResponseTopic,
                    CorrelationData=properties.CorrelationData,
                )
            )

        transport.publish.side_effect = publish
        async with RawMiniconf(transport, "test") as client:
            with self.assertRaisesRegex(MiniconfException, "Missing response code"):
                await client.set("/value", 1, timeout=1)
            transport.publish.side_effect = None
            await client.set("/value", 2, response=False, timeout=1)

    async def test_snapshot_replay_does_not_close_existing_watch(self):
        transport = Transport()

        async def subscribe(topic, **_kwargs):
            if topic == "test/settings/#":
                transport.incoming.put_nowait(
                    message("test/settings/value", UserProperty=[("auth", "")])
                )
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with RawMiniconf(transport, "test") as client:
            watch = client.watch()
            self.assertEqual((await anext(watch)).value, 1)
            self.assertEqual(await client.snapshot(abs_timeout=0.01), {"/value": 1})
            transport.unsubscribe.assert_not_awaited()
            self.assertEqual((await anext(watch)).value, 1)
            transport.incoming.put_nowait(
                message("test/settings/value", b"2", UserProperty=[("auth", "")])
            )
            self.assertEqual((await anext(watch)).value, 2)
            await watch.aclose()

    async def test_snapshot_deadline_is_not_quiescence(self):
        transport = Transport()
        async with RawMiniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.snapshot(timeout=0.01, abs_timeout=1)

    async def test_cancelled_snapshot_preserves_existing_watch(self):
        transport = Transport()

        async def subscribe(topic, **_kwargs):
            if topic == "test/settings/#":
                transport.incoming.put_nowait(
                    message("test/settings/value", UserProperty=[("auth", "")])
                )
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with RawMiniconf(transport, "test") as client:
            watch = client.watch()
            await anext(watch)
            started = asyncio.Event()

            async def blocked_subscribe(*_args, **_kwargs):
                started.set()
                await asyncio.Future()

            transport.subscribe.side_effect = blocked_subscribe
            pending = asyncio.create_task(client.snapshot())
            await started.wait()
            pending.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await pending
            transport.unsubscribe.assert_not_awaited()
            transport.incoming.put_nowait(
                message("test/settings/value", b"2", UserProperty=[("auth", "")])
            )
            self.assertEqual((await anext(watch)).value, 2)
            await watch.aclose()

    async def test_protocol_change_invalidates_schema(self):
        transport = Transport()
        manifest = dict(proto=1, epoch=1, schema_rev=1, pages=1)
        fixture = Path(__file__).resolve().parents[2] / "fixtures/compact-schema.ndjson"

        async def subscribe(topic, **_kwargs):
            if topic == "test/alive":
                transport.incoming.put_nowait(
                    message(topic, json.dumps(manifest).encode())
                )
            elif topic == "test/schema/#":
                transport.incoming.put_nowait(
                    message("test/schema/0", fixture.read_bytes())
                )
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with Miniconf(transport, "test") as client:
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            manifest["proto"] = 2
            transport.incoming.put_nowait(
                message("test/alive", json.dumps(manifest).encode())
            )
            with self.assertRaisesRegex(MiniconfException, "expected 1"):
                await client.set("/value", 1)
            transport.publish.assert_not_awaited()

    async def test_schema_and_get_share_deadline(self):
        transport = Transport()
        fixture = Path(__file__).resolve().parents[2] / "fixtures/compact-schema.ndjson"

        async def subscribe(topic, **_kwargs):
            if topic == "test/alive":
                transport.incoming.put_nowait(
                    message(topic, b'{"proto":1,"epoch":1,"schema_rev":1,"pages":1}')
                )
            elif topic == "test/schema/#":
                await asyncio.sleep(0.04)
                transport.incoming.put_nowait(
                    message("test/schema/0", fixture.read_bytes())
                )
            elif topic == "test/settings/value":
                await asyncio.sleep(0.04)
                transport.incoming.put_nowait(
                    message(topic, UserProperty=[("auth", "")])
                )
            return (1,)

        transport.subscribe.side_effect = subscribe
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.get("/value", timeout=0.06)
