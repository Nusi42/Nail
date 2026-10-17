#!/usr/bin/env node
/**
 * Records the README's animations and stills into `docs/media/`
 * (`npm run readme:media`, ADR 0197) with `e2e/readme/playwright.readme.config.ts`
 * against the demo seed. Review the files before committing them.
 *
 * Usage: node scripts/readme-media.mjs [-- <playwright args>]  (e.g. `-- -g safety`)
 * Needs `npm run build -w ui` and `cargo build -p nail-cli` (or NAIL_BIN);
 * NAIL_README_FRAMES=<dir> also writes every frame as a PNG for review.
 */
import { spawnSync } from "node:child_process";
import path from "node:path";

import { repoRoot, runMain } from "./lib/program.mjs";
import { assertWorkFolder } from "./lib/work-folder.mjs";

/** @param {string[]} argv */
function main(argv) {
  assertWorkFolder("npm run readme:media");
  const rest = argv[0] === "--" ? argv.slice(1) : argv;
  // The Playwright CLI through node itself (no shell for `npx.cmd`).
  const cli = path.join(repoRoot, "node_modules", "@playwright", "test", "cli.js");
  const args = ["test", "-c", path.join("e2e", "readme", "playwright.readme.config.ts"), ...rest];
  console.log(`> playwright ${args.join(" ")}`);
  return (
    spawnSync(process.execPath, [cli, ...args], { cwd: repoRoot, stdio: "inherit" }).status ?? 1
  );
}

await runMain(import.meta.url, main);
