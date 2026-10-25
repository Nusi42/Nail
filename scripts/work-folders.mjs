#!/usr/bin/env node
/**
 * Work folders (ADRs 0132, 0199; helpers in scripts/lib/work-folder.mjs).
 * Optional: only for a maintainer who keeps a master checkout. Run from an
 * existing work folder; the master is its `origin`.
 *
 *   node scripts/work-folders.mjs guard-master
 *       mark the `origin` checkout as the master (NAIL_CHECKOUT_ROLE=master in
 *       its gitignored .env.local) and make Cargo refuse every build there (a
 *       rustc wrapper in its gitignored .cargo/config.local.toml)
 *   node scripts/work-folders.mjs create <n> [--no-install]
 *       clone the master into <parent of this folder>/<prefix><n> on branch
 *       <branch prefix><n> (default nail-work-<N> on work-<N>; taken from the
 *       master when it exists, else created from `main` and pushed), give it
 *       the master's commit identity (ADR 0198), write its
 *       .claude/settings.local.json, `npm ci`, `npm run build`
 */
import { execFileSync, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  HOOKS_PATH_ARGS,
  LOCAL_ENV,
  MASTER_RUSTC_WRAPPER,
  REPO_ROOT,
  assertWorkFolder,
  branchOf,
  claudeLocalSettings,
  folderOf,
  identityConfig,
  isMaster,
  originOf,
  localSettings,
  markedAsMaster,
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

/**
 * A key of the repository's own config, if it sets one.
 *
 * @param {string} cwd
 * @param {string} key
 * @returns {string | undefined}
 */
function localValue(cwd, key) {
  try {
    return git(cwd, ["config", "--local", "--get", key]) || undefined;
  } catch {
    return undefined;
  }
}

/** This work folder's `origin`, which must be a local checkout. */
function originPath() {
  const url = originOf(REPO_ROOT);
  if (url === undefined) throw new Error(`${REPO_ROOT} has no origin`);
  if (!fs.existsSync(path.join(url, ".git"))) {
    throw new Error(`origin of ${REPO_ROOT} is ${url}, not a local checkout`);
  }
  return url;
}

/** The master repository: this work folder's `origin`, marked as the master. */
function masterPath() {
  const url = originPath();
  if (!isMaster(url)) {
    throw new Error(
      `origin of ${REPO_ROOT} is ${url}, which is not marked as the master ` +
        "(run `node scripts/work-folders.mjs guard-master` first)",
    );
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
  const settings = localSettings(master);
  const target = path.join(path.dirname(REPO_ROOT), folderOf(n, settings));
  if (fs.existsSync(target)) throw new Error(`${target} exists already`);
  const branch = branchOf(n, settings);
  execFileSync("git", ["clone", master, target], { stdio: "inherit" });
  for (const args of identityConfig((key) => localValue(master, key))) git(target, args);
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
    git(target, HOOKS_PATH_ARGS);
  }
  console.log(`${target}: branch ${branch}${known ? " (from the master)" : " (new, from main)"}`);
}

/** Marks `origin` as the master and makes Cargo refuse to build there. */
function guardMaster() {
  const master = originPath();
  const env = path.join(master, LOCAL_ENV);
  fs.writeFileSync(env, markedAsMaster(fs.existsSync(env) ? fs.readFileSync(env, "utf8") : ""));
  const cargoDir = path.join(master, ".cargo");
  fs.mkdirSync(cargoDir, { recursive: true });
  const wrapper = path.join(cargoDir, "master-guard.cmd");
  fs.writeFileSync(wrapper, MASTER_RUSTC_WRAPPER);
  const config = path.join(cargoDir, "config.local.toml");
  fs.writeFileSync(config, masterCargoConfig(path.relative(master, wrapper)));
  console.log(`${master}: marked as the master (${env}); every rustc call there is refused`);
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
