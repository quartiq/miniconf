import { afterEach, describe, expect, it, vi } from "vitest";
import { PrefixSession, type PruningState } from "./backend";
import { staleTopic } from "./prune";
import { Schema } from "./schema";
import { FakeMqttClient } from "./mqtt-test-fixture";

const connectMock = vi.hoisted(() => vi.fn());
vi.mock("mqtt", () => ({ default: { connect: connectMock } }));
afterEach(() => {
  connectMock.mockReset();
  vi.useRealTimers();
});
const defs = [
  { s: "value" },
  { i: { k: "n" as const, c: { leaf: 0, "": 0 } } },
];
const text = defs.map((def) => JSON.stringify(def)).join("\n") + "\n";
let revision = 0x811c9dc5;
for (const byte of new TextEncoder().encode(text))
  revision = Math.imul(revision ^ byte, 0x01000193) >>> 0;
const schema = new Schema(defs, revision);
const alive = { proto: 1, epoch: 1, schema_rev: revision, pages: 1 };

async function connect(rejectedTopic = "", root = "") {
  const mqtt = new FakeMqttClient();
  mqtt.rejectedTopic = rejectedTopic;
  connectMock.mockReturnValueOnce(mqtt);
  const states: PruningState[] = [];
  const pending = PrefixSession.connect("ws://mqtt:8083", "p", root, {
    alive: () => {},
    schema: () => {},
    settings: () => {},
    status: () => {},
    pruning: (state) => states.push(state),
  });
  mqtt.connect();
  const session = await pending;
  return { mqtt, session, states };
}
function announce(mqtt: FakeMqttClient) {
  mqtt.message("p/alive", JSON.stringify(alive));
  mqtt.message("p/schema/0", text);
}

describe("retained-topic pruning", () => {
  it("classifies exact namespace boundaries and schema leaves", () => {
    for (const topic of [
      "p/settings/leaf",
      "p/set/leaf",
      "p/settings/",
      "p/alive",
      "p/schema/9",
      "p/settingsX/old",
      "other/settings/old",
    ])
      expect(staleTopic("p", schema, topic), topic).toBe(false);
    for (const topic of [
      "p/settings/old",
      "p/set/old",
      "p/settings",
      "p/set",
      "p/response",
      "p/response/leaf",
    ])
      expect(staleTopic("p", schema, topic), topic).toBe(true);
    expect(staleTopic("p", new Schema([{ s: "value" }], 2), "p/settings")).toBe(
      false,
    );
  });

  it("observes before payload validation and clears only candidates captured at the click", async () => {
    const { mqtt, session, states } = await connect();
    mqtt.message("p/settings/leaf", "1");
    mqtt.message("p/settings/old", new Uint8Array([255]));
    mqtt.message("p/set/old", "not JSON");
    mqtt.message("p/response/old", "reply");
    mqtt.message("p/settings/live", "1", false);
    expect(states.at(-1)?.count).toBe(0);
    announce(mqtt);
    expect(states.at(-1)?.count).toBe(3);
    expect(connectMock).toHaveBeenCalledTimes(1);
    expect(mqtt.publications).toEqual([]);
    let release!: () => void;
    mqtt.publishWait = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clearing = session.prune();
    mqtt.message("p/settings/later", "1");
    mqtt.publishWait = undefined;
    release();
    await clearing;
    expect(mqtt.publications.map((p) => p.topic)).toEqual([
      "p/settings/old",
      "p/set/old",
      "p/response/old",
    ]);
    for (const publication of mqtt.publications) {
      expect(publication.payload).toBe("");
      expect(publication.options).toEqual({
        qos: 1,
        retain: true,
        properties: { payloadFormatIndicator: true },
      });
    }
    // Broker echoes, not publish acknowledgments, remove observed topics.
    expect(states.at(-1)?.count).toBe(4);
    for (const publication of mqtt.publications)
      mqtt.message(publication.topic, "");
    expect(states.at(-1)?.count).toBe(1);
    mqtt.message("p/settings/later", "");
    expect(states.at(-1)?.count).toBe(0);
    session.close();
  });

  it("retains a replacement observed before the original clear PUBACK", async () => {
    const { mqtt, session, states } = await connect();
    announce(mqtt);
    mqtt.message("p/settings/old", "1");
    let release!: () => void;
    mqtt.publishWait = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clearing = session.prune();
    mqtt.message("p/settings/old", "");
    mqtt.message("p/settings/old", "replacement");
    release();
    await clearing;
    expect(states.at(-1)?.count).toBe(1);
    expect(states.at(-1)?.message).toBe("Cleared 1");
    expect(states.at(-1)?.failed).toBe(false);
    session.close();
  });

  it("keeps subtree browsing writable when optional cleanup subscriptions are rejected", async () => {
    const { mqtt, session, states } = await connect("p/settings/#", "/leaf");
    announce(mqtt);
    expect(session.ready).toBe(true);
    expect(states.at(-1)?.coverageWarning).toContain("p/settings/#");
    mqtt.message("p/set/old", "1");
    await session.prune();
    expect(mqtt.publications).toMatchObject([
      { topic: "p/set/old", payload: "" },
    ]);
    const setting = session.set("/leaf", "2");
    mqtt.respond(1, "Ok");
    await expect(setting).resolves.toMatchObject({ ok: true });
    expect(mqtt.ended).toBe(false);
    session.close();
  });

  it.each(["offline", "epoch", "close"])(
    "stops an in-flight clear on %s without sending the next topic",
    async (reason) => {
      const { mqtt, session, states } = await connect();
      announce(mqtt);
      mqtt.message("p/settings/a", "1");
      mqtt.message("p/settings/b", "2");
      mqtt.publishWait = new Promise(() => {});
      const clearing = session.prune();
      if (reason === "offline") mqtt.disconnect();
      else if (reason === "close") session.close();
      else mqtt.message("p/alive", JSON.stringify({ ...alive, epoch: 2 }));
      await clearing;
      expect(mqtt.publications).toHaveLength(1);
      expect(mqtt.removedPublications).toEqual([1]);
      expect(states.at(-1)?.pending).toBe(false);
      expect(states.at(-1)?.message).toContain("outcome unknown");
      expect(states.at(-1)?.failed).toBe(true);
      session.close();
    },
  );

  it.each(["rejected", "timeout"])(
    "reports partial progress after a %s publish without closing browsing",
    async (failure) => {
      vi.useFakeTimers();
      const { mqtt, session, states } = await connect();
      announce(mqtt);
      for (const name of ["a", "b", "c"])
        mqtt.message(`p/settings/${name}`, "1");
      vi.spyOn(mqtt, "publishAsync")
        .mockImplementationOnce(async (topic, payload, options) => {
          mqtt.publications.push({ topic, payload, options });
          mqtt.message(topic, "");
        })
        .mockImplementationOnce(async (topic, payload, options) => {
          mqtt.publications.push({ topic, payload, options });
          if (failure === "rejected") throw new Error("Not authorized");
          await new Promise(() => {});
        });
      const clearing = session.prune();
      await vi.advanceTimersByTimeAsync(10_000);
      await clearing;
      expect(mqtt.publications.map((p) => p.topic)).toEqual([
        "p/settings/a",
        "p/settings/b",
      ]);
      expect(states.at(-1)).toMatchObject({ count: 2, pending: false });
      expect(states.at(-1)?.message).toContain("Cleared 1;");
      expect(session.ready).toBe(true);
      session.close();
    },
  );
});
