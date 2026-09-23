import { expect, it, vi } from "vitest";
import type { SetResponse } from "./backend";
import { Schema } from "./schema";
import { BrowseModel } from "./browse-model.svelte";

function fixture() {
  const lifetime = new AbortController();
  let respond!: (response: SetResponse) => void;
  const session = {
    set: vi.fn(
      () =>
        new Promise<SetResponse>((resolve) => {
          respond = resolve;
        }),
    ),
    prune: vi.fn(async () => ({ cleared: 3 })),
  };
  const log = vi.fn();
  const model = new BrowseModel(
    () => ({ session, signal: lifetime.signal }),
    log,
  );
  model.loadSchema(
    new Schema([{ s: "value" }, { i: { k: "n", c: { leaf: 0 } } }], 7),
    "",
  );
  model.commitSettings({
    settings: new Map([["/leaf", "1"]]),
    touched: new Set(["/leaf"]),
    activity: new Set(),
  });
  model.select("/leaf");
  model.edit("2");
  return {
    model,
    session,
    lifetime,
    log,
    reply: (ok = true) =>
      respond({
        path: "/leaf",
        ok,
        code: ok ? "Ok" : "Error",
        message: ok ? "" : "denied",
        responseMs: 5,
      }),
  };
}

it("validates locally and reports errors only for the matching draft", async () => {
  const { model, session } = fixture();
  model.edit("{");
  await model.submit();
  expect(session.set).not.toHaveBeenCalled();
  expect(model.editorError).toContain("Invalid JSON");
  model.edit("2");
  expect(model.editorError).toBe("");
});

it("returns an acknowledged draft to the observed value without inventing an echo", async () => {
  const { model, session, reply } = fixture();
  const pending = model.submit();
  expect(session.set).toHaveBeenCalledWith("/leaf", "2");
  expect(model.canSet).toBe(false);
  reply();
  await pending;
  expect(model.editor).toBe("1");
  expect(model.dirty).toBe(false);
  expect(model.result?.text).toBe("Set succeeded");
});

it("keeps edits made while a Set is pending", async () => {
  const { model, reply } = fixture();
  const pending = model.submit();
  model.edit("3");
  reply();
  await pending;
  expect(model.editor).toBe("3");
  expect(model.dirty).toBe(true);
});

it("ignores completion from an abandoned connection", async () => {
  const { model, lifetime, reply, log } = fixture();
  const pending = model.submit();
  lifetime.abort();
  model.reset(false);
  reply();
  await pending;
  expect(model.result).toBeUndefined();
  expect(model.pending.size).toBe(0);
  expect(log).not.toHaveBeenCalled();
});

it("preserves a Set failure when independent pruning succeeds", async () => {
  const { model, session, reply } = fixture();
  let finish!: () => void;
  session.prune.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = () => resolve({ cleared: 3 });
      }),
  );
  const pruning = model.prune();
  const pending = model.submit();
  reply(false);
  await pending;
  finish();
  await pruning;
  expect(model.result).toMatchObject({
    text: "Set failed: denied",
    failed: true,
  });
  expect(model.editor).toBe("2");
  expect(model.pending.size).toBe(0);
});

it("an explicit new action replaces an earlier failure", async () => {
  const { model, reply } = fixture();
  const pending = model.submit();
  reply(false);
  await pending;
  await model.prune();
  expect(model.result).toMatchObject({ text: "Cleared 3", failed: false });
});
