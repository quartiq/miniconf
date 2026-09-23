import { expect, it } from "vitest";
import {
  BrowseModel,
  rememberRoute,
  type BrowseMemory,
} from "./browse-model.svelte";
import { Schema } from "./schema";

const schema = new Schema(
  [{ s: "value" }, { i: { k: "n", c: { leaf: 0 } } }],
  7,
);
function loaded(text = "1") {
  const model = new BrowseModel(
    () => undefined,
    () => {},
  );
  model.loadSchema(schema, "");
  update(model, text);
  model.select("/leaf");
  return model;
}
function update(model: BrowseModel, text?: string) {
  model.commitSettings({
    settings: new Map(text === undefined ? [] : [["/leaf", text]]),
    touched: new Set(["/leaf"]),
    activity: new Set(["/leaf"]),
    rev: "2",
  });
}

it("remembers 32 recent routes with independent folds", () => {
  const memory = new Map<string, BrowseMemory>();
  const model = loaded();
  for (let i = 0; i < 32; i++) rememberRoute(memory, String(i), model.state);
  rememberRoute(memory, "0", model.state);
  rememberRoute(memory, "32", model.state);
  expect(memory.size).toBe(32);
  expect(memory.has("0")).toBe(true);
  expect(memory.has("1")).toBe(false);
  model.setExpanded("", false);
  expect(memory.get("0")).toEqual({
    selectedPath: "/leaf",
    expanded: new Set([""]),
    userClosed: new Set(),
  });
});

it("reuses unchanged rows and preserves earlier snapshots", () => {
  const model = loaded();
  const before = model.state;
  const leaf = before.tree.get("/leaf")!;
  update(model, "1");
  expect(model.state.tree).toBe(before.tree);
  expect(model.state.expanded).toBe(before.expanded);
  expect([...model.activity.keys()].sort()).toEqual(["", "/leaf"]);
  expect(model.revision).toBe("2");
  update(model, "2");
  const changed = model.state;
  expect(before.tree.get("/leaf")?.value).toBe("1");
  expect(changed.tree.get("/leaf")).toEqual({ ...leaf, value: "2" });
  expect(changed.tree.get("")).toBe(before.tree.get(""));
  expect(changed.tree.get("/leaf")?.children).toBe(leaf.children);
  expect(changed.expanded).toBe(before.expanded);
  update(model);
  expect(model.state.tree.get("/leaf")?.value).toBeUndefined();
  expect(changed.tree.get("/leaf")?.value).toBe("2");
  model.loadSchema(new Schema([{ s: "new" }], 8), "");
  expect([...model.state.tree.keys()]).toEqual([""]);
  expect(model.state.tree.get("")?.sem).toBe("new");
});

it("preserves exact JSON through clean updates", () => {
  const text = '{"n":9007199254740993,"small":1.0000000000000001,"e":1e400}';
  const model = loaded(text);
  expect(model.editor).toBe(text);
  update(model, "-9007199254740993");
  expect(model.editor).toBe("-9007199254740993");
});

it("reselection, no-op navigation and folding preserve a draft", () => {
  const model = loaded();
  model.edit("99");
  model.select("/leaf");
  model.navigate("/leaf", "child");
  model.setExpanded("", false);
  expect(model.state.selectedPath).toBe("/leaf");
  expect(model.editor).toBe("99");
  model.select("");
  expect(model.editor).toBe("");
  model.select("/leaf");
  expect(model.editor).toBe("1");
});

it.each([true, false])(
  "matching updates do not release a draft (coalesced: %s)",
  (coalesced) => {
    const model = loaded();
    model.edit("3");
    if (!coalesced) update(model, "3");
    update(model, "4");
    expect(model.editor).toBe("3");
    model.revert();
    expect(model.editor).toBe("4");
  },
);

it("distinguishes JSON null, absence and empty drafts", () => {
  const model = loaded("null");
  expect(model.editor).toBe("null");
  update(model);
  expect(model.editor).toBe("");
  model.edit("null");
  update(model);
  expect(model.editor).toBe("null");
});

it.each(["", "99"])(
  "preserves draft %j through clear, schema replacement and replay",
  (draft) => {
    const model = loaded();
    model.edit(draft);
    model.loadSchema(schema, "");
    expect(model.editor).toBe(draft);
    model.reset(true);
    model.loadSchema(new Schema([{ i: { k: "n", c: {} } }], 8), "");
    expect(model.state.selectedPath).toBe("/leaf");
    expect(model.editor).toBe(draft);
    expect(model.selected).toBeUndefined();
    model.loadSchema(schema, "");
    update(model, "2");
    expect(model.editor).toBe(draft);
    model.revert();
    expect(model.editor).toBe("2");
    expect(model.state.draft).toBeUndefined();
  },
);
