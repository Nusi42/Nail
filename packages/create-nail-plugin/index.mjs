#!/usr/bin/env node
/**
 * create-nail-plugin — scaffolds a Nail plug-in folder (English output; a
 * developer tool, not part of Nail's UI).
 *
 *   node packages/create-nail-plugin/index.mjs <dir> [--id ID] [--name NAME] [--sdk SPEC] [--force]
 *
 * `--sdk` is the version spec written for `@nail/plugin-api` in the new
 * package.json; the default is a `file:` path to this repository's SDK.
 */
import path from "node:path";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { scaffold, ScaffoldError } from "./scaffold.mjs";

const USAGE = "usage: create-nail-plugin <dir> [--id ID] [--name NAME] [--sdk SPEC] [--force]";

/**
 * Runs the CLI.
 * @param {string[]} argv arguments after the program name
 * @param {{ log: (s: string) => void, error: (s: string) => void }} [io]
 * @returns {number} exit code
 */
export function main(argv, io = console) {
  /** @type {import("./scaffold.mjs").ScaffoldOptions} */
  const options = { dir: "" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i] ?? "";
    const value = argv[i + 1];
    if (arg === "--force") options.force = true;
    else if ((arg === "--id" || arg === "--name" || arg === "--sdk") && value !== undefined) {
      if (arg === "--id") options.id = value;
      else if (arg === "--name") options.name = value;
      else options.sdk = value;
      i += 1;
    } else if (!arg.startsWith("--") && !options.dir) options.dir = arg;
    else {
      io.error(USAGE);
      return 2;
    }
  }
  if (!options.dir) {
    io.error(USAGE);
    return 2;
  }
  try {
    const result = scaffold(options);
    const rel = path.relative(process.cwd(), result.dir) || ".";
    io.log(`Created ${result.name} (${result.id}) in ${rel}:`);
    for (const file of result.files) io.log(`  ${file}`);
    io.log("");
    io.log("Next steps:");
    io.log(`  cd ${rel}`);
    io.log("  npm install");
    io.log(`  npm run package      # -> dist/${result.id}.nailplugin`);
    io.log("  nail plugin dev .    # or load it unpacked, with hot reload (npm run watch)");
    return 0;
  } catch (error) {
    if (error instanceof ScaffoldError) {
      io.error(error.message);
      return 1;
    }
    throw error;
  }
}

/** Whether this module is the program (also through npm's bin links). */
function isMain() {
  try {
    return realpathSync(process.argv[1] ?? "") === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isMain()) {
  process.exitCode = main(process.argv.slice(2));
}
