import type { IClientPublishOptions, IPublishPacket, ISubscriptionMap } from "mqtt";
import { MqttSession, type MqttAuth, type MqttMessage, type MqttSessionStatus } from "./mqtt-session";
import { randomId } from "./random-id";
import { displayPath, Schema, subtreeMatch, type CompactDef } from "./schema";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

const MINICONF_MQTT_PROTO = 1;
const SCHEMA_TIMEOUT_MS = 10_000;
const SET_TIMEOUT_MS = 3000;
const RETAINED = { qos: 1, rap: true, rh: 0 } as const;
const LIVE = { qos: 1, rap: false, rh: 2 } as const;

export type AliveManifest = {
  proto: number;
  epoch: number;
  schema_rev: number;
  pages: number;
};

export type DiscoveredPrefix = {
  prefix: string;
  aliveManifest: AliveManifest;
};

export type SetResponse = {
  path: string;
  ok: boolean;
  code: string;
  message: string;
};

export type DiscoverySessionCallbacks = {
  prefixes: (prefixes: DiscoveredPrefix[]) => void;
  error: (error: string) => void;
  status: (status: string) => void;
};

export type PrefixSessionCallbacks = {
  error: (error: string) => void;
  alive: (alive: AliveManifest | undefined) => void;
  response: (response: SetResponse) => void;
  schema: (schema: Schema, root: string) => void;
  settings: (commit: SettingsCommit) => void;
  status: (status: string) => void;
};

type PendingResponse = {
  path: string;
  resolve: (response: SetResponse) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof globalThis.setTimeout>;
};

type PacketProperties = {
  userProperties?: Record<string, string | string[]>;
  correlationData?: unknown;
};

export class DiscoverySession {
  private mqtt: MqttSession | undefined;
  private readonly found = new Map<string, AliveManifest>();

  private constructor(
    private readonly pattern: string,
    private readonly callbacks: DiscoverySessionCallbacks,
  ) {}

  static async connect(
    broker: string,
    pattern: string,
    callbacks: DiscoverySessionCallbacks,
    auth?: Partial<MqttAuth>,
  ): Promise<DiscoverySession> {
    if (pattern.split("/").includes("#")) {
      throw new Error("Discovery filter cannot contain #; it must leave room for /alive");
    }
    const session = new DiscoverySession(pattern, callbacks);
    const filter = `${pattern}/alive`;
    session.mqtt = await MqttSession.connect(
      broker,
      { [filter]: RETAINED },
      {
        message: (message) => session.handle(message),
        reset: () => session.reset(),
        status: (status) => session.noteStatus(status),
      },
      auth,
    );
    callbacks.status("Watching discovery");
    return session;
  }

  close(): void {
    this.mqtt?.close();
    this.mqtt = undefined;
    this.found.clear();
  }

  private reset(): void {
    this.found.clear();
    this.callbacks.prefixes([]);
  }

  private handle(message: MqttMessage): void {
    const suffix = "/alive";
    if (!message.topic.endsWith(suffix)) return;
    const prefix = message.topic.slice(0, -suffix.length);
    if (!message.payload.byteLength) {
      this.found.delete(prefix);
    } else {
      try {
        this.found.set(prefix, aliveManifest(jsonParse(message.payload)));
      } catch {
        return;
      }
    }
    this.callbacks.prefixes(
      [...this.found].map(([foundPrefix, alive]) => ({ prefix: foundPrefix, aliveManifest: alive })),
    );
  }

  private noteStatus(status: MqttSessionStatus): void {
    switch (status.state) {
      case "connected": this.callbacks.status("Watching discovery"); break;
      case "restoring": this.callbacks.status("Broker reconnected; restoring discovery"); break;
      case "reconnecting": this.callbacks.status("Broker reconnecting"); break;
      case "offline": this.callbacks.status("Broker disconnected"); break;
      case "error":
        this.callbacks.status("Broker connection error");
        this.callbacks.error(status.error);
        break;
    }
  }
}

export class PrefixSession {
  private mqtt: MqttSession | undefined;
  private alive: AliveManifest | undefined;
  private schema: Schema | undefined;
  private readonly pages = new Map<number, Uint8Array>();
  private readonly pending = new Map<string, PendingResponse>();
  private schemaTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
  private readonly responseTopic: string;
  private readonly mirror: SettingsMirror;

  private constructor(
    private readonly prefix: string,
    private readonly subtreePath: string,
    private readonly callbacks: PrefixSessionCallbacks,
  ) {
    this.responseTopic = `${prefix}/response/${randomId()}`;
    this.mirror = new SettingsMirror(callbacks.settings);
  }

  static async connect(
    broker: string,
    prefix: string,
    subtreePath: string,
    callbacks: PrefixSessionCallbacks,
    auth?: Partial<MqttAuth>,
  ): Promise<PrefixSession> {
    const root = miniconfPath(subtreePath, "Subtree path");
    const session = new PrefixSession(prefix, root, callbacks);
    const subscriptions: ISubscriptionMap = {
      [`${prefix}/alive`]: RETAINED,
      [`${prefix}/schema/#`]: RETAINED,
      [`${prefix}/settings${root}/#`]: RETAINED,
      [session.responseTopic]: LIVE,
    };
    session.mqtt = await MqttSession.connect(
      broker,
      subscriptions,
      {
        message: (message) => session.handle(message),
        reset: () => session.resetRetained(),
        status: (status) => session.noteStatus(status),
      },
      auth,
    );
    session.showProgress();
    return session;
  }

  async set(path: string, value: unknown, timeout = SET_TIMEOUT_MS): Promise<SetResponse> {
    if (!this.ready()) throw new Error("Settings are not ready");
    const settingsPath = miniconfPath(path);
    const payload = JSON.stringify(value);
    if (payload === undefined) throw new Error("Set value must be JSON-serializable");
    const correlation = randomCorrelation();
    const key = bytesKey(correlation);
    this.callbacks.status(`Setting ${displayPath(settingsPath)}`);

    return await new Promise<SetResponse>((resolve, reject) => {
      const timer = globalThis.setTimeout(() => {
        this.reject(key, pending, new Error("Timed out waiting for set response"));
      }, timeout);
      const pending = { path: settingsPath, resolve, reject, timer };
      this.pending.set(key, pending);
      const options: IClientPublishOptions = {
        qos: 1,
        properties: {
          responseTopic: this.responseTopic,
          correlationData: correlation as never,
          payloadFormatIndicator: true,
          messageExpiryInterval: Math.max(1, Math.ceil(timeout / 1000)),
        },
      };
      this.mqtt!.publish(`${this.prefix}/set${settingsPath}`, payload, options).catch((error) => {
        this.reject(key, pending, error instanceof Error ? error : new Error(String(error)));
      });
    });
  }

  close(): void {
    this.mqtt?.close();
    this.mqtt = undefined;
    this.clearSchemaTimer();
    this.rejectPending(new Error("Prefix session closed"));
    this.mirror.dispose();
  }

  private ready(): boolean {
    return Boolean(
      this.mqtt?.ready &&
      this.alive &&
      this.schema &&
      this.schema.rev === this.alive.schema_rev,
    );
  }

  private handle(message: MqttMessage): void {
    if (message.topic === `${this.prefix}/alive`) {
      this.handleAlive(message.payload);
    } else if (message.topic.startsWith(`${this.prefix}/schema/`)) {
      this.handleSchemaPage(message);
    } else if (message.topic.startsWith(`${this.prefix}/settings`)) {
      this.handleSetting(message);
    } else if (message.topic === this.responseTopic) {
      this.handleResponse(message);
    }
  }

  private handleAlive(payload: Uint8Array): void {
    if (!payload.byteLength) {
      this.alive = undefined;
      this.pages.clear();
      this.clearSchemaTimer();
      this.mirror.clear();
      this.rejectPending(new Error("Connection lost; setting outcome unknown. Check the current value."));
      this.callbacks.alive(undefined);
      this.callbacks.status("Prefix offline; waiting for alive");
      return;
    }
    let next: AliveManifest;
    try {
      next = aliveManifest(jsonParse(payload));
    } catch {
      return;
    }
    this.alive = next;
    this.callbacks.alive(next);
    if (this.schema?.rev !== next.schema_rev) this.startSchemaTimer();
    this.trySchema();
    this.showProgress();
  }

  private handleSchemaPage(message: MqttMessage): void {
    const suffix = message.topic.slice(`${this.prefix}/schema/`.length);
    if (!/^\d+$/.test(suffix)) return;
    this.pages.set(Number(suffix), new Uint8Array(message.payload));
    this.trySchema();
  }

  private trySchema(): void {
    const alive = this.alive;
    if (!alive || this.schema?.rev === alive.schema_rev) return;
    const pages = Array.from({ length: alive.pages }, (_unused, index) => this.pages.get(index));
    if (pages.some((page) => page === undefined)) return;
    const complete = pages as Uint8Array[];
    if (fnv1a(complete) !== alive.schema_rev) return;
    try {
      const defs = complete.flatMap((page) =>
        decode(page).split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line) as CompactDef),
      );
      const schema = new Schema(defs, alive.schema_rev);
      const root = schema.path(this.subtreePath);
      this.schema = schema;
      this.clearSchemaTimer();
      this.callbacks.schema(schema, root);
      this.showProgress();
    } catch (error) {
      this.clearSchemaTimer();
      this.callbacks.status("Schema load failed");
      this.callbacks.error(error instanceof Error ? error.message : String(error));
    }
  }

  private handleSetting(message: MqttMessage): void {
    try {
      const change = settingChange(this.prefix, this.subtreePath, message);
      if (change) this.mirror.ingest(change.path, change.value, change.present, change.rev);
    } catch {
      // Ignore one malformed retained publication without stopping the stream.
    }
  }

  private handleResponse(message: MqttMessage): void {
    const key = bytesKey(properties(message.packet).correlationData);
    const pending = this.pending.get(key);
    if (!pending) return;
    this.pending.delete(key);
    globalThis.clearTimeout(pending.timer);
    const code = userProperty(message.packet, "code") || "Error";
    const response = {
      path: pending.path,
      ok: code === "Ok",
      code,
      message: decode(message.payload),
    };
    pending.resolve(response);
    this.callbacks.response(response);
  }

  private resetRetained(): void {
    this.alive = undefined;
    this.pages.clear();
    this.clearSchemaTimer();
    this.mirror.clear();
    this.rejectPending(new Error("Connection lost; setting outcome unknown. Check the current value."));
    this.callbacks.alive(undefined);
  }

  private noteStatus(status: MqttSessionStatus): void {
    switch (status.state) {
      case "connected": this.showProgress(); break;
      case "restoring": this.callbacks.status("Broker reconnected; restoring retained state"); break;
      case "reconnecting": this.callbacks.status("Broker reconnecting"); break;
      case "offline":
        this.rejectPending(new Error("Connection lost; setting outcome unknown. Check the current value."));
        this.callbacks.status("Broker disconnected");
        break;
      case "error":
        this.callbacks.status("Broker connection error");
        this.callbacks.error(status.error);
        break;
    }
  }

  private showProgress(): void {
    if (!this.mqtt?.ready) return;
    if (!this.alive) {
      this.callbacks.status("Waiting for alive");
    } else if (this.schema?.rev !== this.alive.schema_rev) {
      this.callbacks.status(
        `Loading schema rev ${this.alive.schema_rev} (${this.alive.pages} page${this.alive.pages === 1 ? "" : "s"})`,
      );
    } else {
      this.callbacks.status("Watching settings");
    }
  }

  private startSchemaTimer(): void {
    this.clearSchemaTimer();
    this.schemaTimer = globalThis.setTimeout(() => {
      this.schemaTimer = undefined;
      const alive = this.alive;
      if (!alive || this.schema?.rev === alive.schema_rev) return;
      const missing = Array.from({ length: alive.pages }, (_unused, index) => index)
        .filter((index) => !this.pages.has(index));
      this.callbacks.status("Schema load failed");
      this.callbacks.error(
        missing.length
          ? `Timed out waiting for schema page${missing.length === 1 ? "" : "s"} ${missing.join(", ")}`
          : `Schema pages do not match revision ${alive.schema_rev}`,
      );
    }, SCHEMA_TIMEOUT_MS);
  }

  private clearSchemaTimer(): void {
    if (this.schemaTimer === undefined) return;
    globalThis.clearTimeout(this.schemaTimer);
    this.schemaTimer = undefined;
  }

  private reject(key: string, pending: PendingResponse, error: Error): void {
    if (!this.pending.delete(key)) return;
    globalThis.clearTimeout(pending.timer);
    pending.reject(error);
  }

  private rejectPending(error: Error): void {
    for (const [key, pending] of this.pending) this.reject(key, pending, error);
  }
}

function miniconfPath(path: string, label = "Path"): string {
  if (path === "" || path.startsWith("/")) return path;
  throw new Error(`${label} must be empty or start with "/"`);
}

function aliveManifest(value: unknown): AliveManifest {
  if (!value || typeof value !== "object") throw new Error("Invalid alive manifest");
  const alive = value as Partial<AliveManifest>;
  if (
    alive.proto !== MINICONF_MQTT_PROTO ||
    !Number.isInteger(alive.epoch) ||
    !Number.isInteger(alive.schema_rev) ||
    !Number.isInteger(alive.pages) ||
    alive.pages! < 0
  ) throw new Error("Invalid alive manifest");
  return alive as AliveManifest;
}

function fnv1a(pages: Uint8Array[]): number {
  let hash = 0x811c9dc5;
  for (const page of pages) {
    for (const byte of page) hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
  }
  return hash;
}

function settingChange(prefix: string, root: string, message: MqttMessage) {
  const auth = userPropertyValues(message.packet, "auth");
  if (!message.packet.retain || auth.length !== 1 || auth[0] !== "") return;
  const path = message.topic.slice(`${prefix}/settings`.length);
  if ((path && !path.startsWith("/")) || !subtreeMatch(path, root)) return;
  const rev = userProperty(message.packet, "rev");
  return message.payload.byteLength
    ? { path, value: jsonParse(message.payload), present: true, rev }
    : { path, value: undefined, present: false, rev };
}

function properties(packet: IPublishPacket): PacketProperties {
  return (packet as IPublishPacket & { properties?: PacketProperties }).properties ?? {};
}

function userPropertyValues(packet: IPublishPacket, name: string): string[] {
  const value = properties(packet).userProperties?.[name];
  if (value === undefined) return [];
  return Array.isArray(value) ? value.map(String) : [String(value)];
}

function userProperty(packet: IPublishPacket, name: string): string | undefined {
  return userPropertyValues(packet, name)[0];
}

function bytesKey(value: unknown): string {
  if (value instanceof Uint8Array) {
    return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
  }
  return typeof value === "string" ? value : "";
}

function randomCorrelation(): Uint8Array {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return bytes;
}

function decode(payload: Uint8Array): string {
  return new TextDecoder().decode(payload);
}

function jsonParse(payload: Uint8Array): unknown {
  return JSON.parse(decode(payload));
}
