import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("schema presentation and coalesced settings logs", async ({
  browsePage: page,
  device,
}) => {
  const leaf = page.locator('[data-tree-path="/leaf"]');
  await expect(leaf.locator(".summary")).toHaveText("(i32)");
  await expect(leaf).toHaveAttribute(
    "title",
    /Edge metadata:[\s\S]*Node metadata:/,
  );
  await expect(leaf).toHaveAttribute("title", /Signed digital mixer step/);
  await expect(leaf).not.toHaveAttribute("title", /Arrows\//);
  await expect(leaf).toContainText(" = ");
  await expect(page.locator('[data-tree-path="/obsolete"]')).toHaveCount(0);
  await expect(page.locator(".identity-details")).toHaveText("2 settings");
  await page.locator(".log summary").click();
  await expect(page.locator(".log pre")).toHaveCount(0);
  expect(
    await page
      .locator(".log-body")
      .evaluate((node) =>
        Math.abs(
          node.clientHeight -
            10 * parseFloat(getComputedStyle(node).lineHeight),
        ),
      ),
  ).toBeLessThan(1);
  device.publish(device.prefix + "/settings/leaf", "9007199254740993");
  device.publish(device.prefix + "/settings/leaf", "9007199254740993");
  await expect(page.locator(".log pre")).toContainText(
    "settings: 0 changed · 1 observed",
  );
  device.publish(device.prefix + "/settings/leaf", "20");
  device.publish(device.prefix + "/settings/leaf", "20.0");
  await expect(page.locator(".log pre")).toContainText(
    "settings: 1 changed · 1 observed",
  );
  await expect(page.locator("textarea")).toHaveValue("20.0");
});

test("folding, keyboard navigation, and history preserve editor ownership", async ({
  browsePage: page,
  device,
}) => {
  const editor = page.locator("textarea");
  const root = page.locator('[data-tree-path=""]');
  const leaf = page.locator('[data-tree-path="/leaf"]');
  const other = page.locator('[data-tree-path="/other"]');
  const tree = page.locator(".tree");
  await editor.fill("-9007199254740993");
  await leaf.click();
  await expect(editor).toHaveValue("-9007199254740993");
  await root.locator("button").click();
  await expect(root).toHaveAttribute("tabindex", "0");
  await editor.press("Escape");
  await expect(root).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(leaf).toBeVisible();
  await expect(editor).toHaveValue("-9007199254740993");
  const historyLength = await page.evaluate(() => history.length);
  await tree.evaluate((node) => {
    node.style.height = "60px";
  });
  await root.focus();
  await page.keyboard.press("End");
  await expect(other).toBeFocused();
  await expect
    .poll(() => tree.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(0);
  await tree.evaluate((node) => {
    node.scrollTop = 0;
  });
  await page.keyboard.press("End");
  await expect
    .poll(() => tree.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(0);
  const scroll = await tree.evaluate((node) => node.scrollTop);
  await page.keyboard.press("Enter");
  await expect(editor).toBeFocused();
  await editor.fill("77");
  device.publish(device.prefix + "/settings/other", "1234");
  await expect(other.locator(".value")).toHaveText("1234");
  await expect(editor).toHaveValue("77");
  const revert = page.getByRole("button", { name: "Revert" });
  await expect(revert).toBeEnabled();
  expect(await tree.evaluate((node) => node.scrollTop)).toBe(scroll);
  await editor.press("Escape");
  await expect(other).toBeFocused();
  expect(await tree.evaluate((node) => node.scrollTop)).toBe(scroll);
  await revert.click();
  await expect(editor).toHaveValue("1234");
  await expect(revert).toBeDisabled();
  expect(await tree.evaluate((node) => node.scrollTop)).toBe(scroll);
  expect(await page.evaluate(() => history.length)).toBe(historyLength);
  await page.locator(".back").click();
  await expect(page.locator("input[name=broker]")).toBeVisible();
  await page.goBack();
  await expect(other).toHaveAttribute("aria-selected", "true");
});

test("pending Set preserves newer edits and reports its correlated response", async ({
  browsePage: page,
  device,
}) => {
  const editor = page.locator("textarea");
  const set = page.getByRole("button", { name: "Set", exact: true });
  await page.locator(".log summary").click();
  await editor.fill("-9007199254740993");
  device.holdResponse = true;
  await set.click();
  await expect.poll(() => typeof device.respond).toBe("function");
  await expect(set).toBeDisabled();
  await editor.fill("123");
  await editor.press("Control+Enter");
  expect(device.writes).toHaveLength(1);
  expect(device.writes[0].payload.toString()).toBe("-9007199254740993");
  device.respond();
  await expect(page.locator(".status [role=status]")).toHaveText(
    "Set succeeded",
  );
  await expect(editor).toHaveValue("123");
  await expect(page.locator(".log pre")).toContainText(
    /set: \/leaf: Set succeeded · \d+ ms/,
  );
});

test("invalid JSON, unsolicited settings, and Revert preserve drafts", async ({
  browsePage: page,
  device,
}) => {
  const editor = page.locator("textarea");
  const value = page.locator('[data-tree-path="/leaf"] .value');
  const revert = page.getByRole("button", { name: "Revert" });
  await editor.fill("{");
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect(page.locator("#editor-error")).toContainText("Invalid JSON");
  await expect(page.locator(".status [role=status]")).toHaveText("Ready");
  await expect(page.locator(".selected [role=status]")).toHaveCount(0);
  expect(device.writes).toHaveLength(0);
  await editor.fill("123");
  await expect(page.locator("#editor-error")).toHaveCount(0);
  device.publish(device.prefix + "/settings/leaf", "456");
  await expect(value).toHaveText("456");
  await expect(editor).toHaveValue("123");
  await revert.click();
  await expect(editor).toHaveValue("456");
  await editor.fill("457");
  device.publish(device.prefix + "/settings/leaf", "458");
  await expect(value).toHaveText("458");
  await editor.fill("456");
  await revert.click();
  await expect(editor).toHaveValue("458");
  await editor.fill("3");
  device.publish(device.prefix + "/settings/leaf", "3");
  await expect(value).toHaveText("3");
  await expect(revert).toBeDisabled();
  device.publish(device.prefix + "/settings/leaf", "4");
  await expect(value).toHaveText("4");
  await expect(editor).toHaveValue("3");
  await expect(revert).toBeEnabled();
});

test("device formatting wins with either echo and response order", async ({
  browsePage: page,
  device,
}) => {
  device.holdResponse = true;
  for (const [input, value, echoFirst] of [
    ["20", "20.0", true],
    ["30", "30.0", false],
  ]) {
    device.respond = undefined;
    await page.locator("textarea").fill(input);
    await page.getByRole("button", { name: "Set", exact: true }).click();
    await expect.poll(() => typeof device.respond).toBe("function");
    device.respond(value, echoFirst);
    await expect(page.locator("textarea")).toHaveValue(value);
    await expect(page.getByRole("button", { name: "Revert" })).toBeDisabled();
  }
});

test("action failures remain readable until an explicit retry", async ({
  browsePage: page,
  device,
}) => {
  await page.setViewportSize({ width: 390, height: 850 });
  device.holdResponse = true;
  await page.locator("textarea").fill("999");
  await page.getByRole("button", { name: "Set", exact: true }).tap();
  await expect.poll(() => typeof device.respond).toBe("function");
  const rejection =
    "Value exceeds the supported range for this setting. Choose a smaller value and try again.";
  device.respond(rejection, true, "Error");
  const status = page.locator(".status [role=status]");
  await expect(status).toHaveText("Set failed: " + rejection);
  await expect(status).toHaveClass(/failed/);
  await expect(page.locator("textarea")).toHaveValue("999");
  expect(
    await status.evaluate(
      (node) =>
        node.scrollWidth <= node.clientWidth &&
        node.scrollHeight <= node.clientHeight &&
        node.clientHeight > parseFloat(getComputedStyle(node).lineHeight),
    ),
  ).toBe(true);
  device.publish(device.prefix + "/settings/leaf", "31.0");
  await expect(page.locator('[data-tree-path="/leaf"] .value')).toHaveText(
    "31.0",
  );
  await expect(status).toHaveText("Set failed: " + rejection);
  device.holdResponse = false;
  await page.getByRole("button", { name: "Set", exact: true }).tap();
  await expect(status).toHaveText("Set succeeded");
  await expect(status).not.toHaveClass(/failed/);
});
