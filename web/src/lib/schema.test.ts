import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  describeSemantics,
  schemaSummary,
  Schema,
  type CompactDef,
} from "./schema";

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
  it("resolves every path walked in the shared Rust export", () => {
    const defs = readFileSync(
      resolve("../miniconf/tests/common.ndjson"),
      "utf8",
    )
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => JSON.parse(line) as CompactDef);
    const schema = new Schema(defs, 1);
    const nodes = schema.walk();
    expect(nodes.length).toBeGreaterThan(1);
    for (const node of nodes) expect(schema.node(node.path)).toEqual(node);
  });

  it("walks shared definitions in preorder with path-specific edge metadata", () => {
    const schema = new Schema(
      [
        { s: { ty: "f32" }, m: { doc: "shared" } },
        { i: { k: "h", l: 2, c: { r: 0, m: "element" } } },
        { i: { k: "d", c: [0, { r: 1, m: "vector" }] } },
        {
          i: {
            k: "n",
            c: { "": { r: 2, m: "empty" }, other: { r: 2, m: "other" } },
          },
        },
      ],
      1,
    );
    const paths = [
      "",
      "/",
      "//0",
      "//1",
      "//1/0",
      "//1/1",
      "/other",
      "/other/0",
      "/other/1",
      "/other/1/0",
      "/other/1/1",
    ];
    expect(schema.walk()).toEqual(paths.map((path) => schema.node(path)));
    expect(schema.walk("/other")).toEqual(
      paths.slice(6).map((path) => schema.node(path)),
    );
    expect(schema.walk("//0")).toEqual([schema.node("//0")]);
    expect(new Schema([{}], 1).walk()).toEqual([new Schema([{}], 1).node()]);
  });

  it.each([
    [null],
    [{ i: { k: "n", c: { bad: 9 } } }],
    [{ i: { k: "d", c: [0] } }],
    [{}, { i: { k: "d", c: [0.5] } }],
    [{}, { i: { k: "h", c: 0, l: -1 } }],
    [{}, { i: { k: "n", c: null } }],
    [{}, { i: { k: "unknown" } }],
  ])(
    "rejects malformed compact definitions before traversal: %j",
    (...defs) => {
      expect(() => new Schema(defs as CompactDef[], 1)).toThrow();
    },
  );

  it("preserves literal named segments and canonical array indices", () => {
    const names = JSON.parse('{"":0,"__proto__":0,"constructor":0,"01":0}');
    const schema = new Schema([{}, { i: { k: "n", c: names } }], 1);
    for (const name of Object.keys(names))
      expect(schema.node(`/${name}`).kind).toBe("leaf");
    expect(() => schema.node("/toString")).toThrow();
    for (const path of [
      "values/0",
      "/values/",
      "/values/00",
      "/values/+0",
      "/values/-0",
      "/values/1.0",
      "/values/1e0",
      "/values/ 1",
      "/values/-1",
      "/values/2",
      "/tuple/2",
      "/tuple/01",
    ]) {
      expect(() => indexedSchema.node(path), path).toThrow();
    }
  });

  it("reads homogeneous definitions linearly when walking and directly when resolving", () => {
    let reads = 0;
    let refs = 0;
    const schema = new Schema(
      [
        {},
        {
          get i() {
            reads++;
            return {
              k: "h" as const,
              l: 512,
              get c() {
                refs++;
                return 0;
              },
            };
          },
        },
      ],
      1,
    );
    expect(schema.walk()).toHaveLength(513);
    expect(reads).toBeLessThan(10);
    reads = 0;
    refs = 0;
    expect(schema.node("/511").kind).toBe("leaf");
    expect(reads).toBe(1);
    expect(refs).toBe(1);
  });

  it("uses the empty path for the root", () => {
    expect(indexedSchema.path("")).toBe("");
    expect(() => indexedSchema.path("/")).toThrow();
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
    expect(schemaSummary(indexedSchema.node("/values"))).toBe(
      "homogeneous · len=2",
    );
    expect(schemaSummary(indexedSchema.node("/tuple"))).toBe(
      "numbered · len=2",
    );
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
    expect(describeSemantics(schema.node("").sem)).toEqual({
      summary: ["mutually exclusive children", "may be absent"],
      extra: { ty: "future", extra: 7 },
    });
    expect(
      describeSemantics({ ty: "u16", oneof: false, maybe_absent: false }),
    ).toEqual({
      summary: ["u16"],
      extra: undefined,
    });
    expect(describeSemantics({ oneof: "future" }).extra).toEqual({
      oneof: "future",
    });
    expect(describeSemantics(null).extra).toBeNull();
    expect(schemaSummary(new Schema([{}], 1).node(""))).toBe("leaf");
  });
});
