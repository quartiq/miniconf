import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("autofill and discovery links use the applied broker credentials", async ({
  page,
  device,
  baseURL,
}) => {
  await page.goto(baseURL);
  const broker = page.locator("input[name=broker]");
  const user = page.locator("input[name=username]");
  const password = page.locator("input[name=password]");
  const submit = page.locator("button[type=submit]");
  const link = page.locator('a[data-tree-path="/dt/test/device"]');
  await broker.fill("ws://127.0.0.1:" + device.port);
  await page.locator("input[name=discovery-filter]").fill("dt/test/+");
  for (const width of [1200, 761]) {
    await page.setViewportSize({ width, height: 850 });
    expect(
      Math.abs(
        (await broker.boundingBox()).y -
          (await page.locator("input[name=discovery-filter]").boundingBox()).y,
      ),
    ).toBeLessThan(1);
  }
  await submit.click();
  await expect(link).toBeVisible();
  const href = await link.getAttribute("href");
  for (const [name, secret, autofill] of [
    ["test-user", "first-secret", false],
    ["autofill-user", "replacement-secret", true],
    ["", "", true],
    ["active-user", "active-secret", false],
  ]) {
    if (autofill) {
      await user.evaluate((input, text) => {
        input.value = text;
      }, name);
      await password.evaluate((input, text) => {
        input.value = text;
      }, secret);
    } else {
      await user.fill(name);
      await password.fill(secret);
    }
    const before = device.connections;
    await submit.click();
    await expect.poll(() => device.connections).toBe(before + 1);
    await expect(link).toBeVisible();
    expect(device.lastConnect.username ?? "").toBe(name);
    expect(device.lastConnect.password?.toString() ?? "").toBe(secret);
  }
  await user.fill("rejected-user");
  await submit.click();
  await expect(page.locator(".log summary")).toContainText("Connection failed");
  expect(
    await page.evaluate(
      () =>
        JSON.parse(sessionStorage.getItem("miniconf-web.connection")).username,
    ),
  ).toBe("active-user");
  await page.reload();
  await expect(link).toBeVisible();
  expect(device.lastConnect.username).toBe("active-user");
  await password.fill("unsubmitted-secret");
  const branch = page.locator('[data-tree-path="/dt/test"]');
  await branch.locator(".toggle").click();
  device.publish(
    "dt/test/another/alive",
    device.retained.get(device.prefix + "/alive").text,
  );
  await expect(page.locator(".prefixes .meta")).toHaveText("2 found");
  await expect(branch).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator('[data-tree-path="/dt/test/another"]')).toHaveCount(
    0,
  );
  await branch.locator(".toggle").click();
  await expect(
    page.locator('[data-tree-path="/dt/test/another"]'),
  ).toBeVisible();
  device.publish("dt/test/another/alive", "");
  await expect(page.locator(".prefixes .meta")).toHaveText("1 found");
  await broker.fill("ws://different.invalid:99");
  await expect(link).toHaveAttribute("href", href);
  await link.click();
  await expect(page.locator('[data-tree-path="/leaf"]')).toBeVisible();
  await page.reload();
  await expect(page.locator('[data-tree-path="/leaf"]')).toBeVisible();
  expect(device.lastConnect.username).toBe("active-user");
  expect(device.lastConnect.password?.toString()).toBe("active-secret");
  expect(
    await page.evaluate(() =>
      [
        location.href,
        JSON.stringify(history.state),
        JSON.stringify(localStorage),
      ].join(),
    ),
  ).not.toContain("active-secret");
  expect(
    await page.evaluate(
      () =>
        JSON.parse(sessionStorage.getItem("miniconf-web.connection")).password,
    ),
  ).toBe("active-secret");
});

test("missing credentials prompt; denied storage still permits reconnect", async ({
  browsePage: page,
  device,
  browseURL,
}) => {
  await page.locator(".back").click();
  await page.locator("input[name=username]").fill("reload-user");
  await page.locator("input[name=password]").fill("reload-secret");
  await page.locator("button[type=submit]").click();
  await expect(page.locator(".prefixes a")).toBeVisible();
  await page.evaluate((hash) => {
    location.hash = hash + "&path=%2Fother";
  }, new URL(browseURL).hash);
  await expect(page.locator("textarea")).toHaveValue("null");
  const resumeURL = page.url();
  const connections = device.connections;
  await page.evaluate(() =>
    sessionStorage.removeItem("miniconf-web.connection"),
  );
  await page.reload();
  await expect(page.locator(".log summary")).toContainText(
    "Enter credentials to reconnect",
  );
  expect(device.connections).toBe(connections);
  expect(page.url()).toBe(resumeURL);
  await page.locator("input[name=username]").fill("resume-user");
  await page.locator("input[name=password]").fill("resume-secret");
  await page.evaluate(() => {
    Storage.prototype.setItem = () => {
      throw new DOMException("Storage denied", "SecurityError");
    };
  });
  await page.locator("button[type=submit]").click();
  await expect(page.locator(".selected h2")).toHaveText("/other");
  await expect(page.locator("textarea")).toHaveValue("null");
  expect(device.lastConnect.username).toBe("resume-user");
  expect(device.lastConnect.password?.toString()).toBe("resume-secret");
  await expect(page.locator(".status")).toContainText("storage unavailable");
  expect(page.url()).toBe(resumeURL);
});

test("broker edits and history never borrow another endpoint's credentials", async ({
  page,
  device,
  baseURL,
}) => {
  await page.goto(baseURL);
  const broker = page.locator("input[name=broker]");
  const user = page.locator("input[name=username]");
  const password = page.locator("input[name=password]");
  const submit = page.locator("button[type=submit]");
  await broker.fill("ws://127.0.0.1:" + device.port);
  await page.locator("input[name=discovery-filter]").fill("dt/test/+");
  await user.fill("resume-user");
  await password.fill("resume-secret");
  await submit.click();
  await page.locator('a[data-tree-path="/dt/test/device"]').click();
  await page.locator(".back").click();
  const other = "ws://127.0.0.1:" + device.port + "/other";
  await broker.fill(other);
  await expect(user).toHaveValue("");
  await expect(password).toHaveValue("");
  await user.fill("must-not-leak");
  await password.fill("must-not-leak");
  const beforeEdit = device.connections;
  await broker.evaluate((input) => {
    input.value += "/silent";
  });
  await submit.click();
  await expect(page.locator(".connection")).toContainText("Broker changed");
  expect(device.connections).toBe(beforeEdit);
  await expect(password).toHaveValue("");
  await broker.fill(other);
  await expect(user).toHaveValue("");
  await expect(password).toHaveValue("");
  await user.fill("other-user");
  await password.fill("other-secret");
  await submit.click();
  await expect.poll(() => device.lastConnect.username).toBe("other-user");
  await expect(page.locator(".prefixes a")).toBeVisible();
  const beforeBack = device.connections;
  await page.goBack();
  await expect(page.locator(".log summary")).toContainText(
    "Enter credentials to reconnect",
  );
  expect(device.connections).toBe(beforeBack);
  await expect(password).toHaveValue("");
  expect(
    await page.evaluate(() =>
      sessionStorage.getItem("miniconf-web.connection"),
    ),
  ).toBeNull();
  await page.goForward();
  await expect(broker).toHaveValue(other);
  expect(device.connections).toBe(beforeBack);
  await user.fill("other-user");
  await password.fill("other-secret");
  await submit.click();
  await expect(page.locator(".prefixes a")).toBeVisible();
  await page.evaluate(() => {
    location.hash = "";
  });
  await expect(broker).toHaveValue("");
  expect(
    await page.evaluate(() =>
      sessionStorage.getItem("miniconf-web.connection"),
    ),
  ).toBeNull();
});
