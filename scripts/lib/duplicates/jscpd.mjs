// Runs jscpd (the copy/paste detector, MIT; jscpd 5 is a self-contained Rust
// binary installed by npm) and reads its JSON report (wave Q2, ADR 0171).
// Tests in scripts/test/duplicates.test.mjs.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { readJson } from "../json.mjs";
import { IGNORES, JSCPD } from "./config.mjs";

/**
 * @typedef {object} ClonePart
 * @property {string} name file (the report's separators)
 * @property {number} start first line
 * @property {number} end last line
 *
 * @typedef {object} Clone
 * @property {string} format
 * @property {number} lines
 * @property {number} tokens
 * @property {string} fragment
 * @property {boolean | null} [isNew] set when a baseline was given
 * @property {ClonePart} firstFile
 * @property {ClonePart} secondFile
 *
 * @typedef {{ duplicates: Clone[], statistics: { total: { sources: number, lines: number, duplicatedLines: number, clones: number } } }} Report
 */

/**
 * jscpd's command line for one scan.
 *
 * `ignore: false` drops the repository's ignore list and `.gitignore` (for
 * the CSS-in-TS copies, which live in the gitignored build directory).
 *
 * @param {{ paths: string[], out: string, baseline?: string, formats?: string[], ignore?: boolean, updateBaseline?: boolean }} options
 * @returns {string[]}
 */
export function jscpdArgs(options) {
  const args = [
    ...options.paths,
    "--min-tokens",
    String(JSCPD.minTokens),
    "--min-lines",
    String(JSCPD.minLines),
    "--mode",
    JSCPD.mode,
    "--format",
    (options.formats ?? JSCPD.formats).join(","),
    "--reporters",
    "json",
    "--output",
    options.out,
    "--no-tips",
    "--no-colors",
    "--silent",
  ];
  if (options.ignore === false) args.push("--no-gitignore");
  else args.push("--ignore", IGNORES.map((i) => i.glob).join(","));
  if (options.baseline !== undefined) {
    args.push("--baseline", options.baseline);
    if (options.updateBaseline === true) args.push("--update-baseline");
  }
  return args;
}

/**
 * The jscpd launcher script of the installed npm package.
 *
 * @param {string} root
 */
export function jscpdScript(root) {
  const dir = path.join(root, "node_modules", "jscpd");
  const manifest = /** @type {{ bin?: Record<string, string> }} */ (
    readJson(path.join(dir, "package.json"))
  );
  const bin = manifest.bin?.jscpd;
  if (bin === undefined) throw new Error("jscpd has no bin; run npm ci");
  return path.join(dir, bin);
}

/**
 * Runs one jscpd scan in `cwd` and returns its report.
 *
 * @param {string} root the checkout (for node_modules)
 * @param {string} cwd where the scanned paths are relative to
 * @param {Parameters<typeof jscpdArgs>[0]} options
 * @returns {Report}
 */
export function runJscpd(root, cwd, options) {
  fs.mkdirSync(options.out, { recursive: true });
  const report = path.join(options.out, "jscpd-report.json");
  fs.rmSync(report, { force: true });
  const result = spawnSync(process.execPath, [jscpdScript(root), ...jscpdArgs(options)], {
    cwd,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  });
  if (result.status !== 0 || !fs.existsSync(report)) {
    throw new Error(
      `jscpd failed (exit ${String(result.status)}): ${result.stderr || result.stdout || String(result.error)}`,
    );
  }
  return /** @type {Report} */ (readJson(report));
}

/**
 * A clone's place as `file:start-end` with `/` separators.
 *
 * @param {ClonePart} part
 * @param {(name: string) => string} [mapName] maps a scanned file back to the repository file
 */
export function clonePlace(part, mapName = (n) => n) {
  return `${mapName(part.name.split("\\").join("/"))}:${String(part.start)}-${String(part.end)}`;
}
