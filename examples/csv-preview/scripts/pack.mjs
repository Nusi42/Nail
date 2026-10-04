#!/usr/bin/env node
/**
 * Packs the built plug-in into dist/<id>.nailplugin (a zip with
 * manifest.json, the bundle, the locales and assets/), ready to install
 * in Nail. Run `npm run build` first.
 *
 *   node scripts/pack.mjs [--out FILE]
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

import { PackError, packPlugin } from "@nail/plugin-api/tools";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const at = process.argv.indexOf("--out");
const out = at >= 0 ? process.argv[at + 1] : undefined;

try {
  const result = packPlugin({ dir: root, out });
  console.log(path.relative(process.cwd(), result.file) || result.file);
  console.log(
    `  ${String(result.files.length)} files, ${String(result.size)} bytes, sha256 ${result.sha256}`,
  );
} catch (error) {
  if (!(error instanceof PackError)) throw error;
  console.error(error.message);
  process.exitCode = 1;
}
