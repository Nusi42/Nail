/**
 * Master repository and work folders (ADR 0132, made configurable by
 * ADR 0199). Optional: a plain clone is a work folder and nothing is
 * refused. A maintainer may keep one checkout as the *master*, which holds
 * `main`, receives pushes and fast-forward merges, and where nothing is
 * built, tested, run or committed; all work then happens in clones named
 * `<prefix><N>` on branch `<branch prefix><N>` (default `nail-work-<N>`
 * on `work-<N>`).
 *
 * The settings come from the environment or from the checkout's gitignored
 * `.env.local` (`KEY=value` lines; the environment wins):
 * `NAIL_CHECKOUT_ROLE=master` marks the master, `NAIL_WORK_FOLDER_PREFIX`
 * and `NAIL_WORK_BRANCH_PREFIX` name the work folders.
 *
 * The development entry points (`npm run dev` / `check` / `test` / `e2e` /
 * `visual`, `npm run tauri`, the pre-commit hook, `npm ci`'s `prepare`) call
 * {@link assertWorkFolder}; in the master Cargo refuses through a rustc
 * wrapper (`node scripts/work-folders.mjs guard-master`). Pure helpers here;
 * the CLI is scripts/work-folders.mjs, the tests
 * scripts/test/work-folder.test.mjs.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseEnv } from "node:util";

/** The checkout's local settings file (gitignored, never committed). */
export const LOCAL_ENV = ".env.local";

/** The role that makes a checkout the master. */
export const MASTER_ROLE = "master";

/**
 * The checkout's settings: `.env.local` overlaid with `env`.
 *
 * @param {string} root the checkout's root
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {Record<string, string | undefined>}
 */
export function localSettings(root, env = process.env) {
  const file = path.join(root, LOCAL_ENV);
  const local = fs.existsSync(file) ? parseEnv(fs.readFileSync(file, "utf8")) : {};
  return { ...local, ...env };
}

/**
 * Whether the checkout at `root` is the master.
 *
 * @param {string} root
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {boolean}
 */
export function isMaster(root, env = process.env) {
  return localSettings(root, env).NAIL_CHECKOUT_ROLE === MASTER_ROLE;
}

/**
 * `.env.local` text with the master role set (other lines kept).
 *
 * @param {string} text the current `.env.local`, empty when there is none
 * @returns {string}
 */
export function markedAsMaster(text) {
  const kept = text
    .split(/\r?\n/)
    .filter((line) => line.trim() !== "" && !/^\s*NAIL_CHECKOUT_ROLE\s*=/.test(line));
  return `${[...kept, `NAIL_CHECKOUT_ROLE=${MASTER_ROLE}`].join("\n")}\n`;
}

/**
 * How work folders and their branches are named.
 *
 * @param {Record<string, string | undefined>} [settings]
 * @returns {{ folder: string, branch: string }}
 */
export function namingOf(settings = {}) {
  return {
    folder: settings.NAIL_WORK_FOLDER_PREFIX ?? "nail-work-",
    branch: settings.NAIL_WORK_BRANCH_PREFIX ?? "work-",
  };
}

/**
 * Branch of work folder `n`: `work-<N>` by default.
 *
 * @param {number} n
 * @param {Record<string, string | undefined>} [settings]
 * @returns {string}
 */
export function branchOf(n, settings) {
  return `${namingOf(settings).branch}${String(n)}`;
}

/**
 * Folder name of work folder `n`: `nail-work-<N>` by default.
 *
 * @param {number} n
 * @param {Record<string, string | undefined>} [settings]
 * @returns {string}
 */
export function folderOf(n, settings) {
  return `${namingOf(settings).folder}${String(n)}`;
}

/**
 * Parses a work-folder number (1–99): throws on anything else.
 *
 * @param {string | undefined} text
 * @returns {number}
 */
export function parseFolderNumber(text) {
  const n = Number(text);
  if (text === undefined || !/^\d{1,2}$/.test(text) || n < 1) {
    throw new Error(`a work folder number is 1-99, not "${String(text)}"`);
  }
  return n;
}

/**
 * The message for a development command started in the master, or
 * `undefined` when the checkout is not the master.
 *
 * @param {string} root the checkout's root
 * @param {string} what the refused action, e.g. "npm run check"
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {string | undefined}
 */
export function masterRefusal(root, what, env = process.env) {
  if (!isMaster(root, env)) return undefined;
  return (
    `${what}: refused in ${root}.\n` +
    `This checkout is the master repository (NAIL_CHECKOUT_ROLE=master, ${LOCAL_ENV}): it only\n` +
    "receives pushes and fast-forward merges. Nothing is built, tested, run or committed here.\n" +
    `Work in a work folder (${folderOf(1, localSettings(root, env))} and up); create one with\n` +
    "`node scripts/work-folders.mjs create <n>` from an existing work folder\n" +
    '(docs/dev-setup.md, "Work folders").'
  );
}

/** The repository's git hooks (pre-commit, pre-push; ADR 0123), relative to a checkout. */
export const HOOKS_DIR = ".githooks";

/** The `git` arguments that point a checkout at {@link HOOKS_DIR}. */
export const HOOKS_PATH_ARGS = ["config", "core.hooksPath", HOOKS_DIR];

/** The commit identity a work folder copies from the master (ADR 0198). */
export const IDENTITY_KEYS = ["user.name", "user.email"];

/**
 * The `git config` calls that give a new work folder the master's own
 * commit identity (the public one, ADR 0198), never the machine's global
 * name and address: one per key the master sets locally.
 *
 * @param {(key: string) => string | undefined} masterValue the master's local value of a key
 * @returns {string[][]}
 */
export function identityConfig(masterValue) {
  return IDENTITY_KEYS.flatMap((key) => {
    const value = masterValue(key);
    return value ? [["config", key, value]] : [];
  });
}

/** Root of the checkout this module belongs to. */
export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * The `origin` of the checkout at `root` (a URL or a local path), or
 * `undefined` when it has none.
 *
 * @param {string} [root]
 * @returns {string | undefined}
 */
export function originOf(root = REPO_ROOT) {
  try {
    const url = execFileSync("git", ["remote", "get-url", "origin"], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    return url.trim() || undefined;
  } catch {
    return undefined;
  }
}

/**
 * Exits with an explanation when the checkout at `root` is the master.
 *
 * @param {string} what the refused action, e.g. "npm run check"
 * @param {string} [root]
 */
export function assertWorkFolder(what, root = REPO_ROOT) {
  const refusal = masterRefusal(root, what);
  if (refusal === undefined) return;
  console.error(refusal);
  process.exit(1);
}

/**
 * The rustc wrapper the master's `.cargo/config.local.toml` names: a batch
 * file that refuses every compilation (also `cargo metadata`, which asks
 * rustc for its version through the wrapper).
 */
export const MASTER_RUSTC_WRAPPER = [
  "@echo off",
  "echo error: this checkout is the Nail master repository: nothing is built here. 1>&2",
  "echo Work in a work folder (docs/dev-setup.md, Work folders). 1>&2",
  "exit /b 1",
  "",
].join("\r\n");

/**
 * The master's `.cargo/config.local.toml` (gitignored; the committed
 * `.cargo/config.toml` includes it).
 *
 * @param {string} wrapper path of the wrapper, relative to the checkout root
 * @returns {string}
 */
export function masterCargoConfig(wrapper) {
  return [
    "# Local only (gitignored), written by `node scripts/work-folders.mjs guard-master`.",
    "# This checkout is the master repository: nothing is built here, so every rustc call",
    "# is refused. Work in a work folder (docs/dev-setup.md, Work folders).",
    "[build]",
    `rustc-wrapper = ${JSON.stringify(wrapper.replaceAll("\\", "/"))}`,
    "",
  ].join("\n");
}

/**
 * The local (gitignored) `.claude/settings.local.json` of a work folder:
 * Claude Code's memory lives in the checkout (`.claude/memory`).
 *
 * @param {string} folder the work folder's absolute path
 * @returns {string}
 */
export function claudeLocalSettings(folder) {
  const memory = path.join(folder, ".claude", "memory").replaceAll("\\", "/");
  return `${JSON.stringify({ autoMemoryDirectory: memory }, null, 2)}\n`;
}
