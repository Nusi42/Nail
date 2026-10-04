#!/usr/bin/env node
/**
 * Generates the winget manifests of a release (plan M4b WP112; format in
 * scripts/lib/winget.mjs):
 *
 *   node scripts/winget-manifest.mjs --version 0.2.0 --artifacts <dir> [--artifacts <dir>] \
 *     --installer-url-template "https://github.com/<owner>/<repo>/releases/download/v{{version}}/{{file}}" \
 *     --out <dir> [--release-notes-url <url>] [--release-date YYYY-MM-DD]
 *
 * Takes the per-user NSIS installers (`Nail_<v>_<arch>-setup.exe`, x64 and
 * arm64) from the artifact folders, hashes them, and writes
 * <out>/manifests/n/Nail/Nail/<version>/*.yaml, ready for a pull request
 * to microsoft/winget-pkgs (or `winget validate` / `winget install
 * --manifest`). Package facts: packaging/winget/nail.winget.json.
 */
import { createHash } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { readJson } from "./lib/json.mjs";
import { classifyArtifact } from "./lib/update-manifest.mjs";
import { expandUrl, wingetManifests } from "./lib/winget.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** @param {string[]} argv */
function parse(argv) {
  /**
   * @type {{ artifacts: string[], version?: string, urlTemplate?: string, out?: string,
   *   releaseNotesUrl?: string, releaseDate?: string, package?: string }}
   */
  const o = { artifacts: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    const value = () => {
      const v = argv[++i];
      if (v === undefined || v.startsWith("--")) throw new Error(`${arg} needs a value`);
      return v;
    };
    if (arg === "--version") o.version = value();
    else if (arg === "--artifacts") o.artifacts.push(value());
    else if (arg === "--installer-url-template") o.urlTemplate = value();
    else if (arg === "--out") o.out = value();
    else if (arg === "--release-notes-url") o.releaseNotesUrl = value();
    else if (arg === "--release-date") o.releaseDate = value();
    else if (arg === "--package") o.package = value();
    else throw new Error(`unknown option ${arg}`);
  }
  const { version, urlTemplate, out } = o;
  if (!version || !urlTemplate || !out || !o.artifacts.length) {
    throw new Error("--version, --artifacts, --installer-url-template and --out are required");
  }
  return { ...o, version, urlTemplate, out };
}

function main() {
  const o = parse(process.argv.slice(2));
  const pkg = /** @type {import("./lib/winget.mjs").WingetInput} */ (
    readJson(o.package ?? path.join(repoRoot, "packaging", "winget", "nail.winget.json"))
  );
  /** @type {{ arch: string, url: string, sha256: string }[]} */
  const installers = [];
  for (const dir of o.artifacts) {
    for (const name of readdirSync(dir)) {
      const kind = classifyArtifact(name);
      if (kind?.installer !== "nsis" || kind.version !== o.version) continue;
      const sha256 = createHash("sha256")
        .update(readFileSync(path.join(dir, name)))
        .digest("hex");
      installers.push({
        arch: kind.arch,
        url: expandUrl(o.urlTemplate, { version: o.version, file: name }),
        sha256,
      });
    }
  }
  const files = wingetManifests({
    ...pkg,
    version: o.version,
    releaseNotesUrl: o.releaseNotesUrl,
    releaseDate: o.releaseDate ?? new Date().toISOString().slice(0, 10),
    installers,
  });
  for (const [rel, text] of Object.entries(files)) {
    const file = path.join(o.out, ...rel.split("/"));
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, text);
    console.log(file);
  }
}

main();
