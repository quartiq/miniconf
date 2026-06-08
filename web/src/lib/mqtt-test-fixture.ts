import { EventEmitter } from "node:events";

export class FakeMqttClient extends EventEmitter {
  options: { reconnectPeriod?: number } = {};
  connected = true;
  ended = false;
  publications: string[] = [];
  subscriptions: string[] = [];
  unsubscriptions: string[] = [];

  end() {
    this.ended = true;
  }

  async publishAsync(topic: string) {
    this.publications.push(topic);
  }

  async subscribeAsync(topic: string) {
    this.subscriptions.push(topic);
    return undefined;
  }

  async unsubscribeAsync(topic: string) {
    this.unsubscriptions.push(topic);
  }

  completeSubscribe(): void {
    // Compatibility hook for tests that want to mark the subscribe point.
  }

  publishRetained(topic: string, payload: string, userProperties?: Record<string, string | string[]>): void {
    this.emit("message", topic, new TextEncoder().encode(payload), {
      retain: true,
      properties: { userProperties },
    });
  }
}

export class ResponseMqttClient extends EventEmitter {
  readonly connected = true;
  readonly publications: {
    topic: string;
    payload: string;
    properties: { correlationData?: unknown; responseTopic?: string };
  }[] = [];

  async subscribeAsync(_topic: string, _options: unknown) {
    return undefined;
  }

  async unsubscribeAsync(_topic: string) {
    return undefined;
  }

  async publishAsync(
    topic: string,
    payload: string,
    options: { properties?: { correlationData?: unknown; responseTopic?: string } },
  ) {
    this.publications.push({
      topic,
      payload,
      properties: options.properties ?? {},
    });
  }

  respond(index: number, code: string, payload = ""): void {
    this.emit(
      "message",
      this.publications[index].properties.responseTopic,
      new TextEncoder().encode(payload),
      {
        properties: {
          correlationData: this.publications[index].properties.correlationData,
          userProperties: { code },
        },
      },
    );
  }

  respondToSet(path: string, code: string, payload = ""): void {
    const index = this.publications.findIndex((publication) => publication.topic.endsWith(path));
    if (index < 0) {
      throw new Error(`No publication for ${path}`);
    }
    this.respond(index, code, payload);
  }

  end() {}
}
