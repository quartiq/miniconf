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
  it("preserves edits until reverted or matched by the device", () => {
    let state = browse.updateEditor(loaded(), "99");
    state = update(state, "1");
    expect(browse.editor(state)).toBe("99");
    state = update(state, "2");
    expect(browse.editor(state)).toBe("99");
    state = browse.loadEditor(state);
    expect(browse.editor(state)).toBe("2");
    state = browse.updateEditor(state, "3");
    state = update(state, "3");
    state = update(state, "4");
    expect(browse.editor(state)).toBe("4");
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
