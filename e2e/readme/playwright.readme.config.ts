import path from "node:path";
import { defineConfig, devices } from "@playwright/test";
import { buildOutputDir, nailBinary } from "../support/nail";

/**
 * The README media (ADR 0197): `npm run readme:media` records the GIFs and
 * stills in `docs/media/` against `nail serve --seed demo-multi` (made-up
 * personas and companies only), with the visual harness's fixed clock.
 * Not part of `npm run e2e`; review the files before committing them.
 * Prerequisites: `npm run build -w ui` and `cargo build -p nail-cli` (or
 * `NAIL_BIN`).
 */
process.env.NAIL_BIN ??= nailBinary();

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.readme\.ts$/,
  fullyParallel: true,
  workers: 2,
  retries: 0,
  timeout: 5 * 60_000,
  outputDir: path.join(buildOutputDir("readme"), "test-results"),
  reporter: "list",
  expect: { timeout: 20_000 },
  use: { actionTimeout: 20_000, trace: "retain-on-failure" },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 760 },
        deviceScaleFactor: 1,
        locale: "en-US",
        timezoneId: "UTC",
        colorScheme: "light",
        reducedMotion: "reduce",
      },
    },
  ],
});
