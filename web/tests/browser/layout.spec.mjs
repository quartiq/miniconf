import { expect } from "@playwright/test";
import { test } from "./fixtures.mjs";

test("build identity stays inside both headers without crowding content", async ({
  page,
  baseURL,
  browseURL,
}, testInfo) => {
  for (const [view, url] of [
    ["connect", baseURL],
    ["browse", browseURL],
  ]) {
    await page.goto(url);
    if (view === "browse")
      await expect(page.getByRole("tree", { name: "Settings" })).toBeVisible();
    for (const width of [320, 761, 1200]) {
      await page.setViewportSize({ width, height: 850 });
      const identity = page.locator("header .build-id");
      await expect(identity).toBeVisible();
      const build = await identity.boundingBox();
      const header = await page.locator("header").boundingBox();
      expect(build.x).toBeGreaterThanOrEqual(header.x);
      expect(build.x + build.width).toBeLessThanOrEqual(
        header.x + header.width,
      );
      expect(build.y).toBeGreaterThanOrEqual(header.y);
      expect(build.y + build.height).toBeLessThanOrEqual(
        header.y + header.height,
      );
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth),
      ).toBeLessThanOrEqual(width);
      if (view === "browse") {
        await expect(page.locator('[role="status"]')).toHaveText("Ready");
        expect(
          await page
            .locator('[role="status"]')
            .evaluate((node) => node.scrollWidth <= node.clientWidth),
        ).toBe(true);
        const status = await page.locator('[role="status"]').boundingBox();
        expect(status.x + status.width).toBeLessThan(build.x);
        if (width === 1200)
          expect(
            await page.evaluate(() => document.documentElement.scrollHeight),
          ).toBeLessThanOrEqual(850);
      }
      if (width !== 761)
        await page.screenshot({
          path: testInfo.outputPath(`${view}-${width}.png`),
          fullPage: true,
        });
    }
  }
});

test("metadata and responsive layout keep the workspace usable", async ({
  browsePage: page,
  device,
}) => {
  const editor = page.locator("textarea");
  const root = page.locator('[data-tree-path=""]');
  const leaf = page.locator('[data-tree-path="/leaf"]');
  await expect(page.locator(".schema-body")).toContainText("i32");
  await expect(page.locator(".selected details")).toHaveCount(0);
  await root.click();
  await expect(editor).toHaveCount(0);
  await expect(page.locator(".schema-body")).toBeVisible();
  await leaf.click();
  await page.locator('[data-tree-path="/other"]').click();
  await leaf.click();
  await expect(page.locator(".selected h2")).toHaveText("/leaf");
  await expect(page.getByLabel("Edge metadata", { exact: true })).toContainText(
    "edge note",
  );
  for (const text of ["first line\nsecond line", "false", "null"])
    await expect(
      page.getByLabel("Node metadata", { exact: true }),
    ).toContainText(text);
  await expect(page.getByLabel("Semantics", { exact: true })).toContainText(
    "opaque",
  );
  for (const width of [320, 390, 761, 1200]) {
    await page.setViewportSize({ width, height: 850 });
    for (const selector of [".back", ".context h1", ".status"])
      await expect(page.locator(selector)).toBeVisible();
    for (const selector of [".back", ".context h1"])
      expect(
        await page
          .locator(selector)
          .evaluate(
            (node) =>
              node.scrollWidth <= node.clientWidth + 1 &&
              getComputedStyle(node).textOverflow !== "ellipsis",
          ),
      ).toBe(true);
    if (width === 390) {
      const draft = await editor.inputValue();
      await root.locator("button").tap();
      await expect(leaf).toHaveCount(0);
      await expect(editor).toHaveValue(draft);
      await root.locator("button").tap();
      await expect(leaf).toBeVisible();
    }
    await expect(page.locator(".app-header details")).toHaveCount(0);
    const editorBox = await editor.boundingBox();
    const actions = await page.locator(".actions").boundingBox();
    const schema = await page.locator(".schema-body").boundingBox();
    const parent = await page.locator(".value-editor").boundingBox();
    expect(actions.y).toBeGreaterThanOrEqual(editorBox.y + editorBox.height);
    expect(schema.y).toBeGreaterThanOrEqual(actions.y + actions.height);
    expect(Math.abs(editorBox.width - parent.width)).toBeLessThan(1);
    expect(editorBox.width).toBeGreaterThan(240);
    expect(
      await page
        .locator(".schema-body")
        .evaluate((node) => node.scrollHeight - node.clientHeight),
    ).toBeLessThanOrEqual(1);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await expect(
      page.getByRole("textbox", { name: "Setting value" }),
    ).toBeVisible();
    await expect(page.getByRole("tree", { name: "Settings" })).toBeVisible();
    await expect(page.locator('[role=treeitem][tabindex="0"]')).toHaveCount(1);
  }
  await page.setViewportSize({ width: 1024, height: 400 });
  await expect(page.locator(".context h1")).toHaveText(device.prefix);
  await expect(page.locator(".context h1")).toHaveAttribute(
    "title",
    new RegExp(String(device.revision)),
  );
  await expect(page.locator(".back")).toHaveText(
    "ws://127.0.0.1:" + device.port,
  );
  await expect(page.locator(".identity-details")).toHaveText("2 settings");
  await page.locator(".log summary").click();
  expect(
    (await page.locator(".workspace").boundingBox()).height,
  ).toBeGreaterThanOrEqual(240);
});
