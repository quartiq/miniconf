import { type IClientSubscribeOptions, type Packet } from "mqtt";
import { nanoid } from "nanoid";
import {
  MqttBus,
  type MqttAuth,
  type MqttConnectionEvent,
  type MqttMessage,
} from "./mqtt-bus";
import { Schema, type CompactDef, subtreeMatch } from "./schema";

// Miniconf MQTT protocol operations. This layer speaks topics/properties and
// exposes parsed alive manifests, schemas, settings changes, and correlated /set
// responses to the rest of the app.
const MINICONF_MQTT_PROTO = 1;
const TRANSIENT_EXPIRY_S = 30;
const SUBSCRIBE: IClientSubscribeOptions = { qos: 1 };
const RETAINED_SUBSCRIBE: IClientSubscribeOptions = {
  qos: 1,
  rap: true,
  rh: 0,
};

export type DiscoveredPrefix = {
  prefix: string;
  aliveManifest: AliveManifest;
};

export type AliveManifest = {
  proto: number;
  epoch: number;
  schema_rev: number;
  pages: number;
};

export type SettingsChange = {
  path: string;
  value?: unknown;
  present: boolean;
  rev?: string;
};

export type SetResponse = {
  path: string;
  ok: boolean;
  code: string;
  message: string;
};

type PacketProperties = {
  userProperties?: Record<string, string | string[]>;
  correlationData?: unknown;
};

export type MiniconfMqttTransport = Pick<
  MqttBus,
  | "close"
  | "publish"
  | "subscribe"
  | "watch"
  | "watchConnection"
>;

type PendingSetResponse = {
  path: string;
  resolve: (response: SetResponse) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof globalThis.setTimeout>;
};

function properties(packet: Packet): PacketProperties {
  return (packet as Packet & { properties?: PacketProperties }).properties ?? {};
}

function decode(payload: Uint8Array): string {
  return new TextDecoder().decode(payload);
}

function jsonParse(payload: Uint8Array): unknown {
  return JSON.parse(decode(payload));
}

function userPropertyValues(packet: Packet, name: string): string[] {
  const raw = properties(packet).userProperties ?? {};
  const value = raw[name];
  if (value === undefined) {
    return [];
  }
  return Array.isArray(value) ? value.map(String) : [String(value)];
}

function userProperty(packet: Packet, name: string): string | undefined {
  return userPropertyValues(packet, name)[0];
}

function isAuthoritative(packet: Packet): boolean {
  // Retained/live settings are only authoritative with exactly one empty auth
  // user property. ACK/NAK response packets do not update the settings model.
  const values = userPropertyValues(packet, "auth");
  return values.length === 1 && values[0] === "";
}

function isRetained(packet: Packet): boolean {
  return Boolean((packet as Packet & { retain?: boolean }).retain);
}

function validateAliveManifest(payload: unknown): AliveManifest {
  if (!payload || typeof payload !== "object") {
    throw new Error("Invalid alive manifest");
  }
  const alive = payload as Partial<AliveManifest>;
  if (
    alive.proto !== MINICONF_MQTT_PROTO ||
    typeof alive.epoch !== "number" ||
    typeof alive.schema_rev !== "number" ||
    typeof alive.pages !== "number"
  ) {
    throw new Error("Invalid alive manifest");
  }
  return {
    proto: alive.proto,
    epoch: alive.epoch,
    schema_rev: alive.schema_rev,
    pages: alive.pages,
  };
}

function bytesKey(value: unknown): string {
  if (value instanceof Uint8Array) {
    return Array.from(value)
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
  }
  if (typeof value === "string") {
    return value;
  }
  return "";
}

function randomCorrelation(): Uint8Array {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return bytes;
}

function miniconfPath(path: string, label = "Path"): string {
  if (path === "" || path.startsWith("/")) {
    return path;
  }
  throw new Error(`${label} must be empty or start with "/"`);
}

function settingsFilter(prefix: string, root: string): string {
  // Miniconf paths are either empty or start with "/"; root therefore becomes
  // ".../settings/#" and a subtree becomes ".../settings/foo/#".
  return `${prefix}/settings${miniconfPath(root, "Settings root")}/#`;
}

export class MiniconfMqttClient {
  constructor(private readonly bus: MiniconfMqttTransport) {}

  static async connect(broker: string, auth?: Partial<MqttAuth>): Promise<MiniconfMqttClient> {
    return new MiniconfMqttClient(await MqttBus.connect(broker, auth));
  }

  close(): void {
    this.bus.close();
  }

  watchConnection(onChange: (event: MqttConnectionEvent) => void): () => void {
    return this.bus.watchConnection(onChange);
  }

  watchDiscovery(prefixFilter: string, onChange: (prefixes: DiscoveredPrefix[]) => void): () => void {
    const topic = `${prefixFilter}/alive`;
    const suffix = "/alive";
    const found = new Map<string, AliveManifest>();
    const emit = () => {
      onChange([...found.entries()].map(([prefix, aliveManifest]) => ({ prefix, aliveManifest })));
    };
    const stopConnection = this.bus.watchConnection((event) => {
      if (event.state === "connected") {
        found.clear();
        emit();
      }
    });
    const stopDiscovery = this.bus.watch(topic, RETAINED_SUBSCRIBE, (message) => {
      const prefix = message.topic.slice(0, -suffix.length);
      if (!message.payload.byteLength) {
        found.delete(prefix);
        emit();
        return;
      }
      try {
        found.set(prefix, validateAliveManifest(jsonParse(message.payload)));
        emit();
      } catch {
        // Discovery ignores invalid alive payloads.
      }
    });
    return () => {
      stopConnection();
      stopDiscovery();
    };
  }

  async schema(prefix: string, alive: AliveManifest, signal?: AbortSignal): Promise<Schema> {
    const topic = `${prefix}/schema/#`;
    const pages: (string | undefined)[] = Array.from(
      { length: alive.pages },
      () => undefined,
    );
    await new Promise<void>((resolve, reject) => {
      if (signal?.aborted) {
        reject(new Error("Schema load cancelled"));
        return;
      }
      let stop: (() => void) | undefined;
      let settled = false;
      const settle = (complete: () => void) => {
        if (settled) {
          return;
        }
        settled = true;
        signal?.removeEventListener("abort", onAbort);
        stop?.();
        complete();
      };
      const onAbort = () => {
        settle(() => reject(new Error("Schema load cancelled")));
      };
      signal?.addEventListener("abort", onAbort, { once: true });
      const finish = () => {
        settle(resolve);
      };
      if (!pages.length) {
        finish();
        return;
      }
      stop = this.bus.watch(topic, RETAINED_SUBSCRIBE, (message) => {
        const suffix = message.topic.slice(`${prefix}/schema/`.length);
        const page = Number.parseInt(suffix, 10);
        if (Number.isInteger(page) && page >= 0 && page < pages.length) {
          pages[page] = decode(message.payload);
        }
        if (pages.every((page) => page !== undefined)) {
          finish();
        }
      });
    });
    const defs = pages.flatMap((page) =>
      page
        ?.split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line) as CompactDef) ?? [],
    );
    return new Schema(defs, alive.schema_rev);
  }

  watchAlive(prefix: string, onChange: (alive: AliveManifest | undefined) => void): () => void {
    const topic = `${prefix}/alive`;
    return this.bus.watch(topic, RETAINED_SUBSCRIBE, (message) => {
      if (!message.payload.byteLength) {
        onChange(undefined);
        return;
      }
      try {
        onChange(validateAliveManifest(jsonParse(message.payload)));
      } catch {
        // Ignore malformed or unsupported alive payloads.
      }
    });
  }

  watchSettings(prefix: string, root: string, onChange: (change: SettingsChange) => void): () => void {
    const settingsRoot = miniconfPath(root, "Settings root");
    const filter = settingsFilter(prefix, settingsRoot);
    return this.bus.watch(filter, RETAINED_SUBSCRIBE, (message) => {
      const change = settingsChange(prefix, settingsRoot, message);
      if (!change) {
        return;
      }
      onChange(change);
    });
  }

  async openResponseChannel(prefix: string): Promise<SetResponseChannel> {
    const topic = `${prefix}/response/${nanoid()}`;
    let channel: SetResponseChannel | undefined;
    const stop = await this.bus.subscribe(topic, SUBSCRIBE, (message) => {
      channel?.handle(message);
    });
    channel = new SetResponseChannel(this.bus, prefix, topic, stop);
    return channel;
  }
}

export class SetResponseChannel {
  private readonly pending = new Map<string, PendingSetResponse>();
  private closed = false;

  constructor(
    private readonly bus: Pick<MiniconfMqttTransport, "publish">,
    private readonly prefix: string,
    private readonly topic: string,
    private readonly stop: () => void,
  ) {}

  close(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.stop();
    for (const [key, pending] of this.pending) {
      this.reject(key, pending, new Error("Response channel closed"));
    }
  }

  async set(path: string, value: unknown, timeout = 3000): Promise<SetResponse> {
    if (this.closed) {
      throw new Error("Response channel closed");
    }
    const settingsPath = miniconfPath(path);
    const payload = JSON.stringify(value);
    if (payload === undefined) {
      throw new Error("Set value must be JSON-serializable");
    }
    const correlation = randomCorrelation();
    const key = bytesKey(correlation);

    return await new Promise<SetResponse>((resolve, reject) => {
      const timer = globalThis.setTimeout(() => {
        this.reject(key, pending, new Error("Timed out waiting for set response"));
      }, timeout);
      const pending: PendingSetResponse = {
        path: settingsPath,
        resolve,
        reject,
        timer,
      };
      this.pending.set(key, pending);
      this.bus.publish(
        `${this.prefix}/set${settingsPath}`,
        payload,
        {
          qos: 1,
          properties: {
            responseTopic: this.topic,
            correlationData: correlation as never,
            payloadFormatIndicator: true,
            messageExpiryInterval: Math.max(1, Math.ceil(timeout / 1000)) || TRANSIENT_EXPIRY_S,
          },
        },
      ).catch((error: unknown) => {
        this.reject(
          key,
          pending,
          error instanceof Error ? error : new Error(String(error)),
        );
      });
    });
  }

  handle(message: MqttMessage): void {
    const key = bytesKey(properties(message.packet).correlationData);
    const pending = this.pending.get(key);
    if (!pending) {
      return;
    }
    this.pending.delete(key);
    globalThis.clearTimeout(pending.timer);
    const code = userProperty(message.packet, "code") || "Error";
    pending.resolve({
      path: pending.path,
      ok: code === "Ok",
      code,
      message: decode(message.payload),
    });
  }

  private reject(key: string, pending: PendingSetResponse, error: Error): void {
    if (!this.pending.delete(key)) {
      return;
    }
    globalThis.clearTimeout(pending.timer);
    pending.reject(error);
  }
}

function settingsChange(
  prefix: string,
  root: string,
  message: MqttMessage,
): SettingsChange | undefined {
  if (!message.topic.startsWith(`${prefix}/settings`)) {
    return undefined;
  }
  if (!isRetained(message.packet) || !isAuthoritative(message.packet)) {
    return undefined;
  }
  const path = message.topic.slice(`${prefix}/settings`.length);
  if (path && !path.startsWith("/")) {
    return undefined;
  }
  if (!subtreeMatch(path, root)) {
    return undefined;
  }
  if (!message.payload.byteLength) {
    return { path, present: false, rev: userProperty(message.packet, "rev") };
  }
  return { path, value: jsonParse(message.payload), present: true, rev: userProperty(message.packet, "rev") };
}
