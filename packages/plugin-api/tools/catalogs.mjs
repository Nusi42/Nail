/**
 * Reads a plug-in's Fluent catalogs (`contributes.locales`) for the test
 * host's `nail.l10n` (`createTestHost({ catalogs: loadCatalogs(dir) })`).
 */
import { readFileSync } from "node:fs";
import path from "node:path";

import { isPackagePath } from "./manifest.mjs";

/**
 * The catalogs of the plug-in in `dir`: language tag → `.ftl` text.
 * @param {string} dir the plug-in folder (with manifest.json)
 * @param {any} [manifest] the parsed manifest (read from `dir` when absent)
 * @returns {Record<string, string>}
 */
export function loadCatalogs(dir, manifest) {
  const m = manifest ?? JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8"));
  /** @type {Record<string, string>} */
  const out = {};
  for (const [tag, file] of Object.entries(m?.contributes?.locales ?? {})) {
    if (typeof file !== "string" || !isPackagePath(file)) {
      throw new Error(`invalid catalog path for ${tag}: ${String(file)}`);
    }
    out[tag] = readFileSync(path.join(dir, ...file.split("/")), "utf8");
  }
  return out;
}
