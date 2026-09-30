import { tmpdir } from "node:os";
import path from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  // Keep Vite's cache out of the repository (OneDrive).
  cacheDir: path.join(tmpdir(), "nail-vite", "create-nail-plugin"),
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
    // Scaffolding, bundling and type-checking take a few seconds each.
    testTimeout: 120_000,
    hookTimeout: 120_000,
  },
});
