import { tmpdir } from "node:os";
import path from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  // Keep Vite's cache out of the repository (OneDrive).
  cacheDir: path.join(tmpdir(), "nail-vite", "create-nail-plugin"),
  // `@nail/plugin-api` is a workspace symlink in node_modules. When the
  // real path cannot be resolved (node_modules in a OneDrive cloud folder is
  // a reparse point), Vite treats the package as external and Node loads its
  // TypeScript sources natively, failing on their extensionless imports.
  // Keeping the symlinked path lets Vite transform it (as in ui/vite.config.ts).
  resolve: { preserveSymlinks: true },
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
    // Scaffolding, bundling and type-checking take a few seconds each.
    testTimeout: 120_000,
    hookTimeout: 120_000,
  },
});
