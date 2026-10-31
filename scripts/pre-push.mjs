#!/usr/bin/env node
// The pre-push hook (.githooks/pre-push, ADR 0205): decides from the
// checkout's role and the remote's URL what the push runs
// (scripts/lib/push-plan.mjs) and runs it in the pushing checkout. A
// refusal names the command to use instead. git passes the remote's name
// and URL as arguments and the ref lines on stdin.
//
// Runs on load, not through runMain: a hook that silently did nothing would
// let every push through.
//
// Never bypass it with `git push --no-verify`: fix the finding instead.
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { resolveTool } from "./lib/check-steps.mjs";
import { checkoutRole, masterOf, parsePushLines, pushPlan } from "./lib/push-plan.mjs";

/** The checkout these scripts belong to (the hook's own). */
const scriptsRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/**
 * @param {string[]} argv `<remote name> <remote URL>`
 * @returns {number} the exit code
 */
function main(argv) {
  const [remote = "", url = remote] = argv;
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
  const input = fs.readFileSync(0, "utf8");
  const plan = pushPlan({
    role: checkoutRole(root),
    url,
    refs: parsePushLines(input),
    master: masterOf(root),
  });
  if ("refusal" in plan) {
    console.error(`pre-push: refused: ${plan.refusal}`);
    return 1;
  }
  for (const step of plan.steps) {
    console.log(`pre-push: ${step.title}`);
    const executable = step.tool === "node" ? process.execPath : resolveTool(step.tool);
    if (executable === null) {
      console.error(`pre-push: ${step.tool} not found. Install: ${step.install}`);
      return 1;
    }
    // Node steps name a script of this checkout; they check the pushing one.
    const args =
      step.tool === "node"
        ? [path.join(scriptsRoot, ...(step.args[0] ?? "").split("/")), ...step.args.slice(1)]
        : step.args;
    const result = spawnSync(executable, args, {
      cwd: root,
      input: step.stdin === true ? input : "",
      stdio: ["pipe", "inherit", "inherit"],
    });
    if (result.status !== 0) {
      console.error(`pre-push: ${step.id} failed. Fix the findings; never push with --no-verify.`);
      return 1;
    }
  }
  return 0;
}

try {
  process.exitCode = main(process.argv.slice(2));
} catch (error) {
  console.error(`pre-push: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
