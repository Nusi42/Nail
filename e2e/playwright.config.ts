import { rmSync } from "node:fs";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";
import { buildOutputDir, freePort, nailBinary, repoRoot, tempDir } from "./support/nail";

/**
 * End-to-end tests against the web UI mode: the prebuilt
 * `nail serve --seed demo` with temporary data and config directories.
 * Nothing is built here; see README.md.
 *
 * The config file is evaluated by the runner and again by every worker;
 * values chosen once are handed to the workers through the environment.
 */
process.env.NAIL_BIN ??= nailBinary();
if (!process.env.NAIL_E2E_DATA || !process.env.NAIL_E2E_CONFIG) {
  // First evaluation (the runner): create the shared server's directories
  // and remove them when the runner exits, after the web server is stopped.
  const dirs = [tempDir("shared-data"), tempDir("shared-config")] as const;
  [process.env.NAIL_E2E_DATA, process.env.NAIL_E2E_CONFIG] = dirs;
  process.on("exit", () => {
    for (const dir of dirs) {
      try {
        rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
      } catch {
        // Best effort: the OS cleans the temp directory eventually.
      }
    }
  });
}
// A free port for the shared server, chosen once by the runner (like the
// directories above) unless NAIL_E2E_PORT pins one.
process.env.NAIL_E2E_PORT ??= String(freePort());
const port = Number(process.env.NAIL_E2E_PORT);
// Traces, screenshots and the HTML report stay out of the (OneDrive-synced)
// repository, next to cargo's target directory.
const outputRoot = buildOutputDir("e2e");
const quote = (s: string) => `"${s}"`;

export default defineConfig({
  testDir: "./tests",
  // Builds the "Follow up" example plug-in for plugins.spec.ts (M2d).
  globalSetup: "./support/global-setup.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  outputDir: path.join(outputRoot, "test-results"),
  reporter: process.env.CI
    ? [["github"], ["html", { open: "never", outputFolder: path.join(outputRoot, "report") }]]
    : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  // M5b: a taller window than Playwright's 720 px, so the message list
  // (below the list header) renders every row of the demo inboxes.
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 1000 } },
    },
  ],
  webServer: {
    command: [
      quote(process.env.NAIL_BIN),
      "serve",
      "--seed demo",
      `--port ${port}`,
      `--data-dir ${quote(process.env.NAIL_E2E_DATA)}`,
      `--config-dir ${quote(process.env.NAIL_E2E_CONFIG)}`,
      // Always the freshly built bundle (a `ui` folder beside the binary,
      // e.g. Tauri's resource copy in the target directory, may be stale).
      `--ui-dir ${quote(path.join(repoRoot, "ui", "dist"))}`,
    ].join(" "),
    cwd: repoRoot,
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: false,
    env: { NAIL_LOG: process.env.NAIL_LOG ?? "warn" },
    stdout: "ignore",
    stderr: "pipe",
    timeout: 60_000,
  },
});
