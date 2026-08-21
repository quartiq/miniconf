import { describe, expect, it } from "vitest";
import { DiscoverySession, PrefixSession, type DiscoveredPrefix } from "./backend";
import type { Schema } from "./schema";

const broker = process.env.MINICONF_WEB_BROKER;
const discoveryPattern = process.env.MINICONF_WEB_FILTER ?? "dt/sinara/+/+";

describe.skipIf(!broker)("Miniconf WebSocket broker", () => {
  it("discovers one target and resolves its retained schema", async () => {
    const discovered = await new Promise<DiscoveredPrefix>((resolve, reject) => {
      let session: DiscoverySession | undefined;
      let done = false;
      const timer = globalThis.setTimeout(() => {
        session?.close();
        reject(new Error("Timed out waiting for discovery"));
      }, 3_000);
      void DiscoverySession.connect(broker!, discoveryPattern, {
        prefixes: (prefixes) => {
          if (!prefixes.length) return;
          done = true;
          globalThis.clearTimeout(timer);
          session?.close();
          resolve(prefixes[0]);
        },
        error: reject,
        status: () => {},
      }).then((next) => {
        session = next;
        if (done) next.close();
      }, reject);
    });

    const schema = await new Promise<Schema>(
      (resolve, reject) => {
        let session: PrefixSession | undefined;
        let done = false;
        const timer = globalThis.setTimeout(() => {
          session?.close();
          reject(new Error("Timed out waiting for schema"));
        }, 5_000);
        void PrefixSession.connect(broker!, discovered.prefix, "", {
          error: reject,
          alive: () => {},
          response: () => {},
          schema: (next) => {
            done = true;
            globalThis.clearTimeout(timer);
            session?.close();
            resolve(next);
          },
          settings: () => {},
          status: () => {},
        }).then((next) => {
          session = next;
          if (done) next.close();
        }, reject);
      },
    );

    expect(schema.node("").kind).not.toBe("leaf");
    expect(schema.walk().length).toBeGreaterThan(1);
  }, 10_000);
});
