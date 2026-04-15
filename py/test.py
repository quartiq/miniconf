#!/usr/bin/env python3

from __future__ import annotations

import subprocess
import time
from pathlib import Path
from queue import Empty, Queue

import paho.mqtt.client as mqtt

ROOT = Path(__file__).resolve().parents[1]

from miniconf.common import MiniconfException
from miniconf.sync import Miniconf

PREFIX = "test"
TARGET = f"{PREFIX}/id"
EXAMPLE = ROOT / "target" / "debug" / "examples" / "mqtt"


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
                raise TimeoutError(f"Timed out waiting for {topic or self.topic}") from exc
            if topic is None or mqtt.topic_matches_sub(topic, candidate[0]):
                return candidate[1]

    def close(self) -> None:
        self.client.loop_stop()
        self.client.disconnect()


def main() -> None:
    alive = TopicWatcher(f"{PREFIX}/+/alive")
    settings = TopicWatcher(f"{PREFIX}/+/settings/#")
    dut = None
    client = None
    mc = None
    try:
        payloads = alive.collect(1.0)
        assert all(payload in ("", "0") for _, payload in payloads), payloads
        alive.drain()
        settings.drain()

        subprocess.run(
            [
                "cargo",
                "build",
                "-p",
                "miniconf_mqtt",
                "--example",
                "mqtt",
                "--features",
                "introspection",
            ],
            cwd=ROOT,
            check=True,
        )
        dut = subprocess.Popen([str(EXAMPLE)], cwd=ROOT)

        settings_dump = settings.collect(3.0)
        assert len(settings_dump) == 11, settings_dump
        assert alive.wait_payload(1.0) == '"hello"'
        alive.drain()
        settings.drain()

        client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, protocol=mqtt.MQTTv5)
        client.connect("localhost")
        client.loop_start()
        mc = Miniconf(client, TARGET)

        control = mc.schema("/control")
        tag = next(child for child in control["internal"]["children"] if child["name"] == "tag")
        assert tag["attrs"]["switches"] == "mode", control
        mode = mc.schema("/control/mode")
        assert mode["sem"]["oneof"] is True, mode
        mode_state = mc.state("/control/mode")
        assert mode_state == {"state": "present", "active": "A"}, mode_state
        schema = mc.schema("/afe")
        assert schema["internal"]["kind"] == "homogeneous", schema
        assert schema["internal"]["len"] == 2, schema
        state = mc.state("/opt")
        assert state["state"] == "absent", state

        mc.set("/stream", "192.0.2.16:9293")
        assert mc.get("/stream") == "192.0.2.16:9293"

        assert mc.get("/afe/0") == "G1"
        mc.set("/afe/0", "G10")
        assert mc.get("/afe/0") == "G10"
        assert mc.clear("/afe/0") == "G10"
        assert mc.list("/afe") == ["/afe/0", "/afe/1"]
        mc.dump("/afe")
        dump = settings.collect(1.5, f"{TARGET}/settings/afe/#")
        values = {(topic, payload) for topic, payload in dump if payload}
        assert (f"{TARGET}/settings/afe/0", '"G10"') in values, dump
        assert (f"{TARGET}/settings/afe/1", '"G1"') in values, dump

        mc.set("/four", 5)
        try:
            mc.set("/four", 2)
        except MiniconfException:
            pass
        else:
            raise AssertionError("expected validation error for /four=2")

        mc.set("/exit", True)
        dut.wait(timeout=5.0)
        dut = None

        payloads = alive.collect(1.0)
        assert all(payload in ("", "0") for _, payload in payloads), payloads
    finally:
        if mc is not None:
            mc.close()
        if client is not None:
            client.loop_stop()
            client.disconnect()
        settings.close()
        alive.close()
        if dut is not None:
            dut.kill()
            dut.wait()


if __name__ == "__main__":
    main()
