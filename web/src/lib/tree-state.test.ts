import { describe, expect, it } from "vitest";
import { toggleExpansion } from "./tree-navigation";
import { cuePaths, flatTreeNodes, formatLeafValue, parentPath, revealPresentSettings, treeViewNodes } from "./tree-state";

describe("tree state", () => {
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
        ["/a/b/c", 1],
        ["/d/e", 2],
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

    const opened = toggleExpansion(closed.expanded, closed.userClosed, "/a", true);
    expect(opened.expanded.has("/a")).toBe(true);
    expect(opened.userClosed.has("/a")).toBe(false);
  });

  it("cues changed paths and ancestors", () => {
    expect([...cuePaths(["/a/b"], "")].sort()).toEqual(["", "/a", "/a/b"]);
  });

  it("formats present leaf values for inline display", () => {
    expect(formatLeafValue({
      path: "/a",
      kind: "leaf",
      children: [],
      present: true,
      value: { x: 1 },
    })).toBe('{"x":1}');
    expect(formatLeafValue({
      path: "/b",
      kind: "leaf",
      children: [],
      present: false,
    })).toBe("");
  });

  it("flattens schema rows for keyboard navigation", () => {
    expect([...flatTreeNodes([
      { path: "", kind: "named", children: [], present: false },
      { path: "/a", kind: "leaf", children: [], present: true, value: 1 },
    ]).values()]).toEqual([
      { path: "", children: ["/a"] },
      { path: "/a", children: [] },
    ]);
  });

  it("renders root as an empty fold row", () => {
    expect(treeViewNodes([
      { path: "", kind: "named", children: [], present: false },
    ], "").get("")?.label).toBe("");
  });
});
