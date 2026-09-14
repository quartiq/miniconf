import { afterEach, describe, expect, it, vi } from "vitest";
import { MqttSession, type MqttSessionStatus } from "./mqtt-session";
import { FakeMqttClient } from "./mqtt-test-fixture";

const originalLocation = Object.getOwnPropertyDescriptor(
  globalThis,
  "location",
);
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
    await expect(
      MqttSession.connect("ws://mqtt:8083", {}, callbacks()),
    ).rejects.toThrow("HTTPS pages cannot connect to ws:// brokers");
    expect(connectMock).not.toHaveBeenCalled();
  });

  it("starts one-shot with explicit options, then enables reconnect", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const subscriptions = {
      "dt/device/alive": { qos: 1 as const },
      "dt/device/settings/#": { qos: 1 as const },
    };
    const connecting = MqttSession.connect(
      "ws://mqtt:8083",
      subscriptions,
      callbacks(),
      { auth: { username: "", password: "secret" } },
    );
    expect(connectMock.mock.calls[0][1]).toMatchObject({
      clean: true,
      protocolVersion: 5,
      queueQoSZero: false,
      reconnectPeriod: 0,
      resubscribe: false,
      username: "",
      password: "secret",
    });
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;

    expect(mqtt.subscriptions).toEqual([subscriptions]);
    expect(mqtt.options.reconnectPeriod).toBe(1000);
    expect(session.ready).toBe(true);
    session.close();
  });

  it("restores the same subscription map once on reconnect", async () => {
    const mqtt = new FakeMqttClient();
    const statuses: MqttSessionStatus[] = [];
    const resets: string[] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const subscriptions = { "dt/device/#": { qos: 1 as const } };
    const connecting = MqttSession.connect(
      "ws://mqtt:8083",
      subscriptions,
      callbacks(statuses, resets),
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
      "ws://mqtt:8083",
      subscriptions,
      callbacks(statuses),
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
      state: "failed",
      error: "MQTT subscription rejected: dt/device/alive",
    });
  });

  it("fails a one-shot initial close without entering a reconnect loop", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = MqttSession.connect("ws://mqtt:8083", {}, callbacks());
    mqtt.emit("close");
    await expect(connecting).rejects.toThrow(
      "Could not connect to ws://mqtt:8083",
    );
    expect(mqtt.ended).toBe(true);
    expect(connectMock.mock.calls[0][1].reconnectPeriod).toBe(0);
  });

  it("fails a rejected initial SUBACK and closes the client", async () => {
    const mqtt = new FakeMqttClient();
    mqtt.rejectedTopic = "dt/device/alive";
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = MqttSession.connect(
      "ws://mqtt:8083",
      { "dt/device/alive": { qos: 1 } },
      callbacks(),
    );
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    await expect(connecting).rejects.toThrow(
      "MQTT subscription rejected: dt/device/alive",
    );
    expect(mqtt.ended).toBe(true);
  });
});

it("cancels initial connection and pending subscriptions", async () => {
  for (const connected of [false, true]) {
    const mqtt = new FakeMqttClient();
    mqtt.subscribeWait = new Promise(() => {});
    const controller = new AbortController();
    connectMock.mockReturnValueOnce(mqtt);
    const pending = MqttSession.connect(
      "ws://mqtt:8083",
      { "a/#": { qos: 1 } },
      callbacks(),
      { signal: controller.signal },
    );
    if (connected) {
      mqtt.connect();
      await Promise.resolve();
    }
    controller.abort();
    await expect(pending).rejects.toThrow(/cancelled|aborted/);
    expect(mqtt.ended).toBe(true);
  }
});

it("bounds SUBACK waits and ignores a delayed result after failure", async () => {
  vi.useFakeTimers();
  try {
    const mqtt = new FakeMqttClient();
    let release!: () => void;
    mqtt.subscribeWait = new Promise<void>((resolve) => {
      release = resolve;
    });
    connectMock.mockReturnValueOnce(mqtt);
    const pending = MqttSession.connect(
      "ws://mqtt:8083",
      { "a/#": { qos: 1 } },
      callbacks(),
    );
    const rejected = expect(pending).rejects.toThrow(
      "acknowledgment timed out",
    );
    mqtt.connect();
    await vi.advanceTimersByTimeAsync(10_000);
    await rejected;
    release();
    await Promise.resolve();
    expect(mqtt.ended).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});

it("coalesces refresh requests without losing a generation during SUBACK", async () => {
  const mqtt = new FakeMqttClient();
  connectMock.mockReturnValueOnce(mqtt);
  const connecting = MqttSession.connect(
    "ws://mqtt:8083",
    { "a/#": { qos: 1 } },
    callbacks(),
  );
  mqtt.connect();
  const session = await connecting;
  let release!: () => void;
  mqtt.subscribeWait = new Promise<void>((resolve) => {
    release = resolve;
  });
  const first = session.refresh();
  const second = session.refresh();
  expect(first).toBe(second);
  expect(session.ready).toBe(false);
  mqtt.subscribeWait = undefined;
  release();
  await second;
  expect(mqtt.subscriptions).toHaveLength(3);
  expect(session.ready).toBe(true);
  session.close();
});

it("removes only the cancelled publication from the MQTT outgoing store", async () => {
  const mqtt = new FakeMqttClient();
  connectMock.mockReturnValueOnce(mqtt);
  const connecting = MqttSession.connect(
    "ws://mqtt:8083",
    { "p/#": { qos: 1 } },
    callbacks(),
  );
  mqtt.connect();
  const session = await connecting;
  let release!: () => void;
  mqtt.publishWait = new Promise<void>((resolve) => {
    release = resolve;
  });
  const controller = new AbortController();
  const first = session.publish(
    "p/settings/old",
    "",
    { qos: 1, retain: true },
    controller.signal,
  );
  const rejected = expect(first).rejects.toThrow("cancelled");
  const second = session.publish("p/set/leaf", "2", { qos: 1 });
  controller.abort();
  await rejected;
  expect(mqtt.removedPublications).toEqual([1]);
  expect(mqtt.ended).toBe(false);
  release();
  await second;
  session.close();
});
