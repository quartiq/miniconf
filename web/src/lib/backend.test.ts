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
  private settingsResult: Map<string, unknown> = new Map([["/leaf", 1]]);

  async aliveManifest(prefix: string) {
    this.calls.push(`alive ${prefix}`);
    return this.aliveResult;
  }

  async schema(prefix: string) {
    this.calls.push(`schema ${prefix}`);
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
    return () => this.calls.push(`stopAlive ${prefix}`);
  }

  watchSettings(prefix: string, root: string, listener: (change: SettingsChange) => void) {
    this.calls.push(`watchSettings ${prefix} ${root}`);
    this.settingsListener = listener;
    return () => this.calls.push(`stopSettings ${prefix} ${root}`);
  }

  async settings(prefix: string, root: string) {
    this.calls.push(`settings ${prefix} ${root}`);
    return this.settingsResult;
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

  reconnect() {
    this.connectionListener?.({ state: "connected" });
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
        settings: (commit) => commits.push(`${commit.changed.size}`),
        status: (status) => statuses.push(status),
      };
      const session = new PrefixSession(client as never, "dt/device", "", callbacks);

      await session.open();
      expect(client.calls).toEqual([
        "watchConnection",
        "alive dt/device",
        "schema dt/device",
        "watchSettings dt/device ",
        "watchAlive dt/device",
        "settings dt/device ",
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

  it("reloads retained state without dropping the permanent settings watcher", async () => {
    vi.useFakeTimers();
    try {
      const client = new FakeClient();
      const session = new PrefixSession(client as never, "dt/device", "", {
        error: () => {},
        alive: () => {},
        response: () => {},
        schema: () => {},
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
        "alive dt/device",
        "schema dt/device",
        "watchSettings dt/device ",
        "watchAlive dt/device",
        "settings dt/device ",
        "schema dt/device",
        "settings dt/device ",
      ]);
      expect(client.calls).not.toContain("stopSettings dt/device ");
    } finally {
      vi.useRealTimers();
    }
  });

  it("refreshes retained settings after broker reconnect", async () => {
    vi.useFakeTimers();
    try {
      const client = new FakeClient();
      const commits: SettingsChange[][] = [];
      const session = new PrefixSession(client as never, "dt/device", "", {
        error: () => {},
        alive: () => {},
        response: () => {},
        schema: () => {},
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
      expect(commits.at(-1)).toEqual([{ path: "/leaf", present: true, value: 1 }]);

      client.setSettings(new Map());
      client.reconnect();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      expect(commits.at(-1)).toEqual([]);
      expect(client.calls).not.toContain("stopSettings dt/device ");
    } finally {
      vi.useRealTimers();
    }
  });
});
