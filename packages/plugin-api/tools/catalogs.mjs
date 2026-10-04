/**
 * Reads a plug-in's Fluent catalogs (`contributes.locales`) for the test
 * host's `nail.l10n` (`createTestHost({ catalogs: loadCatalogs(dir) })`).
 */
import { readFileSync } from "node:fs";
import path from "node:path";

import { isPackagePath } from "./manifest.mjs";

/**
 * `contributes.locales` of an untrusted manifest; `{}` when it has none.
 *
 * @param {unknown} manifest
 * @returns {Record<string, unknown>}
 */
function localesOf(manifest) {
  /** @type {unknown} */
  let value = manifest;
  for (const key of ["contributes", "locales"]) {
    if (typeof value !== "object" || value === null) return {};
    value = /** @type {Record<string, unknown>} */ (value)[key];
  }
  return typeof value === "object" && value !== null
    ? /** @type {Record<string, unknown>} */ (value)
    : {};
}

/**
 * The catalogs of the plug-in in `dir`: language tag → `.ftl` text.
 * @param {string} dir the plug-in folder (with manifest.json)
 * @param {unknown} [manifest] the parsed manifest (read from `dir` when absent)
 * @returns {Record<string, string>}
 */
export function loadCatalogs(dir, manifest) {
  /** @type {unknown} */
  const m = manifest ?? JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8"));
  /** @type {Record<string, string>} */
  const out = {};
  for (const [tag, file] of Object.entries(localesOf(m))) {
    if (typeof file !== "string" || !isPackagePath(file)) {
      throw new Error(`invalid catalog path for ${tag}: ${String(file)}`);
    }
    out[tag] = readFileSync(path.join(dir, ...file.split("/")), "utf8");
  }
  return out;
}
