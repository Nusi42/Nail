/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const localesDir = fileURLToPath(new URL("../locales", import.meta.url));
// Reference theme/layout fixtures of nail-api, used by the mock transport (`?mock=1`).
const fixturesDir = fileURLToPath(new URL("../crates/nail-api/fixtures", import.meta.url));
// The built-in theme files, read by the mock core (`features/appearance/themes.ts`).
const themesDir = fileURLToPath(new URL("../defaults/themes", import.meta.url));
// The shipped layout, offered to pre-rail layouts (`features/customization/LayoutMigration.tsx`).
const defaultLayoutFile = fileURLToPath(new URL("../defaults/layout.kdl", import.meta.url));

// No fixed development ports (plan section 10): `npm run dev` / `npm run tauri
// dev` (scripts/dev.mjs) pick a free port and pass it as NAIL_DEV_PORT, which
// Vite then uses exactly. Run directly, Vite starts at 5791 and moves on to the
// next free port.
const devPort = Number(process.env.NAIL_DEV_PORT) || 5791;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // Fluent catalogs live at the repository root (shared with the Rust core).
      "@locales": localesDir,
    },
  },
  server: {
    host: "127.0.0.1",
    port: devPort,
    strictPort: Boolean(process.env.NAIL_DEV_PORT),
    fs: { allow: [".", localesDir, fixturesDir, themesDir, defaultLayoutFile] },
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
