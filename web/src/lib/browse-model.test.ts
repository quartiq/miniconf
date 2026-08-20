import { describe, expect, it } from "vitest";
import * as browse from "./browse-model";
import { Schema } from "./schema";

describe("browse model", () => {
  it("keeps browse tree, editor, and settings commits together", () => {
    let state = browse.emptyState();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    state = browse.loadSchema(state, schema, "");
    expect(state.root).toBe("");
    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 3]]),
      changed: new Set(["/leaf"]),
      activity: new Set(),
      rev: "42",
    }).state;

    expect(state.tree.nodeByPath.get(state.root)?.path).toBe("");
    expect(state.selectedPath).toBe("");

    state = browse.loadSelected(state, "/leaf");
    expect(state.editor).toBe("3");
    expect(browse.parseEditor(state)).toBe(3);
  });

  it("does not rewrite an open editor when settings updates arrive", () => {
    let state = browse.emptyState();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    state = browse.loadSchema(state, schema, "");
    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 1]]),
      changed: new Set(["/leaf"]),
      activity: new Set(),
    }).state;
    state = browse.loadSelected(state, "/leaf");
    state = browse.updateEditor(state, "123");

    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 2]]),
      changed: new Set(["/leaf"]),
      activity: new Set(["/leaf"]),
    }).state;

    expect(state.editor).toBe("123");
    expect(browse.selected(state)?.value).toBe(2);
    expect(state.editorDirty).toBe(true);
    expect(state.editorStale).toBe(true);

    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 3]]),
      changed: new Set(["/leaf"]),
      activity: new Set(["/leaf"]),
    }).state;

    expect(state.editor).toBe("123");
    expect(browse.selected(state)?.value).toBe(3);

    state = browse.loadEditor(state);
    expect(state.editor).toBe("3");
    expect(state.editorDirty).toBe(false);
    expect(state.editorStale).toBe(false);
  });

  it("refreshes untouched editors and accepts equivalent authoritative echoes", () => {
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);
    let state = browse.loadSchema(browse.emptyState(), schema, "");
    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", { a: 1, b: 2 }]]),
      changed: new Set(["/leaf"]),
      activity: new Set(),
    }).state;
    state = browse.loadSelected(state, "/leaf");

    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", { a: 2 }]]),
      changed: new Set(["/leaf"]),
      activity: new Set(["/leaf"]),
    }).state;
    expect(state.editor).toBe('{\n  "a": 2\n}');

    state = browse.updateEditor(state, '{"b":2,"a":3}');
    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", { a: 3, b: 2 }]]),
      changed: new Set(["/leaf"]),
      activity: new Set(["/leaf"]),
    }).state;
    expect(state.editorDirty).toBe(false);
    expect(state.editorStale).toBe(false);
  });

  it("loads editor text only when selection is explicitly loaded", () => {
    let state = browse.emptyState();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    state = browse.loadSchema(state, schema, "");
    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 4]]),
      changed: new Set(["/leaf"]),
      activity: new Set(),
    }).state;

    state = browse.select(state, "/leaf");
    expect(state.selectedPath).toBe("/leaf");
    expect(state.editor).toBe("null");

    state = browse.loadSelected(state, "/leaf");
    expect(state.editor).toBe("4");
  });

  it("restores only fold and selection paths present in the next schema", () => {
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { keep: 0 } } },
      { i: { k: "n", c: { group: 1, leaf: 0 } } },
    ], 1);
    let state = browse.loadSchema(browse.emptyState(), schema, "");
    state = browse.setExpanded(state, "", true);
    state = browse.setExpanded(state, "/group", true);
    state = browse.loadSelected(state, "/group/keep");

    const restored = browse.loadSchema(browse.emptyState(), schema, "", state);
    expect(restored.expanded).toEqual(new Set(["", "/group"]));
    expect(restored.selectedPath).toBe("/group/keep");

    const closed = browse.setExpanded(restored, "/group", false);
    let reloaded = browse.loadSchema(browse.emptyState(), schema, "", closed);
    reloaded = browse.commitSettings(reloaded, {
      settings: new Map([["/group/keep", 1]]),
      changed: new Set(["/group/keep"]),
      activity: new Set(),
    }).state;
    expect(reloaded.userClosed).toContain("/group");
    expect(reloaded.expanded).not.toContain("/group");

    const changed = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } } },
    ], 2);
    const pruned = browse.loadSchema(browse.emptyState(), changed, "", state);
    expect(pruned.expanded).toEqual(new Set([""]));
    expect(pruned.selectedPath).toBe("");
  });
});
