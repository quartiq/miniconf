import { describe, expect, it } from "vitest";
import {
  DiscoverySession,
  PrefixSession,
  type DiscoveredPrefix,
} from "./backend";
import type { Schema } from "./schema";

const broker = process.env.MINICONF_WEB_BROKER;
const discoveryFilter = process.env.MINICONF_WEB_FILTER ?? "dt/sinara/+/+";

describe.skipIf(!broker)("Miniconf WebSocket broker", () => {
  it("discovers one target and resolves its retained schema", async () => {
    const discovered = await new Promise<DiscoveredPrefix>(
      (resolve, reject) => {
        let session: DiscoverySession | undefined;
        const controller = new AbortController();
        let done = false;
        const timer = globalThis.setTimeout(() => {
          done = true;
          controller.abort();
          session?.close();
          reject(new Error("Timed out waiting for discovery"));
        }, 3_000);
        void DiscoverySession.connect(
          broker!,
          discoveryFilter,
          {
            prefixes: (prefixes) => {
              if (!prefixes.length) return;
              done = true;
              globalThis.clearTimeout(timer);
              session?.close();
              resolve(prefixes[0]);
            },
            status: (status) => {
              if (status.state !== "failed") return;
              done = true;
              globalThis.clearTimeout(timer);
              controller.abort();
              reject(new Error(status.error));
            },
          },
          { signal: controller.signal },
        ).then((next) => {
          session = next;
          if (done) next.close();
        }, reject);
      },
    );

    const schema = await new Promise<Schema>((resolve, reject) => {
      let session: PrefixSession | undefined;
      const controller = new AbortController();
      let done = false;
      const timer = globalThis.setTimeout(() => {
        done = true;
        controller.abort();
        session?.close();
        reject(new Error("Timed out waiting for schema"));
      }, 5_000);
      void PrefixSession.connect(
        broker!,
        discovered.prefix,
        "",
        {
          alive: () => {},
          schema: (next) => {
            done = true;
            globalThis.clearTimeout(timer);
            session?.close();
            resolve(next);
          },
          settings: () => {},
          status: (status) => {
            if (status.state !== "failed") return;
            done = true;
            globalThis.clearTimeout(timer);
            controller.abort();
            reject(new Error(status.error));
          },
        },
        { signal: controller.signal },
      ).then((next) => {
        session = next;
        if (done) next.close();
      }, reject);
    });

    expect(schema.node("").kind).not.toBe("leaf");
    expect(schema.walk().length).toBeGreaterThan(1);
  }, 10_000);
});
