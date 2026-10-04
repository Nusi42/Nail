#!/usr/bin/env node
/**
 * Prints the app version (app/src-tauri/tauri.conf.json) for CI:
 *
 *   node scripts/release-version.mjs                 0.1.0
 *   node scripts/release-version.mjs --next          0.1.1 (the smoke test's update)
 *   node scripts/release-version.mjs --tag v0.1.0    0.1.0, or fails when the tag
 *                                                    names another version
 *   node scripts/release-version.mjs --channel       stable, or beta for a pre-release
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

import { bumpPatch } from "./lib/build.mjs";
import { readJson } from "./lib/json.mjs";
import { parseVersion } from "./lib/update-manifest.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const conf = /** @type {{ version: string }} */ (
  readJson(path.join(repoRoot, "app", "src-tauri", "tauri.conf.json"))
);
const version = conf.version;
const args = process.argv.slice(2);

if (args[0] === "--next") {
  console.log(bumpPatch(version));
} else if (args[0] === "--channel") {
  console.log(parseVersion(version).pre.length ? "beta" : "stable");
} else if (args[0] === "--tag") {
  const tag = (args[1] ?? "").replace(/^refs\/tags\//, "");
  if (tag.replace(/^v/, "") !== version) {
    console.error(`tag ${tag} does not match the app version ${version} (tauri.conf.json)`);
    process.exit(1);
  }
  console.log(version);
} else if (args.length === 0) {
  console.log(version);
} else {
  console.error(`unknown option ${args[0] ?? ""}`);
  process.exit(2);
}
