import { describe, expect, it, vi } from "vitest";
import { PrefixSession, type PrefixSessionCallbacks } from "./backend";
import { type SettingsChange } from "./miniconf-mqtt-client";
import { type MqttConnectionEvent } from "./mqtt-bus";
import { Schema } from "./schema";

class FakeClient {
  readonly calls: string[] = [];
  readonly schemaValue = new Schema([
    { s: "value" },
    { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
  ], 7);
  private connectionListener: ((event: MqttConnectionEvent) => void) | undefined;
  private settingsListener: ((change: SettingsChange) => void) | undefined;
  private aliveListener: ((alive: { proto: number; epoch: number; schema_rev: number; pages: number } | undefined) => void) | undefined;
  private aliveResult = { proto: 1, epoch: 1, schema_rev: 7, pages: 1 };
  private replayAlive = true;
  private settingsResult: Map<string, unknown> = new Map([["/leaf", 1]]);

  async schema(
    prefix: string,
    _alive: unknown,
    options?: { progress?: (progress: { received: number; total: number }) => void },
  ) {
    this.calls.push(`schema ${prefix}`);
    options?.progress?.({ received: 1, total: 1 });
    return this.schemaValue;
  }

  watchConnection(listener: (event: MqttConnectionEvent) => void) {
    this.calls.push("watchConnection");
    this.connectionListener = listener;
    return () => this.calls.push("stopConnection");
  }

  watchAlive(
    prefix: string,
    listener: (alive: { proto: number; epoch: number; schema_rev: number; pages: number } | undefined) => void,
  ) {
    this.calls.push(`watchAlive ${prefix}`);
    this.aliveListener = listener;
    if (this.replayAlive) {
      queueMicrotask(() => listener(this.aliveResult));
    }
    return () => this.calls.push(`stopAlive ${prefix}`);
  }

  watchSettings(prefix: string, root: string, listener: (change: SettingsChange) => void) {
    this.calls.push(`watchSettings ${prefix} ${root}`);
    this.settingsListener = listener;
    queueMicrotask(() => this.replaySettings());
    return () => this.calls.push(`stopSettings ${prefix} ${root}`);
  }

  async set(prefix: string, path: string, value: unknown) {
    this.calls.push(`set ${prefix} ${path} ${JSON.stringify(value)}`);
    return { path, ok: true, code: "Ok", message: "" };
  }

  publishSetting(change: SettingsChange) {
    this.settingsListener?.(change);
  }

  publishAlive(alive: { proto: number; epoch: number; schema_rev: number; pages: number }) {
    this.aliveResult = alive;
    this.settingsResult = new Map([["/leaf", alive.schema_rev]]);
    this.aliveListener?.(alive);
  }

  setSettings(settings: Map<string, unknown>) {
    this.settingsResult = settings;
  }

  setReplayAlive(replay: boolean) {
    this.replayAlive = replay;
  }

  reconnect() {
    this.connectionListener?.({ state: "connected" });
    this.connectionListener?.({ state: "subscriptions-restored" });
    queueMicrotask(() => this.replaySettings());
  }

  clearAlive() {
    this.aliveListener?.(undefined);
  }

  connectionError(error: string, transient = false) {
    this.connectionListener?.({ state: "error", error, transient });
  }

  private replaySettings() {
    for (const [path, value] of this.settingsResult) {
      this.settingsListener?.({ path, present: true, value });
    }
  }
}

describe("PrefixSession", () => {
  it("owns active prefix protocol flow behind callback events", async () => {
    vi.useFakeTimers();
    try {
      const client = new FakeClient();
      const commits: string[] = [];
      const statuses: string[] = [];
      const callbacks: PrefixSessionCallbacks = {
        error: (message) => statuses.push(`error ${message}`),
        alive: (alive) => statuses.push(`alive ${alive?.epoch ?? "none"}`),
        response: (response) => statuses.push(`response ${response.code} ${response.path}`),
        schema: (_schema, root) => statuses.push(`schema ${root || "/"}`),
        schemaProgress: ({ received, total }) => statuses.push(`progress ${received}/${total}`),
        settings: (commit) => commits.push(`${commit.changed.size}`),
        status: (status) => statuses.push(status),
      };
      const session = new PrefixSession(client as never, "dt/device", "", callbacks);

      await session.open();
      await vi.advanceTimersByTimeAsync(100);
      expect(client.calls).toEqual([
        "watchConnection",
        "watchAlive dt/device",
        "schema dt/device",
        "watchSettings dt/device ",
      ]);
      expect(commits).toEqual(["1"]);

      client.publishSetting({ path: "/leaf", present: true, value: 2 });
      await vi.advanceTimersByTimeAsync(100);
      expect(commits).toEqual(["1", "1"]);

      await session.set("/leaf", 3);
      session.close();
      expect(client.calls.slice(-4)).toEqual([
        "set dt/device /leaf 3",
        "stopConnection",
        "stopSettings dt/device ",
        "stopAlive dt/device",
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("restarts the settings stream after schema reload", async () => {
    vi.useFakeTimers();
    try {
      const client = new FakeClient();
      const session = new PrefixSession(client as never, "dt/device", "", {
        error: () => {},
        alive: () => {},
        response: () => {},
        schema: () => {},
        schemaProgress: () => {},
        settings: () => {},
        status: () => {},
      });

      await session.open();
      client.publishAlive({ proto: 1, epoch: 2, schema_rev: 8, pages: 1 });
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      expect(client.calls).toEqual([
        "watchConnection",
        "watchAlive dt/device",
        "schema dt/device",
        "watchSettings dt/device ",
        "schema dt/device",
        "stopSettings dt/device ",
        "watchSettings dt/device ",
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears and streams settings again after broker reconnect", async () => {
    vi.useFakeTimers();
    try {
      const client = new FakeClient();
      const commits: SettingsChange[][] = [];
      const session = new PrefixSession(client as never, "dt/device", "", {
        error: () => {},
        alive: () => {},
        response: () => {},
        schema: () => {},
        schemaProgress: () => {},
        settings: (commit) => {
          commits.push([...commit.settings].map(([path, value]) => ({
            path,
            present: true,
            value,
          })));
        },
        status: () => {},
      });

      await session.open();
      await vi.advanceTimersByTimeAsync(100);
      expect(commits.at(-1)).toEqual([{ path: "/leaf", present: true, value: 1 }]);

      client.setSettings(new Map([["/leaf", 2]]));
      client.reconnect();
      await vi.advanceTimersByTimeAsync(100);

      expect(commits.at(-1)).toEqual([{ path: "/leaf", present: true, value: 2 }]);
      expect(client.calls).not.toContain("stopSettings dt/device ");
    } finally {
      vi.useRealTimers();
    }
  });

  it("cancels the initial alive wait when the session closes", async () => {
    const client = new FakeClient();
    client.setReplayAlive(false);
    const session = new PrefixSession(client as never, "dt/device", "", {
      error: () => {},
      alive: () => {},
      response: () => {},
      schema: () => {},
      schemaProgress: () => {},
      settings: () => {},
      status: () => {},
    });

    const opened = session.open();
    await Promise.resolve();
    session.close();

    await expect(opened).rejects.toThrow("Prefix session closed");
    expect(client.calls).toContain("stopAlive dt/device");
  });

  it("does not surface transient reconnect timeouts as app errors", async () => {
    const client = new FakeClient();
    const errors: string[] = [];
    const statuses: string[] = [];
    const session = new PrefixSession(client as never, "dt/device", "", {
      error: (error) => errors.push(error),
      alive: () => {},
      response: () => {},
      schema: () => {},
      schemaProgress: () => {},
      settings: () => {},
      status: (status) => statuses.push(status),
    });

    await session.open();
    client.connectionError("connack timeout", true);
    client.connectionError("bad credentials", false);

    expect(statuses).toContain("Broker reconnecting");
    expect(errors).toEqual(["bad credentials"]);
  });

  it("reports retained-empty alive as offline while opening", async () => {
    const client = new FakeClient();
    client.setReplayAlive(false);
    const statuses: string[] = [];
    const alive: string[] = [];
    const session = new PrefixSession(client as never, "dt/device", "", {
      error: () => {},
      alive: (next) => alive.push(String(next?.epoch ?? "none")),
      response: () => {},
      schema: () => {},
      schemaProgress: () => {},
      settings: () => {},
      status: (status) => statuses.push(status),
    });

    const opened = session.open();
    await Promise.resolve();
    client.clearAlive();

    expect(statuses).toContain("Prefix offline; waiting for alive");
    expect(alive).toContain("none");

    session.close();
    await expect(opened).rejects.toThrow("Prefix session closed");
  });
});
