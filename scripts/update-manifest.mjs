#!/usr/bin/env node
/**
 * Writes the signed update manifest of a release (plan M4b WP112; format
 * in scripts/lib/update-manifest.mjs):
 *
 *   node scripts/update-manifest.mjs --version 0.2.0 \
 *     --artifacts <dir> [--artifacts <dir> …] --download-base <url> \
 *     --out <dir> [--notes <file.md>] [--pub-date <RFC 3339>]
 *
 * The update key's public half comes from NAIL_UPDATER_PUBKEY or
 * NAIL_UPDATER_PUBKEY_FILE (or --pubkey-file). Every installer's `.sig` is
 * checked against it and against the version before anything is written.
 * Output: <out>/latest.json. Publish it at the base URL the app was built
 * with (--update-base; a release: an asset of the version's GitHub
 * release, ADR 0210).
 */
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

import {
  MANIFEST_FILE,
  buildManifest,
  collectPlatforms,
  manifestJson,
  verifyPlatforms,
} from "./lib/update-manifest.mjs";
import { forEachArg } from "./lib/args.mjs";
import { runMain } from "./lib/program.mjs";

/**
 * @param {string[]} argv
 */
function parse(argv) {
  /**
   * @type {{ artifacts: string[], version?: string, downloadBase?: string,
   *   out?: string, notes?: string, pubDate?: string, pubkeyFile?: string }}
   */
  const o = { artifacts: [] };
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--artifacts") o.artifacts.push(value());
    else if (arg === "--version") o.version = value();
    else if (arg === "--download-base") o.downloadBase = value();
    else if (arg === "--out") o.out = value();
    else if (arg === "--notes") o.notes = value();
    else if (arg === "--pub-date") o.pubDate = value();
    else if (arg === "--pubkey-file") o.pubkeyFile = value();
    else throw new Error(`unknown option ${arg}`);
  });
  const { version, downloadBase, out } = o;
  /** @param {string} key */
  const missing = (key) =>
    new Error(`--${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)} is required`);
  if (!version) throw missing("version");
  if (!downloadBase) throw missing("downloadBase");
  if (!out) throw missing("out");
  if (!o.artifacts.length) throw new Error("--artifacts is required");
  return { ...o, version, downloadBase, out };
}

/**
 * Writes the manifest of a release (`<out>/latest.json`) after checking
 * every installer's signature; answers the file written.
 *
 * @param {{ version: string, artifacts: string[], downloadBase: string,
 *   out: string, notes: string, pubDate: string, publicKey: string }} o
 */
export function writeUpdateManifest(o) {
  /** @type {Map<string, string>} */
  const located = new Map();
  for (const dir of o.artifacts) {
    for (const name of readdirSync(dir)) {
      if (located.has(name)) throw new Error(`${name} is in two artifact folders`);
      located.set(name, path.join(dir, name));
    }
  }
  const files = [...located.keys()]
    .filter((name) => !name.endsWith(".sig"))
    .map((name) => {
      const sig = located.get(`${name}.sig`);
      return { name, signature: sig ? readFileSync(sig, "utf8") : null };
    });
  const entries = collectPlatforms({ files, version: o.version, downloadBase: o.downloadBase });
  verifyPlatforms({
    entries,
    publicKey: o.publicKey,
    version: o.version,
    read: (name) => readFileSync(located.get(name) ?? name),
  });
  const manifest = buildManifest({
    version: o.version,
    notes: o.notes,
    pubDate: o.pubDate,
    entries,
  });
  const file = path.join(o.out, MANIFEST_FILE);
  mkdirSync(o.out, { recursive: true });
  writeFileSync(file, manifestJson(manifest));
  console.log(`${file}: ${Object.keys(manifest.platforms).join(", ")}`);
  return file;
}

/** @param {string[]} argv */
function main(argv) {
  const o = parse(argv);
  const pubkeyFile = o.pubkeyFile ?? process.env.NAIL_UPDATER_PUBKEY_FILE;
  // An empty NAIL_UPDATER_PUBKEY counts as unset.
  let publicKey = process.env.NAIL_UPDATER_PUBKEY;
  if (!publicKey && pubkeyFile) publicKey = readFileSync(pubkeyFile, "utf8");
  if (!publicKey) throw new Error("no update public key (NAIL_UPDATER_PUBKEY or --pubkey-file)");
  writeUpdateManifest({
    ...o,
    notes: o.notes ? readFileSync(o.notes, "utf8") : "",
    pubDate: o.pubDate ?? new Date().toISOString(),
    publicKey,
  });
}

await runMain(import.meta.url, main);
