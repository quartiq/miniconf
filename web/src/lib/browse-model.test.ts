import { describe, expect, it } from "vitest";
import * as browse from "./browse-model";
import { Schema } from "./schema";

const schema = new Schema(
  [{ s: "value" }, { i: { k: "n", c: { leaf: 0 } } }],
  7,
);
function loaded(text = "1") {
  let state = browse.loadSchema(browse.emptyState(), schema, "");
  state = update(state, text);
  return browse.loadSelected(state, "/leaf");
}
function update(state: browse.BrowseState, text?: string) {
  return browse.commitSettings(state, {
    settings: new Map(text === undefined ? [] : [["/leaf", text]]),
    touched: new Set(["/leaf"]),
    activity: new Set(["/leaf"]),
  }).state;
}

describe("leaf editor ownership", () => {
  it("remembers only the 32 most recently saved routes with independent folds", () => {
    const memory = new Map<string, browse.BrowseMemory>();
    const state = loaded();
    for (let i = 0; i < 32; i++) browse.rememberRoute(memory, String(i), state);
    browse.rememberRoute(memory, "0", state);
    browse.rememberRoute(memory, "32", state);
    expect(memory.size).toBe(32);
    expect(memory.has("0")).toBe(true);
    expect(memory.has("1")).toBe(false);
    expect(memory.get("0")?.selectedPath).toBe("/leaf");
    state.expanded.clear();
    state.userClosed.add("");
    expect(memory.get("0")?.expanded).toEqual(new Set([""]));
    expect(memory.get("0")?.userClosed.size).toBe(0);
  });

  it("reuses schema rows while preserving snapshots and repeated-observation cues", () => {
    const state = loaded();
    const leaf = state.tree.get("/leaf")!;
    const repeated = browse.commitSettings(state, {
      settings: new Map(state.settings),
      touched: new Set(["/leaf"]),
      activity: new Set(["/leaf"]),
      rev: "2",
    });
    expect(repeated.state.tree).toBe(state.tree);
    expect(repeated.state.expanded).toBe(state.expanded);
    expect(repeated.cues).toEqual(new Set(["/leaf", ""]));
    expect(repeated.rev).toBe("2");
    const changed = update(state, "2");
    expect(state.tree.get("/leaf")?.value).toBe("1");
    expect(changed.tree.get("/leaf")).toEqual({ ...leaf, value: "2" });
    expect(changed.tree.get("")).toBe(state.tree.get(""));
    expect(changed.tree.get("/leaf")?.children).toBe(leaf.children);
    expect(changed.expanded).toBe(state.expanded);
    const cleared = update(changed);
    expect(cleared.tree.get("/leaf")?.value).toBeUndefined();
    expect(changed.tree.get("/leaf")?.value).toBe("2");
    const replaced = browse.loadSchema(
      changed,
      new Schema([{ s: "new" }], 8),
      "",
    );
    expect([...replaced.tree.keys()]).toEqual([""]);
    expect(replaced.tree.get("")?.sem).toBe("new");
  });

  it("preserves exact JSON through loading and clean updates", () => {
    const text = '{"n":9007199254740993,"small":1.0000000000000001,"e":1e400}';
    let state = loaded(text);
    expect(browse.editor(state)).toBe(text);
    state = update(state, "-9007199254740993");
    expect(browse.editor(state)).toBe("-9007199254740993");
  });
  it("preserves a draft on reselection, no-op navigation and schema rebuild", () => {
    const state = browse.updateEditor(loaded(), "99");
    expect(browse.editor(browse.loadSelected(state, "/leaf"))).toBe("99");
    expect(browse.editor(browse.navigate(state, "/leaf", "child").state)).toBe(
      "99",
    );
    expect(browse.editor(browse.loadSchema(state, schema, ""))).toBe("99");
    const missing = browse.loadSchema(
      state,
      new Schema([{ i: { k: "n", c: {} } }], 8),
      "",
    );
    expect(missing.selectedPath).toBe("/leaf");
    expect(browse.editor(missing)).toBe("99");
    expect(browse.selected(missing)).toBeUndefined();
  });
  it("preserves edits until reverted, including across matching device updates", () => {
    let state = browse.updateEditor(loaded(), "99");
    state = update(state, "1");
    expect(browse.editor(state)).toBe("99");
    state = update(state, "2");
    expect(browse.editor(state)).toBe("99");
    state = browse.loadEditor(state);
    expect(browse.editor(state)).toBe("2");
    const draft = browse.updateEditor(state, "3");
    // Separate UI batches and a coalesced burst have the same ownership result.
    for (const next of [update(update(draft, "3"), "4"), update(draft, "4")]) {
      expect(browse.editor(next)).toBe("3");
      expect(browse.editor(browse.loadEditor(next))).toBe("4");
    }
  });
  it("distinguishes JSON null, absent values and empty drafts", () => {
    let state = loaded("null");
    expect(browse.editor(state)).toBe("null");
    state = update(state);
    expect(browse.editor(state)).toBe("");
    state = browse.updateEditor(state, "null");
    state = update(state);
    expect(browse.editor(state)).toBe("null");
  });
  it("keeps the draft when folded and replaces it on deliberate selection", () => {
    let state = browse.updateEditor(loaded(), "99");
    state = browse.setExpanded(state, "", false);
    expect(state.selectedPath).toBe("/leaf");
    expect(browse.editor(state)).toBe("99");
    state = browse.loadSelected(state, "");
    expect(browse.editor(state)).toBe("");
    expect(browse.editor(browse.loadSelected(state, "/leaf"))).toBe("1");
  });

  it.each(["", "99"])(
    "preserves draft %j through clear, schema change and replay",
    (draft) => {
      let state = browse.updateEditor(loaded(), draft);
      state = update(state);
      state = browse.loadSchema(
        state,
        new Schema([{ i: { k: "n", c: {} } }], 8),
        "",
      );
      expect(state.selectedPath).toBe("/leaf");
      expect(browse.editor(state)).toBe(draft);
      expect(state.draft).toBe(draft);
      state = browse.loadSchema(state, schema, "");
      state = update(state, "2");
      expect(browse.editor(state)).toBe(draft);
      state = browse.loadEditor(state);
      expect(browse.editor(state)).toBe("2");
      expect(state.draft).toBeUndefined();
    },
  );
});
