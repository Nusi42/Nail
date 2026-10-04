import { execFileSync } from "node:child_process";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";
import { nailBinary, repoRoot } from "../support/nail";
import { probeFromAnswer, type CoreAnswer, type ThemeProbe } from "./lib/matrix.ts";
import { baselinesRoot, resultsDir, tolerance, VIEWPORT } from "./lib/paths.ts";

/**
 * The visual-regression harness (plan M5a WP154, contract ADR 0102):
 * screenshots of the main window, composer, settings, calendar, a menu and
 * a dialog for every built-in theme × density, against
 * `nail serve --seed demo-multi` (one private server per worker, free
 * ports), compared with the reviewed baselines in
 * `baselines/<platform>/<theme>-<density>/<screen>.png`.
 *
 * Not part of `npm run e2e`. Run through `node e2e/visual/run-visual.mjs`
 * (`npm run visual`, `npm run visual:update`), which also builds the
 * review page; prerequisites: `npm run build -w ui` and
 * `cargo build -p nail-cli` (or `NAIL_BIN`).
 *
 * Deterministic pages: fixed clock (screens.ts), time zone UTC, locale
 * en-US, light colour scheme (the theme decides the colours, not the OS),
 * reduced motion and disabled animations, hidden caret, device scale 1,
 * fonts settled, and a capture only once two consecutive screenshots are
 * identical. Fonts differ between operating systems, so baselines are kept
 * per platform (`{platform}`); CI and the integrator use Windows.
 *
 * Baselines: `e2e/visual/baselines/` unless `NAIL_VISUAL_BASELINES` names
 * another folder (the self-test uses a temporary one).
 *
 * Tolerance: `NAIL_VISUAL_MAX_DIFF_RATIO` (share of pixels that may
 * differ, default 0.002) and `NAIL_VISUAL_THRESHOLD` (per-pixel colour
 * distance 0–1, default 0.15). Filters: `NAIL_VISUAL_THEMES`,
 * `NAIL_VISUAL_DENSITIES`, `NAIL_VISUAL_SCREENS` (comma-separated ids;
 * the theme `current` = the server's own theme, not switched, on request only).
 */
process.env.NAIL_BIN ??= nailBinary();

/**
 * The core's theme list, asked once by the runner (a throw-away server)
 * and handed to the workers through the environment, like the main
 * config's shared values.
 */
function probeThemes(): ThemeProbe {
  const script = path.join(repoRoot, "e2e", "visual", "lib", "probe-themes.mjs");
  const output = execFileSync(
    process.execPath,
    [script, "--bin", process.env.NAIL_BIN ?? "", "--ui-dir", path.join(repoRoot, "ui", "dist")],
    { encoding: "utf8", timeout: 120_000 },
  );
  const result = JSON.parse(output.trim().split("\n").pop() ?? "{}") as {
    answer?: CoreAnswer;
    error?: string;
  };
  if (result.answer) return probeFromAnswer(result.answer);
  return { ok: false, reason: result.error ?? "no answer from the theme probe" };
}
process.env.NAIL_VISUAL_PROBE ??= JSON.stringify(probeThemes());

const number = (value: string | undefined, fallback: number) => {
  const n = Number(value);
  return value !== undefined && value !== "" && Number.isFinite(n) ? n : fallback;
};
const outputRoot = resultsDir();

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.visual\.ts$/,
  globalSetup: "./global-setup.ts",
  fullyParallel: true,
  // The machine is shared: two workers (each with its own server) unless asked.
  workers: number(process.env.NAIL_VISUAL_WORKERS, 2),
  retries: 0,
  timeout: 5 * 60_000,
  forbidOnly: !!process.env.CI,
  outputDir: path.join(outputRoot, "test-results"),
  snapshotPathTemplate: path.join(baselinesRoot(), "{platform}", "{arg}{ext}"),
  // Baselines are written only on request (`--update-snapshots`).
  updateSnapshots: "none",
  expect: {
    timeout: 20_000,
    toMatchSnapshot: tolerance(),
  },
  reporter: process.env.CI
    ? [
        ["github"],
        ["list"],
        ["html", { open: "never", outputFolder: path.join(outputRoot, "report") }],
      ]
    : "list",
  use: {
    trace: "retain-on-failure",
    actionTimeout: 20_000,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: VIEWPORT,
        deviceScaleFactor: 1,
        locale: "en-US",
        timezoneId: "UTC",
        colorScheme: "light",
        reducedMotion: "reduce",
        forcedColors: "none",
        contrast: "no-preference",
      },
    },
  ],
});
