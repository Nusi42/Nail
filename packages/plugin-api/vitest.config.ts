import { tmpdir } from "node:os";
import path from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  // Keep Vite's cache out of the repository (it may be in a synced folder).
  cacheDir: path.join(tmpdir(), "nail-vite", "plugin-api"),
  test: {
    include: ["src/**/*.test.{ts,tsx}", "tools/**/*.test.ts", "scripts/**/*.test.ts"],
    environment: "node",
  },
});
