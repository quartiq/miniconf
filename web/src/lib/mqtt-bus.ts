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
// ordering so reconnect handlers run before retained replay resumes.
export type MqttMessage = {
  topic: string;
  payload: Uint8Array;
  packet: Packet;
};

type Listener = (message: MqttMessage) => void;

type Subscription = {
  options: IClientSubscribeOptions;
};

export type MqttWatch = {
  ready: Promise<void>;
  close: () => void;
};

export type MqttAuth = {
  username: string;
  password: string;
};

export type MqttConnectionEvent = {
  state: "connected" | "subscriptions-restored" | "reconnecting" | "offline" | "closed" | "error";
  error?: string;
  transient?: boolean;
};

function isTransientMqttJsConnectionError(error: Error): boolean {
  // MQTT.js exposes reconnect-time transport failures as generic Error events.
  // Keep this presentation policy here so protocol/session code does not
  // string-match browser transport messages.
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
  private generation = 0;
  private connectionListeners = new Set<(event: MqttConnectionEvent) => void>();
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
    this.client.on("connect", () => this.handleConnect(++this.generation));
    this.client.on("reconnect", () => this.notifyConnection({ state: "reconnecting" }));
    this.client.on("offline", () => {
      this.generation += 1;
      this.notifyConnection({ state: "offline" });
    });
    this.client.on("close", () => {
      this.generation += 1;
      if (!this.closing) {
        this.notifyConnection({ state: "closed" });
      }
    });
    this.client.on("error", (error: Error) => {
      this.notifyConnection({
        state: "error",
        error: error.message,
        transient: isTransientMqttJsConnectionError(error),
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
    this.generation += 1;
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
  ): MqttWatch {
    this.reserveSubscription(filter, options);
    const stop = this.listen(filter, onMessage);
    let closed = false;
    const close = () => {
      if (closed) {
        return;
      }
      closed = true;
      stop();
      void this.unsubscribe(filter).catch(() => {});
    };
    const ready = this.subscribeReserved(filter, options).catch((error) => {
      close();
      throw error;
    });
    return { ready, close };
  }

  private listen(filter: string, onMessage: (message: MqttMessage) => void): () => void {
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

  private reserveSubscription(
    topic: string,
    options: IClientSubscribeOptions,
  ): void {
    if (this.subscriptions.has(topic)) {
      throw new Error(`MQTT topic filter already subscribed: ${topic}`);
    }
    this.subscriptions.set(topic, { options });
  }

  private async subscribeReserved(
    topic: string,
    options: IClientSubscribeOptions,
  ): Promise<void> {
    if (!this.client.connected) {
      throw new Error("MQTT broker disconnected");
    }
    const grants = await this.client.subscribeAsync(topic, options);
    if (grants.some(({ qos }) => qos === 128)) {
      throw new Error(`MQTT subscription rejected: ${topic}`);
    }
  }

  private async unsubscribe(topic: string): Promise<void> {
    if (!this.subscriptions.delete(topic) || !this.client.connected) {
      return;
    }
    await this.client.unsubscribeAsync(topic);
  }

  private handleConnect(generation: number): void {
    this.notifyConnection({ state: "connected" });
    void this.restoreSubscriptions(generation);
  }

  private async restoreSubscriptions(generation: number): Promise<void> {
    for (const [topic, subscription] of this.subscriptions) {
      try {
        await this.subscribeReserved(topic, subscription.options);
      } catch (error) {
        if (generation === this.generation && this.client.connected && !this.closing) {
          this.notifyConnection({
            state: "error",
            error: error instanceof Error ? error.message : String(error),
            transient: false,
          });
        }
        return;
      }
    }
    if (generation === this.generation && this.client.connected && !this.closing) {
      this.notifyConnection({ state: "subscriptions-restored" });
    }
  }

  private notifyConnection(event: MqttConnectionEvent): void {
    for (const listener of [...this.connectionListeners]) {
      listener(event);
    }
  }
}
