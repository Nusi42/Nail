#!/usr/bin/env node
// Checks files against .editorconfig (ADR 0123): the given files, or every
// file git tracks. Exit code 1 when a file does not conform.
//
//   node scripts/check-editorconfig.mjs [file...]
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkFiles } from "./lib/editorconfig-check.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const files =
  args.length > 0
    ? args
    : execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8" })
        .split("\0")
        .filter(Boolean);

const problems = checkFiles(files, root);
for (const problem of problems) console.error(problem);
if (problems.length > 0) {
  console.error(
    `editorconfig: ${String(problems.length)} problem(s) in ${String(files.length)} files`,
  );
  process.exitCode = 1;
} else {
  console.log(`editorconfig: ${String(files.length)} files OK`);
}
