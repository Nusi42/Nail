#!/usr/bin/env node
// The pre-commit hook (.githooks/pre-commit; core.hooksPath is set by
// `npm install` / `npm ci` through the `prepare` script). Fast checks on the
// staged files only: rustfmt, Prettier, ESLint, taplo, markdownlint, typos,
// EditorConfig (ADR 0123). The checks run in parallel and read the working
// tree copy of each staged file, so stage the whole file you fixed.
//
// Never bypass it with `git commit --no-verify`: fix the finding, or add a
// narrow, reasoned exception (`#[expect(lint, reason = "...")]`,
// `// eslint-disable-next-line rule -- reason`).
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pythonScriptDirs, resolveArgs, resolveTool } from "./lib/check-steps.mjs";
import { chunkFiles, hookChecks, parseStaged } from "./lib/staged.mjs";
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
const checks = hookChecks(staged);
if (checks.length === 0) process.exit(0);

/** @type {string[] | undefined} */
let extraDirs;

/**
 * Runs one command, collecting its output.
 *
 * @param {string} executable
 * @param {string[]} args
 * @returns {Promise<{ code: number, output: string }>}
 */
function run(executable, args) {
  return new Promise((resolve) => {
    const child = spawn(executable, args, { cwd: root });
    let output = "";
    child.stdout.on("data", (/** @type {Buffer} */ d) => (output += d.toString()));
    child.stderr.on("data", (/** @type {Buffer} */ d) => (output += d.toString()));
    child.on("error", (error) => {
      resolve({ code: 1, output: String(error) });
    });
    child.on("close", (code) => {
      resolve({ code: code ?? 1, output });
    });
  });
}

const started = performance.now();
const results = await Promise.all(
  checks.map(async (check) => {
    let executable = check.tool === "node" ? process.execPath : resolveTool(check.tool);
    if (executable === null) {
      extraDirs ??= pythonScriptDirs();
      executable = resolveTool(check.tool, { extraDirs });
    }
    if (executable === null) {
      return { check, code: 1, output: `${check.tool} not found. Install: ${check.install}\n` };
    }
    const args = resolveArgs(check.args, { root });
    let code = 0;
    let output = "";
    for (const files of chunkFiles(check.files)) {
      const result = await run(executable, [...args, ...files]);
      if (result.code !== 0) code = result.code;
      output += result.output;
    }
    return { check, code, output };
  }),
);

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
    `pre-commit: ${String(checks.length)} checks passed on ${String(staged.length)} files (${seconds} s)`,
  );
}
