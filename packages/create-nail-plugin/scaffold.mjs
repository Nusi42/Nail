/**
 * Scaffolds a Nail plug-in folder from `template/`: manifest.json,
 * src/index.tsx, tsconfig.json, package.json, scripts/build.mjs (esbuild),
 * scripts/pack.mjs (.nailplugin), locales/en.ftl, README.md, .gitignore.
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { isKebabName } from "@nail/plugin-api/tools";

const here = path.dirname(fileURLToPath(import.meta.url));

/** The template folder. */
export const TEMPLATE_DIR = path.join(here, "template");
/** The SDK package in this repository (`packages/plugin-api`). */
export const SDK_DIR = path.resolve(here, "..", "plugin-api");

/** Template files whose names npm would drop or mangle when publishing. */
const RENAMES = { gitignore: ".gitignore" };
/** Files whose placeholders are JSON / JavaScript string literals. */
const CODE = /\.(json|mjs|js|ts|tsx)$/;

/** A scaffolding failure (bad arguments, target not empty). */
export class ScaffoldError extends Error {
  /** @param {string} message */
  constructor(message) {
    super(message);
    this.name = "ScaffoldError";
  }
}

/**
 * A plug-in id from a folder or display name: `My Plug-in!` → `my-plug-in`.
 * @param {string} name
 * @returns {string}
 */
export function toPluginId(name) {
  const id = name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^[^a-z]+/, "")
    .replace(/-+$/, "")
    .slice(0, 64)
    .replace(/-+$/, "");
  return id || "my-plugin";
}

/**
 * A display name from an id: `follow-up` → `Follow up`.
 * @param {string} id
 * @returns {string}
 */
export function toDisplayName(id) {
  const words = id.split("-").join(" ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * @param {string} dir
 * @returns {string[]} paths relative to `dir`, `/`-separated
 */
function walk(dir, rel = "") {
  const out = [];
  for (const entry of readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    const child = rel ? `${rel}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...walk(dir, child));
    else out.push(child);
  }
  return out.sort();
}

/**
 * The version spec of a dev dependency of the SDK (esbuild, typescript).
 * @param {string} name
 * @returns {string}
 */
function sdkDevDependency(name) {
  const pkg = JSON.parse(readFileSync(path.join(SDK_DIR, "package.json"), "utf8"));
  const spec = pkg.devDependencies?.[name];
  if (typeof spec !== "string") throw new Error(`the SDK has no dev dependency ${name}`);
  return spec;
}

/**
 * Replaces the template placeholders.
 * @param {string} file template path
 * @param {string} text
 * @param {Record<string, string>} values
 */
export function fill(file, text, values) {
  let out = text;
  for (const [key, value] of Object.entries(values)) {
    const token = `__${key}__`;
    if (CODE.test(file)) out = out.split(`"${token}"`).join(JSON.stringify(value));
    out = out.split(token).join(value);
  }
  return out;
}

/**
 * @typedef {object} ScaffoldOptions
 * @property {string} dir the new plug-in folder (created; must be empty unless `force`)
 * @property {string} [id] plug-in id (default: from the folder name)
 * @property {string} [name] display name (default: from the id)
 * @property {string} [sdk] version spec of `@nail/plugin-api` (default: `file:` path to this repository's SDK)
 * @property {boolean} [force] write into a non-empty folder
 */

/**
 * Creates a plug-in folder.
 * @param {ScaffoldOptions} options
 * @returns {{ dir: string, id: string, name: string, files: string[] }}
 */
export function scaffold(options) {
  const dir = path.resolve(options.dir);
  if (existsSync(dir) && readdirSync(dir).length > 0 && !options.force) {
    throw new ScaffoldError(`${dir} is not empty (use --force to write into it)`);
  }
  const id = options.id ?? toPluginId(path.basename(dir));
  if (!isKebabName(id)) {
    throw new ScaffoldError(
      `invalid plug-in id "${id}": use 1-64 lower-case letters, digits and single hyphens, starting with a letter`,
    );
  }
  const name = options.name ?? toDisplayName(id);
  if (!name.trim() || name.length > 100 || /[\r\n]/.test(name)) {
    throw new ScaffoldError("the name must be one line of 1-100 characters");
  }
  const sdk = options.sdk ?? `file:${path.relative(dir, SDK_DIR).split(path.sep).join("/") || "."}`;
  const values = {
    PLUGIN_ID: id,
    PLUGIN_NAME: name,
    SDK_SPEC: sdk,
    ESBUILD_SPEC: sdkDevDependency("esbuild"),
    TYPESCRIPT_SPEC: sdkDevDependency("typescript"),
  };

  const files = [];
  for (const rel of walk(TEMPLATE_DIR)) {
    const base = path.posix.basename(rel);
    const target = path.posix.join(
      path.posix.dirname(rel),
      RENAMES[/** @type {keyof typeof RENAMES} */ (base)] ?? base,
    );
    const text = fill(rel, readFileSync(path.join(TEMPLATE_DIR, rel), "utf8"), values);
    const out = path.join(dir, ...target.split("/"));
    mkdirSync(path.dirname(out), { recursive: true });
    writeFileSync(out, text);
    files.push(target.startsWith("./") ? target.slice(2) : target);
  }
  return { dir, id, name, files: files.sort() };
}
