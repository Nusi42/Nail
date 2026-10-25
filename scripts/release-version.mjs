#!/usr/bin/env node
/**
 * Prints the app version (`[workspace.package]` in Cargo.toml, ADR 0203):
 *
 *   node scripts/release-version.mjs                 0.1.0
 *   node scripts/release-version.mjs --next          0.1.1 (the smoke test's update)
 *   node scripts/release-version.mjs --tag v0.1.0    0.1.0, or fails when the tag
 *                                                    names another version
 *   node scripts/release-version.mjs --channel       stable, or beta for a pre-release
 */
import { readFileSync } from "node:fs";
import path from "node:path";

import { bumpPatch } from "./lib/build.mjs";
import { repoRoot } from "./lib/program.mjs";
import { channelOf } from "./lib/release.mjs";
import { CARGO_TOML, workspaceVersion } from "./lib/version.mjs";

const version = workspaceVersion(readFileSync(path.join(repoRoot, CARGO_TOML), "utf8"));
const args = process.argv.slice(2);

if (args[0] === "--next") {
  console.log(bumpPatch(version));
} else if (args[0] === "--channel") {
  console.log(channelOf(version));
} else if (args[0] === "--tag") {
  const tag = (args[1] ?? "").replace(/^refs\/tags\//, "");
  if (tag.replace(/^v/, "") !== version) {
    console.error(`tag ${tag} does not match the app version ${version} (${CARGO_TOML})`);
    process.exit(1);
  }
  console.log(version);
} else if (args.length === 0) {
  console.log(version);
} else {
  console.error(`unknown option ${args[0] ?? ""}`);
  process.exit(2);
}
