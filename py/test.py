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


async def close_client(client: Client, timeout: float = 1.0) -> None:
    """Bound aiomqtt teardown so the harness cannot hang on disconnect."""

    try:
        await asyncio.wait_for(client.__aexit__(None, None, None), timeout)
    except TimeoutError:
        client._client.disconnect()  # type: ignore[attr-defined]


async def wait_cached(tracked, path: str, expected, timeout: float = 3.0):
    end = asyncio.get_running_loop().time() + timeout
    while True:
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
            struct_tree = schema.node("/struct_tree")
            array_tree2 = schema.node("/array_tree2")
            struct_schema = schema.compact("/struct_tree")
            assert struct_tree.kind == "named", struct_tree
            assert array_tree2.kind == "homogeneous", array_tree2
            assert array_tree2.schema["internal"]["len"] == 2, array_tree2
            assert struct_schema["path"] == "/struct_tree", struct_schema
            assert struct_schema["defs"][-1]["i"]["k"] == "n", struct_schema
            assert set(struct_schema["defs"][-1]["i"]["c"]) == {
                "a",
                "b",
            }, struct_schema
            assert schema.node("/struct_tree").schema["internal"]["kind"] == "named"
            assert schema.node("/struct_tree").node["typename"] == "MyStruct"
            assert schema.node("/struct_tree/b").edge["doc"] == "Outer doc"
            assert schema.node("/enum_tree/C") == SchemaNode(
                "/enum_tree/C", schema.node("/enum_tree/C").schema
            )
            assert schema.path("/enum_tree/C/0/a") == "/enum_tree/C/0/a"
            try:
                schema.path("/missing")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for /missing")
            assert [node.path for node in schema.walk("/struct_tree")] == [
                "/struct_tree",
                "/struct_tree/a",
                "/struct_tree/b",
            ]
            assert SchemaNode(
                "/enum_tree/C", schema.node("/enum_tree/C").schema
            ) in schema.children("/enum_tree")
            assert schema.children("/struct_tree") == [
                SchemaNode("/struct_tree/a", schema.node("/struct_tree/a").schema),
                SchemaNode("/struct_tree/b", schema.node("/struct_tree/b").schema),
            ]
            assert schema.node("/enum_tree/C/0/a").kind == "leaf"
            assert schema.node("/struct_tree").kind == "named"
            assert schema.path(schema.indices("/array_tree2/1")) == "/array_tree2/1"
            assert (
                schema.path(Indices(schema.indices("/struct_tree/a")))
                == "/struct_tree/a"
            )
            assert (
                schema.path(Packed(schema.packed("/struct_tree/a").value))
                == "/struct_tree/a"
            )
            try:
                schema.path("/")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for '/'")
            async with mc.track("/struct_tree/a") as tracked:
                assert tracked.cached() == 0
            try:
                async with mc.track("/struct_tree") as tracked:
                    tracked.cached()
            except MiniconfException as err:
                assert err.code == "LeafRequired", err
            else:
                raise AssertionError("expected leaf-required error for /struct_tree")

            async with mc.track("/struct_tree") as tracked:
                assert tracked.cached("/struct_tree/a") == 0
                dump = tracked.snapshot()
                assert dump["/struct_tree/a"] == 0
                assert dump["/struct_tree/b"] == 0
                assert render_value_tree(schema, dump, tracked.root).splitlines() == [
                    "struct_tree",
                    "├─ a = 0",
                    "└─ b = 0",
                ]
                try:
                    async with mc.track("/foo"):
                        raise AssertionError("expected tracked-subtree conflict")
                except MiniconfException as err:
                    assert err.code == "Tracked", err

            await mc.set("/foo", True)
            async with mc.track("/foo") as tracked:
                assert tracked.cached() is True
            settings.drain()
            await mc.set("/foo", False, response=False)
            assert settings.wait_payload(3.0, f"{TARGET}/settings/foo") == "false"
            async with mc.track("/array_tree2") as tracked:
                assert tracked.cached("/array_tree2/0/a") == 0
                await mc.set("/array_tree2/0/a", 3)
                await wait_cached(tracked, "/array_tree2/0/a", 3)
                tree_dump = tracked.snapshot()
                assert tree_dump["/array_tree2/0/a"] == 3, tree_dump
        finally:
            await close_client(client)
            client = None
        client = await Client(BROKER, protocol=MQTTv5).__aenter__()
        raw = RawMiniconf(client, TARGET)
        try:
            assert await raw.get("/struct_tree/a") == 0
            assert await raw.get("/foo") is False
            await raw.set("/foo", True)
            assert settings.wait_payload(3.0, f"{TARGET}/settings/foo") == "true"
            assert await raw.get("/foo") is True
            settings.drain()
            await raw.set("/foo", False, response=False)
            assert settings.wait_payload(3.0, f"{TARGET}/settings/foo") == "false"
        finally:
            await raw.close()
            await close_client(client)
            client = None
        schema_out = cli_stdout(TARGET, "?").splitlines()
        assert any("struct_tree" in line for line in schema_out), schema_out
        schema_raw = cli_stdout(TARGET, "??").splitlines()
        schema_dump = [json.loads(line) for line in schema_raw]
        assert schema_dump[-1]["i"]["k"] == "n", schema_dump
        assert "struct_tree" in schema_dump[-1]["i"]["c"], schema_dump
        dump_out = cli_stdout(TARGET, "!").splitlines()
        assert any("struct_tree" in line for line in dump_out), dump_out
        assert any("─ a " in line for line in dump_out), dump_out
        leaf_dump_rel = cli_stdout(TARGET, "foo!").strip()
        assert leaf_dump_rel == "foo = false", leaf_dump_rel
        leaf_dump = cli_stdout(TARGET, "/foo!").strip()
        assert leaf_dump == "foo = false", leaf_dump
        dump_raw = cli_stdout(TARGET, "!!").splitlines()
        assert any(line.startswith("/struct_tree/a=") for line in dump_raw), dump_raw
        leaf_dump_raw_rel = cli_stdout(TARGET, "foo!!").strip()
        assert leaf_dump_raw_rel == "/foo=false", leaf_dump_raw_rel
        raw_out = cli_stdout("--raw", TARGET, "/struct_tree/a").strip()
        assert raw_out == "/struct_tree/a=0", raw_out
        raw_discover_out = cli_stdout("--raw", "-d", f"{PREFIX}/+", "/foo").strip()
        assert raw_discover_out == "/foo=false", raw_discover_out
        for command in ("/", "/?", "/!"):
            invalid = cli(TARGET, command)
            assert invalid.returncode != 0, invalid
            assert "NotFound:" in invalid.stdout, invalid.stdout
        branch_invalid = cli(TARGET, "/struct_tree")
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
