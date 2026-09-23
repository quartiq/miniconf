import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("pruning captures candidates, preserves settings, and retains concurrent Set failures", async ({
  browsePage: page,
  device,
}) => {
  await page.setViewportSize({ width: 390, height: 850 });
  const prune = page.getByRole("button", { name: "Prune (3)", exact: true });
  await expect(prune).toBeVisible();
  await expect(prune).toHaveAttribute("title", /entire device prefix/);
  expect(device.writes.filter((m) => m.retain)).toHaveLength(0);
  const header = page.locator(".app-header");
  const height = (await header.boundingBox()).height;
  device.holdClear = true;
  await prune.click();
  expect((await header.boundingBox()).height).toBe(height);
  device.publish(`${device.prefix}/settings/later`, "not JSON");
  device.holdResponse = true;
  await page.getByRole("textbox", { name: "Setting value" }).fill("123");
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect.poll(() => typeof device.respond).toBe("function");
  device.respond("Rejected during pruning", true, "Error");
  const status = page.locator(".status [role=status]");
  await expect(status).toHaveText("Set failed: Rejected during pruning");
  device.holdClear = false;
  device.acknowledgeClear();
  const remaining = page.getByRole("button", {
    name: "Prune (1)",
    exact: true,
  });
  await expect(remaining).toBeEnabled();
  await expect(status).toHaveText("Set failed: Rejected during pruning");
  expect(
    device.writes
      .filter((m) => m.retain)
      .map((m) => m.topic)
      .sort(),
  ).toEqual(
    ["response", "set", "settings"]
      .map((ns) => `${device.prefix}/${ns}/obsolete`)
      .sort(),
  );
  expect(device.retained.has(`${device.prefix}/settings/leaf`)).toBe(true);
  await remaining.click();
  await expect(status).toHaveText("Cleared 1");
  await expect(page.locator(".prune")).toHaveCount(0);
  expect((await header.boundingBox()).height).toBe(height);
});

test("a failed clear reports partial completion and can be retried", async ({
  browsePage: page,
  device,
}) => {
  device.rejectClearAt = 2;
  await page.getByRole("button", { name: "Prune (3)", exact: true }).click();
  await expect(page.locator(".status [role=status]")).toContainText(
    "Cleared 1;",
  );
  expect(device.writes.filter((m) => m.retain)).toHaveLength(2);
  device.rejectClearAt = 0;
  await page.getByRole("button", { name: "Prune (2)", exact: true }).click();
  await expect(page.locator(".status [role=status]")).toHaveText("Cleared 2");
  await expect(page.locator(".prune")).toHaveCount(0);
  expect(device.retained.has(`${device.prefix}/settings/leaf`)).toBe(true);
});

test("an unacknowledged clear is not replayed after reconnect", async ({
  browsePage: page,
  device,
}) => {
  device.holdClear = true;
  await page.getByRole("button", { name: "Prune (3)", exact: true }).click();
  await expect.poll(() => device.writes.filter((m) => m.retain).length).toBe(1);
  for (const socket of device.broker.clients) socket.terminate();
  device.holdClear = false;
  await expect(page.locator(".status [role=status]")).toContainText(
    "pruning interrupted",
  );
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeEnabled();
  expect(device.writes.filter((m) => m.retain)).toHaveLength(1);
});

test("optional cleanup permissions do not prevent editing or observed-topic pruning", async ({
  page,
  device,
  browseURL,
}) => {
  device.rejectCleanup = true;
  await page.goto(browseURL);
  await expect(page.getByText("Partial pruning coverage")).toBeVisible();
  await page.locator('[data-tree-path="/leaf"]').click();
  device.publish(`${device.prefix}/settings/partial`, "1");
  // Settings and response topics remain observable when set/# is denied.
  await page.getByRole("button", { name: "Prune (3)", exact: true }).click();
  await expect(page.locator(".status [role=status]")).toHaveText("Cleared 3");
  expect(device.retained.has(`${device.prefix}/settings/partial`)).toBe(false);
  expect(device.retained.has(`${device.prefix}/set/obsolete`)).toBe(true);
  await page.getByRole("textbox", { name: "Setting value" }).fill("789");
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect(page.locator(".status [role=status]")).toHaveText(
    "Set succeeded",
  );
});

test("activity changes its dot without changing row selection or geometry", async ({
  browsePage: page,
  device,
}) => {
  const row = page.locator('[data-tree-path="/leaf"]');
  const before = await row.evaluate((node) => ({
    background: getComputedStyle(node).backgroundColor,
    height: node.getBoundingClientRect().height,
  }));
  device.publish(`${device.prefix}/settings/leaf`, "456");
  await expect(row.locator(".activity-dot")).toHaveCSS("opacity", "1");
  expect(
    await row.evaluate((node) => ({
      background: getComputedStyle(node).backgroundColor,
      height: node.getBoundingClientRect().height,
    })),
  ).toEqual(before);
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect(row.locator(".activity-dot")).toHaveCSS("opacity", "0");
});
