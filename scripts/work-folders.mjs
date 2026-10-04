#!/usr/bin/env node
/**
 * Work folders (ADR-WORK-FOLDERS; helpers in scripts/lib/work-folder.mjs).
 * Run from an existing work folder; the master is its `origin`.
 *
 *   node scripts/work-folders.mjs create <n> [--no-install]
 *       clone the master into <parent of this folder>\nail-work-<N> on branch
 *       work-<N> (taken from the master when it exists, else created from
 *       `main` and pushed), write its .claude/settings.local.json, `npm ci`,
 *       `npm run build`
 *   node scripts/work-folders.mjs guard-master
 *       make Cargo refuse every build in the master (a rustc wrapper in its
 *       gitignored .cargo/config.local.toml)
 */
import { execFileSync, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  MASTER_RUSTC_WRAPPER,
  REPO_ROOT,
  assertWorkFolder,
  branchOf,
  claudeLocalSettings,
  folderOf,
  isInOneDrive,
  masterCargoConfig,
  parseFolderNumber,
} from "./lib/work-folder.mjs";

/**
 * Runs git and returns its trimmed output.
 *
 * @param {string} cwd
 * @param {string[]} args
 * @returns {string}
 */
function git(cwd, args) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

/** The master repository: this work folder's `origin`. */
function masterPath() {
  const url = git(REPO_ROOT, ["remote", "get-url", "origin"]);
  if (!isInOneDrive(url) || !fs.existsSync(path.join(url, ".git"))) {
    throw new Error(`origin of ${REPO_ROOT} is ${url}, not the master checkout in OneDrive`);
  }
  return url;
}

/**
 * Creates work folder `n` next to this one.
 *
 * @param {number} n
 * @param {boolean} install
 */
function create(n, install) {
  const master = masterPath();
  const target = path.join(path.dirname(REPO_ROOT), folderOf(n));
  if (fs.existsSync(target)) throw new Error(`${target} exists already`);
  const branch = branchOf(n);
  execFileSync("git", ["clone", master, target], { stdio: "inherit" });
  const known = git(target, ["branch", "-r", "--list", `origin/${branch}`]) !== "";
  if (known) {
    git(target, ["switch", branch]);
  } else {
    git(target, ["switch", "-c", branch]);
    git(target, ["push", "-u", "origin", branch]);
  }
  fs.writeFileSync(
    path.join(target, ".claude", "settings.local.json"),
    claudeLocalSettings(target),
  );
  if (install) {
    // `prepare` points core.hooksPath at .githooks.
    // One command string: npm is a batch file on Windows, which needs a shell.
    execSync("npm ci --no-audit --no-fund", { cwd: target, stdio: "inherit" });
    // ui/dist: the Tauri shell embeds it, so `cargo … --workspace` (and the
    // pre-push `npm run check`) needs it in a fresh folder.
    execSync("npm run build", { cwd: target, stdio: "inherit" });
  } else {
    git(target, ["config", "core.hooksPath", ".githooks"]);
  }
  console.log(`${target}: branch ${branch}${known ? " (from the master)" : " (new, from main)"}`);
}

/** Makes Cargo refuse to build in the master. */
function guardMaster() {
  const master = masterPath();
  const cargoDir = path.join(master, ".cargo");
  fs.mkdirSync(cargoDir, { recursive: true });
  const wrapper = path.join(cargoDir, "master-guard.cmd");
  fs.writeFileSync(wrapper, MASTER_RUSTC_WRAPPER);
  const config = path.join(cargoDir, "config.local.toml");
  fs.writeFileSync(config, masterCargoConfig(path.relative(master, wrapper)));
  console.log(`${config}: every rustc call in the master is refused (${wrapper})`);
}

assertWorkFolder("node scripts/work-folders.mjs");
const [command, arg, ...rest] = process.argv.slice(2);
try {
  if (command === "create") {
    create(parseFolderNumber(arg), !rest.includes("--no-install"));
  } else if (command === "guard-master" && arg === undefined) {
    guardMaster();
  } else {
    console.error("usage: node scripts/work-folders.mjs create <n> [--no-install] | guard-master");
    process.exit(2);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
