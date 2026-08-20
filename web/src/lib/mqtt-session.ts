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
  | { state: "error"; error: string };

export type MqttSessionCallbacks = {
  message: (message: MqttMessage) => void;
  reset: () => void;
  status: (status: MqttSessionStatus) => void;
};

export type MqttAuth = {
  username: string;
  password: string;
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
  private closing = false;
  private generation = 0;
  private offline = false;
  private subscribed = false;

  private constructor(
    private readonly client: MqttClient,
    private readonly subscriptions: ISubscriptionMap,
    private readonly callbacks: MqttSessionCallbacks,
  ) {
    client.on("message", (topic, payload, packet) => {
      if (!this.closing) callbacks.message({ topic, payload, packet });
    });
    client.on("connect", () => {
      if (this.closing) return;
      this.offline = false;
      this.subscribed = false;
      callbacks.reset();
      callbacks.status({ state: "restoring" });
      void this.connected(++this.generation);
    });
    client.on("reconnect", () => {
      if (!this.closing) callbacks.status({ state: "reconnecting" });
    });
    client.on("offline", () => this.noteOffline());
    client.on("close", () => this.noteOffline());
    client.on("error", (error: Error) => {
      if (!this.closing) callbacks.status({ state: "error", error: error.message });
    });
  }

  get ready(): boolean {
    return this.subscribed && this.client.connected && !this.closing;
  }

  static async connect(
    broker: string,
    subscriptions: ISubscriptionMap,
    callbacks: MqttSessionCallbacks,
    auth?: Partial<MqttAuth>,
  ): Promise<MqttSession> {
    const url = new URL(broker);
    if (url.protocol !== "ws:" && url.protocol !== "wss:") {
      throw new Error("Broker URL must start with ws:// or wss://");
    }
    if (globalThis.location?.protocol === "https:" && url.protocol === "ws:") {
      throw new Error("HTTPS pages cannot connect to ws:// brokers; open the app over HTTP or use a wss:// broker.");
    }
    const client = mqtt.connect(broker, clientOptions(auth));
    await new Promise<void>((resolve, reject) => {
      const cleanup = () => {
        client.off("connect", connected);
        client.off("close", closed);
        client.off("error", failed);
      };
      const connected = () => {
        cleanup();
        resolve();
      };
      const closed = () => {
        cleanup();
        client.end(true);
        reject(new Error(`Could not connect to ${broker}`));
      };
      const failed = (error: Error) => {
        cleanup();
        client.end(true);
        reject(error);
      };
      client.once("connect", connected);
      client.once("close", closed);
      client.once("error", failed);
    });

    const session = new MqttSession(client, subscriptions, callbacks);
    const generation = ++session.generation;
    callbacks.reset();
    try {
      await session.subscribe();
      if (generation !== session.generation || !client.connected) {
        throw new Error("Connection closed while subscribing");
      }
    } catch (error) {
      session.close();
      throw error;
    }
    session.subscribed = true;
    client.options.reconnectPeriod = 1000;
    return session;
  }

  async publish(topic: string, payload: string, options: IClientPublishOptions): Promise<void> {
    if (!this.ready) throw new Error("MQTT session is not ready");
    await this.client.publishAsync(topic, payload, options);
  }

  close(): void {
    if (this.closing) return;
    this.closing = true;
    this.subscribed = false;
    this.generation += 1;
    this.client.end(true);
  }

  private noteOffline(): void {
    if (this.closing || this.offline) return;
    this.offline = true;
    this.subscribed = false;
    this.generation += 1;
    this.callbacks.status({ state: "offline" });
  }

  private async subscribe(): Promise<void> {
    const grants = await this.client.subscribeAsync(this.subscriptions);
    const rejected = grants.filter(({ qos }) => qos === 128).map(({ topic }) => topic);
    if (rejected.length) throw new Error(`MQTT subscription rejected: ${rejected.join(", ")}`);
  }

  private async connected(generation: number): Promise<void> {
    try {
      await this.subscribe();
    } catch (error) {
      if (generation === this.generation && this.client.connected && !this.closing) {
        this.closing = true;
        this.subscribed = false;
        this.client.end(true);
        this.callbacks.status({
          state: "error",
          error: error instanceof Error ? error.message : String(error),
        });
      }
      return;
    }
    if (generation === this.generation && this.client.connected && !this.closing) {
      this.subscribed = true;
      this.callbacks.status({ state: "connected" });
    }
  }
}
