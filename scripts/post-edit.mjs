#!/usr/bin/env node
// The Claude Code PostToolUse hook (.claude/settings.json, ADR 0132): runs
// the per-file checks of the pre-commit hook on the file an agent just
// edited. Claude Code passes the tool call as JSON on stdin
// (`tool_input.file_path`, or `notebook_path` for notebooks). On a finding
// the short report goes to stderr with exit code 2, which Claude Code feeds
// back to the agent.
//
// Claude Code starts this file from the session's folder
// ($CLAUDE_PROJECT_DIR), often the master, whose copy lags behind until the
// master is fast-forwarded. So it stays small and stable: it finds the
// checkout from the edited file and hands the file to that checkout's own
// scripts/lib/after-edit.mjs, which does the work (clean a note, run the
// checks, skip a master checkout).
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseJson } from "./lib/json.mjs";
import { isMain } from "./lib/program.mjs";

/** Exit code that makes Claude Code show stderr to the agent. */
const FEEDBACK_EXIT_CODE = 2;

/** The module that handles an edited file, relative to a checkout's root. */
export const AFTER_EDIT = path.join("scripts", "lib", "after-edit.mjs");

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
 * `.git` and {@link AFTER_EDIT}), or `undefined`.
 *
 * @param {string} file an absolute path
 * @returns {string | undefined}
 */
export function checkoutOf(file) {
  let dir = path.dirname(path.resolve(file));
  for (;;) {
    if (fs.existsSync(path.join(dir, ".git")) && fs.existsSync(path.join(dir, AFTER_EDIT))) {
      return dir;
    }
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
  if (!fs.existsSync(file)) return;
  const root = checkoutOf(file);
  const rel = root === undefined ? undefined : relativeTo(root, file);
  if (root === undefined || rel === undefined) return;
  /** @type {unknown} */
  const loaded = await import(pathToFileURL(path.join(root, AFTER_EDIT)).href);
  const handler = /** @type {typeof import("./lib/after-edit.mjs")} */ (loaded);
  const report = await handler.afterEdit(root, file, rel);
  if (report === undefined) return;
  console.error(report);
  process.exitCode = FEEDBACK_EXIT_CODE;
}

// Not runMain: a broken hook must not block edits (exit code 2 is feedback).
if (isMain(import.meta.url)) await main();
