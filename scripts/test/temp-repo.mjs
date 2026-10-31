// Throw-away git repositories for the script tests (no test of its own).
// Synthetic identities only (example.net).
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** The checkout these tests belong to. */
export const ROOT = path.resolve(import.meta.dirname, "..", "..");

/**
 * Runs git in `dir` and returns its output.
 *
 * @param {string} dir
 * @param {string[]} args
 * @returns {string}
 */
export const git = (dir, args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" });

/**
 * A new git repository at `dir` on `main`, with a synthetic identity and
 * one commit; its hooks are off unless `hooks` names a hooks directory.
 *
 * @param {string} dir
 * @param {{ hooks?: string }} [options]
 * @returns {string} `dir`
 */
export function initRepo(dir, { hooks = "no-hooks" } = {}) {
  fs.mkdirSync(dir, { recursive: true });
  git(dir, ["init", "-q", "-b", "main"]);
  git(dir, ["config", "user.email", "sam@example.net"]);
  git(dir, ["config", "user.name", "Sam"]);
  git(dir, ["config", "core.hooksPath", hooks]);
  fs.writeFileSync(path.join(dir, "a.txt"), "clean\n");
  git(dir, ["add", "a.txt"]);
  git(dir, ["commit", "-q", "-m", "clean"]);
  return dir;
}

/**
 * Runs `body` with a fresh temporary directory and removes it afterwards.
 *
 * @template T
 * @param {string} prefix
 * @param {(dir: string) => T} body
 * @returns {T}
 */
export function withTempDir(prefix, body) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  try {
    return body(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

/** The object id git's pre-push hook reports for a ref that does not exist. */
export const ZERO_SHA = "0".repeat(40);

/** A network remote's URL (never contacted). */
export const NETWORK_URL = "https://example.org/nail.git";
