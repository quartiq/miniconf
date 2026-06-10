import { describe, expect, it } from "vitest";
import { discoveryTree, flatDiscoveryNodes } from "./discovery-tree";
import { movePath, visibleTreePaths } from "./tree-navigation";

describe("tree navigation", () => {
  it("walks visible rows in display order", () => {
    const nodes = new Map([
      ["", { path: "", children: ["/a", "/b"] }],
      ["/a", { path: "/a", parent: "", children: ["/a/x"] }],
      ["/a/x", { path: "/a/x", parent: "/a", children: [] }],
      ["/b", { path: "/b", parent: "", children: [] }],
    ]);

    const visible = visibleTreePaths("", nodes, new Set(["", "/a"]));
    expect(visible).toEqual(["", "/a", "/a/x", "/b"]);
    expect(movePath(visible, "/a", "next", nodes)).toBe("/a/x");
    expect(movePath(visible, "/a", "previous", nodes)).toBe("");
    expect(movePath(visible, "/a", "first", nodes)).toBe("");
    expect(movePath(visible, "/a", "last", nodes)).toBe("/b");
    expect(movePath(visible, "/a", "child", nodes)).toBe("/a/x");
    expect(movePath(visible, "/a/x", "parent", nodes)).toBe("/a");
    expect(movePath(visible, "", "parent", nodes)).toBe("");
    expect(movePath(["/a", "/a/x"], "/a", "parent", nodes)).toBe("/a");
    expect(movePath(visible, "", "pageNext", nodes, 2)).toBe("/a/x");
    expect(movePath(visible, "/b", "pagePrevious", nodes, 2)).toBe("/a");
    expect(movePath(visible, "/a", "pagePrevious", nodes, 20)).toBe("");
    expect(movePath(visible, "/a", "pageNext", nodes, 20)).toBe("/b");
  });

  it("navigates by explicit structure rather than path syntax", () => {
    const nodes = new Map([
      ["root", { path: "root", children: ["left", "right"] }],
      ["left", { path: "left", parent: "root", children: ["leaf"] }],
      ["leaf", { path: "leaf", parent: "left", children: [] }],
      ["right", { path: "right", parent: "root", children: [] }],
    ]);
    const visible = visibleTreePaths("root", nodes, new Set(["root", "left"]));

    expect(visible).toEqual(["root", "left", "leaf", "right"]);
    expect(movePath(visible, "left", "child", nodes)).toBe("leaf");
    expect(movePath(visible, "leaf", "parent", nodes)).toBe("left");
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
