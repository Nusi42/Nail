#!/usr/bin/env node
/**
 * Writes the signed per-channel update manifests of a release (plan M4b
 * WP112; format in scripts/lib/update-manifest.mjs):
 *
 *   node scripts/update-manifest.mjs --version 0.2.0 --channel stable \
 *     --artifacts <dir> [--artifacts <dir> …] --download-base <url> \
 *     --out <dir> [--notes <file.md>] [--pub-date <RFC 3339>] \
 *     [--existing-beta <beta/latest.json>]
 *
 * The update key's public half comes from NAIL_UPDATER_PUBKEY or
 * NAIL_UPDATER_PUBKEY_FILE (or --pubkey-file). Every installer's `.sig` is
 * checked against it and against the version before anything is written.
 * Output: <out>/<channel>/latest.json (a stable release also refreshes
 * <out>/beta/latest.json unless the existing beta is newer). Publish the
 * folder at the base URL the app was built with (--update-base).
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

import {
  buildManifest,
  collectPlatforms,
  manifestJson,
  manifestsForRelease,
  verifyPlatforms,
} from "./lib/update-manifest.mjs";

function parse(argv) {
  const o = { artifacts: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined || v.startsWith("--")) throw new Error(`${arg} needs a value`);
      return v;
    };
    if (arg === "--version") o.version = value();
    else if (arg === "--channel") o.channel = value();
    else if (arg === "--artifacts") o.artifacts.push(value());
    else if (arg === "--download-base") o.downloadBase = value();
    else if (arg === "--out") o.out = value();
    else if (arg === "--notes") o.notes = value();
    else if (arg === "--pub-date") o.pubDate = value();
    else if (arg === "--existing-beta") o.existingBeta = value();
    else if (arg === "--pubkey-file") o.pubkeyFile = value();
    else throw new Error(`unknown option ${arg}`);
  }
  for (const key of ["version", "channel", "downloadBase", "out"]) {
    if (!o[key])
      throw new Error(`--${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)} is required`);
  }
  if (!o.artifacts.length) throw new Error("--artifacts is required");
  return o;
}

function main() {
  const o = parse(process.argv.slice(2));
  const pubkeyFile = o.pubkeyFile ?? process.env.NAIL_UPDATER_PUBKEY_FILE;
  const publicKey =
    process.env.NAIL_UPDATER_PUBKEY || (pubkeyFile && readFileSync(pubkeyFile, "utf8"));
  if (!publicKey) throw new Error("no update public key (NAIL_UPDATER_PUBKEY or --pubkey-file)");

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
    publicKey,
    version: o.version,
    read: (name) => readFileSync(located.get(name)),
  });
  const manifest = buildManifest({
    version: o.version,
    channel: o.channel,
    notes: o.notes ? readFileSync(o.notes, "utf8") : "",
    pubDate: o.pubDate ?? new Date().toISOString(),
    entries,
  });
  const existingBeta =
    o.existingBeta && existsSync(o.existingBeta)
      ? JSON.parse(readFileSync(o.existingBeta, "utf8"))
      : null;
  for (const { path: rel, manifest: m } of manifestsForRelease({
    manifest,
    channel: o.channel,
    existingBeta,
  })) {
    const file = path.join(o.out, ...rel.split("/"));
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, manifestJson(m));
    console.log(`${file}: ${Object.keys(m.platforms).join(", ")}`);
  }
}

main();
