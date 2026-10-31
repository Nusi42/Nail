#!/usr/bin/env node
// The project notes carry no session data (scripts/lib/notes.mjs, ADR 0200).
//
//   node scripts/check-notes.mjs                 every tracked note (`npm run check`)
//   node scripts/check-notes.mjs <file>...       these files (pre-commit, post-edit)
//   node scripts/check-notes.mjs --fix [<file>...]  remove the keys Claude Code adds instead
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { NOTES, SESSION_KEY, hasSessionLine, hasToolKeys, withoutToolKeys } from "./lib/notes.mjs";
import { repoRoot, runMain } from "./lib/program.mjs";

/** @param {string[]} argv */
function main(argv) {
  const fix = argv.includes("--fix");
  const named = argv.filter((a) => a !== "--fix");
  const files =
    named.length > 0
      ? named
      : execFileSync("git", ["ls-files", "-z"], { cwd: repoRoot, encoding: "utf8" })
          .split("\0")
          .filter((f) => NOTES.test(f));
  /** @type {string[]} */
  const found = [];
  for (const file of files) {
    const full = path.resolve(repoRoot, file);
    if (!fs.existsSync(full)) continue;
    const text = fs.readFileSync(full, "utf8");
    if (fix) {
      if (hasToolKeys(text)) fs.writeFileSync(full, withoutToolKeys(text));
    } else if (hasSessionLine(text)) {
      found.push(file);
    }
  }
  if (found.length === 0) {
    console.log(`notes: ${String(files.length)} notes without session data`);
    return 0;
  }
  for (const file of found) {
    console.error(`${file}: an ${SESSION_KEY} line (session data; remove it or run with --fix)`);
  }
  return 1;
}

await runMain(import.meta.url, main);
