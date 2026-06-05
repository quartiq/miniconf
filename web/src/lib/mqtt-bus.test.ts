import { EventEmitter } from "node:events";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MqttBus, topicMatches } from "./mqtt-bus";

const originalLocation = Object.getOwnPropertyDescriptor(globalThis, "location");
const connectMock = vi.hoisted(() => vi.fn());

vi.mock("mqtt", () => ({
  default: { connect: connectMock },
}));

class FakeMqttClient extends EventEmitter {
  options: { reconnectPeriod?: number } = {};
  ended = false;

  end() {
    this.ended = true;
  }
}

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
      resubscribe: true,
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
});
