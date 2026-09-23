import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests/browser",
  outputDir: ".codex/test-results",
  fullyParallel: true,
  workers: 8,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  retries: 0,
  forbidOnly: Boolean(process.env.CI),
  reporter: "list",
  use: {
    channel: "chromium",
    viewport: { width: 1200, height: 850 },
    hasTouch: true,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    launchOptions: { executablePath: process.env.CHROME_BIN },
  },
  projects: [
    { name: "http" },
    { name: "file" },
    ...(process.env.MINICONF_WEB_DEV_URL ? [{ name: "dev" }] : []),
  ],
});
