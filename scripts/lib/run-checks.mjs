// Runs the per-file checks of scripts/lib/staged.mjs (rustfmt, Prettier,
// ESLint, taplo, markdownlint, typos, EditorConfig) on a list of files, in
// parallel. Used by the pre-commit hook (staged files, ADR 0123) and the
// Claude Code post-edit hook (the edited file, ADR 0132); tests in
// scripts/test/run-checks.test.mjs.
import { spawn } from "node:child_process";

import { pythonScriptDirs, resolveArgs, resolveTool } from "./check-steps.mjs";
import { chunkFiles, hookChecks } from "./staged.mjs";

/**
 * @typedef {import("./staged.mjs").HookCheck} HookCheck
 * @typedef {{ check: HookCheck, code: number, output: string }} CheckResult
 */

/** Lines a post-edit report keeps, so the agent's context stays small. */
export const MAX_REPORT_LINES = 20;

/**
 * Runs one command, collecting its output.
 *
 * @param {string} root
 * @param {string} executable
 * @param {string[]} args
 * @returns {Promise<{ code: number, output: string }>}
 */
function run(root, executable, args) {
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

/**
 * Runs the checks for `files` (repository-relative, `/` separators).
 *
 * @param {string} root the checkout's root
 * @param {string[]} files
 * @returns {Promise<CheckResult[]>}
 */
export async function runChecks(root, files) {
  /** @type {string[] | undefined} */
  let extraDirs;
  return Promise.all(
    hookChecks(files).map(async (check) => {
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
      for (const chunk of chunkFiles(check.files)) {
        const result = await run(root, executable, [...args, ...chunk]);
        if (result.code !== 0) code = result.code;
        output += result.output;
      }
      return { check, code, output };
    }),
  );
}

/**
 * The short report of the failed checks: their non-empty output lines, at
 * most {@link MAX_REPORT_LINES}, each check headed by its id.
 *
 * @param {CheckResult[]} results
 * @returns {string}
 */
export function shortReport(results) {
  const lines = results
    .filter((r) => r.code !== 0)
    .flatMap((r) => [
      `${r.check.id} failed:`,
      ...r.output.split(/\r?\n/).filter((line) => line.trim() !== ""),
    ]);
  const kept = lines.slice(0, MAX_REPORT_LINES);
  if (lines.length > MAX_REPORT_LINES) kept.push("...");
  return kept.join("\n");
}
