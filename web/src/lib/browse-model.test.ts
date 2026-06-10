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
    }).state;
    state = browse.loadSelected(state, "/leaf");
    state = browse.updateEditor(state, "123");

    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 2]]),
      changed: new Set(["/leaf"]),
    }).state;

    expect(state.editor).toBe("123");
    expect(browse.selected(state)?.value).toBe(2);

    state = browse.commitSettings(state, {
      settings: new Map([["/leaf", 3]]),
      changed: new Set(["/leaf"]),
    }).state;

    expect(state.editor).toBe("123");
    expect(browse.selected(state)?.value).toBe(3);

    state = browse.loadEditor(state);
    expect(state.editor).toBe("3");
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
    }).state;

    state = browse.select(state, "/leaf");
    expect(state.selectedPath).toBe("/leaf");
    expect(state.editor).toBe("null");

    state = browse.loadSelected(state, "/leaf");
    expect(state.editor).toBe("4");
  });
});
