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
};
export type MqttAuth = { username: string; password: string };
export type ConnectOptions = { auth?: Partial<MqttAuth>; signal?: AbortSignal };

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
  private readonly lifetime = new AbortController();
  private generation = 0;
  private offline = false;
  private subscribed = false;
  private refreshNeeded = false;
  private refreshing: Promise<void> | undefined;
  private removeAbortListener: (() => void) | undefined;

  private constructor(
    private readonly client: MqttClient,
    private readonly subscriptions: ISubscriptionMap,
    private readonly callbacks: MqttSessionCallbacks,
  ) {
    client.on("message", (topic, payload, packet) => {
      if (!this.lifetime.signal.aborted)
        callbacks.message({ topic, payload, packet });
    });
    client.on("connect", () => {
      if (this.lifetime.signal.aborted) return;
      this.offline = false;
      void this.refresh();
    });
    client.on("reconnect", () => {
      if (!this.lifetime.signal.aborted)
        callbacks.status({ state: "reconnecting" });
    });
    client.on("offline", () => this.noteOffline());
    client.on("close", () => this.noteOffline());
    client.on("error", (error: Error) => {
      if (!this.lifetime.signal.aborted)
        callbacks.status({ state: "error", error: error.message });
    });
  }

  get ready(): boolean {
    return (
      this.subscribed && this.client.connected && !this.lifetime.signal.aborted
    );
  }

  static async connect(
    broker: string,
    subscriptions: ISubscriptionMap,
    callbacks: MqttSessionCallbacks,
    { auth, signal }: ConnectOptions = {},
  ): Promise<MqttSession> {
    signal?.throwIfAborted();
    const url = new URL(broker);
    if (url.protocol !== "ws:" && url.protocol !== "wss:") {
      throw new Error("Broker URL must start with ws:// or wss://");
    }
    if (globalThis.location?.protocol === "https:" && url.protocol === "ws:") {
      throw new Error(
        "HTTPS pages cannot connect to ws:// brokers; open the app over HTTP or use a wss:// broker.",
      );
    }
    const client = mqtt.connect(broker, clientOptions(auth));
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
    const session = new MqttSession(client, subscriptions, callbacks);
    const abort = () => session.close();
    signal?.addEventListener("abort", abort, { once: true });
    session.removeAbortListener = () =>
      signal?.removeEventListener("abort", abort);
    try {
      signal?.throwIfAborted();
      const generation = session.generation;
      callbacks.reset();
      await session.subscribe();
      if (generation !== session.generation || !client.connected)
        throw new Error("Connection closed while subscribing");
      session.subscribed = true;
      client.options.reconnectPeriod = 1000;
      return session;
    } catch (error) {
      session.close();
      throw error;
    }
  }

  async publish(
    topic: string,
    payload: string,
    options: IClientPublishOptions,
  ): Promise<void> {
    if (!this.ready) throw new Error("MQTT session is not ready");
    await this.acknowledged(this.client.publishAsync(topic, payload, options));
  }

  // One fixed subscription owner. A generation arriving during SUBACK requests
  // another pass; it must not be lost by joining the already-running operation.
  refresh(): Promise<void> {
    if (this.lifetime.signal.aborted || !this.client.connected)
      return Promise.resolve();
    this.refreshNeeded = true;
    this.subscribed = false;
    this.callbacks.reset();
    this.callbacks.status({ state: "restoring" });
    if (this.refreshing) return this.refreshing;
    const generation = this.generation;
    const pending = (async () => {
      try {
        while (this.refreshNeeded) {
          this.refreshNeeded = false;
          await this.subscribe();
          if (generation !== this.generation || this.lifetime.signal.aborted)
            return;
        }
        this.subscribed = true;
        this.callbacks.status({ state: "connected" });
      } catch (error) {
        if (generation !== this.generation || this.lifetime.signal.aborted)
          return;
        this.close();
        this.callbacks.status({
          state: "failed",
          error: error instanceof Error ? error.message : String(error),
        });
      }
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
    const grants = await this.acknowledged(
      this.client.subscribeAsync(this.subscriptions),
    );
    const rejected = grants
      .filter(({ qos }) => qos >= 128)
      .map(({ topic }) => topic);
    if (grants.length !== Object.keys(this.subscriptions).length)
      throw new Error("Incomplete subscription acknowledgment");
    if (rejected.length)
      throw new Error(`MQTT subscription rejected: ${rejected.join(", ")}`);
  }

  private async acknowledged<T>(operation: Promise<T>): Promise<T> {
    const signal = this.lifetime.signal;
    signal.throwIfAborted();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let abort: () => void = () => {};
    try {
      return await Promise.race([
        operation,
        new Promise<never>((_resolve, reject) => {
          abort = () => reject(new Error("Connection cancelled"));
          signal.addEventListener("abort", abort, { once: true });
          timer = setTimeout(
            () => reject(new Error("MQTT acknowledgment timed out")),
            10_000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
    }
  }
}
