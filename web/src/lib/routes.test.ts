import { describe, expect, it } from "vitest";
import { browsePath, discoveryPath, readRoute } from "./routes";

describe("semantic routes", () => {
  it.each(["a//b", "/a", "a/", "/", "a///b", "a/%/b"])(
    "preserves exact MQTT identity %s in browse and discovery routes",
    (prefix) => {
      expect(
        readRoute({ hash: browsePath("ws://mqtt:8083", prefix) }).activePrefix,
      ).toBe(prefix);
      expect(
        readRoute({ hash: discoveryPath("ws://mqtt:8083", prefix) })
          .discoveryFilter,
      ).toBe(prefix);
    },
  );
  it("builds readable discovery and browse paths", () => {
    expect(discoveryPath("ws://mqtt:8083", "dt/sinara/+/+")).toBe(
      "#/discover/mqtt:8083/dt/sinara/+/+",
    );
    expect(browsePath("ws://mqtt:8083", "dt/sinara/thermostat-eem/host")).toBe(
      "#/browse/mqtt:8083/dt/sinara/thermostat-eem/host",
    );
    expect(
      browsePath("ws://mqtt:8083", "dt/sinara/thermostat-eem/host", "/pid"),
    ).toBe("#/browse/mqtt:8083/dt/sinara/thermostat-eem/host?path=%2Fpid");
    expect(browsePath("ws://mqtt:8083", "lab/a/b", "", "lab/+/+")).toBe(
      "#/browse/mqtt:8083/lab/a/b?discover=lab%2F%2B%2F%2B",
    );
  });

  it("round-trips WebSocket endpoint paths and queries", () => {
    const broker = "wss://mqtt.quartiq.de:1239/path/to/socket?token=a%2Fb";
    const path = discoveryPath(broker, "dt/sinara/+/+");
    expect(path).toBe(
      "#/discover/wss+mqtt.quartiq.de:1239/dt/sinara/+/+?endpoint=%2Fpath%2Fto%2Fsocket%3Ftoken%3Da%252Fb",
    );
    expect(readRoute({ hash: path }).broker).toBe(broker);
    expect(
      readRoute({ hash: browsePath(broker, "dt/device", "/pid", "dt/+") }),
    ).toMatchObject({
      broker,
      activePrefix: "dt/device",
      subtreePath: "/pid",
      discoveryFilter: "dt/+",
    });
  });

  it("rejects route endpoints that could replace the broker authority", () => {
    expect(
      readRoute({
        hash: "#/discover/wss+mqtt.quartiq.de/dt/+?endpoint=%40evil.example",
      }),
    ).toMatchObject({ page: "landing", broker: "" });
  });

  it("parses hash routes", () => {
    expect(readRoute({ hash: "#/discover/mqtt:8083/dt/sinara/+/+" })).toEqual({
      page: "discover",
      broker: "ws://mqtt:8083",
      discoveryFilter: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
    expect(
      readRoute({
        hash: "#/browse/mqtt:8083/dt/sinara/thermostat-eem/host?path=%2Fpid&discover=dt%2Fsinara%2Fthermostat-eem%2F%2B",
      }),
    ).toEqual({
      page: "browse",
      broker: "ws://mqtt:8083",
      discoveryFilter: "dt/sinara/thermostat-eem/+",
      activePrefix: "dt/sinara/thermostat-eem/host",
      subtreePath: "/pid",
    });
  });

  it("keeps the landing route idle", () => {
    expect(readRoute({ hash: "" })).toEqual({
      page: "landing",
      broker: "",
      discoveryFilter: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
  });

  it("does not start a hidden browse session for an empty prefix", () => {
    expect(readRoute({ hash: "#/browse/mqtt:8083/" })).toMatchObject({
      page: "landing",
      broker: "",
      activePrefix: "",
    });
  });

  it("lands idle instead of inventing a broker for malformed route input", () => {
    expect(readRoute({ hash: "#/discover/%zz/dt/sinara/+/+" })).toEqual({
      page: "landing",
      broker: "",
      discoveryFilter: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
    expect(() => discoveryPath("http://[", "dt/sinara/+/+")).toThrow();
  });

  it("accepts only browser MQTT transports and keeps credentials out of routes", () => {
    expect(() => discoveryPath("mqtt://broker.example", "dt/+")).toThrow(
      "ws:// or wss://",
    );
    expect(() =>
      discoveryPath("wss://user:secret@broker.example", "dt/+"),
    ).toThrow("username and password fields");
  });
});
