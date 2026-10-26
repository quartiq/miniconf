"""Session, deadline, and retained-stream contracts without a broker."""

import asyncio
import json
import socket
import time
from unittest import IsolatedAsyncioTestCase
from unittest.mock import AsyncMock, call, patch

from aiomqtt import Message, MqttError
from paho.mqtt.packettypes import PacketTypes
from paho.mqtt.properties import Properties
from paho.mqtt.reasoncodes import ReasonCode

from miniconf.client import Miniconf, RawMiniconf
from miniconf.common import RETAINED, _Deadline, MiniconfException, mqtt_client
from miniconf._ops import discover


def message(topic, payload=b"1", **properties):
    props = Properties(PacketTypes.PUBLISH)
    for name, value in properties.items():
        setattr(props, name, value)
    return Message(topic, payload, 1, True, 0, props)


class Transport:
    def __init__(self, retained=()):
        self.retained = retained
        self.incoming = asyncio.Queue()
        self.subscriptions = asyncio.Queue()
        self.messages = self.stream()
        self.subscribe = AsyncMock(side_effect=self.subscribed)
        self.unsubscribe = AsyncMock()
        self.publish = AsyncMock()

    async def subscribed(self, topic, **kwargs):
        topics = (
            [(topic, kwargs.get("options", RETAINED))]
            if isinstance(topic, str)
            else topic
        )
        for topic, _options in topics:
            self.subscriptions.put_nowait(topic)
        for item in self.retained:
            if any(
                item.topic.matches(topic) and options.retainHandling != 2
                for topic, options in topics
            ):
                self.incoming.put_nowait(item)
        return (1,) * len(topics)

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


ALIVE = dict(proto=1, epoch=1, schema_rev=0xE92760DE, pages=1)


def device_transport():
    return Transport(
        [
            message("test/alive", json.dumps(ALIVE).encode()),
            message(
                "test/schema/0",
                b'{}\n{"i":{"k":"n","c":{"leaf":0}}}\n'
                b'{"i":{"k":"n","c":{"value":{"r":0,"m":{"role":"selector"}},"nested":1}}}\n',
            ),
            message("test/settings/value", UserProperty=[("auth", "")]),
        ]
    )


class ClientTests(IsolatedAsyncioTestCase):
    async def test_subscribe_is_ready_without_initial_replay(self):
        for client_type in (Miniconf, RawMiniconf):
            transport = device_transport()
            async with client_type(transport, "test") as client:
                async with client.subscribe() as events:
                    self.assertEqual(transport.subscribe.await_count, 1)
                    topic, options = transport.subscribe.call_args.args[0][-1]
                    self.assertEqual(topic, "test/settings/#")
                    self.assertEqual(options.retainHandling, 2)
                    transport.incoming.put_nowait(
                        message(
                            "test/settings/value", b"2", UserProperty=[("auth", "")]
                        )
                    )
                    self.assertEqual(
                        (await asyncio.wait_for(anext(events), 1)).value, 2
                    )
                    await client.snapshot(abs_timeout=0.01)
                    self.assertEqual(
                        (await asyncio.wait_for(anext(events), 1)).value, 1
                    )
                    transport.unsubscribe.assert_not_awaited()
                transport.unsubscribe.assert_awaited_once_with(
                    "test/settings/#", timeout=1.0
                )

    async def test_connection_options(self):
        nodelay = (socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        keepalive = (socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1)
        override = (socket.IPPROTO_TCP, socket.TCP_NODELAY, 0)
        for options in ([], [keepalive], [keepalive, override]):
            with (
                self.subTest(options=options),
                patch("miniconf.common.Client") as client,
            ):
                transport = mqtt_client(
                    "[::1]:1884", socket_options=iter(options), username="reader"
                )
                self.assertIs(transport, client.return_value)
                client.assert_called_once_with(
                    "::1",
                    protocol=5,
                    port=1884,
                    username="reader",
                    socket_options=[nodelay, *options],
                )

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
        for rejected in range(3):
            with self.subTest(rejected=rejected):
                transport = Transport()
                codes = [1, 1, 1]
                codes[rejected] = ReasonCode(PacketTypes.SUBACK, "Not authorized")
                transport.subscribe.return_value = codes
                transport.subscribe.side_effect = None
                async with Miniconf(transport, "test") as client:
                    with self.assertRaisesRegex(MqttError, "Not authorized"):
                        await asyncio.wait_for(client.set("/value", 1), 1)
                transport.unsubscribe.assert_awaited_once_with(
                    [client.response_topic, client.alive_topic, "test/schema/#"],
                    timeout=1.0,
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

    async def test_schema_replay_is_in_initial_batch(self):
        transport = device_transport()
        # Brokers need not replay alive before schema pages.
        transport.retained.reverse()
        async with Miniconf(transport, "test") as client:
            schema = await client.schema()
            self.assertEqual(schema.node("/value").kind, "leaf")
            self.assertIs(await client.schema(), schema)
            transport.subscribe.assert_awaited_once_with(
                [
                    (client.response_topic, RETAINED),
                    (client.alive_topic, RETAINED),
                    ("test/schema/#", RETAINED),
                ]
            )

    async def test_listener_consumes_replay_before_suback(self):
        transport = device_transport()
        release = asyncio.Event()

        async def subscribing(topic, **kwargs):
            codes = await transport.subscribed(topic, **kwargs)
            await release.wait()
            return codes

        transport.subscribe.side_effect = subscribing
        async with Miniconf(transport, "test") as client:
            consumed = asyncio.Event()
            handle = client._handle_message

            def received(item):
                handle(item)
                if client._schema_pages:
                    consumed.set()

            with patch.object(client, "_handle_message", side_effect=received):
                await asyncio.wait_for(consumed.wait(), 1)
                self.assertFalse(client._ready.is_set())
                release.set()
                await client.schema()

    async def test_missing_initial_replay_recovers_on_next_operation(self):
        transport = device_transport()
        complete = transport.retained
        transport.retained = complete[:1]
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.schema(timeout=0.01)
            transport.retained = complete
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            self.assertEqual(transport.subscribe.await_count, 2)

    async def test_schema_parse_cannot_install_after_deadline(self):
        transport = device_transport()
        async with Miniconf(transport, "test") as client:
            await client._load_manifest(_Deadline(1))
            deadline = _Deadline(1)
            from miniconf.schema import Schema

            parse = Schema.from_defs

            def slow_parse(*args):
                result = parse(*args)
                deadline.end = 0
                return result

            with patch("miniconf.client.Schema.from_defs", side_effect=slow_parse):
                with self.assertRaises(TimeoutError):
                    await client._load_schema(deadline)
            self.assertIsNone(client._schema)

    async def test_schema_replay_reorders_pages_and_ignores_stale_indices(self):
        transport = device_transport()
        payload = transport.retained[1].payload
        first, rest = payload.split(b"\n", 1)
        transport.retained = [
            message("test/schema/1", rest),
            message("test/schema/99", b"obsolete"),
            message("test/schema/0", first + b"\n"),
            message("test/alive", json.dumps(ALIVE | {"pages": 2}).encode()),
        ]
        async with Miniconf(transport, "test") as client:
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            transport.subscribe.assert_awaited_once()

    async def test_schema_waits_for_matching_generation(self):
        transport = device_transport()
        correct = transport.retained[1].payload
        transport.retained[1] = message("test/schema/0", correct + b"\n")
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.schema(timeout=0.01)
            # A timed-out reader can request retained replay on its next attempt.
            transport.retained[1] = message("test/schema/0", correct)
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            self.assertEqual(transport.subscribe.await_count, 2)

    async def test_schema_readers_share_replay(self):
        transport = device_transport()
        page = transport.retained[1]
        transport.retained = transport.retained[:1]
        async with Miniconf(transport, "test") as client:
            waiting = asyncio.Event()
            readers = 0
            wait = client._schema_changed.wait

            async def wait_for_page():
                nonlocal readers
                readers += 1
                if readers == 2:
                    waiting.set()
                await wait()

            with patch.object(
                client._schema_changed, "wait", side_effect=wait_for_page
            ):
                first = asyncio.create_task(client.schema())
                second = asyncio.create_task(client.schema())
                await asyncio.wait_for(waiting.wait(), 1)
                transport.incoming.put_nowait(page)
                first, second = await asyncio.gather(first, second)
            self.assertIs(first, second)
            transport.subscribe.assert_awaited_once()

    async def test_schema_ignores_unretained_pages_and_tombstones(self):
        transport = device_transport()
        correct = transport.retained[1]
        correct.retain = False
        transport.retained.append(message("test/schema/0", b""))
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.schema(timeout=0.01)
            correct.retain = True
            transport.retained = transport.retained[:2]
            transport.incoming.put_nowait(correct)
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            self.assertEqual(transport.subscribe.await_count, 2)

    async def test_schema_detects_manifest_change_without_more_pages(self):
        transport = device_transport()
        transport.retained = transport.retained[:1]
        async with Miniconf(transport, "test") as client:
            manifest = await client._load_manifest(_Deadline(1))
            # Pin the manifest captured by this reader before the restart arrives.
            with patch.object(client, "_load_manifest", return_value=manifest):
                pending = asyncio.create_task(client.schema())
                await asyncio.sleep(0)
                transport.incoming.put_nowait(
                    message("test/alive", json.dumps(ALIVE | {"epoch": 2}).encode())
                )
                with self.assertRaisesRegex(MiniconfException, "Manifest changed"):
                    await asyncio.wait_for(pending, 1)

    async def test_protocol_change_invalidates_schema(self):
        transport = device_transport()
        async with Miniconf(transport, "test") as client:
            self.assertEqual((await client.schema()).node("/value").kind, "leaf")
            transport.incoming.put_nowait(
                message("test/alive", json.dumps(ALIVE | {"proto": 2}).encode())
            )
            with self.assertRaisesRegex(MiniconfException, "expected 1"):
                await client.set("/value", 1)
            transport.publish.assert_not_awaited()

    async def test_watch_manifest_changes(self):
        for change, code in (
            (None, "Offline"),
            ({"epoch": 2}, "Changed"),
            ({"schema_rev": 2}, "Changed"),
            ({"pages": 2}, "Changed"),
            ({"proto": 2}, "Protocol"),
        ):
            with self.subTest(change=change):
                transport = device_transport()
                async with Miniconf(transport, "test") as client:
                    watch = client.watch()
                    self.assertEqual((await anext(watch)).value, 1)
                    payload = json.dumps(ALIVE | change).encode() if change else b""
                    transport.incoming.put_nowait(message("test/alive", payload))
                    with self.assertRaises(MiniconfException) as error:
                        await asyncio.wait_for(anext(watch), 1)
                    self.assertEqual(error.exception.code, code)
                    self.assertEqual(
                        transport.unsubscribe.await_args_list,
                        [call("test/settings/#", timeout=1.0)],
                    )

    async def test_watch_shares_alive_subscription_and_reopens(self):
        transport = device_transport()
        async with Miniconf(transport, "test") as client:
            first, second = client.watch(), client.watch()
            self.assertEqual((await anext(first)).value, 1)
            self.assertEqual((await anext(second)).value, 1)
            await first.aclose()
            transport.unsubscribe.assert_not_awaited()

            # Equivalent JSON is harmless; a later setting proves the reader advances.
            transport.incoming.put_nowait(
                message("test/alive", json.dumps(ALIVE, indent=2).encode())
            )
            transport.incoming.put_nowait(
                message("test/settings/value", UserProperty=[("auth", "")])
            )
            self.assertEqual((await anext(second)).value, 1)

            # Recovery cannot erase an offline notification already in the stream.
            transport.incoming.put_nowait(message("test/alive", b""))
            transport.incoming.put_nowait(
                message("test/alive", json.dumps(ALIVE).encode())
            )
            with self.assertRaises(MiniconfException) as error:
                await asyncio.wait_for(anext(second), 1)
            self.assertEqual(error.exception.code, "Offline")
            reopened = client.watch()
            self.assertEqual((await asyncio.wait_for(anext(reopened), 1)).value, 1)
            await reopened.aclose()
            self.assertEqual(
                sum(
                    topic == "test/alive"
                    for args in transport.subscribe.await_args_list
                    for topic in (
                        [args.args[0]]
                        if isinstance(args.args[0], str)
                        else [item[0] for item in args.args[0]]
                    )
                ),
                1,
            )

    async def test_watch_observes_alive_during_subscribe(self):
        transport = device_transport()

        async def subscribe(topic, **kwargs):
            if topic == "test/settings/#":
                transport.incoming.put_nowait(message("test/alive", b""))
                transport.incoming.put_nowait(
                    message("test/alive", json.dumps(ALIVE).encode())
                )
                return (1,)  # No settings arrive to wake the watch.
            return await transport.subscribed(topic, **kwargs)

        transport.subscribe.side_effect = subscribe
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(MiniconfException) as error:
                await asyncio.wait_for(anext(client.watch()), 1)
            self.assertEqual(error.exception.code, "Offline")

    async def test_schema_and_get_share_deadline(self):
        transport = device_transport()

        async def subscribe(topic, **kwargs):
            if isinstance(topic, list) or topic == "test/settings/value":
                await asyncio.sleep(0.04)
            return await transport.subscribed(topic, **kwargs)

        transport.subscribe.side_effect = subscribe
        async with Miniconf(transport, "test") as client:
            with self.assertRaises(TimeoutError):
                await client.get("/value", timeout=0.06)
