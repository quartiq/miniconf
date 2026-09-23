import { expect } from "@playwright/test";
import { once } from "node:events";
import { test } from "./fixtures.mjs";

test("subscription failure offers a retry", async ({
  browsePage: page,
  device,
}) => {
  const set = page.getByRole("button", { name: "Set", exact: true });
  device.rejectSubscriptions = true;
  for (const socket of device.broker.clients) socket.terminate();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(set).toBeDisabled();
  device.rejectSubscriptions = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(set).toBeEnabled();
});

test("epoch refresh preserves the draft and resumes observation", async ({
  browsePage: page,
  device,
}) => {
  await page.locator("textarea").fill("789");
  const alive = JSON.parse(device.retained.get(device.prefix + "/alive").text);
  device.publish(
    device.prefix + "/alive",
    JSON.stringify({ ...alive, epoch: ++device.epoch }),
  );
  await expect(page.locator(".context h1")).toHaveAttribute("title", /Epoch 2/);
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeEnabled();
  await expect(page.locator("textarea")).toHaveValue("789");
  await expect(page.locator('[data-tree-path="/leaf"] .value')).toHaveText(
    "9007199254740993",
  );
});

test("pending Set is not replayed after reconnect", async ({
  browsePage: page,
  device,
}) => {
  await page.locator("textarea").fill("789");
  device.holdSetAck = device.holdResponse = true;
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect.poll(() => device.writes.length).toBe(1);
  for (const socket of device.broker.clients) socket.terminate();
  await expect(page.locator(".status")).toContainText("outcome unknown");
  device.holdSetAck = device.holdResponse = false;
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeEnabled();
  expect(device.writes).toHaveLength(1);
  await expect(page.locator("textarea")).toHaveValue("789");
});

test("malformed announcements preserve drafts and recover without reconnect", async ({
  browsePage: page,
  device,
}) => {
  await page.locator("textarea").fill("789");
  const alive = device.retained.get(device.prefix + "/alive").text;
  const connections = device.connections;
  device.publish(device.prefix + "/alive", "{}");
  await expect(page.locator(".status")).toContainText("Device unavailable");
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeDisabled();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
  device.publish(device.prefix + "/alive", alive);
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeEnabled();
  expect(device.connections).toBe(connections);
  await expect(page.locator('[data-tree-path="/leaf"] .value')).toHaveText(
    "9007199254740993",
  );
  await expect(page.locator("textarea")).toHaveValue("789");
});

test("route changes abandon pending connections, Set, and Prune", async ({
  browsePage: page,
  device,
  browseURL,
}) => {
  await page.locator(".log summary").click();
  const origin = await page.evaluate(() => performance.timeOrigin);
  const hash = new URL(browseURL).hash;
  await page.evaluate(() => {
    location.hash = "";
  });
  await expect(page.locator("input[name=broker]")).toBeVisible();
  device.resetFixture();
  device.holdConnect = true;
  const connecting = once(device.broker, "held-connect", {
    signal: AbortSignal.timeout(5000),
  });
  await page.evaluate((hash) => {
    location.hash = hash;
  }, hash);
  const [accept] = await connecting;
  await page.evaluate(() => {
    location.hash = "";
  });
  await expect(page.locator(".log summary")).toContainText("Not connected");
  device.holdConnect = false;
  accept();
  await page.evaluate((hash) => {
    location.hash = hash;
  }, hash);
  await page.locator('[data-tree-path="/leaf"]').click();
  device.holdResponse = device.holdSetAck = device.holdClear = true;
  await page.locator("textarea").fill("42");
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect.poll(() => typeof device.respond).toBe("function");
  await page.getByRole("button", { name: "Prune (3)", exact: true }).click();
  await expect.poll(() => typeof device.acknowledgeClear).toBe("function");
  const lateResponse = device.respond;
  const lateClear = device.acknowledgeClear;
  await page.evaluate((hash) => {
    location.hash = hash + "&path=%2Fother";
  }, hash);
  await expect(page.locator(".selected h2")).toHaveText("/other");
  lateResponse();
  lateClear();
  device.holdResponse = device.holdSetAck = device.holdClear = false;
  await expect(page.locator(".status [role=status]")).toHaveText("Ready");
  await expect(page.locator("textarea")).toHaveValue("null");
  await expect(
    page.getByRole("button", { name: "Set", exact: true }),
  ).toBeEnabled();
  await expect(page.locator(".identity-details")).toHaveText([
    "1 setting in subtree",
    "subtree /other",
  ]);
  await expect(page.locator(".log pre")).toContainText(
    "settings: 0 changed · 1 observed",
  );
  expect(await page.evaluate(() => performance.timeOrigin)).toBe(origin);
});
