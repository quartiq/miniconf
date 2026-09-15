import { describe, expect, it } from "vitest";
import { toggleExpansion } from "./tree-navigation";
import { Schema } from "./schema";
import {
  cuePaths,
  parentPath,
  revealPresentSettings,
  treeViewNodes,
  treeSnapshot,
} from "./tree-state";

describe("tree state", () => {
  it("shows portable semantics inline and preserves opaque metadata in tooltips", () => {
    const row = treeViewNodes([
      {
        path: "/amplitude",
        kind: "leaf",
        children: [],
        present: true,
        value: "1.0",
        sem: { ty: "f32", future: ["opaque"] },
        edge: { note: "edge note" },
        node: {
          note: "first line\nsecond line",
          flag: false,
          empty: null,
          "": 0,
        },
      },
    ]).get("/amplitude")!;
    expect(row.summary).toBe("f32");
    expect(row.value).toBe("1.0");
    expect(row.title).toContain('Semantics:\nty: f32\nfuture: ["opaque"]');
    expect(row.title).toContain("Edge metadata:\nnote: edge note");
    expect(row.title).toContain(
      'Node metadata:\nnote: first line\nsecond line\nflag: false\nempty: null\n"": 0',
    );
  });
  it("derives parent paths", () => {
    expect(parentPath("")).toBeUndefined();
    expect(parentPath("/a")).toBe("");
    expect(parentPath("/a/b")).toBe("/a");
  });

  it("reveals present setting ancestors without reopening user-closed branches", () => {
    const userClosed = new Set(["/a"]);
    const expanded = revealPresentSettings(
      new Set([""]),
      userClosed,
      ["/a/b/c", "/d/e"],
      new Map([
        ["/a/b/c", "1"],
        ["/d/e", "2"],
      ]),
      "",
    );

    expect(expanded.has("/a")).toBe(false);
    expect(expanded.has("/a/b")).toBe(false);
    expect(expanded.has("/d")).toBe(true);
    expect(expanded.has("")).toBe(true);
  });

  it("tracks user toggles separately from expansion", () => {
    const closed = toggleExpansion(new Set(["/a"]), new Set(), "/a", false);
    expect(closed.expanded.has("/a")).toBe(false);
    expect(closed.userClosed.has("/a")).toBe(true);

    const opened = toggleExpansion(
      closed.expanded,
      closed.userClosed,
      "/a",
      true,
    );
    expect(opened.expanded.has("/a")).toBe(true);
    expect(opened.userClosed.has("/a")).toBe(false);
  });

  it("cues touched paths and ancestors", () => {
    expect([...cuePaths(["/a/b"], "")].sort()).toEqual(["", "/a", "/a/b"]);
  });

  it("includes keyboard navigation structure in displayed rows", () => {
    const schema = new Schema([{ s: {} }, { i: { k: "n", c: { a: 0 } } }], 1);
    const tree = treeSnapshot(schema, "", new Map([["/a", "1"]]));
    expect([...tree.nodeViews.values()]).toMatchObject([
      { path: "", children: ["/a"] },
      { path: "/a", parent: "", children: [] },
    ]);
  });

  it("distinguishes the root from an empty-name child", () => {
    const schema = new Schema([{ s: {} }, { i: { k: "n", c: { "": 0 } } }], 1);
    const { nodeViews: views } = treeSnapshot(schema, "", new Map());
    expect(views.get("")?.label).toBe("(root)");
    expect(views.get("")?.children).toEqual(["/"]);
    expect(views.get("/")?.label).toBe('\"\"');
  });
});
