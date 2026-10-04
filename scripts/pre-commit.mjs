#!/usr/bin/env node
// The pre-commit hook (.githooks/pre-commit; core.hooksPath is set by
// `npm install` / `npm ci` through the `prepare` script). Fast checks on the
// staged files only: rustfmt, Prettier, ESLint, taplo, markdownlint, typos,
// EditorConfig (ADR 0123; runner in scripts/lib/run-checks.mjs). The checks
// run in parallel and read the working tree copy of each staged file, so
// stage the whole file you fixed.
//
// Never bypass it with `git commit --no-verify`: fix the finding, or add a
// narrow, reasoned exception (`#[expect(lint, reason = "...")]`,
// `// eslint-disable-next-line rule -- reason`).
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runChecks } from "./lib/run-checks.mjs";
import { parseStaged } from "./lib/staged.mjs";
import { assertWorkFolder } from "./lib/work-folder.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// The master only receives pushes and fast-forward merges, never commits.
assertWorkFolder("git commit");

const staged = parseStaged(
  execFileSync("git", ["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], {
    cwd: root,
    encoding: "utf8",
  }),
);

const started = performance.now();
const results = await runChecks(root, staged);
if (results.length === 0) process.exit(0);

const failed = results.filter((r) => r.code !== 0);
for (const { check, output } of failed) {
  console.error(`\n--- pre-commit: ${check.id} failed\n${output.trimEnd()}`);
}
const seconds = ((performance.now() - started) / 1000).toFixed(1);
if (failed.length > 0) {
  console.error(
    `\npre-commit: ${failed.map((r) => r.check.id).join(", ")} failed (${seconds} s). ` +
      "Fix the findings and stage the files again; never commit with --no-verify.",
  );
  process.exitCode = 1;
} else {
  console.log(
    `pre-commit: ${String(results.length)} checks passed on ${String(staged.length)} files (${seconds} s)`,
  );
}
