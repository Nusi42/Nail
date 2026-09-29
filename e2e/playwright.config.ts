import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests run against the web UI mode: `nail serve --seed demo` on a
 * free port with a temporary data directory. The `webServer` entry is wired up
 * in M0 WP6; until then every spec is skipped.
 */
const port = Number(process.env.NAIL_E2E_PORT ?? 4780);

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  // WP6: webServer: { command: "cargo run -p nail-cli -- serve --seed demo --port ...", url: ..., reuseExistingServer: !process.env.CI },
});
