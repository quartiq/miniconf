#!/usr/bin/env python3

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from queue import Empty, Queue

import paho.mqtt.client as mqtt
from miniconf.client import Miniconf, RawMiniconf
from miniconf.cli import _normalize_command_path
from miniconf.common import MiniconfException
from miniconf._mqtt import Client
from miniconf.render import render_schema_tree, render_value_tree
from miniconf.schema import Indices, Packed, Schema, SchemaNode

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "testdata" / "compact-schema" / "fixture.ndjson"

PREFIX = "test"
TARGET = f"{PREFIX}/common"
BROKER = os.environ.get("BROKER", "localhost")
CONTROL = "/control"
ENABLED = "/control/enabled"
MODE = "/control/mode"
DAC = "/output/dac"
DAC0 = "/output/dac/0"
DAC1 = "/output/dac/1"
SERIAL = "/serial"
EXAMPLE = Path(
    os.environ.get(
        "MINICONF_EXAMPLE",
        str(ROOT / "target" / "debug" / "examples" / "miniconf"),
    )
)


def cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-m", "miniconf", "-b", BROKER, *args],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )


def cli_stdout(*args: str) -> str:
    proc = cli(*args)
    if proc.returncode != 0:
        raise AssertionError(
            f"cli failed: {args}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout


def fixture_schema() -> Schema:
    defs = [json.loads(line) for line in FIXTURE.read_text().splitlines() if line]
    return Schema.from_defs(defs, 1)


def schema_node(schema: Schema, path: str) -> SchemaNode:
    return SchemaNode(path, schema.node(path).schema)


class TopicWatcher:
    def __init__(self, topic: str):
        self.topic = topic
        self._messages: Queue[tuple[str, str]] = Queue()
        self._subscribed = False
        self.client = mqtt.Client(
            mqtt.CallbackAPIVersion.VERSION2, protocol=mqtt.MQTTv5
        )
        self.client.on_message = self._on_message
        self.client.on_subscribe = self._on_subscribe
        self.client.connect(BROKER)
        self.client.loop_start()
        self.client.subscribe(self.topic)
        end = time.monotonic() + 1.0
        while not self._subscribed:
            if time.monotonic() >= end:
                raise TimeoutError("MQTT subscribe timed out")
            time.sleep(0.01)

    def _on_message(self, _client, _userdata, message):
        self._messages.put((message.topic, message.payload.decode("utf-8")))

    def _on_subscribe(self, _client, _userdata, _mid, _reason, _properties):
        self._subscribed = True

    def drain(self) -> None:
        while True:
            try:
                self._messages.get_nowait()
            except Empty:
                return

    def collect(
        self, timeout: float, topic: str | None = None
    ) -> list[tuple[str, str]]:
        end = time.monotonic() + timeout
        messages = []
        while True:
            remaining = end - time.monotonic()
            if remaining <= 0:
                return messages
            try:
                candidate = self._messages.get(timeout=remaining)
            except Empty:
                return messages
            if topic is None or mqtt.topic_matches_sub(topic, candidate[0]):
                messages.append(candidate)

    def wait_payload(self, timeout: float, topic: str | None = None) -> str:
        end = time.monotonic() + timeout
        while True:
            remaining = end - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"Timed out waiting for {topic or self.topic}")
            try:
                candidate = self._messages.get(timeout=remaining)
            except Empty as exc:
                raise TimeoutError(
                    f"Timed out waiting for {topic or self.topic}"
                ) from exc
            if topic is None or mqtt.topic_matches_sub(topic, candidate[0]):
                return candidate[1]

    def wait_nonempty_payload(self, timeout: float, topic: str | None = None) -> str:
        end = time.monotonic() + timeout
        while True:
            payload = self.wait_payload(max(0.0, end - time.monotonic()), topic)
            if payload:
                return payload

    def wait_schema_pages(
        self, timeout: float, prefix: str, pages: int
    ) -> list[tuple[str, str]]:
        expected = {f"{prefix}/schema/{index}" for index in range(pages)}
        seen: dict[str, str] = {}
        end = time.monotonic() + timeout
        while expected - seen.keys():
            remaining = end - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"Timed out waiting for schema pages under {prefix}")
            try:
                topic, payload = self._messages.get(timeout=remaining)
            except Empty as exc:
                raise TimeoutError(
                    f"Timed out waiting for schema pages under {prefix}"
                ) from exc
            if not mqtt.topic_matches_sub(f"{prefix}/schema/#", topic):
                continue
            seen[topic] = payload
        return sorted(seen.items())

    def close(self) -> None:
        self.client.disconnect()
        self.client.loop_stop()


class FakeMessages:
    def __aiter__(self):
        return self

    async def __anext__(self):
        await asyncio.Future()


class FakeClient:
    def __init__(self):
        self.messages = FakeMessages()

    async def subscribe(self, *_args, **_kwargs):
        pass

    async def unsubscribe(self, *_args, **_kwargs):
        pass


async def test_listener_close_tolerates_released_subscription() -> None:
    client = RawMiniconf(FakeClient(), "test")
    await asyncio.wait_for(client._subscribed.wait(), 1.0)
    del client._subscriptions[client.response_topic]
    await client.close()


async def close_client(client: Client, timeout: float = 1.0) -> None:
    """Bound MQTT teardown so the harness cannot hang on disconnect."""

    await asyncio.wait_for(client.__aexit__(None, None, None), timeout)


async def wait_event_value(events, path: str, expected, timeout: float = 3.0):
    end = asyncio.get_running_loop().time() + timeout
    while True:
        remaining = end - asyncio.get_running_loop().time()
        if remaining <= 0:
            raise TimeoutError(f"Timed out waiting for event {path}={expected!r}")
        event = await asyncio.wait_for(events.__anext__(), remaining)
        if event.path == path and event.present and event.value == expected:
            return event


async def wait_event_delete(events, path: str, timeout: float = 3.0):
    end = asyncio.get_running_loop().time() + timeout
    while True:
        remaining = end - asyncio.get_running_loop().time()
        if remaining <= 0:
            raise TimeoutError(f"Timed out waiting for delete event {path}")
        event = await asyncio.wait_for(events.__anext__(), remaining)
        if event.path == path and not event.present:
            return event


async def wait_snapshot_value(
    client, root: str, path: str, expected, timeout: float = 3.0
):
    end = asyncio.get_running_loop().time() + timeout
    while True:
        snapshot = await client.snapshot(
            root, timeout=max(0.1, end - asyncio.get_running_loop().time())
        )
        if snapshot.get(path, object()) == expected:
            return snapshot
        remaining = end - asyncio.get_running_loop().time()
        if remaining <= 0:
            raise TimeoutError(f"Timed out waiting for snapshot {path}={expected!r}")
        await asyncio.sleep(min(0.01, remaining))


async def main() -> None:
    await test_listener_close_tolerates_released_subscription()

    assert _normalize_command_path("", "/channel/0") == ("", "/channel/0")
    assert _normalize_command_path("/channel/0/demodulate", "") == (
        "/channel/0/demodulate",
        "/channel/0/demodulate",
    )
    assert _normalize_command_path("frequency", "/channel/0/demodulate") == (
        "/channel/0/demodulate/frequency",
        "/channel/0/demodulate",
    )
    assert _normalize_command_path("attenuation", "/channel/0/demodulate") == (
        "/channel/0/demodulate/attenuation",
        "/channel/0/demodulate",
    )
    assert _normalize_command_path(
        "/channel/0/demodulate/frequency", "", subtree=False
    ) == ("/channel/0/demodulate/frequency", "/channel/0/demodulate")
    assert _normalize_command_path("phase", "/channel/0/demodulate") == (
        "/channel/0/demodulate/phase",
        "/channel/0/demodulate",
    )

    schema_fixture = fixture_schema()
    assert [node.path for node in schema_fixture.walk()] == [
        "",
        "/value",
        "/nested",
        "/nested/leaf",
    ]
    assert schema_fixture.node().kind == "named"
    assert schema_fixture.node("/nested").kind == "named"
    assert schema_fixture.node("/value").kind == "leaf"
    assert schema_fixture.node("/value").edge == {"role": "selector"}
    assert schema_fixture.node("/nested").edge is None
    assert schema_fixture.compact("/nested") == {
        "path": "/nested",
        "rev": 1,
        "defs": [
            {},
            {"i": {"k": "n", "c": {"leaf": 0}}},
        ],
    }

    compressed_sem = render_schema_tree(
        Schema.from_defs(
            [
                {"s": {"ty": "i32"}},
                {"i": {"k": "h", "l": 2, "c": 0}},
                {"i": {"k": "n", "c": {"array_tree": 1}}},
            ],
            1,
        )
    ).splitlines()
    assert compressed_sem == [
        "└─ array_tree [homogeneous]",
        "   └─ 0..2 [sem ty=i32]",
    ], compressed_sem
    quoted_meta = render_schema_tree(
        Schema.from_defs(
            [
                {"m": {"typename": "InnerType"}},
                {"i": {"k": "n", "c": {"node": {"r": 0, "m": {"doc": "Outer doc"}}}}},
            ],
            1,
        )
    ).splitlines()
    assert quoted_meta == [
        '└─ node [edge doc="Outer doc"] [node typename="InnerType"]'
    ], quoted_meta

    alive = TopicWatcher(f"{PREFIX}/+/alive")
    settings = TopicWatcher(f"{PREFIX}/+/settings/#")
    schema_topics = TopicWatcher(f"{PREFIX}/+/schema/#")
    dut = None
    mc = None
    client = None
    try:
        alive.collect(1.0)
        alive.drain()
        settings.drain()
        schema_topics.drain()

        dut = subprocess.Popen([str(EXAMPLE)], cwd=ROOT)

        manifest = json.loads(alive.wait_nonempty_payload(5.0, f"{TARGET}/alive"))
        assert manifest["proto"] == 1, manifest
        assert manifest["epoch"] > 0, manifest
        assert manifest["pages"] > 0, manifest
        assert manifest["schema_rev"], manifest
        schema_topics.wait_schema_pages(3.0, TARGET, manifest["pages"])
        alive.drain()
        settings.drain()
        schema_topics.drain()

        client = Client(BROKER)
        await client.__aenter__()
        try:
            mc = Miniconf(client, TARGET)

            schema = await mc.schema()
            control = schema.node(CONTROL)
            dac = schema.node(DAC)
            control_schema = schema.compact(CONTROL)
            assert control.kind == "named", control
            assert dac.kind == "homogeneous", dac
            assert dac.schema["internal"]["len"] == 2, dac
            assert control_schema["path"] == CONTROL, control_schema
            assert control_schema["defs"][-1]["i"]["k"] == "n", control_schema
            assert set(control_schema["defs"][-1]["i"]["c"]) == {
                "enabled",
                "mode",
            }, control_schema
            assert control.schema["internal"]["kind"] == "named"
            assert control.node["typename"] == "Control"
            assert schema.node(SERIAL).edge["doc"] == "Hardware serial number."
            assert dac.edge["max"] == "4095"
            assert schema.node(MODE) == schema_node(schema, MODE)
            assert schema.path(DAC0) == DAC0
            try:
                schema.path("/missing")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for /missing")
            assert [node.path for node in schema.walk(CONTROL)] == [
                CONTROL,
                ENABLED,
                MODE,
            ]
            assert schema_node(schema, CONTROL) in schema.children("")
            assert schema.children(CONTROL) == [
                schema_node(schema, ENABLED),
                schema_node(schema, MODE),
            ]
            assert schema.node(DAC0).kind == "leaf"
            assert control.kind == "named"
            assert schema.path(schema.indices(DAC1)) == DAC1
            assert schema.path(Indices(schema.indices(ENABLED))) == ENABLED
            assert schema.path(Packed(schema.packed(ENABLED).value)) == ENABLED
            try:
                schema.path("/")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for '/'")
            connected_value = None
            async with Miniconf.connect(BROKER, TARGET) as connected:
                connected_value = await connected.get(ENABLED)
            assert connected_value is True
            snapshot = await mc.snapshot(CONTROL)
            assert snapshot[ENABLED] is True
            assert snapshot[MODE] == "Run"
            try:
                await mc.get(CONTROL)
            except MiniconfException as err:
                assert err.code == "LeafRequired", err
            else:
                raise AssertionError(f"expected leaf-required error for {CONTROL}")

            dump = await mc.snapshot(CONTROL)
            assert dump[ENABLED] is True
            assert dump[MODE] == "Run"
            assert render_value_tree(schema, dump, CONTROL).splitlines() == [
                "control",
                "├─ enabled = true",
                '└─ mode = "Run"',
            ]

            await mc.set(ENABLED, True)
            assert await mc.get(ENABLED) is True
            settings.drain()
            await mc.set(ENABLED, False, response=False)
            assert settings.wait_payload(3.0, f"{TARGET}/settings{ENABLED}") == "false"
            events = mc.watch(ENABLED)
            try:
                await wait_event_value(events, ENABLED, False)
                await mc.set(ENABLED, True)
                await wait_event_value(events, ENABLED, True)
                await mc.set(ENABLED, False)
                await wait_event_value(events, ENABLED, False)
            finally:
                await events.aclose()
            output = await mc.snapshot("/output")
            assert output[DAC0] == 1024
            events = mc.watch("/output")
            try:
                await wait_event_value(events, DAC0, 1024)
                await mc.set(DAC0, 2048)
                await wait_event_value(events, DAC0, 2048)
            finally:
                await events.aclose()
            await wait_snapshot_value(mc, "/output", DAC0, 2048)
        finally:
            await close_client(client)
            client = None
        client = await Client(BROKER).__aenter__()
        raw = RawMiniconf(client, TARGET)
        try:
            assert await raw.get(ENABLED) is False
            assert await raw.get(DAC0) == 2048
            assert (await raw.snapshot(ENABLED))[ENABLED] is False
            null_path = "/raw-null"
            events = raw.watch(null_path)
            try:
                await client.publish(
                    f"{TARGET}/settings{null_path}",
                    payload=b"null",
                    qos=1,
                    retain=True,
                    properties={"user_property": [("auth", "")]},
                )
                event = await wait_event_value(events, null_path, None)
                assert event.retained
                assert event.rev is None
                await client.publish(
                    f"{TARGET}/settings{null_path}",
                    payload=b"",
                    qos=1,
                    retain=True,
                    properties={"user_property": [("auth", "")]},
                )
                event = await wait_event_delete(events, null_path)
                assert event.retained
            finally:
                await events.aclose()
            await raw.set(ENABLED, True)
            assert settings.wait_payload(3.0, f"{TARGET}/settings{ENABLED}") == "true"
            assert await raw.get(ENABLED) is True
            settings.drain()
            await raw.set(ENABLED, False, response=False)
            assert settings.wait_payload(3.0, f"{TARGET}/settings{ENABLED}") == "false"
        finally:
            await raw.close()
            await close_client(client)
            client = None
        schema_out = cli_stdout(TARGET, "?").splitlines()
        assert any("control" in line for line in schema_out), schema_out
        schema_raw = cli_stdout(TARGET, "??").splitlines()
        schema_dump = [json.loads(line) for line in schema_raw]
        assert schema_dump[-1]["i"]["k"] == "n", schema_dump
        assert "control" in schema_dump[-1]["i"]["c"], schema_dump
        dump_out = cli_stdout(TARGET, "!").splitlines()
        assert any("control" in line for line in dump_out), dump_out
        assert any("─ enabled " in line for line in dump_out), dump_out
        leaf_dump_rel = cli_stdout(TARGET, "control/enabled!").strip()
        assert leaf_dump_rel == "enabled = false", leaf_dump_rel
        leaf_dump = cli_stdout(TARGET, f"{ENABLED}!").strip()
        assert leaf_dump == "enabled = false", leaf_dump
        dump_raw = cli_stdout(TARGET, "!!").splitlines()
        assert any(line.startswith(f"{ENABLED}=") for line in dump_raw), dump_raw
        leaf_dump_raw_rel = cli_stdout(TARGET, "control/enabled!!").strip()
        assert leaf_dump_raw_rel == f"{ENABLED}=false", leaf_dump_raw_rel
        raw_out = cli_stdout("--raw", TARGET, ENABLED).strip()
        assert raw_out == f"{ENABLED}=false", raw_out
        raw_discover_out = cli_stdout("--raw", "-d", f"{PREFIX}/+", ENABLED).strip()
        assert raw_discover_out == f"{ENABLED}=false", raw_discover_out
        for command in ("/", "/?", "/!"):
            invalid = cli(TARGET, command)
            assert invalid.returncode != 0, invalid
            assert "NotFound:" in invalid.stdout, invalid.stdout
        branch_invalid = cli(TARGET, CONTROL)
        assert branch_invalid.returncode != 0, branch_invalid
        assert "LeafRequired:" in branch_invalid.stdout, branch_invalid.stdout
        raw_invalid = cli("--raw", TARGET, "/?")
        assert raw_invalid.returncode != 0, raw_invalid
        assert "RawMode:" in raw_invalid.stdout, raw_invalid.stdout

        stale_schema_topic = f"{TARGET}/schema/99"
        stale_setting_topic = f"{TARGET}/settings/obsolete"
        schema_topics.client.publish(stale_schema_topic, b'{"bad":true}', retain=True)
        settings.client.publish(stale_setting_topic, b"1", retain=True)
        time.sleep(0.2)
        prune_out = subprocess.run(
            [
                sys.executable,
                "-m",
                "miniconf",
                "-b",
                "localhost",
                "--prune",
                "",
                TARGET,
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        assert "schema/99" in prune_out, prune_out
        force_prune_out = subprocess.run(
            [
                sys.executable,
                "-m",
                "miniconf",
                "-b",
                "localhost",
                "--force-prune",
                TARGET,
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        assert "settings/obsolete" in force_prune_out, force_prune_out

    finally:
        if mc is not None:
            await mc.close()
        if client is not None:
            with contextlib.suppress(Exception):
                await close_client(client)
        settings.close()
        alive.close()
        schema_topics.close()
        if dut is not None:
            dut.kill()
            with contextlib.suppress(Exception):
                dut.wait(timeout=1.0)


if __name__ == "__main__":
    asyncio.run(main())
