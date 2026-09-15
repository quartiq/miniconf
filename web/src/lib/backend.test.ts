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

function announce(mqtt: FakeMqttClient) {
  const alive = {
    proto: 1,
    epoch: 1,
    schema_rev: hash(...schemaText),
    pages: 2,
  };
  mqtt.message("dt/device/alive", JSON.stringify(alive));
  schemaText.forEach((page, index) =>
    mqtt.message(`dt/device/schema/${index}`, page),
  );
  return alive;
}

function callbacks(
  overrides: Partial<PrefixSessionCallbacks> = {},
): PrefixSessionCallbacks {
  return {
    alive: () => {},
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
  const connecting = PrefixSession.connect(
    "ws://mqtt:8083",
    "dt/device",
    subtree,
    nextCallbacks,
  );
  mqtt.options = connectMock.mock.calls.at(-1)![1];
  mqtt.connect();
  return await connecting;
}

afterEach(() => {
  connectMock.mockReset();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("DiscoverySession", () => {
  it("uses one fixed subscription and replaces retained discovery state after reconnect", async () => {
    const mqtt = new FakeMqttClient();
    const updates: string[][] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const connecting = DiscoverySession.connect("ws://mqtt:8083", "dt/+", {
      prefixes: (prefixes) =>
        updates.push(prefixes.map(({ prefix }) => prefix)),
      status: () => {},
    });
    mqtt.options = connectMock.mock.calls[0][1];
    mqtt.connect();
    const session = await connecting;

    expect(mqtt.subscriptions).toEqual([
      { "dt/+/alive": { qos: 1, rap: true, rh: 0 } },
    ]);
    mqtt.message(
      "dt/device/alive",
      '{"proto":1,"epoch":1,"schema_rev":2,"pages":1}',
    );
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
    await expect(
      DiscoverySession.connect("ws://mqtt:8083", "dt/#", {
        prefixes: () => {},
        status: () => {},
      }),
    ).rejects.toThrow("cannot contain #");
    expect(connectMock).not.toHaveBeenCalled();
  });
});

describe("PrefixSession", () => {
  it("assembles verified schema pages and streams only authoritative retained settings", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const roots: string[] = [];
    const commits: Array<Map<string, unknown>> = [];
    const session = await connectPrefix(
      mqtt,
      callbacks({
        schema: (_schema, root) => roots.push(root),
        settings: (commit) => commits.push(commit.settings),
      }),
    );

    const response = Object.keys(mqtt.subscriptions[0]).find(
      (topic) => topic.includes("/response/") && !topic.endsWith("/#"),
    );
    expect(Object.keys(mqtt.subscriptions[0])).toEqual([
      "dt/device/settings/#",
      "dt/device/set/#",
      "dt/device/response/#",
      "dt/device/alive",
      "dt/device/schema/#",
      response,
    ]);
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({
        proto: 1,
        epoch: 1,
        schema_rev: hash(...schemaText),
        pages: 2,
      }),
    );
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
    expect([...commits.at(-1)!]).toEqual([["/leaf", "4"]]);
    session.close();
  });

  it("invalidates writable state when retained alive becomes malformed", async () => {
    const mqtt = new FakeMqttClient();
    const errors: string[] = [];
    const alive: Array<number | undefined> = [];
    const revision = hash(...schemaText);
    const session = await connectPrefix(
      mqtt,
      callbacks({
        alive: (manifest) => alive.push(manifest?.epoch),
        status: (status) => {
          if ("error" in status) errors.push(status.error);
        },
      }),
    );
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({ proto: 1, epoch: 1, schema_rev: revision, pages: 2 }),
    );
    schemaText.forEach((page, index) =>
      mqtt.message(`dt/device/schema/${index}`, page),
    );
    const setting = session.set("/leaf", "1");
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(1));

    mqtt.message("dt/device/alive", "not json");

    await expect(setting).rejects.toThrow("Invalid alive manifest");
    expect(alive.at(-1)).toBeUndefined();
    expect(errors.at(-1)).toBeTruthy();
    await expect(session.set("/leaf", "2")).rejects.toThrow("not ready");
    session.close();
  });

  it("does not allocate the declared schema range before pages arrive", async () => {
    const mqtt = new FakeMqttClient();
    const session = await connectPrefix(mqtt);
    expect(() =>
      mqtt.message(
        "dt/device/alive",
        JSON.stringify({
          proto: 1,
          epoch: 1,
          schema_rev: 1,
          pages: Number.MAX_SAFE_INTEGER,
        }),
      ),
    ).not.toThrow();
    session.close();
  });

  it("keeps a verified schema but clears settings before retained replay on reconnect", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const schemas: number[] = [];
    const commits: Array<Map<string, unknown>> = [];
    const revision = hash(...schemaText);
    const session = await connectPrefix(
      mqtt,
      callbacks({
        schema: (schema) => schemas.push(schema.rev),
        settings: (commit) => commits.push(commit.settings),
      }),
    );
    announce(mqtt);
    mqtt.message("dt/device/settings/leaf", "1", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);

    mqtt.disconnect();
    mqtt.connect();
    expect([...commits.at(-1)!]).toEqual([]);
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(2));
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({ proto: 1, epoch: 2, schema_rev: revision, pages: 2 }),
    );
    mqtt.message("dt/device/settings/leaf", "2", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);

    expect(schemas).toEqual([revision]);
    expect([...commits.at(-1)!]).toEqual([["/leaf", "2"]]);
    session.close();
  });

  it("matches concurrent set responses and makes pending outcomes unknown on disconnect", async () => {
    const mqtt = new FakeMqttClient();
    const session = await connectPrefix(mqtt);
    announce(mqtt);

    const clock = vi.spyOn(performance, "now").mockReturnValue(100);
    // Device responses, not PUBACK or wall-clock time, terminate each measurement.
    mqtt.publishWait = new Promise(() => {});
    const first = session.set("/leaf", "1");
    clock.mockReturnValue(110);
    const second = session.set("/leaf", "2");
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(2));
    clock.mockReturnValue(150);
    mqtt.respond(1, "Ok");
    clock.mockReturnValue(175);
    mqtt.respond(0, "BadRequest", "invalid");
    await expect(second).resolves.toMatchObject({ ok: true, responseMs: 40 });
    await expect(first).resolves.toMatchObject({
      ok: false,
      message: "invalid",
      responseMs: 75,
    });

    const unknown = session.set("/leaf", "3");
    await vi.waitFor(() => expect(mqtt.publications).toHaveLength(3));
    mqtt.disconnect();
    await expect(unknown).rejects.toThrow("outcome unknown");
    await expect(session.set("/leaf", "4")).rejects.toThrow("not ready");
    session.close();
  });

  it("requires an empty-or-slash subtree before opening MQTT", async () => {
    await expect(
      PrefixSession.connect("ws://mqtt:8083", "dt/device", "sub", callbacks()),
    ).rejects.toThrow("Subtree path must be empty or start with");
    expect(connectMock).not.toHaveBeenCalled();
  });

  it("reports complete schema pages that do not match the manifest revision", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const errors: string[] = [];
    const session = await connectPrefix(
      mqtt,
      callbacks({
        status: (status) => {
          if ("error" in status) errors.push(status.error);
        },
      }),
    );
    mqtt.message(
      "dt/device/alive",
      '{"proto":1,"epoch":1,"schema_rev":1,"pages":2}',
    );
    schemaText.forEach((page, index) =>
      mqtt.message(`dt/device/schema/${index}`, page),
    );
    await vi.advanceTimersByTimeAsync(10_000);
    expect(errors).toEqual(["Schema pages do not match revision 1"]);
    session.close();
  });
});

describe("exact settings and generation boundaries", () => {
  it("reports device failure independently of the socket and recovers on repaired schema", async () => {
    const mqtt = new FakeMqttClient();
    const states: Array<{ state: string; ready: boolean }> = [];
    const session = await connectPrefix(
      mqtt,
      callbacks({
        status: (status, ready) => states.push({ state: status.state, ready }),
      }),
    );
    const broken = "not JSON";
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({
        proto: 1,
        epoch: 1,
        schema_rev: hash(broken),
        pages: 1,
      }),
    );
    mqtt.message("dt/device/schema/0", broken);
    expect(states.at(-1)).toEqual({ state: "device-error", ready: false });
    expect(mqtt.ended).toBe(false);
    await expect(session.set("/leaf", "1")).rejects.toThrow("not ready");
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({
        proto: 1,
        epoch: 2,
        schema_rev: hash(...schemaText),
        pages: 2,
      }),
    );
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(2));
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({
        proto: 1,
        epoch: 2,
        schema_rev: hash(...schemaText),
        pages: 2,
      }),
    );
    schemaText.forEach((page, i) =>
      mqtt.message(`dt/device/schema/${i}`, page),
    );
    await vi.waitFor(() =>
      expect(states.at(-1)).toEqual({ state: "watching", ready: true }),
    );
    expect(session.ready).toBe(true);
    session.close();
  });

  it.each(["timeout", "offline", "epoch", "close"])(
    "cancels outstanding Set delivery on %s",
    async (reason) => {
      vi.useFakeTimers();
      const mqtt = new FakeMqttClient();
      const session = await connectPrefix(mqtt);
      const alive = announce(mqtt);
      mqtt.publishWait = new Promise(() => {});
      const setting = session.set("/leaf", "1");
      const rejected = expect(setting).rejects.toThrow("outcome unknown");
      if (reason === "timeout") await vi.advanceTimersByTimeAsync(3000);
      else if (reason === "offline") mqtt.disconnect();
      else if (reason === "close") session.close();
      else
        mqtt.message("dt/device/alive", JSON.stringify({ ...alive, epoch: 2 }));
      await rejected;
      expect(mqtt.removedPublications).toEqual([1]);
      session.close();
    },
  );

  it("ignores obsolete paths, malformed UTF-8 and retained responses", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const commits: Array<Map<string, string>> = [];
    const session = await connectPrefix(
      mqtt,
      callbacks({ settings: (c) => commits.push(c.settings) }),
    );
    const text = '{"n":9007199254740993,"e":1e400}';
    mqtt.message("dt/device/settings/leaf", text, true, { auth: "" });
    mqtt.message("dt/device/settings/obsolete", "1", true, {
      auth: "",
      rev: "999",
    });
    mqtt.message(
      "dt/device/alive",
      JSON.stringify({
        proto: 1,
        epoch: 1,
        schema_rev: hash(...schemaText),
        pages: 2,
      }),
    );
    schemaText.forEach((page, i) =>
      mqtt.message(`dt/device/schema/${i}`, page),
    );
    await vi.advanceTimersByTimeAsync(100);
    expect([...commits.at(-1)!]).toEqual([["/leaf", text]]);
    mqtt.message(
      "dt/device/settings/leaf",
      new Uint8Array([34, 255, 34]),
      true,
      { auth: "" },
    );
    await vi.advanceTimersByTimeAsync(100);
    expect([...commits.at(-1)!]).toEqual([["/leaf", text]]);
    const setting = session.set("/leaf", text);
    expect(mqtt.publications.at(-1)?.payload).toBe(text);
    const publication = mqtt.publications.at(-1)!;
    mqtt.message(
      publication.options.properties!.responseTopic!,
      "stale response",
      true,
      { code: "Error" },
      publication.options.properties!.correlationData as Uint8Array,
    );
    mqtt.message("dt/device/settings/leaf", "20.0", true, { auth: "" });
    mqtt.respond(0, "Ok");
    await expect(setting).resolves.toMatchObject({ ok: true });
    // A successful reply exposes buffered device text without waiting for the UI batch timer.
    expect(commits.at(-1)?.get("/leaf")).toBe("20.0");
    session.close();
  });

  it("replays the latest schema and settings across overlapping generation refreshes", async () => {
    vi.useFakeTimers();
    const mqtt = new FakeMqttClient();
    const commits: Array<Map<string, string>> = [];
    const session = await connectPrefix(
      mqtt,
      callbacks({ settings: (c) => commits.push(c.settings) }),
    );
    const alive = announce(mqtt);
    mqtt.message("dt/device/settings/leaf", "1", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);
    let release!: () => void;
    mqtt.subscribeWait = new Promise<void>((resolve) => {
      release = resolve;
    });
    mqtt.message("dt/device/alive", JSON.stringify({ ...alive, epoch: 2 }));
    expect(session.ready).toBe(false);
    expect([...commits.at(-1)!]).toEqual([]);
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(2));
    mqtt.message("dt/device/alive", JSON.stringify({ ...alive, epoch: 2 }));
    const replacement = '{"s":"value"}\n{"i":{"k":"n","c":{"new":0}}}\n';
    const latest = {
      proto: 1,
      epoch: 3,
      schema_rev: hash(replacement),
      pages: 1,
    };
    mqtt.message("dt/device/alive", JSON.stringify(latest));
    mqtt.subscribeWait = undefined;
    release();
    await vi.waitFor(() => expect(mqtt.subscriptions).toHaveLength(3));
    mqtt.message("dt/device/alive", JSON.stringify(latest));
    mqtt.message("dt/device/schema/0", replacement);
    mqtt.message("dt/device/settings/leaf", "99", true, { auth: "" });
    mqtt.message("dt/device/settings/new", "2", true, { auth: "" });
    await vi.advanceTimersByTimeAsync(100);
    expect(session.ready).toBe(true);
    expect([...commits.at(-1)!]).toEqual([["/new", "2"]]);
    session.close();
  });
});
