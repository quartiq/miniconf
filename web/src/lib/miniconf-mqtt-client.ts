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

export type SchemaProgress = {
  received: number;
  total: number;
};

export type SchemaLoadOptions = {
  signal?: AbortSignal;
  progress?: (progress: SchemaProgress) => void;
};

type PacketProperties = {
  userProperties?: Record<string, string | string[]>;
  correlationData?: unknown;
};

export type MiniconfMqttTransport = Pick<
  MqttBus,
  | "close"
  | "listen"
  | "publish"
  | "watch"
  | "watchConnection"
  | "withSubscription"
>;

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

  async schema(prefix: string, alive: AliveManifest, options: SchemaLoadOptions = {}): Promise<Schema> {
    const { signal, progress } = options;
    const topic = `${prefix}/schema/#`;
    const pages: (string | undefined)[] = Array.from(
      { length: alive.pages },
      () => undefined,
    );
    const emitProgress = () => {
      progress?.({
        received: pages.filter((page) => page !== undefined).length,
        total: pages.length,
      });
    };
    emitProgress();
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
          emitProgress();
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

  async set(prefix: string, path: string, value: unknown, timeout = 3000): Promise<SetResponse> {
    const settingsPath = miniconfPath(path);
    const payload = JSON.stringify(value);
    if (payload === undefined) {
      throw new Error("Set value must be JSON-serializable");
    }
    const responseTopic = `${prefix}/response/${nanoid()}`;
    const correlation = randomCorrelation();
    const key = bytesKey(correlation);
    return this.bus.withSubscription(responseTopic, SUBSCRIBE, async () => {
      return await new Promise<SetResponse>((resolve, reject) => {
        const timer = globalThis.setTimeout(() => {
          cleanup();
          reject(new Error("Timed out waiting for set response"));
        }, timeout);
        const cleanup = () => {
          globalThis.clearTimeout(timer);
          stop();
        };
        const listener = (message: MqttMessage) => {
          // Multiple /set requests may overlap on the same response topic
          // pattern. Only MQTT v5 correlation data identifies this response.
          if (bytesKey(properties(message.packet).correlationData) !== key) {
            return;
          }
          cleanup();
          const code = userProperty(message.packet, "code") || "Error";
          const response = decode(message.payload);
          resolve({
            path: settingsPath,
            ok: code === "Ok",
            code,
            message: response,
          });
        };
        const stop = this.bus.listen(responseTopic, listener);
        this.bus.publish(
          `${prefix}/set${settingsPath}`,
          payload,
          {
            qos: 1,
            properties: {
              responseTopic,
              correlationData: correlation as never,
              payloadFormatIndicator: true,
              messageExpiryInterval: Math.max(1, Math.ceil(timeout / 1000)) || TRANSIENT_EXPIRY_S,
            },
          },
        ).catch((error: unknown) => {
          cleanup();
          reject(error);
        });
      });
    });
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
