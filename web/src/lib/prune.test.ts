import { afterEach, describe, expect, it, vi } from "vitest";
import { RetainedPruner, staleTopic, type PruneState } from "./prune";
import { Schema } from "./schema";
import { FakeMqttClient } from "./mqtt-test-fixture";
const connectMock = vi.hoisted(() => vi.fn());
vi.mock("mqtt", () => ({ default: { connect: connectMock } }));
afterEach(() => connectMock.mockReset());
const schema = new Schema(
  [{ s: "value" }, { i: { k: "n", c: { leaf: 0, "": 0 } } }],
  7,
);
const alive = { proto: 1, epoch: 1, schema_rev: 7, pages: 1 };

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
    ]) {
      expect(staleTopic("p", schema, topic), topic).toBe(false);
    }
    for (const topic of [
      "p/settings/old",
      "p/set/old",
      "p/settings",
      "p/set",
      "p/response",
      "p/response/leaf",
      "p/response/random",
    ]) {
      expect(staleTopic("p", schema, topic), topic).toBe(true);
    }
    const rootLeaf = new Schema([{ s: "value" }], 2);
    expect(staleTopic("p", rootLeaf, "p/settings")).toBe(false);
  });

  it("previews without writing and clears only the approved retained topics", async () => {
    const mqtt = new FakeMqttClient();
    const states: PruneState[] = [];
    connectMock.mockReturnValueOnce(mqtt);
    const pending = RetainedPruner.connect(
      "ws://mqtt:8083",
      "p",
      { schema, alive },
      (state) => states.push(state),
    );
    mqtt.connect();
    const pruner = await pending;
    mqtt.message("p/alive", JSON.stringify(alive));
    mqtt.message("p/settings/leaf", "1");
    mqtt.message("p/settings/old", "2");
    mqtt.message("p/set/old", "3");
    mqtt.message("p/response/old", "4");
    mqtt.message("p/settings/live", "5", false);
    expect(mqtt.publications).toEqual([]);
    const approved = states.at(-1)!.topics;
    expect(approved).toEqual(["p/response/old", "p/set/old", "p/settings/old"]);
    mqtt.message("p/settings/later", "6");
    await pruner.clear(approved);
    expect(mqtt.publications.map(({ topic }) => topic)).toEqual(approved);
    for (const publication of mqtt.publications) {
      expect(publication.payload).toBe("");
      expect(publication.options).toEqual({
        qos: 1,
        retain: true,
        properties: { payloadFormatIndicator: true },
      });
    }
    expect(states.at(-1)!.topics).toEqual(["p/settings/later"]);
    pruner.close();
  });

  it("refuses clearing after alive changes or disappears", async () => {
    for (const manifest of ["", JSON.stringify({ ...alive, epoch: 2 })]) {
      const mqtt = new FakeMqttClient();
      connectMock.mockReturnValueOnce(mqtt);
      const pending = RetainedPruner.connect(
        "ws://mqtt:8083",
        "p",
        { schema, alive },
        () => {},
      );
      mqtt.connect();
      const pruner = await pending;
      mqtt.message("p/alive", JSON.stringify(alive));
      mqtt.message("p/settings/old", "2");
      mqtt.message("p/alive", manifest);
      await expect(pruner.clear(["p/settings/old"])).rejects.toThrow(
        "no longer ready",
      );
      expect(mqtt.publications).toEqual([]);
      expect(mqtt.ended).toBe(true);
    }
  });

  it("stops a clear in flight when the device changes", async () => {
    const mqtt = new FakeMqttClient();
    connectMock.mockReturnValueOnce(mqtt);
    const states: PruneState[] = [];
    const pending = RetainedPruner.connect(
      "ws://mqtt:8083",
      "p",
      { schema, alive },
      (state) => states.push(state),
    );
    mqtt.connect();
    const pruner = await pending;
    mqtt.message("p/alive", JSON.stringify(alive));
    mqtt.message("p/settings/old", "1");
    mqtt.message("p/set/old", "2");
    mqtt.publishWait = new Promise(() => {});
    const clearing = pruner.clear(states.at(-1)!.topics);
    const rejected = expect(clearing).rejects.toThrow("cancelled");
    mqtt.message("p/alive", JSON.stringify({ ...alive, epoch: 2 }));
    await rejected;
    expect(mqtt.publications).toHaveLength(1);
    expect(states.at(-1)?.ready).toBe(false);
    expect(states.at(-1)?.message).toContain("outcome unknown");
  });

  it.each(["rejected", "timeout"])(
    "preserves partial progress after a %s publish",
    async (failure) => {
      vi.useFakeTimers();
      try {
        const mqtt = new FakeMqttClient();
        connectMock.mockReturnValueOnce(mqtt);
        const states: PruneState[] = [];
        const pending = RetainedPruner.connect(
          "ws://mqtt:8083",
          "p",
          { schema, alive },
          (state) => states.push(state),
        );
        mqtt.connect();
        const pruner = await pending;
        mqtt.message("p/alive", JSON.stringify(alive));
        for (const name of ["a", "b", "c"])
          mqtt.message(`p/settings/${name}`, "1");
        vi.spyOn(mqtt, "publishAsync")
          .mockImplementationOnce(async (topic, payload, options) => {
            mqtt.publications.push({ topic, payload, options });
          })
          .mockImplementationOnce(async (topic, payload, options) => {
            mqtt.publications.push({ topic, payload, options });
            if (failure === "rejected") throw new Error("Not authorized");
            await new Promise(() => {});
          });
        const clearing = pruner.clear(states.at(-1)!.topics);
        const rejected = expect(clearing).rejects.toThrow(
          failure === "rejected" ? "Not authorized" : "timed out",
        );
        await vi.advanceTimersByTimeAsync(10_000);
        await rejected;
        expect(mqtt.publications.map((p) => p.topic)).toEqual([
          "p/settings/a",
          "p/settings/b",
        ]);
        expect(states.at(-1)).toMatchObject({
          ready: false,
          topics: ["p/settings/b", "p/settings/c"],
        });
        expect(states.at(-1)!.message).toContain("Cleared 1;");
        expect(mqtt.ended).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    },
  );
});
