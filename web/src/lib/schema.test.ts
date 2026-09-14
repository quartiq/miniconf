import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { schemaSummary, Schema, type CompactDef } from "./schema";

function fixtureSchema(): Schema {
  const fixture = resolve("../fixtures/compact-schema.ndjson");
  const defs = readFileSync(fixture, "utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line) as CompactDef);
  return new Schema(defs, 1);
}

const indexedSchema = new Schema(
  [
    { s: { ty: "f32" } },
    { i: { k: "h", l: 2, c: 0 }, m: { typename: "Vector" } },
    { i: { k: "d", c: [0, 1] } },
    { i: { k: "n", c: { values: { r: 1, m: { doc: "Values" } }, tuple: 2 } } },
  ],
  1,
);

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

  it("resolves numbered and homogeneous paths", () => {
    expect(indexedSchema.node("/tuple").kind).toBe("numbered");
    expect(indexedSchema.node("/tuple/1/0").sem).toEqual({ ty: "f32" });
    expect(indexedSchema.node("/values/1").sem).toEqual({ ty: "f32" });
    expect(indexedSchema.node("/values")).toMatchObject({
      edge: { doc: "Values" },
      node: { typename: "Vector" },
    });
    expect(indexedSchema.children("/values").map((node) => node.path)).toEqual([
      "/values/0",
      "/values/1",
    ]);
  });

  it("formats schema metadata for selected rows", () => {
    const node = indexedSchema.node("/values/1");

    expect(schemaSummary(node)).toBe("leaf · f32");
  });

  it("renders unicode and multiline schema metadata literally", () => {
    const schema = new Schema(
      [
        { s: { ty: "f32", unit: "Hz²" } },
        {
          i: {
            k: "n",
            c: { leaf: { r: 0, m: { doc: "edge line 1\nedge line 2" } } },
          },
          m: { doc: "node line 1\nnode line 2", typename: "Root" },
        },
      ],
      1,
    );

    expect(schema.node("/leaf")).toMatchObject({
      sem: { ty: "f32", unit: "Hz²" },
      edge: { doc: "edge line 1\nedge line 2" },
    });
    expect(schema.node("").node).toEqual({
      doc: "node line 1\nnode line 2",
      typename: "Root",
    });
    expect(schemaSummary(schema.node("/leaf"))).toBe("leaf · f32");
    expect(schemaSummary(schema.node(""))).toBe("named");
  });
  it("interprets only recognized semantic fields and preserves future semantics", () => {
    const schema = new Schema(
      [
        {
          s: { ty: "future", oneof: true, maybe_absent: true, extra: 7 },
          m: { ty: "i32", oneof: false },
        },
      ],
      1,
    );
    expect(schemaSummary(schema.node(""))).toBe(
      "leaf · mutually exclusive children · may be absent",
    );
    expect(schema.node("").sem).toMatchObject({ ty: "future", extra: 7 });
    expect(schemaSummary(new Schema([{}], 1).node(""))).toBe("leaf");
  });
});
