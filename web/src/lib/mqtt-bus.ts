import mqtt, {
  type IClientOptions,
  type IClientPublishOptions,
  type IClientSubscribeOptions,
  type MqttClient,
  type Packet,
} from "mqtt";
import { nanoid } from "nanoid";

// MQTT.js owns the WebSocket transport. This wrapper keeps browser sessions
// clean/ephemeral, centralizes topic filtering, and tracks durable watchers
// separately from transient retained scans or response subscriptions.
export type MqttMessage = {
  topic: string;
  payload: Uint8Array;
  packet: Packet;
};

type Listener = (message: MqttMessage) => void;

type Subscription = {
  durable: number;
  transient: number;
};

export type MqttAuth = {
  username: string;
  password: string;
};

export type MqttConnectionEvent = {
  state: "connected" | "reconnecting" | "offline" | "closed" | "error";
  error?: string;
};

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
    this.client.on("connect", () => this.notifyConnection({ state: "connected" }));
    this.client.on("reconnect", () => this.notifyConnection({ state: "reconnecting" }));
    this.client.on("offline", () => this.notifyConnection({ state: "offline" }));
    this.client.on("close", () => {
      if (!this.closing) {
        this.notifyConnection({ state: "closed" });
      }
    });
    this.client.on("error", (error: Error) => {
      this.notifyConnection({ state: "error", error: error.message });
    });
  }

  static connect(broker: string, auth?: Partial<MqttAuth>): Promise<MqttBus> {
    const options: IClientOptions = {
      clean: true,
      clientId: `miniconf-web-${nanoid()}`,
      protocolVersion: 5,
      queueQoSZero: false,
      reconnectPeriod: 1000,
      resubscribe: true,
      ...(auth?.username ? { username: auth.username } : {}),
      ...(auth?.password ? { password: auth.password } : {}),
    };
    const client = mqtt.connect(broker, options);
    return new Promise((resolve, reject) => {
      const cleanup = () => {
        client.off("connect", onConnect);
        client.off("error", onError);
      };
      const onConnect = () => {
        cleanup();
        resolve(new MqttBus(client));
      };
      const onError = (error: Error) => {
        cleanup();
        client.end(true);
        reject(error);
      };
      client.once("connect", onConnect);
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
    // Durable watchers describe desired subscriptions and survive reconnect via
    // MQTT.js resubscribe. One-shot scans use withSubscription instead.
    const stop = this.listen(filter, onMessage);
    void this.subscribe(filter, options, { durable: true }).catch(() => stop());
    return () => {
      stop();
      void this.unsubscribe(filter, true);
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

  async firstMessage(
    filter: string,
    options: IClientSubscribeOptions,
    predicate: (message: MqttMessage) => boolean,
    timeout: number,
  ): Promise<MqttMessage> {
    const pending = new Promise<MqttMessage>((resolve, reject) => {
      const timer = globalThis.setTimeout(() => {
        cleanup();
        reject(new Error(`Timed out waiting for ${filter}`));
      }, timeout);
      const cleanup = () => {
        globalThis.clearTimeout(timer);
        this.listeners.delete(listener);
      };
      const listener = (message: MqttMessage) => {
        if (topicMatches(filter, message.topic) && predicate(message)) {
          cleanup();
          resolve(message);
        }
      };
      this.listeners.add(listener);
    });
    return this.withSubscription(filter, options, () => pending);
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
    await this.subscribe(topic, options, {
      refresh: true,
      requireConnected: true,
    });
    try {
      return await body();
    } finally {
      await this.unsubscribe(topic, false);
    }
  }

  private async subscribe(
    topic: string,
    options: IClientSubscribeOptions,
    config: { durable?: boolean; refresh?: boolean; requireConnected?: boolean } = {},
  ): Promise<void> {
    const entry = this.subscriptions.get(topic) ?? { durable: 0, transient: 0 };
    const existing = entry.durable + entry.transient;
    if (config.durable) {
      entry.durable += 1;
    } else {
      entry.transient += 1;
    }
    this.subscriptions.set(topic, entry);

    if (existing && !config.refresh) {
      return;
    }
    if (config.requireConnected && !this.client.connected) {
      this.decrementSubscription(topic, Boolean(config.durable));
      throw new Error("MQTT broker disconnected");
    }
    if (!this.client.connected) {
      return;
    }
    try {
      await this.client.subscribeAsync(topic, options);
    } catch (error) {
      this.decrementSubscription(topic, Boolean(config.durable));
      throw error;
    }
  }

  private async unsubscribe(topic: string, durable: boolean): Promise<void> {
    const remaining = this.decrementSubscription(topic, durable);
    if (remaining > 0 || !this.client.connected) {
      return;
    }
    await this.client.unsubscribeAsync(topic);
  }

  private decrementSubscription(topic: string, durable: boolean): number {
    const entry = this.subscriptions.get(topic);
    if (!entry) {
      return 0;
    }
    if (durable) {
      entry.durable = Math.max(0, entry.durable - 1);
    } else {
      entry.transient = Math.max(0, entry.transient - 1);
    }
    const remaining = entry.durable + entry.transient;
    if (remaining) {
      this.subscriptions.set(topic, entry);
    } else {
      this.subscriptions.delete(topic);
    }
    return remaining;
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
