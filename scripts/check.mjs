#!/usr/bin/env node
// `npm run check`: every formatter check and linter of the repository
// (quality baseline Q1, ADR 0123). Zero warnings, zero errors. Runs all
// steps (unless --fail-fast) and prints a summary; exit code 1 when any step
// failed or its tool is missing.
//
//   node scripts/check.mjs [--only a,b] [--skip a,b] [--advisories] [--fail-fast] [--list]
//
// Cargo runs with `-j $CARGO_BUILD_JOBS` (default 4; the machine is shared).
// `cargo deny` checks advisories only with --advisories or in CI (it fetches
// the RustSec database).
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkSteps,
  parseCheckArgs,
  pythonScriptDirs,
  resolveArgs,
  resolveTool,
  selectSteps,
  summarize,
} from "./lib/check-steps.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

let options;
try {
  options = parseCheckArgs(process.argv.slice(2));
} catch (error) {
  console.error(String(error instanceof Error ? error.message : error));
  process.exit(2);
}

const ci = Boolean(process.env.CI);
const steps = selectSteps(
  checkSteps({ ci, advisories: options.advisories, jobs: process.env.CARGO_BUILD_JOBS ?? "4" }),
  options,
);

if (options.list) {
  for (const step of steps) console.log(`${step.id.padEnd(16)} ${step.title}`);
  process.exit(0);
}

const extraDirs = pythonScriptDirs();
/** @type {import("./lib/check-steps.mjs").StepResult[]} */
const results = [];
for (const step of steps) {
  console.log(`\n=== ${step.id}: ${step.title}`);
  const started = performance.now();
  const executable =
    step.tool === "node" ? process.execPath : resolveTool(step.tool, { extraDirs });
  if (executable === null) {
    console.error(`${step.tool} not found. Install: ${step.install}`);
    results.push({
      id: step.id,
      title: step.title,
      status: "missing",
      seconds: 0,
      note: step.install,
    });
  } else {
    let args;
    try {
      args = resolveArgs(step.args, { root });
    } catch (error) {
      console.error(`${String(error)}. Install: ${step.install}`);
      results.push({
        id: step.id,
        title: step.title,
        status: "missing",
        seconds: 0,
        note: step.install,
      });
      continue;
    }
    const result = spawnSync(executable, args, {
      cwd: root,
      stdio: "inherit",
      env: { ...process.env, ...step.env },
    });
    const seconds = (performance.now() - started) / 1000;
    const ok = result.status === 0;
    results.push({ id: step.id, title: step.title, status: ok ? "ok" : "failed", seconds });
  }
  if (!options.keepGoing && results.at(-1)?.status !== "ok") break;
}

const { text, exitCode } = summarize(results);
console.log(`\n${text}`);
process.exitCode = exitCode;
