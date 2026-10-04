#!/usr/bin/env node
// The Claude Code PostToolUse hook (.claude/settings.json, ADR 0132): runs
// the per-file checks of the pre-commit hook on the file an agent just
// edited. Claude Code passes the tool call as JSON on stdin
// (`tool_input.file_path`, or `notebook_path` for notebooks). On a finding
// the short report goes to stderr with exit code 2, which Claude Code feeds
// back to the agent. The hook command skips the master itself.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseJson } from "./lib/json.mjs";
import { runChecks, shortReport } from "./lib/run-checks.mjs";

/** Exit code that makes Claude Code show stderr to the agent. */
const FEEDBACK_EXIT_CODE = 2;

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/**
 * The edited file named by the hook input, if any.
 *
 * @param {string} stdin
 * @returns {string | undefined}
 */
export function editedFile(stdin) {
  /** @type {unknown} */
  const input = parseJson(stdin);
  if (typeof input !== "object" || input === null || !("tool_input" in input)) return undefined;
  const tool = input.tool_input;
  if (typeof tool !== "object" || tool === null) return undefined;
  for (const key of ["file_path", "notebook_path"]) {
    const value = /** @type {Record<string, unknown>} */ (tool)[key];
    if (typeof value === "string" && value !== "") return value;
  }
  return undefined;
}

/**
 * The repository-relative path (`/` separators) of `file`, or `undefined`
 * when it lies outside `base`.
 *
 * @param {string} base
 * @param {string} file
 * @returns {string | undefined}
 */
export function relativeTo(base, file) {
  const rel = path.relative(base, path.resolve(base, file));
  if (rel === "" || rel.startsWith("..") || path.isAbsolute(rel)) return undefined;
  return rel.split(path.sep).join("/");
}

async function main() {
  const file = editedFile(fs.readFileSync(0, "utf8"));
  const rel = file === undefined ? undefined : relativeTo(root, file);
  if (rel === undefined || !fs.existsSync(path.join(root, rel))) return;
  const results = await runChecks(root, [rel]);
  if (results.every((r) => r.code === 0)) return;
  console.error(shortReport(results));
  process.exitCode = FEEDBACK_EXIT_CODE;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
