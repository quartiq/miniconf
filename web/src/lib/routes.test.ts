import { describe, expect, it } from "vitest";
import { browsePath, discoveryPath, readRoute } from "./routes";

describe("semantic routes", () => {
  it("builds readable discovery and browse paths", () => {
    expect(discoveryPath("ws://mqtt:8083", "dt/sinara/+/+")).toBe(
      "#/discover/mqtt:8083/dt/sinara/+/+",
    );
    expect(browsePath("ws://mqtt:8083", "dt/sinara/thermostat-eem/host")).toBe(
      "#/browse/mqtt:8083/dt/sinara/thermostat-eem/host",
    );
    expect(browsePath("ws://mqtt:8083", "dt/sinara/thermostat-eem/host", "/pid")).toBe(
      "#/browse/mqtt:8083/dt/sinara/thermostat-eem/host?path=%2Fpid",
    );
    expect(browsePath("ws://mqtt:8083", "lab/a/b", "", "lab/+/+")).toBe(
      "#/browse/mqtt:8083/lab/a/b?discover=lab%2F%2B%2F%2B",
    );
  });

  it("round-trips broker paths and query strings", () => {
    const path = discoveryPath("wss://broker.example/mqtt?tenant=lab", "dt/sinara/+/+");
    expect(readRoute({ hash: path })).toMatchObject({
      broker: "wss://broker.example/mqtt?tenant=lab",
      discoveryPattern: "dt/sinara/+/+",
    });
  });

  it("parses hash routes", () => {
    expect(readRoute({ hash: "#/discover/mqtt:8083/dt/sinara/+/+" })).toEqual({
      broker: "ws://mqtt:8083",
      discoveryPattern: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
    expect(
      readRoute({
        hash: "#/browse/mqtt:8083/dt/sinara/thermostat-eem/host?path=%2Fpid&discover=dt%2Fsinara%2Fthermostat-eem%2F%2B",
      }),
    ).toEqual({
      broker: "ws://mqtt:8083",
      discoveryPattern: "dt/sinara/thermostat-eem/+",
      activePrefix: "dt/sinara/thermostat-eem/host",
      subtreePath: "/pid",
    });
  });

  it("ignores legacy query routing", () => {
    expect(readRoute({ hash: "", protocol: "http:" })).toEqual({
      broker: "ws://mqtt:8083",
      discoveryPattern: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
  });

  it("uses a secure default broker on HTTPS pages", () => {
    expect(readRoute({ hash: "", protocol: "https:" })).toMatchObject({
      broker: "wss://mqtt:8084",
    });
  });

  it("falls back instead of throwing on malformed route input", () => {
    expect(readRoute({ hash: "#/discover/%zz/dt/sinara/+/+", protocol: "https:" })).toEqual({
      broker: "wss://mqtt:8084",
      discoveryPattern: "dt/sinara/+/+",
      activePrefix: "",
      subtreePath: "",
    });
    expect(discoveryPath("http://[", "dt/sinara/+/+")).toBe(
      "#/discover/mqtt:8083/dt/sinara/+/+",
    );
  });
});
