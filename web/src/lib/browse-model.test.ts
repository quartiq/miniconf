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
    expect(state.editor).toBe(text);
    state = update(state, "-9007199254740993");
    expect(state.editor).toBe("-9007199254740993");
  });
  it("preserves a draft on reselection, no-op navigation and schema rebuild", () => {
    const state = browse.updateEditor(loaded(), "99");
    expect(browse.loadSelected(state, "/leaf").editor).toBe("99");
    expect(browse.navigate(state, "/leaf", "child").state.editor).toBe("99");
    expect(browse.loadSchema(state, schema, "").editor).toBe("99");
    const missing = browse.loadSchema(
      state,
      new Schema([{ i: { k: "n", c: {} } }], 8),
      "",
    );
    expect(missing.selectedPath).toBe("/leaf");
    expect(missing.editor).toBe("99");
    expect(browse.selected(missing)).toBeUndefined();
  });
  it("keeps text and baseline on remote changes and duplicate publications", () => {
    let state = browse.updateEditor(loaded(), "99");
    state = update(state, "1");
    expect(state.editorBaseline).toBe(browse.selected(state)?.value);
    state = update(state, "2");
    expect(state.editor).toBe("99");
    expect(state.editorBaseline).toBe("1");
    state = browse.loadEditor(state);
    expect(state.editor).toBe("2");
    expect(state.editorBaseline).toBe("2");
  });
  it("distinguishes JSON null, absent values and empty drafts", () => {
    let state = loaded("null");
    expect(state.editor).toBe("null");
    state = update(state);
    expect(state.editor).toBe("");
    expect(state.editorBaseline).toBeUndefined();
    state = browse.updateEditor(state, "null");
    state = update(state);
    expect(state.editor).toBe("null");
    expect(state.editorBaseline).toBeUndefined();
  });
  it("keeps the draft when folded and replaces it on deliberate selection", () => {
    let state = browse.updateEditor(loaded(), "99");
    state = browse.setExpanded(state, "", false);
    expect(state.selectedPath).toBe("/leaf");
    expect(state.editor).toBe("99");
    state = browse.loadSelected(state, "");
    expect(state.editor).toBe("");
    expect(browse.loadSelected(state, "/leaf").editor).toBe("1");
  });
});
