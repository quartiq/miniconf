import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DiscoverySession,
  PrefixSession,
  type PrefixSessionCallbacks,
} from "./backend";
import { FakeMqttClient } from "./mqtt-test-fixture";

const connectMock = vi.hoisted(() => vi.fn());

vi.mock("mqtt", () => ({
  default: { connect: connectMock },
}));

const schemaText = [
  '{"s":"value"}\n',
  '{"i":{"k":"n","c":{"leaf":0}},"m":{"typename":"App"}}\n',
];

function hash(...pages: string[]): number {
  let value = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(pages.join(""))) {
    value = Math.imul(value ^ byte, 0x01000193) >>> 0;
  }
  return value;
}

function callbacks(overrides: Partial<PrefixSessionCallbacks> = {}): PrefixSessionCallbacks {
  return {
    error: () => {},
    alive: () => {},
    response: () => {},
    schema: () => {},
    settings: () => {},
    status: () => {},
    ...overrides,
  };
}

async function connectPrefix(
  mqtt: FakeMqttClient,
  nextCallbacks = callbacks(),
  subtree = "",
): Promise<PrefixSession> {
  connectMock.mockReturnValueOnce(mqtt);
  const connecting = PrefixSession.connect("ws://mqtt:8083", "dt/device", subtree, nextCallbacks);
  mqtt.options = connectMock.mock.calls.at(-1)![1];
  mqtt.connect();
  return await connecting;
}

afterEach(() => {
  connectMock.mockReset();
  vi.useRealTimers();
});

describe("DiscoverySession", () => {
  it("uses one fixed subscription and replaces retained discovery state after reconnect", async () => {
    const mqtt = new FakeMqttClient();
    const updates: string[][] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = DiscoverySession.connect("ws://mqtt:8083", "dt/+", {
      prefixes: (prefixes) => updates.push(prefixes.map(({ prefix }) => prefix)),
      error: () => {},
      status: () => {},
    });
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;

    expect(mqtt.subscriptions).toEqual([{ "dt/+/alive": { qos: 1, rap: true, rh: 0 } }]);
    mqtt.message("dt/device/alive", '{"proto":1,"epoch":1,"schema_rev":2,"pages":1}');
    expect(updates.at(-1)).toEqual(["dt/device"]);
    mqtt.message("dt/device/alive", "", false);
    expect(updates.at(-1)).toEqual(["dt/device"]);
    mqtt.message("dt/device/alive", "not json");
    expect(updates.at(-1)).toEqual([]);

    mqtt.disconnect();
    mqtt.connect();
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(2));
    expect(updates.at(-1)).toEqual([]);
    session.close();
  });

  it("rejects discovery filters that consume the alive suffix", async () => {
    await expect(DiscoverySession.connect("ws://mqtt:8083", "dt/#", {
      prefixes: () => {}, error: () => {}, status: () => {},
    })).rejects.toThrow("cannot contain #");
    expect(connectMock).not.toHaveBeenCalled();
  });
});

describe("PrefixSession", () => {
  it("assembles verified schema pages and streams only authoritative retained settings", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const roots: string[] = [];
    const commits: Array<Map<string, unknown>> = [];
    const session = await connectPrefix(mqtt, callbacks({
      schema: (_schema, root) => roots.push(root),
      settings: (commit) => commits.push(commit.settings),
    }));

    const response = Object.keys(mqtt.subscriptions[0]).find((topic) => topic.includes("/response/"));
    expect(Object.keys(mqtt.subscriptions[0])).toEqual([
      "dt/device/alive",
      "dt/device/schema/#",
      "dt/device/settings/#",
      response,
    ]);
    mqtt.message("dt/device/alive", JSON.stringify({
      proto: 1, epoch: 1, schema_rev: hash(...schemaText), pages: 2,
    }));
    mqtt.message("dt/device/schema/0", schemaText[0], false);
    mqtt.message("dt/device/schema/1", schemaText[1]);
    expect(roots).toEqual([]);
    mqtt.message("dt/device/schema/0", schemaText[0]);
    expect(roots).toEqual([""]);

    mqtt.message("dt/device/settings/leaf", "1");
    mqtt.message("dt/device/settings/leaf", "2", true, { auth: "bad" });
    mqtt.message("dt/device/settings/leaf", "3", true, { auth: ["", ""] });
    mqtt.message("dt/device/settings/leaf", "4", true, { auth: "", rev: "9" });
    await vi.advanceTimersByTimeAsync(100);
    expect([...commits.at(-1)!]).toEqual([["/leaf", 4]]);
    session.close();
  });

  it("invalidates writable state when retained alive becomes malformed", async () => {
    const mqtt = new FakeMqttClient();
    const errors: string[] = [];
    const alive: Array<number | undefined> = [];
    const revision = hash(...schemaText);
    const session = await connectPrefix(mqtt, callbacks({
      alive: (manifest) => alive.push(manifest?.epoch),
      error: (error) => errors.push(error),
    }));
    mqtt.message("dt/device/alive", JSON.stringify({ proto: 1, epoch: 1, schema_rev: revision, pages: 2 }));
    schemaText.forEach((page, index) => mqtt.message(`dt/device/schema/${index}`, page));
    const setting = session.set("/leaf", 1);
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(1));

    mqtt.message("dt/device/alive", "not json");

    await expect(setting).rejects.toThrow("Invalid alive manifest");
    expect(alive.at(-1)).toBeUndefined();
    expect(errors.at(-1)).toBeTruthy();
    await expect(session.set("/leaf", 2)).rejects.toThrow("not ready");
    session.close();
  });

  it("does not allocate the declared schema range before pages arrive", async () => {
    const mqtt = new FakeMqttClient();
    const session = await connectPrefix(mqtt);
    expect(() => mqtt.message("dt/device/alive", JSON.stringify({
      proto: 1,
      epoch: 1,
      schema_rev: 1,
      pages: Number.MAX_SAFE_INTEGER,
    }))).not.toThrow();
    session.close();
  });

  it("keeps a verified schema but clears settings before retained replay on reconnect", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const schemas: number[] = [];
    const commits: Array<Map<string, unknown>> = [];
    const revision = hash(...schemaText);
    const session = await connectPrefix(mqtt, callbacks({
      schema: (schema) => schemas.push(schema.rev),
      settings: (commit) => commits.push(commit.settings),
    }));
    mqtt.message("dt/device/alive", JSON.stringify({ proto: 1, epoch: 1, schema_rev: revision, pages: 2 }));
    schemaText.forEach((page, index) => mqtt.message(`dt/device/schema/${index}`, page));
    mqtt.message("dt/device/settings/leaf", "1", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);

    mqtt.disconnect();
    mqtt.connect();
    expect([...commits.at(-1)!]).toEqual([]);
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(2));
    mqtt.message("dt/device/alive", JSON.stringify({ proto: 1, epoch: 2, schema_rev: revision, pages: 2 }));
    mqtt.message("dt/device/settings/leaf", "2", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);

    expect(schemas).toEqual([revision]);
    expect([...commits.at(-1)!]).toEqual([["/leaf", 2]]);
    session.close();
  });

  it("matches concurrent set responses and makes pending outcomes unknown on disconnect", async () => {
    const mqtt = new FakeMqttClient();
    const revision = hash(...schemaText);
    const session = await connectPrefix(mqtt);
    mqtt.message("dt/device/alive", JSON.stringify({ proto: 1, epoch: 1, schema_rev: revision, pages: 2 }));
    schemaText.forEach((page, index) => mqtt.message(`dt/device/schema/${index}`, page));

    const first = session.set("/leaf", 1);
    const second = session.set("/leaf", 2);
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(2));
    mqtt.respond(1, "Ok");
    mqtt.respond(0, "BadRequest", "invalid");
    await expect(second).resolves.toMatchObject({ ok: true });
    await expect(first).resolves.toMatchObject({ ok: false, message: "invalid" });

    const unknown = session.set("/leaf", 3);
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(3));
    mqtt.disconnect();
    await expect(unknown).rejects.toThrow("outcome unknown");
    await expect(session.set("/leaf", 4)).rejects.toThrow("not ready");
    session.close();
  });

  it("requires an empty-or-slash subtree before opening MQTT", async () => {
    await expect(PrefixSession.connect("ws://mqtt:8083", "dt/device", "sub", callbacks()))
      .rejects.toThrow("Subtree path must be empty or start with");
    expect(connectMock).not.toHaveBeenCalled();
  });

  it("reports complete schema pages that do not match the manifest revision", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const errors: string[] = [];
    const session = await connectPrefix(mqtt, callbacks({ error: (error) => errors.push(error) }));
    mqtt.message("dt/device/alive", '{"proto":1,"epoch":1,"schema_rev":1,"pages":2}');
    schemaText.forEach((page, index) => mqtt.message(`dt/device/schema/${index}`, page));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(errors).toEqual(["Schema pages do not match revision 1"]);
    session.close();
  });
});
