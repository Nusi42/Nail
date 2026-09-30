/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const localesDir = fileURLToPath(new URL("../locales", import.meta.url));
// Reference theme/layout fixtures of nail-api, used by the mock transport (`?mock=1`).
const fixturesDir = fileURLToPath(new URL("../crates/nail-api/fixtures", import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // Fluent catalogs live at the repository root (shared with the Rust core).
      "@locales": localesDir,
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    fs: { allow: [".", localesDir, fixturesDir] },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        // Keep the app's own chunk small (M2c WP54): the React runtime and
        // the other libraries every window needs, and the Fluent catalogs,
        // are separate chunks that load in parallel and stay cached across
        // app updates. The TipTap / CodeMirror editors load on demand
        // (`features/composer/lazyEditors.tsx`).
        codeSplitting: {
          groups: [
            {
              name: "vendor",
              test: /node_modules[\\/](react|react-dom|scheduler|@tanstack|zustand|@fluent)[\\/]/,
              priority: 20,
            },
            { name: "catalogs", test: /[\\/]locales[\\/][^\\/]+[\\/][^\\/]+\.ftl/, priority: 10 },
          ],
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Widget suites drive full dialogs with userEvent; 5 s is too tight when
    // cargo builds run in parallel on the same machine.
    testTimeout: 20_000,
  },
});
