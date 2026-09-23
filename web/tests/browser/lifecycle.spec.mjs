import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("device loss, schema replacement, and reload preserve observation and drafts", async ({
  browsePage: page,
  device,
  browseURL,
}) => {
  const editor = page.locator("textarea");
  const set = page.getByRole("button", { name: "Set", exact: true });
  await editor.fill("");
  device.publish(device.prefix + "/alive", "");
  await expect(page.locator(".status")).toContainText("Waiting for device");
  await expect(set).toBeDisabled();
  await expect(editor).toHaveValue("");
  await expect(page.getByRole("button", { name: "Revert" })).toBeEnabled();
  // Startup values can precede the alive commit marker.
  device.publish(device.prefix + "/settings/leaf", "2");
  device.publish(
    device.prefix + "/alive",
    JSON.stringify({
      proto: 1,
      epoch: ++device.epoch,
      schema_rev: device.revision,
      pages: 1,
    }),
  );
  await expect(page.locator('[data-tree-path="/leaf"] .value')).toHaveText("2");
  await expect(set).toBeEnabled();
  await expect(editor).toHaveValue("");
  await page.getByRole("button", { name: "Revert" }).click();
  await expect(editor).toHaveValue("2");
  await expect(page.locator(".prune")).toHaveText("Prune (3)");
  await editor.fill("99");
  device.publish(device.prefix + "/alive", "");
  await expect(page.locator(".status")).toContainText("Waiting for device");
  const schema = device.schema.replace('"leaf":', '"replacement":');
  let revision = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(schema))
    revision = Math.imul(revision ^ byte, 0x01000193) >>> 0;
  device.publish(device.prefix + "/schema/0", schema);
  device.publish(device.prefix + "/settings/replacement", "3");
  device.publish(
    device.prefix + "/alive",
    JSON.stringify({
      proto: 1,
      epoch: device.epoch,
      schema_rev: revision,
      pages: 1,
    }),
  );
  await expect(
    page.locator('[data-tree-path="/replacement"] .value'),
  ).toHaveText("3");
  await expect(page.locator('[data-tree-path="/leaf"]')).toHaveCount(0);
  await expect(page.getByText("Setting unavailable")).toBeVisible();
  await expect(set).toBeDisabled();
  await expect(editor).toHaveValue("99");
  await expect(page.locator(".prune")).toHaveText("Prune (4)");
  await page.reload();
  await expect(
    page.locator('[data-tree-path="/replacement"] .value'),
  ).toHaveText("3");
  expect(page.url()).toBe(browseURL);
  await expect(editor).toHaveCount(0);
  await page.locator('[data-tree-path="/replacement"]').click();
  await expect(editor).toHaveValue("3");
  expect(device.writes).toHaveLength(0);
});
