/**
 * Scaffolds a Nail plug-in folder from `template/`: manifest.json,
 * src/index.tsx, tsconfig.json, package.json, scripts/build.mjs (esbuild),
 * scripts/pack.mjs (.nailplugin), locales/en.ftl, README.md, .gitignore.
 * A template other than `command` replaces manifest.json, src/index.tsx
 * and locales/en.ftl with those of `templates/<name>/` (M4a: `hook`,
 * `composer`, `attachment`).
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { isKebabName } from "@nail/plugin-api/tools";

const here = path.dirname(fileURLToPath(import.meta.url));

/** The template folder (the `command` template, and the files every template shares). */
export const TEMPLATE_DIR = path.join(here, "template");
/** The folder of the other templates' files. */
export const TEMPLATES_DIR = path.join(here, "templates");
/** The templates: a command with a dialog, mail hooks, composer extensions, an attachment preview. */
export const TEMPLATES = ["command", "hook", "composer", "attachment"];
/** The SDK package in this repository (`packages/plugin-api`). */
export const SDK_DIR = path.resolve(here, "..", "plugin-api");

/** Template files whose names npm would drop or mangle when publishing. */
/** @type {Partial<Record<string, string>>} */
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
 * Whether `value` is a JSON object.
 * @param {unknown} value
 * @returns {value is Record<string, unknown>}
 */
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * The version spec of a dev dependency of the SDK (esbuild, typescript).
 * @param {string} name
 * @returns {string}
 */
function sdkDevDependency(name) {
  /** @type {unknown} */
  const pkg = JSON.parse(readFileSync(path.join(SDK_DIR, "package.json"), "utf8"));
  const devDependencies = isRecord(pkg) ? pkg.devDependencies : undefined;
  const spec = isRecord(devDependencies) ? devDependencies[name] : undefined;
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
 * @property {string} [template] one of {@link TEMPLATES} (default `command`)
 * @property {boolean} [force] write into a non-empty folder
 */

/**
 * Creates a plug-in folder.
 * @param {ScaffoldOptions} options
 * @returns {{ dir: string, id: string, name: string, template: string, files: string[] }}
 */
export function scaffold(options) {
  const template = options.template ?? "command";
  if (!TEMPLATES.includes(template)) {
    throw new ScaffoldError(`unknown template "${template}": use ${TEMPLATES.join(", ")}`);
  }
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

  /** @type {Map<string, string>} template path → source folder */
  const sources = new Map(walk(TEMPLATE_DIR).map((rel) => [rel, TEMPLATE_DIR]));
  if (template !== "command") {
    const overlay = path.join(TEMPLATES_DIR, template);
    for (const rel of walk(overlay)) sources.set(rel, overlay);
  }
  const files = [];
  for (const [rel, source] of [...sources].sort(([a], [b]) => a.localeCompare(b))) {
    const base = path.posix.basename(rel);
    const target = path.posix.join(path.posix.dirname(rel), RENAMES[base] ?? base);
    const text = fill(rel, readFileSync(path.join(source, rel), "utf8"), values);
    const out = path.join(dir, ...target.split("/"));
    mkdirSync(path.dirname(out), { recursive: true });
    writeFileSync(out, text);
    files.push(target.startsWith("./") ? target.slice(2) : target);
  }
  return { dir, id, name, template, files: files.sort() };
}
