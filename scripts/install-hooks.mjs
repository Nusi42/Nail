#!/usr/bin/env node
// `prepare` (npm install / npm ci): points git at the repository's hooks in
// .githooks (the pre-commit checks, ADR 0123). Does nothing outside a git
// checkout (an unpacked tarball) or when git is not installed.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
try {
  execFileSync("git", ["rev-parse", "--git-dir"], { cwd: root, stdio: "ignore" });
} catch {
  process.exit(0);
}
execFileSync("git", ["config", "core.hooksPath", ".githooks"], { cwd: root, stdio: "inherit" });
