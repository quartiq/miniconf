import { afterEach, describe, expect, it, vi } from "vitest";
import { MqttSession, type MqttSessionStatus } from "./mqtt-session";
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

function callbacks(statuses: MqttSessionStatus[] = [], resets: string[] = []) {
  return {
    message: () => {},
    reset: () => resets.push("reset"),
    status: (status: MqttSessionStatus) => statuses.push(status),
  };
}

describe("MqttSession", () => {
  it("rejects insecure WebSocket brokers from HTTPS before opening a socket", async () => {
    Object.defineProperty(globalThis, "location", {
      configurable: true,
      value: { protocol: "https:" },
    });
    await expect(MqttSession.connect("ws://mqtt:8083", {}, callbacks())).rejects.toThrow(
      "HTTPS pages cannot connect to ws:// brokers",
    );
    expect(connectMock).not.toHaveBeenCalled();
  });

  it("starts one-shot, subscribes the complete map, then enables reconnect", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const subscriptions = {
      "dt/device/alive": { qos: 1 as const },
      "dt/device/settings/#": { qos: 1 as const },
    };
    const connecting = MqttSession.connect("ws://mqtt:8083", subscriptions, callbacks());
    expect(connectMock.mock.calls[0][1]).toMatchObject({
      clean: true,
      protocolVersion: 5,
      queueQoSZero: false,
      reconnectPeriod: 0,
      resubscribe: false,
    });
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;

    expect(mqtt.subscriptions).toEqual([subscriptions]);
    expect(mqtt.options.reconnectPeriod).toBe(1000);
    expect(session.ready).toBe(true);
    session.close();
  });

  it("passes an empty username explicitly for password-only credentials", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = MqttSession.connect(
      "ws://mqtt:8083",
      {},
      callbacks(),
      { username: "", password: "secret" },
    );
    expect(connectMock.mock.calls[0][1]).toMatchObject({ username: "", password: "secret" });
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;
    session.close();
  });

  it("restores the same subscription map once on reconnect", async () => {
    const mqtt = new FakeMqttClient();
    const statuses: MqttSessionStatus[] = [];
    const resets: string[] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const subscriptions = { "dt/device/#": { qos: 1 as const } };
    const connecting = MqttSession.connect(
      "ws://mqtt:8083", subscriptions, callbacks(statuses, resets),
    );
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;
    statuses.length = 0;
    resets.length = 0;

    mqtt.disconnect();
    mqtt.emit("reconnect");
    mqtt.connect();
    await vi.waitFor(() => expect(session.ready).toBe(true));

    expect(mqtt.subscriptions).toEqual([subscriptions, subscriptions]);
    expect(resets).toEqual(["reset"]);
    expect(statuses).toEqual([
      { state: "offline" },
      { state: "reconnecting" },
      { state: "restoring" },
      { state: "connected" },
    ]);
    session.close();
  });

  it("closes a connection whose restored subscriptions are rejected", async () => {
    const mqtt = new FakeMqttClient();
    const statuses: MqttSessionStatus[] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const subscriptions = { "dt/device/alive": { qos: 1 as const } };
    const connecting = MqttSession.connect(
      "ws://mqtt:8083", subscriptions, callbacks(statuses),
    );
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;

    mqtt.rejectedTopic = "dt/device/alive";
    mqtt.disconnect();
    mqtt.connect();
    await vi.waitFor(() => expect(mqtt.ended).toBe(true));

    expect(session.ready).toBe(false);
    expect(statuses.at(-1)).toEqual({
      state: "error",
      error: "MQTT subscription rejected: dt/device/alive",
    });
  });

  it("fails a one-shot initial close without entering a reconnect loop", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = MqttSession.connect("ws://mqtt:8083", {}, callbacks());
    mqtt.emit("close");
    await expect(connecting).rejects.toThrow("Could not connect to ws://mqtt:8083");
    expect(mqtt.ended).toBe(true);
    expect(connectMock.mock.calls[0][1].reconnectPeriod).toBe(0);
  });

  it("fails a rejected initial SUBACK and closes the client", async () => {
    const mqtt = new FakeMqttClient();
    mqtt.rejectedTopic = "dt/device/alive";
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = MqttSession.connect(
      "ws://mqtt:8083", { "dt/device/alive": { qos: 1 } }, callbacks(),
    );
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    await expect(connecting).rejects.toThrow("MQTT subscription rejected: dt/device/alive");
    expect(mqtt.ended).toBe(true);
  });
});
