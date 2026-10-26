import mqtt, {
  type IClientOptions,
  type IClientPublishOptions,
  type IPublishPacket,
  type ISubscriptionMap,
  type MqttClient,
} from "mqtt";
import { randomId } from "./random-id";

export type MqttMessage = {
  topic: string;
  payload: Uint8Array;
  packet: IPublishPacket;
};
export type MqttSessionStatus =
  | { state: "connected" | "restoring" | "reconnecting" | "offline" }
  | { state: "error" | "failed"; error: string };
export type MqttSessionCallbacks = {
  message: (message: MqttMessage) => void;
  reset: () => void;
  status: (status: MqttSessionStatus) => void;
  subscriptions?: (rejectedOptional: string[]) => void;
};
export type MqttAuth = { username: string; password: string };
export type ConnectOptions = {
  auth?: Partial<MqttAuth>;
  signal?: AbortSignal;
  optionalSubscriptions?: ISubscriptionMap;
};

function clientOptions(auth?: Partial<MqttAuth>): IClientOptions {
  return {
    clean: true,
    clientId: `miniconf-web-${randomId()}`,
    connectTimeout: 5000,
    protocolVersion: 5,
    queueQoSZero: false,
    reconnectPeriod: 0,
    resubscribe: false,
    ...(auth ? { username: auth.username ?? "" } : {}),
    ...(auth?.password ? { password: auth.password } : {}),
  };
}

export class MqttSession {
  private readonly client: MqttClient;
  readonly connected: Promise<MqttSession>;
  private readonly optionalSubscriptions: ISubscriptionMap;
  private readonly lifetime = new AbortController();
  private generation = 0;
  private offline = false;
  private subscribed = false;
  private refreshNeeded = false;
  private refreshing: Promise<void> | undefined;
  private removeAbortListener: (() => void) | undefined;

  constructor(
    private readonly subscriptions: ISubscriptionMap,
    private readonly callbacks: MqttSessionCallbacks,
    broker: string,
    { auth, signal, optionalSubscriptions = {} }: ConnectOptions = {},
  ) {
    signal?.throwIfAborted();
    const url = new URL(broker);
    if (url.protocol !== "ws:" && url.protocol !== "wss:")
      throw new Error("Broker URL must start with ws:// or wss://");
    if (globalThis.location?.protocol === "https:" && url.protocol === "ws:")
      throw new Error(
        "HTTPS pages cannot connect to ws:// brokers; open the app over HTTP or use a wss:// broker.",
      );
    this.client = mqtt.connect(broker, clientOptions(auth));
    this.optionalSubscriptions = optionalSubscriptions;
    this.connected = this.connect(broker, signal);
  }

  get ready(): boolean {
    return (
      this.subscribed && this.client.connected && !this.lifetime.signal.aborted
    );
  }

  private async connect(
    broker: string,
    signal?: AbortSignal,
  ): Promise<MqttSession> {
    const client = this.client;
    await new Promise<void>((resolve, reject) => {
      const cleanup = () => {
        client.off("connect", connected);
        client.off("close", closed);
        client.off("error", failed);
        signal?.removeEventListener("abort", aborted);
      };
      const connected = () => {
        cleanup();
        resolve();
      };
      const failed = (error: Error) => {
        cleanup();
        client.end(true);
        reject(error);
      };
      const closed = () => failed(new Error(`Could not connect to ${broker}`));
      const aborted = () => failed(new Error("Connection cancelled"));
      client.once("connect", connected);
      client.once("close", closed);
      client.once("error", failed);
      signal?.addEventListener("abort", aborted, { once: true });
    });
    client.on("message", (topic, payload, packet) => {
      if (!this.lifetime.signal.aborted)
        this.callbacks.message({ topic, payload, packet });
    });
    client.on("connect", () => {
      if (this.lifetime.signal.aborted) return;
      this.offline = false;
      void this.refresh();
    });
    client.on("reconnect", () => {
      if (!this.lifetime.signal.aborted)
        this.callbacks.status({ state: "reconnecting" });
    });
    client.on("offline", () => this.noteOffline());
    client.on("close", () => this.noteOffline());
    client.on("error", (error: Error) => {
      if (!this.lifetime.signal.aborted)
        this.callbacks.status({ state: "error", error: error.message });
    });
    const abort = () => this.close();
    signal?.addEventListener("abort", abort, { once: true });
    this.removeAbortListener = () =>
      signal?.removeEventListener("abort", abort);
    try {
      signal?.throwIfAborted();
      await this.restore();
      client.options.reconnectPeriod = 1000;
      return this;
    } catch (error) {
      this.close();
      throw error;
    }
  }

  async publish(
    topic: string,
    payload: string,
    options: IClientPublishOptions,
    signal?: AbortSignal,
  ): Promise<void> {
    if (!this.ready) throw new Error("MQTT session is not ready");
    signal?.throwIfAborted();
    // With the default in-memory store, a ready client allocates the publish ID
    // synchronously. Restored subscriptions are ready only after store replay.
    const publication = this.client.publishAsync(topic, payload, options);
    const messageId = this.client.getLastMessageId();
    try {
      await this.acknowledged(publication, signal);
    } catch (error) {
      // Cancelling a wait alone leaves QoS publications queued for reconnect.
      if (options.qos) this.client.removeOutgoingMessage(messageId);
      throw error;
    }
  }

  refresh(): Promise<void> {
    if (this.lifetime.signal.aborted || !this.client.connected)
      return Promise.resolve();
    const generation = this.generation;
    return this.restore().catch((error) => {
      if (generation !== this.generation || this.lifetime.signal.aborted)
        return;
      this.close();
      this.callbacks.status({
        state: "failed",
        error: error instanceof Error ? error.message : String(error),
      });
    });
  }

  // A generation arriving during SUBACK requests another pass, including during
  // initial connection. Joining the current pass alone would lose that request.
  private restore(): Promise<void> {
    this.refreshNeeded = true;
    this.subscribed = false;
    this.callbacks.reset();
    this.callbacks.status({ state: "restoring" });
    if (this.refreshing) return this.refreshing;
    const generation = this.generation;
    const pending = (async () => {
      while (this.refreshNeeded) {
        this.refreshNeeded = false;
        await this.subscribe();
        if (
          generation !== this.generation ||
          this.lifetime.signal.aborted ||
          !this.client.connected
        )
          throw new Error("Connection closed while subscribing");
      }
      this.subscribed = true;
      this.callbacks.status({ state: "connected" });
    })().finally(() => {
      if (this.refreshing === pending) this.refreshing = undefined;
    });
    this.refreshing = pending;
    return pending;
  }

  close(): void {
    if (this.lifetime.signal.aborted) return;
    this.lifetime.abort();
    this.removeAbortListener?.();
    this.subscribed = false;
    this.generation += 1;
    this.client.end(true);
  }

  private noteOffline(): void {
    if (this.lifetime.signal.aborted || this.offline) return;
    this.offline = true;
    this.subscribed = false;
    this.generation += 1;
    this.refreshing = undefined;
    this.callbacks.status({ state: "offline" });
  }

  private async subscribe(): Promise<void> {
    const generation = this.generation;
    const subscriptions = {
      ...this.optionalSubscriptions,
      ...this.subscriptions,
    };
    const topics = Object.keys(subscriptions);
    // MQTT.js rejects subscribeAsync on *any* denied filter. The SUBACK packet
    // still tells us which required and optional subscriptions were granted.
    const grants = await this.acknowledged(
      new Promise<number[]>((resolve, reject) => {
        this.client.subscribe(subscriptions, (error, _grants, packet) => {
          if (!packet || packet.granted.length !== topics.length) {
            reject(
              error ?? new Error("Incomplete subscription acknowledgment"),
            );
          } else resolve(packet.granted as number[]);
        });
      }),
    );
    const rejected = topics.filter((_topic, index) => grants[index] >= 128);
    const required = rejected.filter((topic) =>
      Object.hasOwn(this.subscriptions, topic),
    );
    if (required.length)
      throw new Error(`MQTT subscription rejected: ${required.join(", ")}`);
    if (generation === this.generation && !this.lifetime.signal.aborted)
      this.callbacks.subscriptions?.(rejected);
  }

  private async acknowledged<T>(
    operation: Promise<T>,
    operationSignal?: AbortSignal,
  ): Promise<T> {
    const signals = operationSignal
      ? [this.lifetime.signal, operationSignal]
      : [this.lifetime.signal];
    for (const signal of signals) signal.throwIfAborted();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let abort: () => void = () => {};
    try {
      return await Promise.race([
        operation,
        new Promise<never>((_resolve, reject) => {
          abort = () => reject(new Error("Connection cancelled"));
          for (const signal of signals)
            signal.addEventListener("abort", abort, { once: true });
          timer = setTimeout(
            () => reject(new Error("MQTT acknowledgment timed out")),
            10_000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
      for (const signal of signals) signal.removeEventListener("abort", abort);
    }
  }
}
