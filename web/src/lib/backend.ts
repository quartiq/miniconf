import type {
  IClientPublishOptions,
  IPublishPacket,
  ISubscriptionMap,
} from "mqtt";
import {
  MqttSession,
  type ConnectOptions,
  type MqttMessage,
  type MqttSessionStatus,
} from "./mqtt-session";
import { randomId } from "./random-id";
import { staleTopic } from "./prune";
import { Schema, subtreeMatch, type CompactDef } from "./schema";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

const MINICONF_MQTT_PROTO = 1;
const SCHEMA_TIMEOUT_MS = 10_000;
const SET_TIMEOUT_MS = 3000;
const RETAINED = { qos: 1, rap: true, rh: 0 } as const;
// Preserve retain even on the exact reply filter: it overlaps cleanup observation.
const LIVE = { qos: 1, rap: true, rh: 2 } as const;

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
  kind?: string;
  message: string;
};

export type SessionStatus =
  | MqttSessionStatus
  | { state: "device-error"; error: string }
  | { state: "idle" | "connecting" | "waiting" | "loading" | "watching" };

export type DiscoverySessionCallbacks = {
  prefixes: (prefixes: DiscoveredPrefix[]) => void;
  status: (status: SessionStatus) => void;
};

export type PrefixSessionCallbacks = {
  alive: (alive: AliveManifest | undefined) => void;
  schema: (schema: Schema, root: string) => void;
  settings: (commit: SettingsCommit) => void;
  status: (status: SessionStatus, ready: boolean) => void;
  pruning?: (state: PruningState) => void;
};

export type PruningState = {
  count: number;
  coverageWarning: string;
};

type PendingResponse = {
  abort: AbortController;
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
    options: ConnectOptions = {},
  ): Promise<DiscoverySession> {
    if (pattern.split("/").includes("#")) {
      throw new Error(
        "Discovery filter cannot contain #; it must leave room for /alive",
      );
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
      options,
    ).catch((error) => {
      session.close();
      throw error;
    });
    callbacks.status({ state: "watching" });
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
    if (!message.packet.retain) return;
    const suffix = "/alive";
    if (!message.topic.endsWith(suffix)) return;
    const prefix = message.topic.slice(0, -suffix.length);
    if (!message.payload.byteLength) {
      this.found.delete(prefix);
    } else {
      try {
        this.found.set(prefix, aliveManifest(jsonParse(message.payload)));
      } catch {
        this.found.delete(prefix);
      }
    }
    this.callbacks.prefixes(
      [...this.found].map(([foundPrefix, alive]) => ({
        prefix: foundPrefix,
        aliveManifest: alive,
      })),
    );
  }

  private noteStatus(status: MqttSessionStatus): void {
    this.callbacks.status(
      status.state === "connected" ? { state: "watching" } : status,
    );
  }
}

export class PrefixSession {
  private readonly stale = new Set<string>();
  private pruneAbort = new AbortController();
  private pruning = false;
  private pruneCoverageWarning = "";
  private mqtt: MqttSession | undefined;
  private alive: AliveManifest | undefined;
  // Initial subscription callbacks can invalidate observations before mqtt is assigned.
  private deferredReplay = false;
  private schema: Schema | undefined;
  private deviceError: string | undefined;
  private readonly waitingSettings = new Map<
    string,
    { text: string | undefined; rev?: string }
  >();
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
    options: ConnectOptions = {},
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
        reset: () => session.clearRetained(),
        status: (status) => session.noteStatus(status),
        subscriptions: (rejected) => {
          session.pruneCoverageWarning = rejected.length
            ? `Some topic namespaces could not be observed: ${rejected.join(", ")}`
            : "";
          session.reportPruning();
        },
      },
      {
        ...options,
        optionalSubscriptions: {
          [`${prefix}/settings/#`]: RETAINED,
          [`${prefix}/set/#`]: RETAINED,
          [`${prefix}/response/#`]: RETAINED,
        },
      },
    ).catch((error) => {
      session.close();
      throw error;
    });
    if (session.deferredReplay) {
      session.deferredReplay = false;
      void session.mqtt.refresh();
    } else session.showProgress();
    return session;
  }

  async set(
    path: string,
    payload: string,
    timeout = SET_TIMEOUT_MS,
  ): Promise<SetResponse> {
    if (!this.ready) throw new Error("Settings are not ready");
    const settingsPath = miniconfPath(path);
    JSON.parse(payload);
    const correlation = randomCorrelation();
    const key = bytesKey(correlation);

    return await new Promise<SetResponse>((resolve, reject) => {
      const timer = globalThis.setTimeout(() => {
        this.reject(
          key,
          pending,
          new Error("Set response timed out; outcome unknown"),
        );
      }, timeout);
      const pending = {
        path: settingsPath,
        resolve,
        reject,
        timer,
        abort: new AbortController(),
      };
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
      this.mqtt!.publish(
        `${this.prefix}/set${settingsPath}`,
        payload,
        options,
        pending.abort.signal,
      ).catch((error) => {
        this.reject(
          key,
          pending,
          error instanceof Error ? error : new Error(String(error)),
        );
      });
    });
  }

  close(): void {
    this.cancelOperations(
      new Error("Prefix session closed; setting outcome unknown"),
    );
    this.mqtt?.close();
    this.mqtt = undefined;
    this.clearSchemaTimer();
    this.mirror.dispose();
  }

  get ready(): boolean {
    return Boolean(
      this.mqtt?.ready &&
      !this.deviceError &&
      this.alive &&
      this.schema &&
      this.schema.rev === this.alive.schema_rev,
    );
  }

  private handle(message: MqttMessage): void {
    if (
      message.packet.retain &&
      staleTopic(this.prefix, undefined, message.topic)
    ) {
      const schema =
        this.alive?.schema_rev === this.schema?.rev ? this.schema : undefined;
      if (
        message.payload.byteLength &&
        staleTopic(this.prefix, schema, message.topic)
      )
        this.stale.add(message.topic);
      else this.stale.delete(message.topic);
      this.reportPruning();
    }
    if (message.topic === `${this.prefix}/alive`) {
      this.handleAlive(message);
    } else if (message.topic.startsWith(`${this.prefix}/schema/`)) {
      this.handleSchemaPage(message);
    } else if (message.topic.startsWith(`${this.prefix}/settings`)) {
      this.handleSetting(message);
    } else if (message.topic === this.responseTopic) {
      this.handleResponse(message);
    }
  }

  private handleAlive(message: MqttMessage): void {
    if (!message.packet.retain) return;
    if (!message.payload.byteLength) {
      this.requestReplay();
      this.showProgress();
      return;
    }
    let next: AliveManifest;
    try {
      next = aliveManifest(jsonParse(message.payload));
    } catch (error) {
      this.clearRetained(
        new Error(
          "Invalid alive manifest; setting outcome unknown. Check the current value.",
        ),
      );
      this.deviceError = error instanceof Error ? error.message : String(error);
      this.showProgress();
      return;
    }
    if (
      (!this.alive && this.deviceError) ||
      (this.alive &&
        (this.alive.epoch !== next.epoch ||
          this.alive.schema_rev !== next.schema_rev))
    ) {
      // Recovery or a new generation needs the observations cleared earlier.
      // Its values may have preceded the alive commit marker.
      // Clear and replay through the existing fixed subscription owner.
      this.requestReplay();
      return;
    }
    if (!this.alive) this.deviceError = undefined;
    this.alive = next;
    if (this.schema?.rev === next.schema_rev) this.classifyStale();
    for (const page of this.pages.keys()) {
      if (page >= next.pages) this.pages.delete(page);
    }
    this.callbacks.alive(next);
    if (this.schema?.rev !== next.schema_rev) this.startSchemaTimer();
    this.trySchema();
    this.flushSettings();
    this.showProgress();
  }

  private handleSchemaPage(message: MqttMessage): void {
    if (!message.packet.retain) return;
    const suffix = message.topic.slice(`${this.prefix}/schema/`.length);
    if (!/^(0|[1-9]\d*)$/.test(suffix)) return;
    const page = Number(suffix);
    if (!Number.isSafeInteger(page) || (this.alive && page >= this.alive.pages))
      return;
    this.pages.set(page, new Uint8Array(message.payload));
    this.trySchema();
  }

  private trySchema(): void {
    const alive = this.alive;
    if (!alive || this.schema?.rev === alive.schema_rev) return;
    if (this.pages.size < alive.pages) return;
    const pages = Array.from({ length: alive.pages }, (_unused, index) =>
      this.pages.get(index),
    );
    if (pages.some((page) => page === undefined)) return;
    const complete = pages as Uint8Array[];
    if (fnv1a(complete) !== alive.schema_rev) return;
    try {
      const defs = complete.flatMap((page) =>
        decode(page)
          .split(/\r?\n/)
          .filter(Boolean)
          .map((line) => JSON.parse(line) as CompactDef),
      );
      const schema = new Schema(defs, alive.schema_rev);
      const root = schema.path(this.subtreePath);
      this.schema = schema;
      this.classifyStale();
      this.deviceError = undefined;
      this.clearSchemaTimer();
      this.callbacks.schema(schema, root);
      this.flushSettings();
      this.showProgress();
    } catch (error) {
      this.clearSchemaTimer();
      this.deviceError = error instanceof Error ? error.message : String(error);
      this.showProgress();
    }
  }

  private handleSetting(message: MqttMessage): void {
    try {
      const change = settingChange(this.prefix, this.subtreePath, message);
      if (!change) return;
      this.waitingSettings.set(change.path, change);
      this.flushSettings();
    } catch {
      // Ignore one malformed retained publication without stopping the stream.
    }
  }

  private flushSettings(): void {
    if (!this.alive || this.schema?.rev !== this.alive.schema_rev) return;
    for (const [path, { text, rev }] of this.waitingSettings) {
      try {
        if (this.schema.node(path).kind === "leaf")
          this.mirror.ingest(path, text, rev);
      } catch {
        // Obsolete retained paths never enter visible state, revision or activity.
      }
    }
    this.waitingSettings.clear();
  }

  private classifyStale(): void {
    for (const topic of this.stale)
      if (!staleTopic(this.prefix, this.schema, topic))
        this.stale.delete(topic);
    this.reportPruning();
  }

  private reportPruning(): void {
    this.callbacks.pruning?.({
      count:
        this.alive && this.schema?.rev === this.alive.schema_rev
          ? this.stale.size
          : 0,
      coverageWarning: this.pruneCoverageWarning,
    });
  }

  async prune(): Promise<{ cleared: number; error?: string }> {
    if (!this.ready || this.pruning) throw new Error("Pruning is not ready");
    const topics = [...this.stale];
    const signal = this.pruneAbort.signal;
    this.pruning = true;
    let cleared = 0;
    try {
      for (const topic of topics) {
        signal.throwIfAborted();
        if (!this.stale.has(topic)) continue;
        await this.mqtt!.publish(
          topic,
          "",
          {
            qos: 1,
            retain: true,
            properties: { payloadFormatIndicator: true },
          },
          signal,
        );
        signal.throwIfAborted();
        // The retained stream owns candidate membership, including replacements
        // arriving before this acknowledgment. PUBACK only confirms progress.
        cleared++;
      }
      return { cleared };
    } catch (error) {
      return {
        cleared,
        error: error instanceof Error ? error.message : String(error),
      };
    } finally {
      this.pruning = false;
    }
  }

  private handleResponse(message: MqttMessage): void {
    if (message.packet.retain) return;
    const key = bytesKey(properties(message.packet).correlationData);
    const pending = this.pending.get(key);
    if (!pending) return;
    this.pending.delete(key);
    globalThis.clearTimeout(pending.timer);
    pending.abort.abort();
    const code = userProperty(message.packet, "code") || "Error";
    // Make already-observed device values visible before the editor adopts them.
    if (code === "Ok") this.mirror.flush();
    const response = {
      path: pending.path,
      ok: code === "Ok",
      code,
      kind: userProperty(message.packet, "kind"),
      message: new TextDecoder().decode(message.payload),
    };
    pending.resolve(response);
  }

  private clearRetained(
    pendingError = new Error(
      "Device session changed; Set outcome unknown. Check the current value.",
    ),
  ): void {
    this.cancelOperations(pendingError);
    this.pruneAbort = new AbortController();
    this.stale.clear();
    this.alive = undefined;
    this.deviceError = undefined;
    this.pages.clear();
    this.waitingSettings.clear();
    this.clearSchemaTimer();
    this.mirror.clear();
    this.callbacks.alive(undefined);
    this.reportPruning();
  }

  private requestReplay(): void {
    if (this.mqtt) {
      void this.mqtt.refresh();
    } else {
      this.clearRetained();
      this.deferredReplay = true;
    }
  }

  private noteStatus(status: MqttSessionStatus): void {
    if (status.state === "connected") {
      this.showProgress();
      return;
    }
    if (status.state === "offline" || status.state === "failed") {
      this.cancelOperations(
        new Error(
          "Connection lost; setting outcome unknown. Check the current value.",
        ),
      );
    }
    this.callbacks.status(status, this.ready);
  }

  private showProgress(): void {
    if (!this.mqtt?.ready) return;
    this.callbacks.status(
      this.deviceError
        ? { state: "device-error", error: this.deviceError }
        : {
            state: !this.alive
              ? "waiting"
              : this.schema?.rev !== this.alive.schema_rev
                ? "loading"
                : "watching",
          },
      this.ready,
    );
  }

  private startSchemaTimer(): void {
    if (this.schemaTimer !== undefined) return;
    this.schemaTimer = globalThis.setTimeout(() => {
      this.schemaTimer = undefined;
      const alive = this.alive;
      if (!alive || this.schema?.rev === alive.schema_rev) return;
      const missing = alive.pages - this.pages.size;
      this.deviceError = missing
        ? `Timed out waiting for ${missing} of ${alive.pages} schema pages`
        : `Schema pages do not match revision ${alive.schema_rev}`;
      this.showProgress();
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
    pending.abort.abort();
    pending.reject(error);
  }

  private cancelOperations(error: Error): void {
    this.pruneAbort.abort();
    for (const [key, pending] of this.pending) this.reject(key, pending, error);
  }
}

function miniconfPath(path: string, label = "Path"): string {
  if (path === "" || path.startsWith("/")) return path;
  throw new Error(`${label} must be empty or start with "/"`);
}

function aliveManifest(value: unknown): AliveManifest {
  if (!value || typeof value !== "object")
    throw new Error("Invalid alive manifest");
  const alive = value as Partial<AliveManifest>;
  if (alive.proto !== MINICONF_MQTT_PROTO)
    throw new Error("Unsupported alive manifest");
  if (
    !Number.isInteger(alive.epoch) ||
    !Number.isInteger(alive.schema_rev) ||
    !Number.isSafeInteger(alive.pages) ||
    alive.pages! < 0
  )
    throw new Error("Invalid alive manifest");
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
  const text = message.payload.byteLength ? decode(message.payload) : undefined;
  if (text !== undefined) JSON.parse(text);
  return { path, text, rev };
}

function properties(packet: IPublishPacket): PacketProperties {
  return (
    (packet as IPublishPacket & { properties?: PacketProperties }).properties ??
    {}
  );
}

function userPropertyValues(packet: IPublishPacket, name: string): string[] {
  const value = properties(packet).userProperties?.[name];
  if (value === undefined) return [];
  return Array.isArray(value) ? value.map(String) : [String(value)];
}

function userProperty(
  packet: IPublishPacket,
  name: string,
): string | undefined {
  return userPropertyValues(packet, name)[0];
}

function bytesKey(value: unknown): string {
  if (value instanceof Uint8Array) {
    return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
      "",
    );
  }
  return typeof value === "string" ? value : "";
}

function randomCorrelation(): Uint8Array {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return bytes;
}

function decode(payload: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: true }).decode(payload);
}

function jsonParse(payload: Uint8Array): unknown {
  return JSON.parse(decode(payload));
}
