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
    // Vite's native resolver canonicalizes every path it resolves; inside a
    // OneDrive folder (a cloud-file reparse point) that fails and no package
    // resolves at all ("Failed to resolve import"). The renderer imports no
    // symlinked workspace package, so keeping paths as they are changes
    // nothing else.
    preserveSymlinks: true,
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
      // Rolldown's plugin-timing notice measures how long JavaScript plugin
      // hooks ran relative to the whole build. It depends on machine load
      // (it fires on a busy machine and not on an idle one for the same
      // input; here: Vite's own asset loader reading the ~100 Fluent
      // catalogs), so it reports nothing about the code and cannot be a
      // deterministic zero-warnings gate (ADR 0129). Every other check
      // stays on.
      checks: { bundlerTimings: false },
      output: {
        // The app's modules are spread over several chunks (groups below);
        // evaluate them in exactly the order of their imports, as without
        // splitting (module-level code reads values of other modules).
        strictExecutionOrder: true,
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
            {
              name: "vendor-ui",
              test: /node_modules[\\/](react-aria|react-aria-components|react-stately|@react-aria|@react-stately|@react-types|@internationalized|@swc[\\/]helpers|lucide-react|clsx|uqr|@simplewebauthn)[\\/]/,
              priority: 15,
            },
            { name: "catalogs", test: /[\\/]locales[\\/][^\\/]+[\\/][^\\/]+\.ftl/, priority: 10 },
            // The shipped default theme (BootTheme styles the first screen
            // with it): data, about 440 kB, that changes only with the theme.
            {
              name: "default-theme",
              test: /[\\/]crates[\\/]nail-api[\\/]fixtures[\\/]default-theme\.json/,
              priority: 10,
            },
            // The app's own code that every window loads at start, in parts
            // of at most ~400 kB that load in parallel (lazy chunks stay
            // lazy: only `$initial` modules are captured).
            {
              name: "app",
              test: /[\\/]ui[\\/]src[\\/]/,
              tags: ["$initial"],
              maxSize: 400_000,
              priority: 5,
            },
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
