#!/usr/bin/env node
/**
 * `nail-plugin` — developer tool of `@nail/plugin-api` (English output; a
 * developer tool, not part of Nail's UI):
 *
 *   nail-plugin check [dir]              check manifest.json
 *   nail-plugin pack [dir] [--out FILE]  pack a built plug-in into a .nailplugin
 */
import path from "node:path";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { PackError, packPlugin, readManifest } from "./pack.mjs";

const USAGE = "usage: nail-plugin check [dir] | nail-plugin pack [dir] [--out FILE]";

/**
 * Runs the CLI.
 * @param {string[]} argv arguments after the program name
 * @param {{ log: (s: string) => void, error: (s: string) => void }} [io]
 * @returns {number} exit code
 */
export function main(argv, io = console) {
  const [command, ...rest] = argv;
  let dir = ".";
  /** @type {string | undefined} */
  let out;
  for (let i = 0; i < rest.length; i += 1) {
    const arg = rest[i] ?? "";
    if (arg === "--out" && rest[i + 1] !== undefined) {
      out = rest[i + 1];
      i += 1;
    } else if (!arg.startsWith("--")) {
      dir = arg;
    } else {
      io.error(USAGE);
      return 2;
    }
  }
  try {
    if (command === "check") {
      const { manifest } = readManifest(dir);
      io.log(`${manifest.id} ${manifest.version}: manifest.json looks fine`);
      return 0;
    }
    if (command === "pack") {
      const result = packPlugin({ dir, out });
      io.log(path.relative(process.cwd(), result.file) || result.file);
      io.log(`  ${result.files.length} files, ${result.size} bytes, sha256 ${result.sha256}`);
      return 0;
    }
  } catch (error) {
    if (error instanceof PackError) {
      io.error(error.message);
      return 1;
    }
    throw error;
  }
  io.error(USAGE);
  return 2;
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
