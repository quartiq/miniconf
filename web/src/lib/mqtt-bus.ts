import mqtt, {
  type IClientOptions,
  type IClientPublishOptions,
  type IClientSubscribeOptions,
  type MqttClient,
  type Packet,
} from "mqtt";
import { nanoid } from "nanoid";

// MQTT.js owns the WebSocket transport. This wrapper keeps browser sessions
// clean/ephemeral, centralizes topic filtering, and owns durable resubscribe
// ordering so retained replay happens after app-level reconnect handlers run.
export type MqttMessage = {
  topic: string;
  payload: Uint8Array;
  packet: Packet;
};

type Listener = (message: MqttMessage) => void;

type Subscription = {
  durable: boolean;
  options: IClientSubscribeOptions;
};

export type MqttAuth = {
  username: string;
  password: string;
};

export type MqttConnectionEvent = {
  state: "connected" | "reconnecting" | "offline" | "closed" | "error";
  error?: string;
  transient?: boolean;
};

function transientConnectionError(error: Error): boolean {
  const message = error.message.toLowerCase();
  return message.includes("connack timeout") || message.includes("keepalive timeout");
}

export function topicMatches(filter: string, topic: string): boolean {
  const filterParts = filter.split("/");
  const topicParts = topic.split("/");
  for (let i = 0; i < filterParts.length; i += 1) {
    const part = filterParts[i];
    if (part === "#") {
      return i === filterParts.length - 1;
    }
    if (part !== "+" && part !== topicParts[i]) {
      return false;
    }
  }
  return filterParts.length === topicParts.length;
}

export class MqttBus {
  private readonly client: MqttClient;
  private closing = false;
  private connectionListeners = new Set<(event: MqttConnectionEvent) => void>();
  private connectionState = "";
  private listeners = new Set<Listener>();
  private subscriptions = new Map<string, Subscription>();

  constructor(client: MqttClient) {
    this.client = client;
    this.client.on("message", (topic, payload, packet) => {
      const message = { topic, payload, packet };
      for (const listener of [...this.listeners]) {
        listener(message);
      }
    });
    this.client.on("connect", () => this.handleConnect());
    this.client.on("reconnect", () => this.notifyConnection({ state: "reconnecting" }));
    this.client.on("offline", () => this.notifyConnection({ state: "offline" }));
    this.client.on("close", () => {
      if (!this.closing) {
        this.notifyConnection({ state: "closed" });
      }
    });
    this.client.on("error", (error: Error) => {
      this.notifyConnection({
        state: "error",
        error: error.message,
        transient: transientConnectionError(error),
      });
    });
  }

  static async connect(broker: string, auth?: Partial<MqttAuth>): Promise<MqttBus> {
    if (globalThis.location?.protocol === "https:" && new URL(broker).protocol === "ws:") {
      throw new Error("HTTPS pages cannot connect to ws:// brokers; open the app over HTTP or use a wss:// broker.");
    }
    const options: IClientOptions = {
      clean: true,
      clientId: `miniconf-web-${nanoid()}`,
      connectTimeout: 5000,
      protocolVersion: 5,
      queueQoSZero: false,
      reconnectPeriod: 0,
      resubscribe: false,
      ...(auth?.username ? { username: auth.username } : {}),
      ...(auth?.password ? { password: auth.password } : {}),
    };
    const client = mqtt.connect(broker, options);
    return new Promise((resolve, reject) => {
      const cleanup = () => {
        client.off("connect", onConnect);
        client.off("close", onClose);
        client.off("error", onError);
      };
      const onConnect = () => {
        cleanup();
        client.options.reconnectPeriod = 1000;
        resolve(new MqttBus(client));
      };
      const onClose = () => {
        cleanup();
        client.end(true);
        reject(new Error(`Could not connect to ${broker}`));
      };
      const onError = (error: Error) => {
        cleanup();
        client.end(true);
        reject(error);
      };
      client.once("connect", onConnect);
      client.once("close", onClose);
      client.once("error", onError);
    });
  }

  close(): void {
    this.closing = true;
    this.connectionListeners.clear();
    this.listeners.clear();
    this.subscriptions.clear();
    this.client.end(true);
  }

  watchConnection(onChange: (event: MqttConnectionEvent) => void): () => void {
    this.connectionListeners.add(onChange);
    return () => {
      this.connectionListeners.delete(onChange);
    };
  }

  watch(
    filter: string,
    options: IClientSubscribeOptions,
    onMessage: (message: MqttMessage) => void,
  ): () => void {
    this.reserveSubscription(filter, options, true, false);
    const stop = this.listen(filter, onMessage);
    void this.subscribeReserved(filter, options, true).catch(() => stop());
    return () => {
      stop();
      void this.unsubscribe(filter);
    };
  }

  listen(filter: string, onMessage: (message: MqttMessage) => void): () => void {
    const listener = (message: MqttMessage) => {
      if (topicMatches(filter, message.topic)) {
        onMessage(message);
      }
    };
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  async collectUntil(
    filter: string,
    options: IClientSubscribeOptions,
    collect: (message: MqttMessage) => void,
    done: () => boolean,
    timeout: number,
    nextCheck?: () => number | undefined,
    onSubscribed?: (subscribeRtt: number) => void,
  ): Promise<void> {
    let start = (_subscribeRtt: number) => {};
    const pending = new Promise<void>((resolve, reject) => {
      let active = false;
      let checkTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
      let hardTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
      let settled = false;
      const finish = (complete: () => void) => {
        if (settled) {
          return;
        }
        settled = true;
        cleanup();
        complete();
      };
      const check = () => {
        if (done()) {
          finish(resolve);
          return;
        }
        scheduleCheck();
      };
      const scheduleCheck = () => {
        if (!active || !nextCheck) {
          return;
        }
        const delay = nextCheck();
        if (delay === undefined || !Number.isFinite(delay)) {
          return;
        }
        globalThis.clearTimeout(checkTimer);
        checkTimer = globalThis.setTimeout(check, Math.max(0, delay));
      };
      const cleanup = () => {
        globalThis.clearTimeout(hardTimer);
        globalThis.clearTimeout(checkTimer);
        this.listeners.delete(listener);
      };
      const listener = (message: MqttMessage) => {
        if (topicMatches(filter, message.topic)) {
          collect(message);
          check();
        }
      };
      this.listeners.add(listener);
      start = (subscribeRtt) => {
        if (settled) {
          return;
        }
        active = true;
        onSubscribed?.(subscribeRtt);
        hardTimer = globalThis.setTimeout(() => {
          finish(() => {
            if (done()) {
              resolve();
            } else {
              reject(new Error(`Timed out waiting for ${filter}`));
            }
          });
        }, timeout);
        check();
      };
    });
    const subscribeStarted = performance.now();
    return this.withSubscription(filter, options, () => {
      start(performance.now() - subscribeStarted);
      return pending;
    });
  }

  async publish(
    topic: string,
    payload: string,
    options: IClientPublishOptions,
  ): Promise<void> {
    if (!this.client.connected) {
      throw new Error("MQTT broker disconnected");
    }
    await this.client.publishAsync(topic, payload, options);
  }

  async withSubscription<T>(
    topic: string,
    options: IClientSubscribeOptions,
    body: () => Promise<T>,
  ): Promise<T> {
    // Transient subscriptions are connected-only by design; do not silently
    // queue retained scans or /set response waits while disconnected.
    this.reserveSubscription(topic, options, false, true);
    await this.subscribeReserved(topic, options, true);
    try {
      return await body();
    } finally {
      await this.unsubscribe(topic);
    }
  }

  private reserveSubscription(
    topic: string,
    options: IClientSubscribeOptions,
    durable: boolean,
    requireConnected: boolean,
  ): void {
    if (this.subscriptions.has(topic)) {
      throw new Error(`MQTT topic filter already subscribed: ${topic}`);
    }
    if (requireConnected && !this.client.connected) {
      throw new Error("MQTT broker disconnected");
    }
    this.subscriptions.set(topic, { durable, options });
  }

  private async subscribeReserved(
    topic: string,
    options: IClientSubscribeOptions,
    removeOnError: boolean,
  ): Promise<void> {
    if (!this.client.connected) {
      return;
    }
    try {
      await this.client.subscribeAsync(topic, options);
    } catch (error) {
      if (removeOnError) {
        this.subscriptions.delete(topic);
      }
      throw error;
    }
  }

  private async unsubscribe(topic: string): Promise<void> {
    if (!this.subscriptions.delete(topic) || !this.client.connected) {
      return;
    }
    await this.client.unsubscribeAsync(topic);
  }

  private handleConnect(): void {
    this.notifyConnection({ state: "connected" });
    for (const [topic, subscription] of this.subscriptions) {
      if (subscription.durable) {
        void this.subscribeReserved(topic, subscription.options, false);
      }
    }
  }

  private notifyConnection(event: MqttConnectionEvent): void {
    const key = `${event.state}:${event.error ?? ""}`;
    if (key === this.connectionState) {
      return;
    }
    this.connectionState = key;
    for (const listener of [...this.connectionListeners]) {
      listener(event);
    }
  }
}
