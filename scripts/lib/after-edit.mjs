// What the Claude Code post-edit hook does with an edited file (ADR 0132),
// in the checkout that holds the file. scripts/post-edit.mjs, which runs
// from the session's folder (often the master, whose code lags behind until
// it is fast-forwarded), only finds the checkout and loads this module from
// it, so the checkout's own, current code decides. Tests in
// scripts/test/hooks.test.mjs and scripts/test/notes.test.mjs.
import fs from "node:fs";

import { NOTES, hasToolKeys, withoutToolKeys } from "./notes.mjs";
import { runChecks, shortReport } from "./run-checks.mjs";
import { isMaster } from "./work-folder.mjs";

/**
 * Removes the front-matter keys Claude Code writes into a note
 * (`originSessionId`, `modified`, `node_type`; ADR 0200). Other files stay
 * as they are.
 *
 * @param {string} file the absolute path
 * @param {string} rel its repository-relative path (`/` separators)
 * @returns {boolean} whether the file changed
 */
export function cleanNote(file, rel) {
  if (!NOTES.test(rel)) return false;
  const text = fs.readFileSync(file, "utf8");
  if (!hasToolKeys(text)) return false;
  fs.writeFileSync(file, withoutToolKeys(text));
  return true;
}

/**
 * Handles one edited file: cleans a note, then runs the per-file checks.
 * Files of a master checkout (ADR 0199) are left alone: nothing runs there.
 *
 * @param {string} root the checkout's root
 * @param {string} file the edited file's absolute path
 * @param {string} rel its repository-relative path (`/` separators)
 * @returns {Promise<string | undefined>} the short report of failed checks
 */
export async function afterEdit(root, file, rel) {
  if (isMaster(root)) return undefined;
  cleanNote(file, rel);
  const results = await runChecks(root, [rel]);
  return results.every((r) => r.code === 0) ? undefined : shortReport(results);
}
