#!/usr/bin/env node
/**
 * Checks a channel manifest for an update and downloads it, verified, the
 * way the app's updater does (scripts/lib/update-check.mjs). Used by the
 * installer smoke tests.
 *
 *   node scripts/update-check.mjs --endpoint <url> --current <version> \
 *     --arch x64|arm64 --installer nsis|msi --pubkey-file <file> --out <file>
 *
 * Prints one JSON line ({available, version, target}); writes the verified
 * installer to --out when there is one. Exit code 0 also when there is no
 * update; 1 on any failure (bad signature, wrong version, …).
 */
import { readFileSync, writeFileSync } from "node:fs";

import { checkForUpdate, downloadVerified } from "./lib/update-check.mjs";

function parse(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const v = argv[++i];
    if (v === undefined || v.startsWith("--")) throw new Error(`${arg} needs a value`);
    const key = {
      "--endpoint": "endpoint",
      "--current": "current",
      "--arch": "arch",
      "--installer": "installer",
      "--pubkey-file": "pubkeyFile",
      "--out": "out",
    }[arg];
    if (!key) throw new Error(`unknown option ${arg}`);
    o[key] = v;
  }
  for (const key of ["endpoint", "current", "arch", "installer", "pubkeyFile", "out"]) {
    if (!o[key]) throw new Error(`missing option for ${key}`);
  }
  return o;
}

try {
  const o = parse(process.argv.slice(2));
  const update = await checkForUpdate({
    endpoint: o.endpoint,
    currentVersion: o.current,
    arch: o.arch,
    installer: o.installer,
  });
  if (update.available) {
    const data = await downloadVerified({ update, publicKey: readFileSync(o.pubkeyFile, "utf8") });
    writeFileSync(o.out, data);
  }
  console.log(
    JSON.stringify({ available: update.available, version: update.version, target: update.target }),
  );
} catch (e) {
  console.error(`update check failed: ${e.message}`);
  process.exit(1);
}
