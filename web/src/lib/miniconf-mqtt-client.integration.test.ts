import { describe, expect, it } from "vitest";

import { MiniconfMqttClient } from "./miniconf-mqtt-client";

const broker = process.env.MINICONF_WEB_BROKER;
const discoveryPattern = process.env.MINICONF_WEB_FILTER ?? "dt/sinara/+/+";

describe.skipIf(!broker)("Miniconf WebSocket broker", () => {
  it("discovers one target and resolves its retained schema", async () => {
    const client = await MiniconfMqttClient.connect(broker!);
    try {
      const prefixes = await client.discover(discoveryPattern);
      expect(prefixes.length).toBeGreaterThan(0);

      let foundMpll = false;
      for (const discovered of prefixes) {
        const aliveManifest = await client.aliveManifest(discovered.prefix);
        const schema = await client.schema(discovered.prefix, aliveManifest);

        expect(schema.node("").kind).not.toBe("leaf");
        expect(schema.walk().length).toBeGreaterThan(1);
        expect(() => schema.path("/")).toThrow("Unknown schema path");

        try {
          expect(schema.node("/mpll/amplitude/1").sem).toEqual({ ty: "f32" });
          foundMpll = true;
          break;
        } catch (err) {
          if (err instanceof Error && err.message.startsWith("Unknown schema path")) {
            continue;
          }
          throw err;
        }
      }

      expect(foundMpll).toBe(true);
    } finally {
      client.close();
    }
  }, 10_000);
});
