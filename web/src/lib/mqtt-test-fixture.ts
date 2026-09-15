import { EventEmitter } from "node:events";
import type {
  IClientOptions,
  IClientPublishOptions,
  ISubscriptionMap,
} from "mqtt";

export class FakeMqttClient extends EventEmitter {
  options: IClientOptions = {};
  connected = false;
  ended = false;
  readonly subscriptions: ISubscriptionMap[] = [];
  readonly publications: Array<{
    topic: string;
    payload: string;
    options: IClientPublishOptions;
  }> = [];
  readonly removedPublications: number[] = [];

  getLastMessageId(): number {
    return this.publications.length;
  }

  removeOutgoingMessage(id: number): this {
    this.removedPublications.push(id);
    return this;
  }
  subscribeError: Error | undefined;
  subscribeWait: Promise<void> | undefined;
  publishWait: Promise<void> | undefined;
  rejectedTopic = "";

  connect(): void {
    this.connected = true;
    this.emit("connect");
  }

  disconnect(): void {
    this.connected = false;
    this.emit("offline");
    this.emit("close");
  }

  end(): this {
    this.connected = false;
    this.ended = true;
    return this;
  }

  subscribe(
    subscriptions: ISubscriptionMap,
    callback: (
      error: Error | null,
      grants?: unknown,
      packet?: { granted: number[] },
    ) => void,
  ): this {
    this.subscriptions.push(subscriptions);
    void Promise.resolve(this.subscribeWait)
      .then(() => {
        if (this.subscribeError) throw this.subscribeError;
        return Object.keys(subscriptions).map((topic) =>
          topic === this.rejectedTopic ? 128 : (subscriptions[topic].qos ?? 0),
        );
      })
      .then(
        (granted) =>
          callback(
            granted.some((qos) => qos >= 128)
              ? new Error("Subscribe error")
              : null,
            undefined,
            { granted },
          ),
        (error) => callback(error),
      );
    return this;
  }

  async publishAsync(
    topic: string,
    payload: string,
    options: IClientPublishOptions,
  ) {
    this.publications.push({ topic, payload, options });
    await this.publishWait;
  }

  message(
    topic: string,
    payload: string | Uint8Array,
    retain = true,
    userProperties?: Record<string, string | string[]>,
    correlationData?: Uint8Array,
  ): void {
    this.emit(
      "message",
      topic,
      typeof payload === "string" ? new TextEncoder().encode(payload) : payload,
      {
        cmd: "publish",
        retain,
        properties: { userProperties, correlationData },
      },
    );
  }

  respond(index: number, code: string, payload = ""): void {
    const publication = this.publications[index];
    this.message(
      publication.options.properties?.responseTopic ?? "",
      payload,
      false,
      { code },
      publication.options.properties?.correlationData as Uint8Array,
    );
  }
}
