import type { AliveManifest } from "./backend";
import {
  MqttSession,
  type ConnectOptions,
  type MqttMessage,
} from "./mqtt-session";
import type { Schema } from "./schema";

// Pruning is an explicit broker maintenance operation, independent of browsing.
// A retained request at a valid leaf is preserved; responses are always transient.
export function staleTopic(
  prefix: string,
  schema: Schema,
  topic: string,
): boolean {
  for (const namespace of ["settings", "set"]) {
    const base = `${prefix}/${namespace}`;
    if (topic !== base && !topic.startsWith(`${base}/`)) continue;
    try {
      return schema.node(topic.slice(base.length)).kind !== "leaf";
    } catch {
      return true;
    }
  }
  const response = `${prefix}/response`;
  return topic === response || topic.startsWith(`${response}/`);
}

export type PruneState = { topics: string[]; ready: boolean; message: string };

export class RetainedPruner {
  private mqtt: MqttSession | undefined;
  private readonly topics = new Set<string>();
  private alive = false;
  private clearing = false;
  private closed = false;
  private message =
    "Review observed stale retained topics; new arrivals may update this list.";

  private constructor(
    private readonly prefix: string,
    private readonly context: { schema: Schema; alive: AliveManifest },
    private readonly update: (state: PruneState) => void,
  ) {}

  static async connect(
    broker: string,
    prefix: string,
    context: { schema: Schema; alive: AliveManifest },
    update: (state: PruneState) => void,
    options: ConnectOptions = {},
  ): Promise<RetainedPruner> {
    const pruner = new RetainedPruner(prefix, context, update);
    const retained = { qos: 1, rap: true, rh: 0 } as const;
    try {
      pruner.mqtt = await MqttSession.connect(
        broker,
        {
          [`${prefix}/alive`]: retained,
          [`${prefix}/settings/#`]: retained,
          [`${prefix}/set/#`]: retained,
          [`${prefix}/response/#`]: retained,
        },
        {
          message: (message) => pruner.handle(message),
          reset: () => {
            pruner.topics.clear();
            pruner.alive = false;
          },
          status: (status) => {
            if (status.state === "offline" || status.state === "failed") {
              pruner.close();
              pruner.report("Connection lost; review again before pruning.");
            }
          },
        },
        options,
      );
      if (pruner.closed) pruner.mqtt.close();
      pruner.report();
      return pruner;
    } catch (error) {
      pruner.close();
      throw error;
    }
  }

  private handle(message: MqttMessage): void {
    if (this.closed || !message.packet.retain) return;
    if (message.topic === `${this.prefix}/alive`) {
      try {
        const alive = JSON.parse(
          new TextDecoder("utf-8", { fatal: true }).decode(message.payload),
        );
        if (
          alive.proto !== this.context.alive.proto ||
          alive.epoch !== this.context.alive.epoch ||
          alive.schema_rev !== this.context.alive.schema_rev ||
          alive.pages !== this.context.alive.pages
        )
          throw new Error();
        this.alive = true;
      } catch {
        this.close();
        this.report(
          "Device announcement changed; review again before pruning.",
        );
        return;
      }
    } else if (staleTopic(this.prefix, this.context.schema, message.topic)) {
      if (message.payload.byteLength) this.topics.add(message.topic);
      else this.topics.delete(message.topic);
    }
    this.report();
  }

  // Clear only the exact preview the user approved, even if more topics arrive.
  async clear(topics: readonly string[]): Promise<void> {
    if (!this.ready || this.clearing)
      throw new Error("Review is no longer ready");
    this.clearing = true;
    this.report("Clearing retained topics…");
    let cleared = 0;
    try {
      for (const topic of topics) {
        if (!this.ready)
          throw new Error("Device or connection changed; review again");
        if (
          !this.topics.has(topic) ||
          !staleTopic(this.prefix, this.context.schema, topic)
        )
          continue;
        // No auth or response properties: this clears broker storage, not device state.
        await this.mqtt!.publish(topic, "", {
          qos: 1,
          retain: true,
          properties: { payloadFormatIndicator: true },
        });
        this.topics.delete(topic);
        cleared += 1;
      }
      this.clearing = false;
      this.report(
        `Cleared ${cleared} retained topic${cleared === 1 ? "" : "s"}.`,
      );
    } catch (error) {
      this.clearing = false;
      this.close();
      this.report(
        `Cleared ${cleared}; remaining outcome unknown. Review again.`,
      );
      throw error;
    }
  }

  close(): void {
    this.closed = true;
    this.alive = false;
    this.mqtt?.close();
  }

  private get ready(): boolean {
    return !this.closed && this.alive && !!this.mqtt?.ready;
  }

  private report(message?: string): void {
    if (message !== undefined) this.message = message;
    this.update({
      topics: [...this.topics].sort(),
      ready: this.ready && !this.clearing,
      message: this.message,
    });
  }
}
