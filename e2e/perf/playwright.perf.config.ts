import path from "node:path";
import { defineConfig, devices } from "@playwright/test";
import { buildOutputDir } from "../support/nail";

/**
 * The performance scenarios (plan M4c WP123, ADR 0094): list scrolling
 * frame times and the first render on the 100k-message benchmark data.
 * Not part of the regular e2e run (`testDir` of the main config is
 * `tests/`); run with
 * `npx playwright test -c e2e/perf/playwright.perf.config.ts` after
 * `cargo build --release -p nail-cli -p nail-bench` and `npm run build -w ui`
 * (or `node e2e/perf/run-perf.mjs`, which does the whole job). One worker,
 * no retries: timings must not compete with each other.
 */
const outputRoot = buildOutputDir("e2e-perf");

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.perf\.ts$/,
  globalSetup: "./global-setup.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 10 * 60_000,
  forbidOnly: !!process.env.CI,
  outputDir: path.join(outputRoot, "test-results"),
  reporter: process.env.CI
    ? [
        ["github"],
        ["list"],
        ["html", { open: "never", outputFolder: path.join(outputRoot, "report") }],
      ]
    : "list",
  use: { trace: "off" },
  // The new headless mode (full Chromium) composits like the desktop
  // browser; the headless shell would measure a different pipeline.
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        channel: "chromium",
        viewport: { width: 1280, height: 900 },
      },
    },
  ],
});
