/**
 * Runs the commands of the build and release scripts, one way: the
 * command is echoed, its output inherited, a failure throws. A child gets
 * the environment without the update key unless the caller passes one
 * explicitly (docs/release.md "Who sees the private key"). npm / npx are
 * `.cmd` files on Windows and run through a shell, which takes one string.
 */
import { execFileSync } from "node:child_process";

import { withoutUpdaterSecrets } from "./build.mjs";
import { repoRoot } from "./program.mjs";

/** Whether commands run through a shell by default (Windows). */
export const SHELL = process.platform === "win32";

/**
 * Quotes an argument for cmd.exe when needed.
 *
 * @param {string} arg
 */
export function quote(arg) {
  return /[\s"&|<>^]/.test(arg) ? `"${arg.replaceAll('"', '""')}"` : arg;
}

/**
 * The spawn options of `command`: the checkout as working directory, the
 * environment without the update key unless `env` is given.
 *
 * @param {{ cwd?: string, env?: NodeJS.ProcessEnv, shell?: boolean }} options
 * @param {import("node:child_process").StdioOptions} stdio
 */
function spawnOptions(options, stdio) {
  return {
    cwd: options.cwd ?? repoRoot,
    stdio,
    env: options.env ?? withoutUpdaterSecrets(process.env),
    shell: options.shell ?? SHELL,
  };
}

/**
 * Runs a command, its output shown; throws when it fails.
 *
 * @param {string} command
 * @param {string[]} args
 * @param {{ cwd?: string, env?: NodeJS.ProcessEnv, shell?: boolean }} [options]
 */
export function run(command, args, options = {}) {
  console.log(`> ${command} ${args.join(" ")}`);
  const spawn = spawnOptions(options, "inherit");
  if (spawn.shell) execFileSync([command, ...args].map(quote).join(" "), spawn);
  else execFileSync(command, args, spawn);
}

/**
 * Runs a command and answers its standard output (trimmed); throws when
 * it fails.
 *
 * @param {string} command
 * @param {string[]} args
 * @param {{ cwd?: string, env?: NodeJS.ProcessEnv, shell?: boolean }} [options]
 */
export function output(command, args, options = {}) {
  const spawn = {
    ...spawnOptions(options, ["ignore", "pipe", "inherit"]),
    encoding: /** @type {const} */ ("utf8"),
  };
  const text = spawn.shell
    ? execFileSync([command, ...args].map(quote).join(" "), spawn)
    : execFileSync(command, args, spawn);
  return text.trim();
}
