#!/usr/bin/env node
// `prepare` (npm install / npm ci): points git at the repository's hooks in
// .githooks (the pre-commit checks, ADR 0123). Does nothing outside a git
// checkout (an unpacked tarball) or when git is not installed.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { HOOKS_PATH_ARGS, assertWorkFolder } from "./lib/work-folder.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// `npm ci` in the master would fill OneDrive with node_modules.
assertWorkFolder("npm install / npm ci");
try {
  execFileSync("git", ["rev-parse", "--git-dir"], { cwd: root, stdio: "ignore" });
} catch {
  process.exit(0);
}
execFileSync("git", HOOKS_PATH_ARGS, { cwd: root, stdio: "inherit" });
