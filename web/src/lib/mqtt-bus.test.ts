import { afterEach, describe, expect, it, vi } from "vitest";
import { MqttBus, topicMatches } from "./mqtt-bus";
import { FakeMqttClient } from "./mqtt-test-fixture";

const originalLocation = Object.getOwnPropertyDescriptor(globalThis, "location");
const connectMock = vi.hoisted(() => vi.fn());

vi.mock("mqtt", () => ({
  default: { connect: connectMock },
}));

afterEach(() => {
  connectMock.mockReset();
  if (originalLocation) {
    Object.defineProperty(globalThis, "location", originalLocation);
  } else {
    Reflect.deleteProperty(globalThis, "location");
  }
});

describe("MQTT browser transport", () => {
  it("matches MQTT wildcards", () => {
    expect(topicMatches("dt/sinara/+/+/alive", "dt/sinara/mpll/host/alive")).toBe(true);
    expect(topicMatches("/settings/#", "/settings")).toBe(true);
    expect(topicMatches("/settings/foo/#", "/settings/foo/bar")).toBe(true);
    expect(topicMatches("/settings/foo/#", "/settings/foobar")).toBe(false);
  });

  it("rejects insecure WebSocket brokers on HTTPS pages before opening a socket", async () => {
    Object.defineProperty(globalThis, "location", {
      configurable: true,
      value: { protocol: "https:" },
    });

    await expect(MqttBus.connect("ws://mqtt.ber.quartiq.de:8083")).rejects.toThrow(
      "HTTPS pages cannot connect to ws:// brokers",
    );
    expect(connectMock).not.toHaveBeenCalled();
  });

  it("uses one-shot initial connects and enables reconnect only after success", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValue(mqtt);

    const connected = MqttBus.connect("ws://mqtt:8083");
    expect(connectMock.mock.calls[0][1]).toMatchObject({
      protocolVersion: 5,
      reconnectPeriod: 0,
      resubscribe: false,
    });

    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.emit("connect");
    await connected;

    expect(mqtt.options.reconnectPeriod).toBe(1000);
  });

  it("does not keep reconnecting after an initial close", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValue(mqtt);

    const connected = MqttBus.connect("ws://mqtt:8083");
    mqtt.emit("close");

    await expect(connected).rejects.toThrow("Could not connect to ws://mqtt:8083");
    expect(connectMock.mock.calls[0][1]).toMatchObject({ reconnectPeriod: 0 });
    expect(mqtt.ended).toBe(true);
  });

  it("marks reconnect transport timeouts as transient connection errors", () => {
    const mqtt = new FakeMqttClient();
    const events: unknown[] = [];
    const bus = new MqttBus(mqtt as never);
    bus.watchConnection((event) => events.push(event));

    mqtt.emit("error", new Error("connack timeout"));
    mqtt.emit("error", new Error("bad credentials"));

    expect(events).toEqual([
      { state: "error", error: "connack timeout", transient: true },
      { state: "error", error: "bad credentials", transient: false },
    ]);
  });

  it("forbids shared exact subscriptions", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);

    const watchA = bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    expect(() => bus.watch("dt/device/settings/#", { qos: 0 }, () => {})).toThrow(
      "MQTT topic filter already subscribed",
    );
    await Promise.resolve();

    watchA.close();
    await Promise.resolve();

    expect(mqtt.subscriptions).toEqual(["dt/device/settings/#"]);
    expect(mqtt.unsubscriptions).toEqual(["dt/device/settings/#"]);
  });

  it("surfaces and cleans up an initial subscription failure", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    mqtt.subscribeAsync = async () => {
      throw new Error("subscribe failed");
    };

    const failed = bus.watch("dt/device/alive", { qos: 1 }, () => {});
    await expect(failed.ready).rejects.toThrow("subscribe failed");

    mqtt.subscribeAsync = async (topic: string) => [{ topic, qos: 1 as const }];
    const retry = bus.watch("dt/device/alive", { qos: 1 }, () => {});
    await expect(retry.ready).resolves.toBeUndefined();
    retry.close();
  });

  it("treats a rejected SUBACK grant as a subscription failure", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    mqtt.subscribeAsync = async (topic: string) => [{ topic, qos: 128 as const }];

    const watch = bus.watch("dt/device/alive", { qos: 1 }, () => {});

    await expect(watch.ready).rejects.toThrow(
      "MQTT subscription rejected: dt/device/alive",
    );
  });

  it("allows a watch to close while its initial SUBACK is pending", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    let resolve!: (grants: { topic: string; qos: 0 }[]) => void;
    mqtt.subscribeAsync = () =>
      new Promise((accept) => {
        resolve = accept;
      });

    const watch = bus.watch("dt/device/alive", { qos: 0 }, () => {});
    watch.close();
    resolve([{ topic: "dt/device/alive", qos: 0 }]);

    await expect(watch.ready).resolves.toBeUndefined();
    expect(mqtt.unsubscriptions).toEqual(["dt/device/alive"]);
  });

  it("notifies reconnect before app-owned durable resubscribe and reports restoration", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    const order: string[] = [];
    mqtt.subscribeAsync = async (topic: string) => {
      order.push(`subscribe ${topic}`);
      return [{ topic, qos: 0 as const }];
    };
    bus.watchConnection((event) => order.push(event.state));

    bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    await Promise.resolve();
    order.length = 0;

    mqtt.emit("connect");
    await Promise.resolve();
    await Promise.resolve();

    expect(order).toEqual(["connected", "subscribe dt/device/settings/#", "subscriptions-restored"]);
  });

  it("surfaces durable resubscribe failures", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    const events: string[] = [];

    bus.watchConnection((event) => events.push(`${event.state}:${event.error ?? ""}`));
    bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    await Promise.resolve();
    mqtt.subscribeAsync = async () => {
      throw new Error("subscribe failed");
    };

    mqtt.emit("connect");
    await Promise.resolve();
    await Promise.resolve();

    expect(events).toEqual(["connected:", "error:subscribe failed"]);
  });

  it("does not report restoration from an interrupted connection", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    const events: string[] = [];
    const initial = bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    await initial.ready;
    let resolveStale!: (grants: { topic: string; qos: 0 }[]) => void;
    let reconnect = 0;
    mqtt.subscribeAsync = (topic: string) => {
      reconnect += 1;
      if (reconnect === 1) {
        return new Promise((resolve) => {
          resolveStale = resolve;
        });
      }
      return Promise.resolve([{ topic, qos: 0 as const }]);
    };
    bus.watchConnection((event) => events.push(event.state));

    mqtt.emit("connect");
    mqtt.connected = false;
    mqtt.emit("close");
    mqtt.connected = true;
    mqtt.emit("connect");
    await Promise.resolve();
    await Promise.resolve();
    resolveStale([{ topic: "dt/device/settings/#", qos: 0 }]);
    await Promise.resolve();

    expect(events.filter((state) => state === "subscriptions-restored")).toHaveLength(1);
    initial.close();
  });

  it("publishes only while connected", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);

    await bus.publish("dt/device/set", "1", {});

    mqtt.connected = false;
    await expect(bus.publish("dt/device/set", "2", {})).rejects.toThrow("MQTT broker disconnected");
    expect(mqtt.publications).toEqual(["dt/device/set"]);
  });
});
