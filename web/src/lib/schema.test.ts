import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { formatSchemaMetadata, renderSchemaTree, Schema, type CompactDef } from "./schema";

function fixtureSchema(): Schema {
  const fixture = resolve("../testdata/compact-schema/fixture.ndjson");
  const defs = readFileSync(fixture, "utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line) as CompactDef);
  return new Schema(defs, 1);
}

function mpllSchema(): Schema {
  return new Schema(
    [
      {},
      { i: { k: "h", l: 2, c: 0 } },
      { s: { ty: "str" } },
      { s: { ty: "f32" } },
      {
        m: {
          doc: "Floating point BA coefficients before quantization",
          typename: "Ba",
        },
        i: {
          k: "n",
          c: {
            ba: {
              r: 0,
              m: { doc: "Coefficient array: [[b0, b1, b2], [a0, a1, a2]]" },
            },
            u: { r: 3, m: { doc: "Summing junction offset" } },
            min: { r: 3, m: { doc: "Output lower limit" } },
            max: { r: 3, m: { doc: "Output upper limit" } },
          },
        },
      },
      { i: { k: "n", c: { i2: 3, i: 3, p: 3, d: 3, d2: 3 } } },
      {
        m: { doc: "PID Controller parameters", typename: "Pid" },
        i: {
          k: "n",
          c: {
            order: { r: 0, m: { doc: "Feedback term order" } },
            gain: { r: 5, m: { doc: "Gain" } },
            limit: { r: 5, m: { doc: "Gain limit" } },
            setpoint: { r: 3, m: { doc: "Setpoint" } },
            min: { r: 3, m: { doc: "Output lower limit" } },
            max: { r: 3, m: { doc: "Output upper limit" } },
          },
        },
      },
      {
        m: { doc: "Standard biquad parametrizations", typename: "FilterRepr" },
        i: {
          k: "n",
          c: {
            typ: { r: 0, m: { doc: "Filter style" } },
            frequency: { r: 3, m: { doc: "Relative critical frequency" } },
            gain_db: { r: 3, m: { doc: "Passband gain in dB" } },
            shelf_db: { r: 3, m: { doc: "Shelf gain in dB" } },
            shape: { r: 0, m: { doc: "Q/Bandwidth/Slope" } },
            offset: { r: 3, m: { doc: "Summing junction offset" } },
            min: { r: 3, m: { doc: "Lower output limit" } },
            max: { r: 3, m: { doc: "Upper output limit" } },
          },
        },
      },
      {
        m: { doc: "Representation of a biquad", typename: "BiquadRepr" },
        s: { oneof: true },
        i: { k: "n", c: { Ba: 4, Raw: 0, Pid: 6, Filter: 7 } },
      },
      { i: { k: "h", l: 2, c: 3 } },
      {
        m: { typename: "MpllConfig" },
        i: {
          k: "n",
          c: {
            lp: { r: 0, m: { doc: "Lowpass filter coefficients" } },
            repr: { r: 2, m: { doc: "Filter representation" } },
            iir: { r: 8, m: { doc: "Phase-to-frequency filter" } },
            amplitude: { r: 9, m: { doc: "Output amplitude" } },
          },
        },
      },
      { s: { ty: "bool" } },
      {
        m: { typename: "App" },
        i: {
          k: "n",
          c: {
            afe: { r: 1, m: { doc: "AFE gain." } },
            mpll: { r: 10, m: { doc: "MPLL DSP configuration." } },
            telemetry_period: {
              r: 3,
              m: { doc: "Specifies the telemetry output period in seconds." },
            },
            stream: { r: 0, m: { doc: "Specifies the target for data streaming." } },
            activate: {
              r: 11,
              m: { doc: "Activate settings immediately on change." },
            },
          },
        },
      },
    ],
    2341646775,
  );
}

describe("Schema", () => {
  it("matches the compact schema fixture paths", () => {
    const schema = fixtureSchema();

    expect(schema.walk().map((node) => node.path)).toEqual([
      "",
      "/value",
      "/nested",
      "/nested/leaf",
    ]);
    expect(schema.node("/value").edge).toEqual({ role: "selector" });
  });

  it("uses the empty path for the root", () => {
    const schema = fixtureSchema();

    expect(schema.path("")).toBe("");
    expect(() => schema.path("/")).toThrow("Unknown schema path");
  });

  it("resolves numbered homogeneous paths from the live MPLL schema shape", () => {
    const schema = mpllSchema();

    expect(schema.node("/afe/0").kind).toBe("leaf");
    expect(schema.node("/mpll/amplitude/1").sem).toEqual({ ty: "f32" });
    expect(schema.node("/mpll/iir/Ba").node).toMatchObject({ typename: "Ba" });
    expect(schema.children("/mpll/amplitude").map((node) => node.path)).toEqual([
      "/mpll/amplitude/0",
      "/mpll/amplitude/1",
    ]);
  });

  it("renders schema trees like the Python CLI", () => {
    const schema = fixtureSchema();

    expect(renderSchemaTree(schema)).toBe(
      '├─ value [edge role="selector"]\n└─ nested\n   └─ leaf',
    );
    expect(renderSchemaTree(mpllSchema(), "/mpll/amplitude")).toBe(
      'amplitude [homogeneous] [edge doc="Output amplitude"]\n└─ 0..2 [homogeneous] [sem ty=f32]',
    );
  });

  it("formats schema metadata for selected rows", () => {
    const node = mpllSchema().node("/mpll/amplitude/1");

    expect(formatSchemaMetadata(node)).toBe("kind leaf\nsem ty=f32");
  });

  it("renders unicode and multiline schema metadata literally", () => {
    const schema = new Schema([
      { s: { ty: "f32", unit: "Hz²" } },
      {
        i: { k: "n", c: { leaf: { r: 0, m: { doc: "edge line 1\nedge line 2" } } } },
        m: { doc: "node line 1\nnode line 2", typename: "Root" },
      },
    ], 1);

    expect(formatSchemaMetadata(schema.node("/leaf"))).toBe(
      "kind leaf\nsem ty=f32\nsem unit=Hz²\nedge doc:\n  edge line 1\n  edge line 2",
    );
    expect(formatSchemaMetadata(schema.node(""))).toBe(
      "kind named\nnode doc:\n  node line 1\n  node line 2\nnode typename=Root",
    );
  });
});
