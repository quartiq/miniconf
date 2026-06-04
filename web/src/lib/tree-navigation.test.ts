import { describe, expect, it } from "vitest";
import { discoveryTree, flatDiscoveryNodes } from "./discovery-tree";
import { movePath, visibleTreePaths } from "./tree-navigation";

describe("tree navigation", () => {
  it("walks visible rows in display order", () => {
    const nodes = new Map([
      ["", { path: "", children: ["/a", "/b"] }],
      ["/a", { path: "/a", children: ["/a/x"] }],
      ["/a/x", { path: "/a/x", children: [] }],
      ["/b", { path: "/b", children: [] }],
    ]);

    const visible = visibleTreePaths("", nodes, new Set(["", "/a"]));
    expect(visible).toEqual(["", "/a", "/a/x", "/b"]);
    expect(movePath(visible, "/a", "next")).toBe("/a/x");
    expect(movePath(visible, "/a", "previous")).toBe("");
    expect(movePath(visible, "/a", "first")).toBe("");
    expect(movePath(visible, "/a", "last")).toBe("/b");
    expect(movePath(visible, "/a", "child")).toBe("/a/x");
    expect(movePath(visible, "/a/x", "parent")).toBe("/a");
    expect(movePath(visible, "", "parent")).toBe("");
    expect(movePath(["/a", "/a/x"], "/a", "parent")).toBe("/a");
    expect(movePath(visible, "", "pageNext", 2)).toBe("/a/x");
    expect(movePath(visible, "/b", "pagePrevious", 2)).toBe("/a");
    expect(movePath(visible, "/a", "pagePrevious", 20)).toBe("");
    expect(movePath(visible, "/a", "pageNext", 20)).toBe("/b");
  });

  it("builds a browsable discovery tree from prefixes", () => {
    const nodes = discoveryTree([
      { prefix: "dt/sinara/a/host" },
      { prefix: "dt/sinara/b/host" },
    ]);

    expect(visibleTreePaths("", flatDiscoveryNodes(nodes), new Set(["", "dt", "dt/sinara"]))).toEqual([
      "",
      "dt",
      "dt/sinara",
      "dt/sinara/a",
      "dt/sinara/b",
    ]);
    expect(nodes.get("dt/sinara/a/host")?.prefix).toBe("dt/sinara/a/host");
  });
});
