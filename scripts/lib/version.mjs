/**
 * The workspace's version and repository (ADR 0203): one definition each,
 * `[workspace.package]` in the root `Cargo.toml`. The version is copied
 * into the JSON files below (Tauri and the npm workspaces read their own)
 * and into their `package-lock.json` entries; `setVersion` writes every
 * copy, and scripts/test/version.test.mjs fails when one differs.
 */
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

import { parseJson } from "./json.mjs";

/** The Cargo manifest holding `[workspace.package]`. */
export const CARGO_TOML = "Cargo.toml";

/** JSON files with a top-level `"version"` that copies the workspace's. */
export const VERSION_JSON_FILES = [
  "app/src-tauri/tauri.conf.json",
  "ui/package.json",
  "e2e/package.json",
  "packages/plugin-api/package.json",
  "packages/create-nail-plugin/package.json",
];

/** The npm lock file, whose workspace entries copy the version too. */
export const PACKAGE_LOCK = "package-lock.json";

/**
 * The `[workspace.package]` table of a Cargo manifest: its text up to the
 * next table.
 *
 * @param {string} toml
 */
function workspacePackage(toml) {
  const start = toml.indexOf("\n[workspace.package]");
  if (start < 0) throw new Error(`${CARGO_TOML} has no [workspace.package]`);
  const rest = toml.slice(start + 1);
  const next = rest.slice(1).search(/^\[/m);
  return { start: start + 1, text: next < 0 ? rest : rest.slice(0, next + 1) };
}

/**
 * A string key of `[workspace.package]`.
 *
 * @param {string} toml
 * @param {string} key
 */
function workspaceString(toml, key) {
  const match = new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, "m").exec(workspacePackage(toml).text);
  if (!match?.[1]) throw new Error(`[workspace.package] in ${CARGO_TOML} has no ${key}`);
  return match[1];
}

/**
 * The workspace version.
 *
 * @param {string} toml the root Cargo manifest
 */
export function workspaceVersion(toml) {
  return workspaceString(toml, "version");
}

/**
 * The public repository (`https://github.com/<owner>/<name>`).
 *
 * @param {string} toml the root Cargo manifest
 */
export function workspaceRepository(toml) {
  return workspaceString(toml, "repository").replace(/\/+$/, "");
}

/**
 * `toml` with the workspace version set to `version`.
 *
 * @param {string} toml
 * @param {string} version
 */
export function withWorkspaceVersion(toml, version) {
  const { start, text } = workspacePackage(toml);
  const changed = text.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
  return toml.slice(0, start) + changed + toml.slice(start + text.length);
}

/**
 * `json` with its top-level version set to `version`, changing only that
 * line (re-serializing would reformat the whole file). The first
 * `"version"` key at two-space indent is the top-level one.
 *
 * @param {string} json
 * @param {string} version
 */
export function withJsonVersion(json, version) {
  const pattern = /^ {2}"version": "[^"]*"/m;
  if (!pattern.test(json)) throw new Error("no top-level version");
  return json.replace(pattern, `  "version": "${version}"`);
}

/**
 * The lock file's entries of the versioned workspaces (`ui`, `e2e`, …):
 * their directory and version.
 *
 * @param {string} lockText
 */
export function lockVersions(lockText) {
  const lock = /** @type {{ packages?: Record<string, { version?: string }> }} */ (
    parseJson(lockText)
  );
  const packages = lock.packages ?? {};
  return VERSION_JSON_FILES.filter((file) => file.endsWith("/package.json")).map((file) => {
    const dir = path.posix.dirname(file);
    return { dir, version: packages[dir]?.version ?? null };
  });
}

/**
 * `lockText` with the versioned workspaces' entries at `version`. Only
 * their `"version"` lines change.
 *
 * @param {string} lockText
 * @param {string} version
 */
export function withLockVersions(lockText, version) {
  let text = lockText;
  for (const { dir } of lockVersions(lockText)) {
    const key = `\n    "${dir}": {`;
    const at = text.indexOf(key);
    if (at < 0) throw new Error(`${PACKAGE_LOCK} has no entry for ${dir}`);
    const end = text.indexOf("\n    }", at);
    const entry = text.slice(at, end);
    text =
      text.slice(0, at) +
      entry.replace(/^ {6}"version": "[^"]*"/m, `      "version": "${version}"`) +
      text.slice(end);
  }
  return text;
}

/**
 * Every copy of the version that differs from the workspace's.
 *
 * @param {string} root the checkout
 * @returns {{ version: string, mismatches: { where: string, version: string | null }[] }}
 */
export function versionMismatches(root) {
  const read = (/** @type {string} */ rel) => readFileSync(path.join(root, rel), "utf8");
  const version = workspaceVersion(read(CARGO_TOML));
  /** @type {{ where: string, version: string | null }[]} */
  const mismatches = [];
  for (const file of VERSION_JSON_FILES) {
    const found = /** @type {{ version?: string }} */ (parseJson(read(file))).version ?? null;
    if (found !== version) mismatches.push({ where: file, version: found });
  }
  for (const entry of lockVersions(read(PACKAGE_LOCK))) {
    if (entry.version !== version) {
      mismatches.push({ where: `${PACKAGE_LOCK} (${entry.dir})`, version: entry.version });
    }
  }
  return { version, mismatches };
}

/**
 * Sets the version in every copy; answers the files it changed.
 *
 * @param {string} root the checkout
 * @param {string} version
 */
export function setVersion(root, version) {
  /** @type {string[]} */
  const changed = [];
  /**
   * @param {string} rel
   * @param {(text: string) => string} edit
   */
  const update = (rel, edit) => {
    const file = path.join(root, rel);
    const before = readFileSync(file, "utf8");
    const after = edit(before);
    if (after !== before) {
      writeFileSync(file, after);
      changed.push(rel);
    }
  };
  update(CARGO_TOML, (t) => withWorkspaceVersion(t, version));
  for (const file of VERSION_JSON_FILES) update(file, (t) => withJsonVersion(t, version));
  update(PACKAGE_LOCK, (t) => withLockVersions(t, version));
  return changed;
}
