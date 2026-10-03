#!/usr/bin/env node
/**
 * The visual-regression run (plan M5a WP154; `npm run visual`,
 * `npm run visual:update`, the CI job `visual`):
 *
 *   1. the Playwright harness (`playwright.visual.config.ts`): every screen
 *      in every built-in theme × density against the baselines, plus the
 *      harness self-check;
 *   2. the review page (`scripts/visual-review/generate.mjs`), built
 *      whatever the outcome, into `<build dir>/visual/review/index.html`.
 *
 * Usage: node e2e/visual/run-visual.mjs [--update] [--no-review] [-- <playwright args>]
 *   --update     write baselines for new and changed screens
 *                (`--update-snapshots=changed`); review them before committing
 *   --no-review  skip the review page
 * Needs `npm run build -w ui` and `cargo build -p nail-cli` (or NAIL_BIN).
 * The exit code is Playwright's (1 = a screen changed, is new or failed).
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");

export function parseArgs(argv) {
  const options = { update: false, review: true, playwright: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--") {
      options.playwright = argv.slice(i + 1);
      break;
    }
    if (arg === "--update") options.update = true;
    else if (arg === "--no-review") options.review = false;
    else throw new Error(`unknown argument ${arg}`);
  }
  return options;
}

/** The Playwright CLI arguments of a run. */
export function playwrightArgs(options) {
  return [
    "test",
    "-c",
    path.join("e2e", "visual", "playwright.visual.config.ts"),
    ...(options.update ? ["--update-snapshots=changed"] : []),
    ...options.playwright,
  ];
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  // The Playwright CLI through node itself (no shell for `npx.cmd`).
  const cli = path.join(repoRoot, "node_modules", "@playwright", "test", "cli.js");
  const args = playwrightArgs(options);
  console.log(`> playwright ${args.join(" ")}`);
  const result = spawnSync(process.execPath, [cli, ...args], { cwd: repoRoot, stdio: "inherit" });
  const status = result.status ?? 1;
  if (options.review) {
    const {
      buildReview,
      parseArgs: reviewArgs,
      STATUSES,
    } = await import("../../scripts/visual-review/generate.mjs");
    const { file, summary, entries } = buildReview(reviewArgs([]));
    // Why variants were skipped, once per reason (the list reporter omits it).
    const reasons = new Set(entries.filter((e) => e.status === "skipped").map((e) => e.message));
    for (const reason of reasons) console.log(`skipped: ${reason}`);
    const counts = STATUSES.filter((s) => summary[s] > 0).map((s) => `${summary[s]} ${s}`);
    console.log(`visual review (${counts.join(", ") || "no results"}): ${file}`);
  }
  return status;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(
    (status) => {
      process.exitCode = status;
    },
    (error) => {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 2;
    },
  );
}
