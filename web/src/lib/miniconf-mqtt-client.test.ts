import { describe, expect, it } from "vitest";
import { MiniconfMqttClient } from "./miniconf-mqtt-client";
import { MqttBus } from "./mqtt-bus";
import { FakeMqttClient, ResponseMqttClient } from "./mqtt-test-fixture";

describe("MiniconfMqttClient subscriptions", () => {
  it("clears discovered prefixes on broker reconnect before retained alive replay", async () => {
    const mqtt = new FakeMqttClient();
    const client = new MiniconfMqttClient(new MqttBus(mqtt as never));
    const updates: string[][] = [];

    const stop = client.watchDiscovery("dt/+", (prefixes) => {
      updates.push(prefixes.map((discovered) => discovered.prefix));
    });
    await Promise.resolve();
    mqtt.completeSubscribe();
    mqtt.publishRetained(
      "dt/device/alive",
      JSON.stringify({ proto: 1, epoch: 1, schema_rev: 1, pages: 1 }),
    );

    expect(updates.at(-1)).toEqual(["dt/device"]);

    mqtt.emit("connect");
    expect(updates.at(-1)).toEqual([]);

    stop();
  });

  it("streams only retained authoritative settings", async () => {
    const mqtt = new FakeMqttClient();
    const client = new MiniconfMqttClient(new MqttBus(mqtt as never));
    const changes: unknown[] = [];

    client.watchSettings("dt/device", "/sub", (change) => changes.push(change));
    await Promise.resolve();
    mqtt.completeSubscribe();
    mqtt.publishRetained("dt/device/settings/sub/a", "1");
    mqtt.publishRetained("dt/device/settings/sub/b", "2", { auth: "bad" });
    mqtt.publishRetained("dt/device/settings/sub/c", "3", { auth: ["", ""] });
    mqtt.publishRetained("dt/device/settings/sub/d", "4", { auth: "" });

    expect(changes).toEqual([{ path: "/sub/d", value: 4, present: true, rev: undefined }]);
  });

  it("resolves schema when all pages arrive", async () => {
    const mqtt = new FakeMqttClient();
    const client = new MiniconfMqttClient(new MqttBus(mqtt as never));
    const schema = client.schema("dt/device", { proto: 1, epoch: 1, schema_rev: 2, pages: 2 });
    await Promise.resolve();
    mqtt.completeSubscribe();

    mqtt.publishRetained("dt/device/schema/1", "{\"i\":{\"k\":\"n\",\"c\":{}},\"m\":{}}\n");
    mqtt.publishRetained("dt/device/schema/0", "{\"s\":\"value\"}\n");

    await expect(schema).resolves.toMatchObject({ rev: 2 });
  });

  it("rejects invalid Miniconf setting roots", async () => {
    const client = new MiniconfMqttClient(new MqttBus(new FakeMqttClient() as never));

    expect(() => client.watchSettings("dt/device", "sub", () => {})).toThrow(
      'Settings root must be empty or start with "/"',
    );
  });
});

describe("MiniconfMqttClient set", () => {
  it("matches overlapping set responses by correlation data", async () => {
    const mqtt = new ResponseMqttClient();
    const client = new MiniconfMqttClient(new MqttBus(mqtt as never));

    const first = client.set("dt/device", "/a", 1);
    const second = client.set("dt/device", "/b", 2);
    for (let i = 0; i < 4; i += 1) {
      await Promise.resolve();
    }

    mqtt.respondToSet("/b", "Ok");
    mqtt.respondToSet("/a", "BadRequest", "invalid");

    await expect(second).resolves.toMatchObject({ path: "/b", ok: true, code: "Ok" });
    await expect(first).resolves.toMatchObject({
      path: "/a",
      ok: false,
      code: "BadRequest",
      message: "invalid",
    });
  });

  it("validates set path and payload at the protocol boundary", async () => {
    const client = new MiniconfMqttClient(new MqttBus(new ResponseMqttClient() as never));

    await expect(client.set("dt/device", "a", 1)).rejects.toThrow(
      'Path must be empty or start with "/"',
    );
    await expect(client.set("dt/device", "/a", undefined)).rejects.toThrow(
      "Set value must be JSON-serializable",
    );
  });
});
