#!/usr/bin/env node
// The Claude Code PostToolUse hook (.claude/settings.json, ADR 0132): runs
// the per-file checks of the pre-commit hook on the file an agent just
// edited. Claude Code passes the tool call as JSON on stdin
// (`tool_input.file_path`, or `notebook_path` for notebooks). On a finding
// the short report goes to stderr with exit code 2, which Claude Code feeds
// back to the agent.
//
// The lead session runs in the master and its agents edit files in
// the work folders, so the checkout to check is found from the edited file,
// not from the session's folder, and that checkout's own checks run (its
// scripts/lib/run-checks.mjs and node_modules). Files inside OneDrive are
// never checked: nothing runs in the master.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { parseJson } from "./lib/json.mjs";
import { isInOneDrive } from "./lib/work-folder.mjs";

/** Exit code that makes Claude Code show stderr to the agent. */
const FEEDBACK_EXIT_CODE = 2;

/** The shared check runner, relative to a checkout's root. */
const RUNNER = path.join("scripts", "lib", "run-checks.mjs");

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
 * The Nail checkout that contains `file` (the nearest folder above it with
 * `.git` and the check runner), or `undefined`.
 *
 * @param {string} file an absolute path
 * @returns {string | undefined}
 */
export function checkoutOf(file) {
  let dir = path.dirname(path.resolve(file));
  for (;;) {
    if (fs.existsSync(path.join(dir, ".git")) && fs.existsSync(path.join(dir, RUNNER))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
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
  const named = editedFile(fs.readFileSync(0, "utf8"));
  if (named === undefined) return;
  const file = path.resolve(named);
  if (!fs.existsSync(file) || isInOneDrive(file)) return;
  const root = checkoutOf(file);
  const rel = root === undefined ? undefined : relativeTo(root, file);
  if (root === undefined || rel === undefined) return;
  // The work folder's own runner (its version may differ from this copy's).
  /** @type {unknown} */
  const loaded = await import(pathToFileURL(path.join(root, RUNNER)).href);
  const runner = /** @type {typeof import("./lib/run-checks.mjs")} */ (loaded);
  const results = await runner.runChecks(root, [rel]);
  if (results.every((r) => r.code === 0)) return;
  console.error(runner.shortReport(results));
  process.exitCode = FEEDBACK_EXIT_CODE;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
