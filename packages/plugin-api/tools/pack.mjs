/**
 * Packs a built plug-in folder into a `.nailplugin` archive: a zip with
 * `manifest.json`, the bundled entry point (`main`), the Fluent catalogs
 * named in `contributes.locales` and everything under `assets/`.
 */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

import { CONTRACT } from "./contract.mjs";
import { checkManifest, isPackagePath } from "./manifest.mjs";
import { createZip } from "./zip.mjs";

/** Largest `manifest.json` and package (bytes), the package file extension: Nail's. */
export const { MAX_MANIFEST_BYTES, MAX_PACKAGE_BYTES, PACKAGE_EXTENSION } = CONTRACT;

/** A packing failure with the manifest issues (if any). */
export class PackError extends Error {
  /**
   * @param {string} message
   * @param {{ kind: string, path: string, detail?: string }[]} [issues]
   */
  constructor(message, issues = []) {
    super(message);
    this.name = "PackError";
    this.issues = issues;
  }
}

/**
 * The fields of a checked manifest these tools read (`index.d.mts` gives
 * callers the full generated `PluginManifest`).
 *
 * @typedef {{ id: string, name: string, version: string, main: string,
 *   contributes?: { locales?: Record<string, string> } }} PluginManifest
 */

/**
 * Reads and checks `<dir>/manifest.json`.
 * @param {string} dir
 * @returns {{ manifest: PluginManifest, bytes: Buffer }}
 */
export function readManifest(dir) {
  const file = path.join(dir, "manifest.json");
  if (!existsSync(file)) throw new PackError(`no manifest.json in ${dir}`);
  const bytes = readFileSync(file);
  if (bytes.length > MAX_MANIFEST_BYTES) {
    throw new PackError("manifest.json is larger than 64 KiB", [{ kind: "too-large", path: "" }]);
  }
  /** @type {unknown} */
  let manifest;
  try {
    manifest = /** @type {unknown} */ (JSON.parse(bytes.toString("utf8")));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new PackError(`manifest.json is not valid JSON: ${message}`, [
      { kind: "invalid-json", path: "", detail: message },
    ]);
  }
  const issues = checkManifest(manifest);
  if (issues.length > 0) {
    const lines = issues.map(
      (i) => `  ${i.kind} at ${i.path || "(root)"}${i.detail ? `: ${i.detail}` : ""}`,
    );
    throw new PackError(`manifest.json has problems:\n${lines.join("\n")}`, issues);
  }
  // checkManifest found nothing: the shape is a plug-in manifest.
  return { manifest: /** @type {PluginManifest} */ (manifest), bytes };
}

/**
 * @param {string} root
 * @param {string} rel
 * @returns {string[]}
 */
function listFiles(root, rel) {
  const out = [];
  for (const entry of readdirSync(path.join(root, rel), { withFileTypes: true })) {
    const child = `${rel}/${entry.name}`;
    if (entry.isDirectory()) out.push(...listFiles(root, child));
    else if (entry.isFile()) out.push(child);
  }
  return out;
}

/**
 * The files a package contains, in archive order (manifest first).
 * @param {string} dir
 * @param {PluginManifest} manifest
 * @returns {string[]}
 */
export function packageFiles(dir, manifest) {
  /** @type {string[]} */
  const files = [manifest.main];
  for (const file of Object.values(manifest.contributes?.locales ?? {})) files.push(file);
  const assets = path.join(dir, "assets");
  if (existsSync(assets) && statSync(assets).isDirectory()) {
    files.push(...listFiles(dir, "assets"));
  }
  const unique = [...new Set(files)].sort();
  for (const file of unique) {
    const invalid = `not a valid package path: ${file}`;
    if (!isPackagePath(file)) throw new PackError(invalid);
    if (file === "manifest.json") throw new PackError("manifest.json cannot be listed twice");
    if (!existsSync(path.join(dir, file))) {
      throw new PackError(
        file === manifest.main
          ? `${file} does not exist: build the plug-in first`
          : `${file} does not exist`,
      );
    }
  }
  return ["manifest.json", ...unique];
}

/**
 * Packs `dir` into `out` (default `<dir>/dist/<id>.nailplugin`).
 * @param {{ dir: string, out?: string | undefined }} options
 * @returns {{ file: string, sha256: string, size: number, files: string[],
 *   manifest: PluginManifest }}
 */
export function packPlugin({ dir, out }) {
  const root = path.resolve(dir);
  const { manifest } = readManifest(root);
  const files = packageFiles(root, manifest);
  const zip = createZip(files.map((name) => ({ name, data: readFileSync(path.join(root, name)) })));
  if (zip.length > MAX_PACKAGE_BYTES) {
    throw new PackError(`the package is larger than ${String(MAX_PACKAGE_BYTES)} bytes`);
  }
  const file = path.resolve(out ?? path.join(root, "dist", `${manifest.id}.${PACKAGE_EXTENSION}`));
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, zip);
  return {
    file,
    sha256: createHash("sha256").update(zip).digest("hex"),
    size: zip.length,
    files,
    manifest,
  };
}
