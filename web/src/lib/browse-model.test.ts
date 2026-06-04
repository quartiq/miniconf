import { describe, expect, it } from "vitest";
import { BrowseModel } from "./browse-model";
import { Schema } from "./schema";

describe("BrowseModel", () => {
  it("keeps browse tree, editor, and settings commits together", () => {
    const model = new BrowseModel();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    expect(model.loadSchema(schema, "")).toBe("");
    const commit = model.commit({
      settings: new Map([["/leaf", 3]]),
      changed: new Set(["/leaf"]),
      rev: "42",
    });

    expect(commit.status).toBe("Updated /leaf");
    expect(commit.rev).toBe("42");
    expect(model.rootNode?.path).toBe("");
    expect(model.selectedPath).toBe("");

    model.loadSelected("/leaf");
    expect(model.editor).toBe("3");
    expect(model.parseEditor()).toBe(3);
  });

  it("does not rewrite an open editor when settings updates arrive", () => {
    const model = new BrowseModel();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    model.loadSchema(schema, "");
    model.commit({
      settings: new Map([["/leaf", 1]]),
      changed: new Set(["/leaf"]),
    });
    model.loadSelected("/leaf");
    model.updateEditor("123");

    model.commit({
      settings: new Map([["/leaf", 2]]),
      changed: new Set(["/leaf"]),
    });

    expect(model.editor).toBe("123");
    expect(model.selected?.value).toBe(2);

    model.commit({
      settings: new Map([["/leaf", 3]]),
      changed: new Set(["/leaf"]),
    });

    expect(model.editor).toBe("123");
    expect(model.selected?.value).toBe(3);

    model.loadEditor();
    expect(model.editor).toBe("3");
  });

  it("loads editor text only when selection is explicitly loaded", () => {
    const model = new BrowseModel();
    const schema = new Schema([
      { s: "value" },
      { i: { k: "n", c: { leaf: 0 } }, m: { typename: "App" } },
    ], 7);

    model.loadSchema(schema, "");
    model.commit({
      settings: new Map([["/leaf", 4]]),
      changed: new Set(["/leaf"]),
    });

    model.select("/leaf");
    expect(model.selectedPath).toBe("/leaf");
    expect(model.editor).toBe("null");

    model.loadSelected("/leaf");
    expect(model.editor).toBe("4");
  });
});
