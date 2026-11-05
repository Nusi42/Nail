/**
 * Asks a tool for one value, quietly: its standard output (trimmed), or
 * `undefined` when it is missing, fails or prints nothing. Used where the
 * answer is optional (a git remote, the free commit charge).
 */
import { execFileSync } from "node:child_process";

/**
 * @param {string} command
 * @param {string[]} args
 * @param {string} [cwd]
 * @returns {string | undefined}
 */
export function probe(command, args, cwd) {
  try {
    const text = execFileSync(command, args, {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    return text.trim() || undefined;
  } catch {
    return undefined;
  }
}
