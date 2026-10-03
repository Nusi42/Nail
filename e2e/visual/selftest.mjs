#!/usr/bin/env node
/**
 * End-to-end self-test of the visual-regression harness (plan M5a WP154)
 * against the real `nail serve --seed demo-multi` and the current UI, in
 * the server's own theme (`NAIL_VISUAL_THEMES=current`), with temporary
 * baselines — it needs no theme support in the core and never touches the
 * checked-in baselines:
 *
 *   1. no baselines           → the run fails, every screen is "new";
 *   2. `--update`             → baselines are written, the run passes;
 *   3. again                  → every screen "passed";
 *   4. one baseline replaced  → that screen "changed" with a diff image,
 *                               the others still pass; the review page
 *                               shows baseline, actual and difference.
 *
 * Usage: node e2e/visual/selftest.mjs   (needs `npm run build -w ui` and
 * `cargo build -p nail-cli` or NAIL_BIN). Output: `<build dir>/visual-selftest/`.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildBase } from "../../scripts/visual-review/generate.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCREENS = ["main", "menu", "dialog"];

/** Checks one step's summary; answers the problems (empty = as expected). */
export function checkStep(name, summary, expected) {
  const problems = [];
  if (summary.exitCode !== expected.exitCode) {
    problems.push(`${name}: exit code ${summary.exitCode}, expected ${expected.exitCode}`);
  }
  const byScreen = new Map(
    summary.entries.filter((e) => e.variant === "current-comfortable").map((e) => [e.screen, e]),
  );
  for (const [screen, status] of Object.entries(expected.statuses)) {
    const entry = byScreen.get(screen);
    if (!entry) problems.push(`${name}: no result for ${screen}`);
    else if (entry.status !== status) {
      problems.push(
        `${name}: ${screen} is ${entry.status}, expected ${status} (${entry.message ?? ""})`,
      );
    }
  }
  return problems;
}

function run(env, args) {
  const result = spawnSync(process.execPath, [path.join(here, "run-visual.mjs"), ...args], {
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
  const summaryFile = path.join(env.NAIL_VISUAL_RESULTS, "review", "summary.json");
  const summary = JSON.parse(readFileSync(summaryFile, "utf8"));
  return { exitCode: result.status ?? 1, entries: summary.entries, file: summaryFile };
}

function main() {
  const baselines = mkdtempSync(path.join(tmpdir(), "nail-visual-selftest-baselines-"));
  const results = path.join(buildBase(), "visual-selftest");
  const env = {
    NAIL_VISUAL_BASELINES: baselines,
    NAIL_VISUAL_RESULTS: results,
    NAIL_VISUAL_THEMES: "current",
    NAIL_VISUAL_DENSITIES: "comfortable",
    NAIL_VISUAL_SCREENS: SCREENS.join(","),
  };
  const all = (status) => Object.fromEntries(SCREENS.map((s) => [s, status]));
  const problems = [];
  try {
    const spec = ["--", "themes.visual.ts"];
    problems.push(
      ...checkStep("1 (no baselines)", run(env, spec), { exitCode: 1, statuses: all("new") }),
    );
    problems.push(
      ...checkStep("2 (--update)", run(env, ["--update", ...spec]), {
        exitCode: 0,
        statuses: all("new"),
      }),
    );
    const dir = path.join(baselines, process.platform, "current-comfortable");
    for (const s of SCREENS) {
      if (!existsSync(path.join(dir, `${s}.png`))) problems.push(`2: no baseline ${s}.png written`);
    }
    problems.push(
      ...checkStep("3 (unchanged)", run(env, spec), { exitCode: 0, statuses: all("passed") }),
    );
    // The main window's baseline now shows the dialog: a real visual change.
    copyFileSync(path.join(dir, "dialog.png"), path.join(dir, "main.png"));
    const changed = run(env, spec);
    problems.push(
      ...checkStep("4 (one changed)", changed, {
        exitCode: 1,
        statuses: { main: "changed", menu: "passed", dialog: "passed" },
      }),
    );
    const main = changed.entries.find(
      (e) => e.variant === "current-comfortable" && e.screen === "main",
    );
    if (!main?.diff || !existsSync(main.diff))
      problems.push("4: the changed screen has no diff image");
    const page = readFileSync(path.join(results, "review", "index.html"), "utf8");
    for (const kind of ["baseline", "actual", "diff"]) {
      const image = `images/current-comfortable/main-${kind}.png`;
      if (!page.includes(image) || !existsSync(path.join(results, "review", ...image.split("/")))) {
        problems.push(`4: the review page lacks the ${kind} image of main`);
      }
    }
  } finally {
    rmSync(baselines, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
  if (problems.length > 0) {
    console.error(`visual self-test FAILED:\n- ${problems.join("\n- ")}`);
    return 1;
  }
  console.log("visual self-test passed: new → update → passed → changed (with diff) as expected");
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 2;
  }
}
