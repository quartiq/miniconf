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
from aiomqtt import Client
from miniconf.client import Miniconf, RawMiniconf
from miniconf.cli import _normalize_command_path
from miniconf.common import MQTTv5, MiniconfException
from miniconf.render import render_schema_tree, render_value_tree
from miniconf.schema import Indices, Packed, Schema, SchemaNode

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "testdata" / "compact-schema" / "fixture.ndjson"

PREFIX = "test"
TARGET = f"{PREFIX}/common"
BROKER = os.environ.get("BROKER", "localhost")
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
    """Bound aiomqtt teardown so the harness cannot hang on disconnect."""

    try:
        await asyncio.wait_for(client.__aexit__(None, None, None), timeout)
    except TimeoutError:
        client._client.disconnect()  # type: ignore[attr-defined]


async def wait_cached(tracked, path: str, expected, timeout: float = 3.0):
    end = asyncio.get_running_loop().time() + timeout
    while True:
        with contextlib.suppress(TimeoutError):
            await tracked.wait_ready(max(0.0, end - asyncio.get_running_loop().time()))
        try:
            value = tracked.cached(path)
        except MiniconfException:
            value = object()
        if value == expected:
            return
        remaining = end - asyncio.get_running_loop().time()
        if remaining <= 0:
            raise TimeoutError(
                f"Timed out waiting for cached {path or '/'}={expected!r}"
            )
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

        client = Client(BROKER, protocol=MQTTv5)
        await client.__aenter__()
        try:
            mc = Miniconf(client, TARGET)

            schema = await mc.schema()
            control = schema.node("/control")
            dac = schema.node("/output/dac")
            control_schema = schema.compact("/control")
            assert control.kind == "named", control
            assert dac.kind == "homogeneous", dac
            assert dac.schema["internal"]["len"] == 2, dac
            assert control_schema["path"] == "/control", control_schema
            assert control_schema["defs"][-1]["i"]["k"] == "n", control_schema
            assert set(control_schema["defs"][-1]["i"]["c"]) == {
                "enabled",
                "mode",
            }, control_schema
            assert schema.node("/control").schema["internal"]["kind"] == "named"
            assert schema.node("/control").node["typename"] == "Control"
            assert schema.node("/serial").edge["doc"] == "Hardware serial number."
            assert schema.node("/output/dac").edge["max"] == "4095"
            assert schema.node("/control/mode") == SchemaNode(
                "/control/mode", schema.node("/control/mode").schema
            )
            assert schema.path("/output/dac/0") == "/output/dac/0"
            try:
                schema.path("/missing")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for /missing")
            assert [node.path for node in schema.walk("/control")] == [
                "/control",
                "/control/enabled",
                "/control/mode",
            ]
            assert SchemaNode(
                "/control", schema.node("/control").schema
            ) in schema.children("")
            assert schema.children("/control") == [
                SchemaNode("/control/enabled", schema.node("/control/enabled").schema),
                SchemaNode("/control/mode", schema.node("/control/mode").schema),
            ]
            assert schema.node("/output/dac/0").kind == "leaf"
            assert schema.node("/control").kind == "named"
            assert schema.path(schema.indices("/output/dac/1")) == "/output/dac/1"
            assert (
                schema.path(Indices(schema.indices("/control/enabled")))
                == "/control/enabled"
            )
            assert (
                schema.path(Packed(schema.packed("/control/enabled").value))
                == "/control/enabled"
            )
            try:
                schema.path("/")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for '/'")
            async with mc.track("/control/enabled") as tracked:
                assert tracked.cached() is True
            try:
                async with mc.track("/control") as tracked:
                    tracked.cached()
            except MiniconfException as err:
                assert err.code == "LeafRequired", err
            else:
                raise AssertionError("expected leaf-required error for /control")

            async with mc.track("/control") as tracked:
                assert tracked.cached("/control/enabled") is True
                dump = tracked.snapshot()
                assert dump["/control/enabled"] is True
                assert dump["/control/mode"] == "Run"
                assert render_value_tree(schema, dump, tracked.root).splitlines() == [
                    "control",
                    "├─ enabled = true",
                    '└─ mode = "Run"',
                ]
                try:
                    async with mc.track("/control/enabled"):
                        raise AssertionError("expected tracked-subtree conflict")
                except MiniconfException as err:
                    assert err.code == "Tracked", err

            await mc.set("/control/enabled", True)
            async with mc.track("/control/enabled") as tracked:
                assert tracked.cached() is True
            settings.drain()
            await mc.set("/control/enabled", False, response=False)
            assert (
                settings.wait_payload(3.0, f"{TARGET}/settings/control/enabled")
                == "false"
            )
            async with mc.track("/output") as tracked:
                assert tracked.cached("/output/dac/0") == 1024
                await mc.set("/output/dac/0", 2048)
                await wait_cached(tracked, "/output/dac/0", 2048)
                tree_dump = tracked.snapshot()
                assert tree_dump["/output/dac/0"] == 2048, tree_dump
        finally:
            await close_client(client)
            client = None
        client = await Client(BROKER, protocol=MQTTv5).__aenter__()
        raw = RawMiniconf(client, TARGET)
        try:
            assert await raw.get("/control/enabled") is False
            assert await raw.get("/output/dac/0") == 2048
            await raw.set("/control/enabled", True)
            assert (
                settings.wait_payload(3.0, f"{TARGET}/settings/control/enabled")
                == "true"
            )
            assert await raw.get("/control/enabled") is True
            settings.drain()
            await raw.set("/control/enabled", False, response=False)
            assert (
                settings.wait_payload(3.0, f"{TARGET}/settings/control/enabled")
                == "false"
            )
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
        leaf_dump = cli_stdout(TARGET, "/control/enabled!").strip()
        assert leaf_dump == "enabled = false", leaf_dump
        dump_raw = cli_stdout(TARGET, "!!").splitlines()
        assert any(line.startswith("/control/enabled=") for line in dump_raw), dump_raw
        leaf_dump_raw_rel = cli_stdout(TARGET, "control/enabled!!").strip()
        assert leaf_dump_raw_rel == "/control/enabled=false", leaf_dump_raw_rel
        raw_out = cli_stdout("--raw", TARGET, "/control/enabled").strip()
        assert raw_out == "/control/enabled=false", raw_out
        raw_discover_out = cli_stdout(
            "--raw", "-d", f"{PREFIX}/+", "/control/enabled"
        ).strip()
        assert raw_discover_out == "/control/enabled=false", raw_discover_out
        for command in ("/", "/?", "/!"):
            invalid = cli(TARGET, command)
            assert invalid.returncode != 0, invalid
            assert "NotFound:" in invalid.stdout, invalid.stdout
        branch_invalid = cli(TARGET, "/control")
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
