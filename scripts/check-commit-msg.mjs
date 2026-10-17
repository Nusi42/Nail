#!/usr/bin/env node
// The commit-msg hook's message rules (ADR 0200, scripts/lib/commit-message.mjs):
// Conventional Commits subjects, no session trailers, no claude.ai links, no
// internal process vocabulary.
//
//   node scripts/check-commit-msg.mjs <message file>
import fs from "node:fs";

import { commitMessageProblems, messageText } from "./lib/commit-message.mjs";
import { runMain } from "./lib/program.mjs";

/** @param {string[]} argv */
function main(argv) {
  const [file] = argv;
  if (file === undefined)
    throw new Error("usage: node scripts/check-commit-msg.mjs <message file>");
  const problems = commitMessageProblems(messageText(fs.readFileSync(file, "utf8")));
  if (problems.length === 0) return 0;
  for (const problem of problems) console.error(`commit message: ${problem}`);
  console.error(
    '\nSee CLAUDE.md "Git" and docs/dev-setup.md "Commit messages"; never use --no-verify.',
  );
  return 1;
}

await runMain(import.meta.url, main);
