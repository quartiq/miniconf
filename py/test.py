#!/usr/bin/env python3

from __future__ import annotations

import asyncio
import contextlib
import json
import subprocess
import time
from pathlib import Path
from queue import Empty, Queue

import paho.mqtt.client as mqtt
from aiomqtt import Client
from miniconf.async_ import MiniconfClient
from miniconf.common import MQTTv5, MiniconfException
from miniconf.render import render_schema_tree
from miniconf.schema import Indices, Packed, Schema, SchemaNode

ROOT = Path(__file__).resolve().parents[1]

PREFIX = "test"
TARGET = f"{PREFIX}/common"
EXAMPLE = ROOT / "target" / "debug" / "examples" / "miniconf"


def cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [".venv/bin/python", "-m", "miniconf", "-b", "localhost", *args],
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
        self.client.connect("localhost")
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


async def main() -> None:
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
        '└─ node [outer doc="Outer doc"] [inner typename="InnerType"]'
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

        subprocess.run(
            ["cargo", "build", "-p", "miniconf_mqtt", "--example", "miniconf"],
            cwd=ROOT,
            check=True,
        )
        dut = subprocess.Popen([str(EXAMPLE)], cwd=ROOT)

        manifest = json.loads(alive.wait_nonempty_payload(5.0, f"{TARGET}/alive"))
        assert manifest["boot_id"] > 0, manifest
        assert manifest["pages"] > 0, manifest
        assert manifest["schema_rev"], manifest
        schema_topics.wait_schema_pages(3.0, TARGET, manifest["pages"])
        alive.drain()
        settings.drain()
        schema_topics.drain()

        client = Client("localhost", protocol=MQTTv5)
        await client.__aenter__()
        try:
            mc = MiniconfClient(client, TARGET)

            schema = await mc.schema()
            await mc.watch("/struct_tree")
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
            assert schema.ty("/struct_tree")["internal"]["kind"] == "named"
            assert schema.inner("/struct_tree")["typename"] == "MyStruct"
            assert schema.outer("/struct_tree/b")["doc"] == "Outer doc"
            assert schema.node("/enum_tree/C") == SchemaNode(
                "/enum_tree/C", schema.ty("/enum_tree/C")
            )
            assert schema.contains("/enum_tree/C/0/a")
            assert not schema.contains("/missing")
            assert schema.paths("/struct_tree") == [
                "/struct_tree",
                "/struct_tree/a",
                "/struct_tree/b",
            ]
            assert SchemaNode(
                "/enum_tree/C", schema.ty("/enum_tree/C")
            ) in schema.children("/enum_tree")
            assert schema.parent("/enum_tree/C/0/a") == SchemaNode(
                "/enum_tree/C/0", schema.ty("/enum_tree/C/0")
            )
            assert schema.siblings("/struct_tree/a") == [
                SchemaNode("/struct_tree/a", schema.ty("/struct_tree/a")),
                SchemaNode("/struct_tree/b", schema.ty("/struct_tree/b")),
            ]
            assert schema.kind("/enum_tree/C/0/a") == "leaf"
            assert schema.kind("/struct_tree") == "named"
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
            assert await mc.cached("/struct_tree/a") == 0
            try:
                await mc.cached("/foo")
            except MiniconfException as err:
                assert err.code == "Untracked", err
            else:
                raise AssertionError("expected untracked error for /foo")
            try:
                await mc.watch("/")
            except MiniconfException as err:
                assert err.code == "NotFound", err
            else:
                raise AssertionError("expected lookup error for watch('/')")

            dump = await mc.snapshot("/struct_tree")
            assert dump["/struct_tree/a"] == 0
            assert dump["/struct_tree/b"] == 0

            await mc.watch("/foo")
            await mc.set("/foo", True)
            assert await mc.cached("/foo") is True
            settings.drain()
            await mc.set("/foo", False, response=False)
            assert settings.wait_payload(3.0, f"{TARGET}/settings/foo") == "false"

            await mc.watch("/array_tree2")
            assert await mc.cached("/array_tree2/0/a") == 0
            await mc.set("/array_tree2/0/a", 3)
            assert await mc.cached("/array_tree2/0/a") == 3
            tree_dump = await mc.snapshot("/array_tree2")
            assert tree_dump["/array_tree2/0/a"] == 3, tree_dump
        finally:
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
        dump_raw = cli_stdout(TARGET, "!!").splitlines()
        assert any(line.startswith("/struct_tree/a=") for line in dump_raw), dump_raw
        for command in ("/", "/?", "/!"):
            invalid = cli(TARGET, command)
            assert invalid.returncode != 0, invalid
            assert "NotFound:" in invalid.stdout, invalid.stdout

        stale_schema_topic = f"{TARGET}/schema/99"
        stale_setting_topic = f"{TARGET}/settings/obsolete"
        schema_topics.client.publish(stale_schema_topic, b'{"bad":true}', retain=True)
        settings.client.publish(stale_setting_topic, b"1", retain=True)
        time.sleep(0.2)
        prune_out = subprocess.run(
            [
                ".venv/bin/python",
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
        assert "/obsolete" in prune_out, prune_out

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
