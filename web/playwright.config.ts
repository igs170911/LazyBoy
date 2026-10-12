import { defineConfig } from "@playwright/test";
import { existsSync } from "node:fs";

const baseURL = process.env.E2E_BASE_URL || "http://127.0.0.1:8790";
const chromiumPath = process.env.PLAYWRIGHT_CHROMIUM ||
  (existsSync("/usr/bin/chromium") ? "/usr/bin/chromium" : undefined);

export default defineConfig({
  testDir: "./e2e",
  timeout: 90_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [["list"], ["json", { outputFile: "e2e-results/playwright-report.json" }]],
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
    locale: "zh-TW",
    browserName: "chromium",
    launchOptions: {
      executablePath: chromiumPath,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
  projects: [
    {
      name: "desktop",
      use: { viewport: { width: 1280, height: 800 }, isMobile: false, hasTouch: false },
    },
    {
      name: "mobile",
      use: {
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
        userAgent:
          "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
      },
    },
  ],
  outputDir: "e2e-results/test-output",
});
