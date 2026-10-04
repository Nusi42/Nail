#!/usr/bin/env node
/**
 * The whole performance check (plan M4c WP123; the CI job `perf`):
 *
 *   1. `nail-bench run` — generates the corpus, syncs it from the mock IMAP
 *      server and measures the Rust metrics (report-rust.json);
 *   2. the Playwright perf scenarios on the synced data (report-ui.json);
 *   3. `nail-bench check` — merges both reports and checks
 *      e2e/perf/budgets.json (report.json); its exit code is the result.
 *
 * Usage: node e2e/perf/run-perf.mjs [--corpus standard|small] [--tolerant]
 * Needs `cargo build --release -p nail-cli -p nail-bench` and
 * `npm run build -w ui` first (binaries: NAIL_BIN / NAIL_BENCH_BIN or
 * cargo's release directory). Output goes to the build directory
 * (`%LOCALAPPDATA%/nail-build/perf/<corpus>`, NAIL_BUILD_DIR), never the
 * repository.
 */
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseJson } from "../../scripts/lib/json.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");

/**
 * @param {string[]} argv
 * @returns {{ corpus: string, tolerant: boolean }}
 */
export function parseArgs(argv) {
  const options = { corpus: "standard", tolerant: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    if (arg === "--tolerant") options.tolerant = true;
    else if (arg === "--corpus") options.corpus = argv[++i] ?? "";
    else throw new Error(`unknown argument ${arg}`);
  }
  if (!["standard", "small"].includes(options.corpus)) {
    throw new Error(`unknown corpus ${options.corpus}`);
  }
  return options;
}

function buildDir() {
  if (process.env.NAIL_BUILD_DIR) return process.env.NAIL_BUILD_DIR;
  return process.env.LOCALAPPDATA
    ? path.join(process.env.LOCALAPPDATA, "nail-build")
    : path.join(tmpdir(), "nail-build");
}

/**
 * A release binary: `$envName`, else cargo's release directory.
 *
 * @param {string} name
 * @param {string} envName
 * @returns {string}
 */
function release(name, envName) {
  const fromEnv = process.env[envName];
  if (fromEnv) return fromEnv;
  const metadata = /** @type {{ target_directory: string }} */ (
    parseJson(
      execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
        cwd: repoRoot,
        encoding: "utf8",
      }),
    )
  );
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const bin = path.join(metadata.target_directory, "release", exe);
  if (!existsSync(bin)) throw new Error(`${bin} is missing: build it with cargo --release`);
  return bin;
}

/**
 * Runs a command with inherited output; answers its exit code.
 *
 * @param {string} command
 * @param {string[]} args
 * @param {Record<string, string>} [env]
 */
function run(command, args, env = {}) {
  console.log(`> ${path.basename(command)} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
  return result.status ?? 1;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const work = path.join(buildDir(), "perf", options.corpus);
  const bench = release("nail-bench", "NAIL_BENCH_BIN");
  const budgets = path.join(repoRoot, "e2e", "perf", "budgets.json");
  const tolerant = options.tolerant ? ["--tolerant"] : [];
  let failed = false;
  // 1. Rust metrics (fresh data every time).
  if (
    run(bench, [
      "run",
      "--corpus",
      options.corpus,
      "--work-dir",
      work,
      "--budgets",
      budgets,
      "--no-check",
      ...tolerant,
    ]) !== 0
  ) {
    console.error("nail-bench run failed");
    return 2;
  }
  // 2. UI metrics on that data.
  const env = {
    NAIL_PERF_CORPUS: options.corpus,
    NAIL_PERF_WORK: work,
    NAIL_PERF_TOLERANT: options.tolerant ? "1" : "0",
    NAIL_BIN: release("nail", "NAIL_BIN"),
    NAIL_BENCH_BIN: bench,
  };
  // The Playwright CLI through node itself (no shell for `npx.cmd`).
  const playwright = path.join(repoRoot, "node_modules", "@playwright", "test", "cli.js");
  const config = "e2e/perf/playwright.perf.config.ts";
  if (run(process.execPath, [playwright, "test", "-c", config], env) !== 0) {
    console.error("the Playwright perf scenarios failed");
    failed = true;
  }
  // 3. Both reports against the budgets.
  const reports = ["report-rust.json", "report-ui.json"]
    .map((f) => path.join(work, f))
    .filter((f) => existsSync(f))
    .flatMap((f) => ["--report", f]);
  const status = run(bench, [
    "check",
    ...reports,
    "--budgets",
    budgets,
    "--out",
    path.join(work, "report.json"),
    ...tolerant,
  ]);
  return failed || status !== 0 ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 2;
  }
}
