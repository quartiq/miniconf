import { describe, expect, it } from "vitest";
import { browsePath, DEFAULT_FILTER, discoveryPath, readRoute } from "./routes";

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
    expect(discoveryPath("ws://mqtt:8083", "dt/test/+/+")).toBe(
      "#/discover/mqtt:8083/dt/test/+/+",
    );
    expect(browsePath("ws://mqtt:8083", "dt/test/device/host")).toBe(
      "#/browse/mqtt:8083/dt/test/device/host",
    );
    expect(browsePath("ws://mqtt:8083", "dt/test/device/host", "/value")).toBe(
      "#/browse/mqtt:8083/dt/test/device/host?path=%2Fvalue",
    );
    expect(browsePath("ws://mqtt:8083", "lab/a/b", "", "lab/+/+")).toBe(
      "#/browse/mqtt:8083/lab/a/b?discover=lab%2F%2B%2F%2B",
    );
  });

  it("round-trips WebSocket endpoint paths and queries", () => {
    const broker = "wss://broker.example:1239/path/to/socket?token=a%2Fb";
    const path = discoveryPath(broker, "dt/test/+/+");
    expect(path).toBe(
      "#/discover/wss+broker.example:1239/dt/test/+/+?endpoint=%2Fpath%2Fto%2Fsocket%3Ftoken%3Da%252Fb",
    );
    expect(readRoute({ hash: path }).broker).toBe(broker);
    expect(
      readRoute({ hash: browsePath(broker, "dt/device", "/value", "dt/+") }),
    ).toMatchObject({
      broker,
      activePrefix: "dt/device",
      subtreePath: "/value",
      discoveryFilter: "dt/+",
    });
  });

  it("rejects route endpoints that could replace the broker authority", () => {
    expect(
      readRoute({
        hash: "#/discover/wss+broker.example/dt/+?endpoint=%40evil.example",
      }),
    ).toMatchObject({ page: "landing", broker: "" });
  });

  it("parses hash routes", () => {
    expect(readRoute({ hash: "#/discover/mqtt:8083/dt/test/+/+" })).toEqual({
      page: "discover",
      broker: "ws://mqtt:8083",
      discoveryFilter: "dt/test/+/+",
      activePrefix: "",
      subtreePath: "",
    });
    expect(
      readRoute({
        hash: "#/browse/mqtt:8083/dt/test/device/host?path=%2Fvalue&discover=dt%2Ftest%2Fdevice%2F%2B",
      }),
    ).toEqual({
      page: "browse",
      broker: "ws://mqtt:8083",
      discoveryFilter: "dt/test/device/+",
      activePrefix: "dt/test/device/host",
      subtreePath: "/value",
    });
  });

  it("keeps the landing route idle", () => {
    expect(readRoute({ hash: "" })).toEqual({
      page: "landing",
      broker: "",
      discoveryFilter: DEFAULT_FILTER,
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
    expect(readRoute({ hash: "#/discover/%zz/dt/test/+/+" })).toEqual({
      page: "landing",
      broker: "",
      discoveryFilter: DEFAULT_FILTER,
      activePrefix: "",
      subtreePath: "",
    });
    expect(() => discoveryPath("http://[", "dt/test/+/+")).toThrow();
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
