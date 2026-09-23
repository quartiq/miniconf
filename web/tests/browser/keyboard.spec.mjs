import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("connection shortcuts, tree focus recovery, and build identity", async ({
  page,
  device,
  baseURL,
}) => {
  await page.goto(baseURL);
  const broker = page.locator('input[name="broker"]');
  await broker.focus();
  await page.keyboard.press("Control+Enter");
  expect(await broker.evaluate((input) => input.validity.valid)).toBe(false);
  expect(device.connections).toBe(0);
  await broker.fill(`ws://127.0.0.1:${device.port}`);
  await page.locator('input[name="discovery-filter"]').fill("dt/test/+");
  await page.keyboard.press("Control+Enter");
  const deviceRow = page.locator('[data-tree-path="/dt/test/device"]');
  await expect(deviceRow).toBeVisible();
  await broker.focus();
  await page.keyboard.press("Meta+Enter");
  await expect.poll(() => device.connections).toBe(2);
  await expect(deviceRow).toBeVisible();
  await deviceRow.focus();
  await page.keyboard.press("Home");
  const root = page.locator('[data-tree-path=""]');
  await expect(root).toBeFocused();
  await page.keyboard.press("End");
  await expect(deviceRow).toBeFocused();
  const alive = device.retained.get(`${device.prefix}/alive`).text;
  device.publish(`${device.prefix}/alive`, "");
  await expect(deviceRow).toHaveCount(0);
  await expect(root).toBeFocused();
  // A discovery update must not steal focus from a form control.
  await broker.focus();
  device.publish(`${device.prefix}/alive`, alive);
  await expect(deviceRow).toBeVisible();
  await expect(broker).toBeFocused();
  const identity = page.locator("header .build-id");
  const commit =
    process.env.MINICONF_WEB_BUILD_COMMIT || process.env.GITHUB_SHA;
  if (commit && /^[0-9a-f]{40}$/i.test(commit)) {
    await expect(page.locator("header a.build-id")).toHaveAttribute(
      "href",
      `https://github.com/quartiq/miniconf/commit/${commit}`,
    );
    await expect(identity).toHaveText(`build ${commit.slice(0, 8)}`);
  } else {
    await expect(identity).toHaveText("local build");
    await expect(page.locator("header a.build-id")).toHaveCount(0);
  }
});
