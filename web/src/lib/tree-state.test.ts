import { describe, expect, it } from "vitest";
import { toggleExpansion } from "./tree-navigation";
import { Schema } from "./schema";
import {
  cuePaths,
  parentPath,
  revealPresentSettings,
  treeSnapshot,
  updateActivity,
} from "./tree-state";

describe("tree state", () => {
  it("expires activity and drops obsolete paths without losing unrelated live cues", () => {
    const tree = treeSnapshot(
      new Schema([{}, { i: { k: "n", c: { a: 0, b: 0 } } }], 1),
      "",
      new Map(),
    );
    const previous = new Map([
      ["", { at: 0 }],
      ["/a", { at: 1 }],
      ["/gone", { at: 999 }],
    ]);
    const kept = updateActivity(previous, [], tree, 1000);
    expect([...kept.keys()]).toEqual(["/a"]);
    expect(kept.get("/a")).toBe(previous.get("/a"));
    const next = updateActivity(kept, ["", "/b"], tree, 1000);
    expect([...next.keys()]).toEqual(["/a", "", "/b"]);
    expect(next.get("")).toBe(next.get("/b"));
    expect(updateActivity(next, [], tree, 2000).size).toBe(0);
    expect(previous.size).toBe(3);
    expect(kept.size).toBe(1);
  });

  it("shows portable semantics inline and preserves opaque metadata in tooltips", () => {
    const schema = new Schema(
      [
        {
          s: { ty: "f32", future: ["opaque"] },
          m: {
            note: "first line\nsecond line",
            flag: false,
            empty: null,
            "": 0,
          },
        },
        { i: { k: "n", c: { amplitude: { r: 0, m: { note: "edge note" } } } } },
      ],
      1,
    );
    const row = treeSnapshot(schema, "", new Map([["/amplitude", "1.0"]])).get(
      "/amplitude",
    )!;
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
    const before = new Set([""]);
    const expanded = revealPresentSettings(
      before,
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
    expect(before).toEqual(new Set([""]));
    expect(
      revealPresentSettings(
        expanded,
        userClosed,
        ["/a/b/c", "/d/e"],
        new Map([
          ["/a/b/c", "3"],
          ["/d/e", "4"],
        ]),
        "",
      ),
    ).toBe(expanded);
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
    expect([...tree.values()]).toMatchObject([
      { path: "", children: ["/a"] },
      { path: "/a", parent: "", children: [] },
    ]);
  });

  it("distinguishes the root from an empty-name child", () => {
    const schema = new Schema([{ s: {} }, { i: { k: "n", c: { "": 0 } } }], 1);
    const views = treeSnapshot(schema, "", new Map());
    expect(views.get("")?.label).toBe("(root)");
    expect(views.get("")?.children).toEqual(["/"]);
    expect(views.get("/")?.label).toBe('\"\"');
  });

  it("preserves empty Miniconf names and exact subtree boundaries", () => {
    const schema = new Schema(
      [
        { s: { ty: "str" } },
        { i: { k: "n", c: { "": 0, child: 0 } } },
        { i: { k: "n", c: { "": 1, sibling: 0 } } },
      ],
      1,
    );
    const tree = treeSnapshot(
      schema,
      "/",
      new Map([
        ["//", '""'],
        ["//child", "null"],
      ]),
    );
    expect([...tree.keys()]).toEqual(["/", "//", "//child"]);
    expect(tree.get("/")?.children).toEqual(["//", "//child"]);
    expect(tree.get("//")?.value).toBe('\"\"');
    expect(tree.get("//child")?.value).toBe("null");
    expect(parentPath("//")).toBe("/");
    expect(parentPath("//child")).toBe("/");
    expect(
      treeSnapshot(schema, "/", new Map()).get("//")?.value,
    ).toBeUndefined();
  });
});
