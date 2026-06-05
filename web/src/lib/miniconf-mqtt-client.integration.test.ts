import { describe, expect, it } from "vitest";

import { MiniconfMqttClient, type DiscoveredPrefix } from "./miniconf-mqtt-client";

const broker = process.env.MINICONF_WEB_BROKER;
const discoveryPattern = process.env.MINICONF_WEB_FILTER ?? "dt/sinara/+/+";

describe.skipIf(!broker)("Miniconf WebSocket broker", () => {
  it("discovers one target and resolves its retained schema", async () => {
    const client = await MiniconfMqttClient.connect(broker!);
    try {
      const prefixes = await new Promise<DiscoveredPrefix[]>((resolve, reject) => {
        let stop: (() => void) | undefined;
        const timer = globalThis.setTimeout(() => {
          stop?.();
          reject(new Error("Timed out waiting for discovery"));
        }, 3_000);
        stop = client.watchDiscovery(discoveryPattern, (next) => {
          if (!next.length) {
            return;
          }
          globalThis.clearTimeout(timer);
          stop?.();
          resolve(next);
        });
      });
      expect(prefixes.length).toBeGreaterThan(0);

      const discovered = prefixes[0];
      const schema = await client.schema(discovered.prefix, discovered.aliveManifest);

      expect(schema.node("").kind).not.toBe("leaf");
      expect(schema.walk().length).toBeGreaterThan(1);
      expect(() => schema.path("/")).toThrow("Unknown schema path");
    } finally {
      client.close();
    }
  }, 10_000);
});
