#!/usr/bin/env node
/**
 * Asks a throw-away `nail serve --seed demo-multi` which themes exist
 * (`ListThemes`, contract ADR 0102) and prints the raw answer as one JSON
 * line on stdout: `{"answer": <Response>}`, or `{"error": "<why>"}` when
 * the server could not be started or reached.
 *
 * The visual config runs this synchronously while Playwright collects the
 * tests, so the matrix follows the core's theme list (lib/matrix.ts).
 *
 * Usage: node probe-themes.mjs --bin <nail> --ui-dir <dir>
 * The server binds 127.0.0.1 on a free port (`--port 0`); its temporary
 * data and config directories are removed afterwards.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { forEachArg } from "../../../scripts/lib/args.mjs";
import { isMain } from "../../../scripts/lib/program.mjs";
import { coreRequest, spawnServe } from "../../support/serve.mjs";

/** @typedef {{ bin: string, uiDir: string, timeoutMs: number }} Options */

/**
 * @param {string[]} argv
 * @returns {Options}
 */
export function parseArgs(argv) {
  const options = { bin: "", uiDir: "", timeoutMs: 60_000 };
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--bin") options.bin = value();
    else if (arg === "--ui-dir") options.uiDir = value();
    else if (arg === "--timeout-ms") options.timeoutMs = Number(value());
    else throw new Error(`unknown argument ${arg}`);
  });
  if (!options.bin) throw new Error("--bin is required");
  if (!options.uiDir) throw new Error("--ui-dir is required");
  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }
  return options;
}

export { coreRequest as rpc };

/** @param {Options} options */
async function probe(options) {
  const dataDir = mkdtempSync(path.join(tmpdir(), "nail-visual-probe-data-"));
  const configDir = mkdtempSync(path.join(tmpdir(), "nail-visual-probe-config-"));
  const dirs = [dataDir, configDir];
  const args = [
    "serve",
    "--port",
    "0",
    "--seed",
    "demo-multi",
    "--data-dir",
    dataDir,
    "--config-dir",
    configDir,
    "--ui-dir",
    options.uiDir,
  ];
  try {
    const serve = await spawnServe(options.bin, args, {
      timeoutMs: options.timeoutMs,
      stderr: "ignore",
    });
    try {
      return { answer: await coreRequest(serve.url, { type: "ListThemes" }, options.timeoutMs) };
    } finally {
      await serve.stop();
    }
  } finally {
    for (const dir of dirs) {
      try {
        rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      } catch {
        // Best effort: the OS cleans the temp directory eventually.
      }
    }
  }
}

if (isMain(import.meta.url)) {
  let result;
  try {
    result = await probe(parseArgs(process.argv.slice(2)));
  } catch (error) {
    result = { error: error instanceof Error ? error.message : String(error) };
  }
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
