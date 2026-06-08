import { displayPath, type Schema } from "./schema";
import {
  MiniconfMqttClient,
  type DiscoveredPrefix,
  type AliveManifest,
  type SchemaProgress,
  type SetResponse,
  type SettingsChange,
} from "./miniconf-mqtt-client";
import type { MqttAuth, MqttConnectionEvent } from "./mqtt-bus";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

// Session orchestration between raw protocol calls and Svelte state. Prefix
// loads are serialized so stale schema work cannot commit after a newer route
// or schema epoch wins.
export type { DiscoveredPrefix, AliveManifest, SetResponse } from "./miniconf-mqtt-client";

export type PrefixSessionCallbacks = {
  error: (error: string) => void;
  alive: (alive: AliveManifest | undefined) => void;
  response: (response: SetResponse) => void;
  schema: (schema: Schema, root: string) => void;
  schemaProgress: (progress: SchemaProgress) => void;
  settings: (commit: SettingsCommit) => void;
  status: (status: string) => void;
};

type SessionState = "closed" | "opening" | "active" | "offline";

type Load = {
  abort: AbortController;
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
  private manifestKey = "";
  private state: SessionState = "closed";
  private load: Load | undefined;
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
    const load = this.beginLoad("opening");
    this.watchConnection();
    this.callbacks.status("Waiting for alive");
    const alive = await this.waitInitialAlive(load);
    if (!this.active(load)) {
      return;
    }
    this.state = "active";
    this.manifestKey = manifestKey(alive);
    this.callbacks.alive(alive);
    await this.loadSchema(alive, load);
  }

  async set(path: string, value: unknown): Promise<SetResponse> {
    this.callbacks.status(`Setting ${displayPath(path)}`);
    const response = await this.client.set(this.prefix, path, value);
    this.callbacks.response(response);
    return response;
  }

  close(): void {
    this.state = "closed";
    this.manifestKey = "";
    this.cancelLoad();
    this.stopConnection?.();
    this.stopSettings?.();
    this.stopAlive?.();
    this.stopConnection = undefined;
    this.stopSettings = undefined;
    this.stopAlive = undefined;
    this.mirror.dispose();
  }

  private async reload(next: AliveManifest, load = this.beginLoad("active")): Promise<void> {
    this.callbacks.status("Device manifest changed; loading schema");
    this.manifestKey = manifestKey(next);
    this.callbacks.alive(next);
    await this.loadSchema(next, load);
  }

  private async loadSchema(alive: AliveManifest, load: Load): Promise<void> {
    this.callbacks.status("Loading schema");
    const schema = await this.client.schema(this.prefix, alive, {
      signal: load.abort.signal,
      progress: (progress) => {
        if (this.active(load)) {
          this.callbacks.schemaProgress(progress);
        }
      },
    });
    if (!this.active(load)) {
      return;
    }
    const root = schema.path(this.subtreePath);
    this.mirror.clear();
    this.callbacks.schema(schema, root);
    this.restartSettings(root);
  }

  private waitInitialAlive(load: Load): Promise<AliveManifest> {
    return new Promise((resolve, reject) => {
      let resolved = false;
      let settled = false;
      const onAbort = () => {
        finish(() => reject(new Error("Prefix session closed")));
      };
      const finish = (complete: () => void) => {
        if (settled) {
          return;
        }
        settled = true;
        load.abort.signal.removeEventListener("abort", onAbort);
        complete();
      };
      load.abort.signal.addEventListener("abort", onAbort, { once: true });
      this.stopAlive = this.client.watchAlive(this.prefix, (next) => {
        if (!next) {
          this.manifestKey = "";
          this.callbacks.alive(undefined);
          if (this.state === "opening") {
            this.callbacks.status("Prefix offline; waiting for alive");
          }
          if (this.state === "active") {
            this.cancelLoad();
            this.state = "offline";
            this.callbacks.status("Prefix offline");
          }
          return;
        }
        if (!resolved) {
          resolved = true;
          finish(() => resolve(next));
          return;
        }
        if (this.state === "opening") {
          return;
        }
        if (this.state === "offline" || this.manifestKey !== manifestKey(next)) {
          void this.reload(next);
        }
      });
    });
  }

  private restartSettings(root: string): void {
    this.stopSettings?.();
    this.callbacks.status("Subscribing to settings");
    this.stopSettings = this.client.watchSettings(this.prefix, root, (change) => {
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
          if (this.state === "active") {
            this.mirror.clear();
            this.callbacks.status("Broker reconnected; restoring subscriptions");
          }
          break;
        case "subscriptions-restored":
          if (this.state === "active") {
            this.callbacks.status("Broker subscriptions restored; waiting for settings");
          }
          break;
        case "reconnecting":
          this.callbacks.status("Broker reconnecting");
          break;
        case "offline":
        case "closed":
          this.callbacks.status("Broker disconnected");
          break;
        case "error":
          this.callbacks.status(event.transient ? "Broker reconnecting" : "Broker connection error");
          if (event.error && !event.transient) {
            this.callbacks.error(event.error);
          }
          break;
      }
    });
  }

  private noteSettingsChange(change: SettingsChange): void {
    this.mirror.ingest(change.path, change.value, change.present, change.rev);
  }

  private beginLoad(state: SessionState): Load {
    this.cancelLoad();
    const load = { abort: new AbortController() };
    this.load = load;
    this.state = state;
    return load;
  }

  private cancelLoad(): void {
    this.load?.abort.abort();
    this.load = undefined;
  }

  private active(load: Load): boolean {
    return this.load === load && !load.abort.signal.aborted;
  }
}

function manifestKey(alive: AliveManifest): string {
  return `${alive.epoch}:${alive.schema_rev}`;
}
