import { displayPath, type Schema } from "./schema";
import {
  MiniconfMqttClient,
  type DiscoveredPrefix,
  type AliveManifest,
  type SetResponse,
  type SettingsChange,
} from "./miniconf-mqtt-client";
import type { MqttAuth, MqttConnectionEvent } from "./mqtt-bus";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

// Session orchestration between raw protocol calls and Svelte state. Prefix
// loads and reconnect refreshes are serialized so stale retained scans cannot
// commit after a newer route or schema epoch wins.
export type { DiscoveredPrefix, AliveManifest, SetResponse } from "./miniconf-mqtt-client";

export type PrefixSessionCallbacks = {
  error: (error: string) => void;
  alive: (alive: AliveManifest | undefined) => void;
  response: (response: SetResponse) => void;
  schema: (schema: Schema, root: string) => void;
  settings: (commit: SettingsCommit) => void;
  status: (status: string) => void;
};

export class MiniconfBackend {
  private constructor(private readonly client: MiniconfMqttClient) {}

  static async connect(broker: string, auth?: Partial<MqttAuth>): Promise<MiniconfBackend> {
    return new MiniconfBackend(await MiniconfMqttClient.connect(broker, auth));
  }

  watchDiscovery(prefixFilter: string, onChange: (prefixes: DiscoveredPrefix[]) => void): () => void {
    return this.client.watchDiscovery(prefixFilter, onChange);
  }

  watchConnection(onChange: (event: MqttConnectionEvent) => void): () => void {
    return this.client.watchConnection(onChange);
  }

  openPrefix(prefix: string, subtreePath: string, callbacks: PrefixSessionCallbacks): PrefixSession {
    return new PrefixSession(this.client, prefix, subtreePath, callbacks);
  }

  close(): void {
    this.client.close();
  }
}

export class PrefixSession {
  private alive: AliveManifest | undefined;
  private root = "";
  private schema: Schema | undefined;
  private settingsRoot: string | undefined;
  private syncSerial = 0;
  private stopConnection: (() => void) | undefined;
  private stopAlive: (() => void) | undefined;
  private stopSettings: (() => void) | undefined;
  private readonly mirror: SettingsMirror;

  constructor(
    private readonly client: MiniconfMqttClient,
    private readonly prefix: string,
    private readonly subtreePath: string,
    private readonly callbacks: PrefixSessionCallbacks,
  ) {
    this.mirror = new SettingsMirror((commit) => callbacks.settings(commit));
  }

  async open(): Promise<void> {
    const serial = ++this.syncSerial;
    this.watchConnection();
    this.callbacks.status("Loading alive manifest");
    const alive = await this.client.aliveManifest(this.prefix);
    if (!this.active(serial)) {
      return;
    }
    this.alive = alive;
    this.callbacks.alive(this.alive);
    await this.loadSchema(this.alive, serial);
    if (!this.active(serial)) {
      return;
    }
    this.watchAlive();
    await this.settleSettings(serial);
  }

  async set(path: string, value: unknown): Promise<SetResponse> {
    this.callbacks.status(`Setting ${displayPath(path)}`);
    const response = await this.client.set(this.prefix, path, value);
    this.callbacks.response(response);
    return response;
  }

  close(): void {
    this.syncSerial += 1;
    this.stopConnection?.();
    this.stopSettings?.();
    this.stopAlive?.();
    this.stopConnection = undefined;
    this.stopSettings = undefined;
    this.stopAlive = undefined;
    this.mirror.dispose();
  }

  private async reload(next: AliveManifest, serial = ++this.syncSerial): Promise<void> {
    this.callbacks.status("Reloading retained state");
    this.alive = next;
    this.callbacks.alive(next);
    await this.loadSchema(next, serial);
    if (!this.active(serial)) {
      return;
    }
    await this.settleSettings(serial);
  }

  private async loadSchema(alive: AliveManifest, serial: number): Promise<void> {
    this.callbacks.status("Loading schema");
    const schema = await this.client.schema(this.prefix, alive);
    if (!this.active(serial)) {
      return;
    }
    this.schema = schema;
    this.root = this.schema.path(this.subtreePath);
    this.mirror.beginRetained();
    this.callbacks.schema(this.schema, this.root);
    // Keep the live /settings watcher active while the retained scan runs.
    // Otherwise the browser can miss publish echoes that arrive during startup.
    this.watchSettings();
  }

  private watchAlive(): void {
    if (this.stopAlive) {
      return;
    }
    this.stopAlive = this.client.watchAlive(this.prefix, (next) => {
      if (!next) {
        this.syncSerial += 1;
        this.callbacks.status("Prefix offline");
        this.callbacks.alive(undefined);
        return;
      }
      if (this.alive?.epoch !== next.epoch || this.alive.schema_rev !== next.schema_rev) {
        void this.reload(next);
      }
    });
  }

  private watchSettings(): void {
    if (this.stopSettings && this.settingsRoot === this.root) {
      return;
    }
    this.stopSettings?.();
    this.callbacks.status("Subscribing to settings");
    this.settingsRoot = this.root;
    this.stopSettings = this.client.watchSettings(this.prefix, this.root, (change) => {
      this.noteSettingsChange(change);
    });
  }

  private watchConnection(): void {
    if (this.stopConnection) {
      return;
    }
    this.stopConnection = this.client.watchConnection((event) => {
      switch (event.state) {
        case "connected":
          void this.refreshRetained();
          break;
        case "reconnecting":
          this.callbacks.status("Broker reconnecting");
          break;
        case "offline":
        case "closed":
          this.callbacks.status("Broker disconnected");
          break;
        case "error":
          this.callbacks.status("Broker connection error");
          if (event.error) {
            this.callbacks.error(event.error);
          }
          break;
      }
    });
  }

  private async refreshRetained(): Promise<void> {
    const serial = ++this.syncSerial;
    this.callbacks.status("Broker reconnected; refreshing retained state");
    try {
      const next = await this.client.aliveManifest(this.prefix);
      if (serial !== this.syncSerial) {
        return;
      }
      if (!this.alive || this.alive.epoch !== next.epoch || this.alive.schema_rev !== next.schema_rev) {
        await this.reload(next, serial);
      } else {
        // Retained clears/deletions cannot be inferred from replaying live
        // updates after reconnect, so refresh retained settings even when the
        // alive manifest did not change.
        this.callbacks.alive(next);
        this.mirror.beginRetained();
        await this.settleSettings(serial);
      }
    } catch {
      if (serial !== this.syncSerial) {
        return;
      }
      this.alive = undefined;
      this.callbacks.alive(undefined);
      this.mirror.failRetained();
      this.callbacks.status("Prefix offline");
    }
  }

  private async settleSettings(serial: number): Promise<void> {
    this.callbacks.status("Loading retained settings");
    try {
      const settled = await this.client.settings(this.prefix, this.root);
      if (!this.active(serial)) {
        return;
      }
      this.mirror.finishRetained(settled);
      this.callbacks.status(`Retained settings settled for ${this.prefix}`);
    } catch (err) {
      if (!this.active(serial)) {
        return;
      }
      const message = err instanceof Error ? err.message : String(err);
      this.mirror.failRetained();
      this.callbacks.status("Retained settings incomplete");
      this.callbacks.error(message);
    }
  }

  private noteSettingsChange(change: SettingsChange): void {
    this.mirror.ingest(change.path, change.value, change.present, change.rev);
  }

  private active(serial: number): boolean {
    return serial === this.syncSerial;
  }
}
