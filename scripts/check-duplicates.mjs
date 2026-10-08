#!/usr/bin/env node
// The duplicate check (wave Q2, SSOT / ODR / DRY, ADR 0171): jscpd over the
// code and the CSS-in-TS, plus the SSOT checks (hand mirrors between Rust and
// TypeScript, constants defined twice, one English text under several Fluent
// ids, theme values in CSS-in-TS, repeated magic numbers). A step of
// `npm run check` and of the git / post-edit hooks.
//
// Ratchet: findings recorded in scripts/duplicates/*baseline.json (the state
// when Q2 started) are reported; new ones fail. The Q2 packages remove the
// recorded ones (`--owner WP-… --list` is a package's work list, `--strict
// --owner WP-…` its gate); the integrator empties the baselines, after which
// every finding fails. Exceptions with a reason: scripts/duplicates/allow.json.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { buildBase } from "./lib/build.mjs";
import { failing, parseArgs, USAGE } from "./lib/duplicates/cli.mjs";
import { collect } from "./lib/duplicates/index.mjs";
import { findingLine, listText, markdownBaseline, summaryText } from "./lib/duplicates/report.mjs";
import { assertWorkFolder } from "./lib/work-folder.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

assertWorkFolder("check-duplicates");

let options;
try {
  options = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(2);
}

// The hooks pass the edited / staged files: nothing to do without code.
if (process.argv.includes("--files") && options.files.length === 0) {
  console.log(USAGE);
  process.exit(0);
}

const result = collect(root, {
  buildDir: buildBase(process.env, root),
  updateBaseline: options.updateBaseline,
});
const bad = failing(result.findings, options);
const hook = options.files.length > 0;

if (options.json !== undefined) {
  fs.writeFileSync(options.json, `${JSON.stringify(result.findings, null, 2)}\n`);
}
if (options.markdown !== undefined) {
  fs.writeFileSync(options.markdown, `${markdownBaseline(result.findings, result.stats)}\n`);
}
if (!hook) {
  console.log(
    `duplicates: findings per check (known + new; per package)\n${summaryText(result.findings)}`,
  );
}
if (options.list) console.log(listText(result.findings, options.owner));
for (const problem of result.problems) console.error(`duplicates: ${problem}`);
if (bad.length > 0) {
  console.error(
    `\nduplicates: ${String(bad.length)} finding(s) fail${options.strict ? " (--strict)" : " (new since the baseline)"}:`,
  );
  for (const f of bad.slice(0, 50)) console.error(`  ${findingLine(f)}`);
  if (bad.length > 50) console.error(`  … and ${String(bad.length - 50)} more (--list)`);
  console.error(
    "Fix: share the code / fact (SSOT, ODR, DRY; ADR 0171). `node scripts/check-duplicates.mjs --list` explains each check.",
  );
}
if (options.updateBaseline) console.log("duplicates: baselines rewritten (scripts/duplicates/)");
process.exitCode = bad.length > 0 || result.problems.length > 0 ? 1 : 0;
