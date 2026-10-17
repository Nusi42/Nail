/**
 * Master repository and work folders (ADR-WORK-FOLDERS): the checkout in
 * OneDrive is the master. It holds `main`, receives pushes and fast-forward
 * merges, and nothing is built, tested, run or committed in it. All work
 * happens in clones outside OneDrive, `<base>\nail-work-<N>` on branch
 * `work-<N>` (`<work root>\nail-work-1` for the lead and
 * integration, `work-2` and up for builders).
 *
 * The development entry points (`npm run dev` / `check` / `test` / `e2e` /
 * `visual`, `npm run tauri`, the pre-commit hook, `npm ci`'s `prepare`) call
 * {@link assertWorkFolder}; in the master Cargo refuses through a rustc
 * wrapper (`node scripts/work-folders.mjs guard-master`). Pure helpers here;
 * the CLI is scripts/work-folders.mjs, the tests
 * scripts/test/work-folder.test.mjs.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Branch of work folder `n`: `work-<N>`.
 *
 * @param {number} n
 * @returns {string}
 */
export function branchOf(n) {
  return `WIN${String(n)}`;
}

/**
 * Folder name of work folder `n`: `nail-work-<N>`.
 *
 * @param {number} n
 * @returns {string}
 */
export function folderOf(n) {
  return `nail.win${String(n)}`;
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
 * Whether `dir` lies in a OneDrive folder (a path segment starting with
 * "OneDrive", as in `C:\Users\x\OneDrive` or `OneDrive - Company`).
 *
 * @param {string} dir
 * @returns {boolean}
 */
export function isInOneDrive(dir) {
  return path
    .resolve(dir)
    .split(/[\\/]/)
    .some((segment) => /^onedrive\b/i.test(segment));
}

/**
 * The message for a development command started in the master, or
 * `undefined` when `root` is a work folder.
 *
 * @param {string} root the checkout's root
 * @param {string} what the refused action, e.g. "npm run check"
 * @returns {string | undefined}
 */
export function masterRefusal(root, what) {
  if (!isInOneDrive(root)) return undefined;
  return (
    `${what}: refused in ${root}.\n` +
    "This checkout lies in OneDrive: it is the master repository, which only receives pushes and\n" +
    "fast-forward merges. Nothing is built, tested, run or committed here.\n" +
    `Work in a clone outside OneDrive, e.g. <work root>\\${folderOf(1)} (branch ${branchOf(1)});\n` +
    "create one with `node scripts/work-folders.mjs create <n>` from an existing work folder\n" +
    '(docs/dev-setup.md, "Work folders").'
  );
}

/** Root of the checkout this module belongs to. */
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

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

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
  "echo error: this checkout is the Nail master repository (OneDrive): nothing is built here. 1>&2",
  "echo Work in a clone outside OneDrive, e.g. <work root>\\nail-work-1 (docs/dev-setup.md, Work folders). 1>&2",
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
    "# This checkout is the master repository (OneDrive): nothing is built here, so every",
    "# rustc call is refused. Work in a clone outside OneDrive (docs/dev-setup.md, Work folders).",
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
