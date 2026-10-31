#!/usr/bin/env node
// No merge commits in what a push adds (ADR 0208): branches are combined by
// rebasing, so `main` stays a straight line (the public repository's
// ruleset requires it). Run by the pre-push hook (scripts/lib/push-plan.mjs)
// on pushes to the master and from the master; reads git's ref lines on
// stdin and checks the pushing checkout.
import { execFileSync } from "node:child_process";
import fs from "node:fs";

import { hasCommit, parsePushLines, pushedRange } from "./lib/push-plan.mjs";
import { runMain } from "./lib/program.mjs";

/** @returns {number} */
function main() {
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
  /** @type {string[]} */
  const merges = [];
  for (const ref of parsePushLines(fs.readFileSync(0, "utf8"))) {
    const range = pushedRange(ref, (sha) => hasCommit(root, sha));
    if (range === undefined) continue;
    const out = execFileSync("git", ["log", "--merges", "--format=%h %s", ...range], {
      cwd: root,
      encoding: "utf8",
    });
    merges.push(...out.split("\n").filter(Boolean));
  }
  if (merges.length === 0) {
    console.log("linear: no merge commits to push");
    return 0;
  }
  for (const merge of merges) console.error(`merge commit ${merge}`);
  console.error(
    `\nlinear: ${String(merges.length)} merge commit(s) in the push. Combine branches by ` +
      "rebasing onto the target branch (`git rebase <target>`, then a fast-forward), " +
      "never with a merge commit (ADR 0208).",
  );
  return 1;
}

await runMain(import.meta.url, main);
