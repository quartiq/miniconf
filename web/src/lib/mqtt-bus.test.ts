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

    const stopA = bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    expect(() => bus.watch("dt/device/settings/#", { qos: 0 }, () => {})).toThrow(
      "MQTT topic filter already subscribed",
    );
    await Promise.resolve();

    stopA();
    await Promise.resolve();

    expect(mqtt.subscriptions).toEqual(["dt/device/settings/#"]);
    expect(mqtt.unsubscriptions).toEqual(["dt/device/settings/#"]);
  });

  it("notifies reconnect before app-owned durable resubscribe and reports restoration", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);
    const order: string[] = [];
    mqtt.subscribeAsync = async (topic: string) => {
      order.push(`subscribe ${topic}`);
    };
    bus.watchConnection((event) => order.push(event.state));

    bus.watch("dt/device/settings/#", { qos: 0 }, () => {});
    await Promise.resolve();
    order.length = 0;

    mqtt.emit("connect");
    await Promise.resolve();
    await Promise.resolve();

    expect(order).toEqual(["connected", "subscribe dt/device/settings/#", "retained-replay-ready"]);
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

  it("does not resubscribe transient subscriptions on reconnect", async () => {
    const mqtt = new FakeMqttClient();
    const bus = new MqttBus(mqtt as never);

    await bus.withSubscription("dt/device/response/1", { qos: 0 }, async () => {
      mqtt.emit("connect");
      await Promise.resolve();
    });

    expect(mqtt.subscriptions).toEqual(["dt/device/response/1"]);
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
