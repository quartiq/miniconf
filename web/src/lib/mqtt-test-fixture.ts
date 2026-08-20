import { EventEmitter } from "node:events";
import type { IClientOptions, IClientPublishOptions, ISubscriptionMap } from "mqtt";

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
  subscribeError: Error | undefined;
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

  async subscribeAsync(subscriptions: ISubscriptionMap) {
    this.subscriptions.push(subscriptions);
    if (this.subscribeError) throw this.subscribeError;
    return Object.keys(subscriptions).map((topic) => ({
      topic,
      qos: topic === this.rejectedTopic ? 128 as const : subscriptions[topic].qos ?? 0,
    }));
  }

  async publishAsync(topic: string, payload: string, options: IClientPublishOptions) {
    this.publications.push({ topic, payload, options });
  }

  message(
    topic: string,
    payload: string,
    retain = true,
    userProperties?: Record<string, string | string[]>,
    correlationData?: Uint8Array,
  ): void {
    this.emit("message", topic, new TextEncoder().encode(payload), {
      cmd: "publish",
      retain,
      properties: { userProperties, correlationData },
    });
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
