/**
 * winget manifests for Nail (plan M4b WP112, design "Windows installer &
 * distribution": the winget package wraps the per-user `setup.exe`).
 * Multi-file manifest, schema 1.10.0: `<id>.yaml` (version),
 * `<id>.installer.yaml`, `<id>.locale.en-US.yaml`, laid out as in the
 * winget-pkgs repository (`manifests/<letter>/<Publisher>/<Package>/<version>/`).
 *
 * The installer is Tauri's NSIS build (`nullsoft`): per user, silent with
 * `/S`, passive with `/P`; its uninstall entry is `HKCU\…\Uninstall\Nail`,
 * so the ProductCode is `Nail`. Defaults live in packaging/winget/
 * package.json; the CLI is scripts/winget-manifest.mjs, tests in
 * scripts/test/winget.test.mjs.
 */
import path from "node:path";

import { parseVersion } from "./update-manifest.mjs";

export const MANIFEST_VERSION = "1.10.0";

/**
 * The package description: packaging/winget/nail.winget.json plus the
 * release facts.
 *
 * @typedef {object} WingetPackage
 * @property {string} identifier
 * @property {string} version
 * @property {string} publisher
 * @property {string} name
 * @property {string} license
 * @property {string} shortDescription
 * @property {string} [description]
 * @property {string} [publisherUrl]
 * @property {string} [packageUrl]
 * @property {string} [licenseUrl]
 * @property {string} [releaseNotesUrl]
 * @property {string} [moniker]
 * @property {string[]} [tags]
 * @property {string} [releaseDate]
 * @property {{ arch: string, url: string, sha256: string }[]} installers
 */

/**
 * A package description before {@link checkPackage}: untrusted JSON, any
 * field may be missing.
 *
 * @typedef {Partial<Omit<WingetPackage, "installers">> &
 *   { installers?: Partial<WingetPackage["installers"][number]>[] }} WingetInput
 */

/**
 * Single-quoted YAML scalar (no escapes needed except doubled quotes).
 *
 * @param {string} text
 */
export function yamlString(text) {
  if (/[\r\n]/.test(text)) throw new Error("multi-line values are not supported here");
  return `'${text.replaceAll("'", "''")}'`;
}

/** @param {string} kind */
function header(kind) {
  return `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${kind}.${MANIFEST_VERSION}.schema.json\n`;
}

/**
 * @param {string} key
 * @param {string[] | undefined} values
 * @param {string} [indent]
 */
function list(key, values, indent = "") {
  if (!values?.length) return "";
  return `${indent}${key}:\n${values.map((v) => `${indent}- ${yamlString(v)}\n`).join("")}`;
}

/**
 * Checks the package description (packaging/winget/package.json + release
 * facts). Takes untrusted JSON, so every field may be missing.
 *
 * @param {WingetInput} pkg
 * @returns {asserts pkg is WingetPackage}
 */
export function checkPackage(pkg) {
  const id = pkg.identifier;
  if (!/^[A-Za-z0-9-]+\.[A-Za-z0-9.-]+$/.test(id ?? "")) {
    throw new Error(`invalid PackageIdentifier ${id ?? "(missing)"}`);
  }
  parseVersion(pkg.version ?? "");
  for (const key of /** @type {const} */ (["publisher", "name", "license", "shortDescription"])) {
    if (!pkg[key]) throw new Error(`winget: ${key} is required`);
  }
  if (!pkg.installers?.length) throw new Error("winget: no installers");
  /** @type {Set<string>} */
  const archs = new Set();
  for (const i of pkg.installers) {
    const arch = i.arch ?? "";
    if (arch !== "x64" && arch !== "arm64") throw new Error(`winget: unsupported arch ${arch}`);
    if (archs.has(arch)) throw new Error(`winget: two installers for ${arch}`);
    archs.add(arch);
    if (!(i.url ?? "").startsWith("https://"))
      throw new Error(`winget: installer URL must be https: ${i.url ?? "(missing)"}`);
    if (!/^[0-9A-Fa-f]{64}$/.test(i.sha256 ?? ""))
      throw new Error(`winget: bad SHA-256 for ${arch}`);
  }
  if (pkg.releaseDate && !/^\d{4}-\d{2}-\d{2}$/.test(pkg.releaseDate)) {
    throw new Error(`winget: releaseDate must be YYYY-MM-DD: ${pkg.releaseDate}`);
  }
}

/**
 * The three manifest files: { "<relative path>": "<yaml>" }.
 * `pkg` = { identifier, version, publisher, name, license, shortDescription,
 * description?, publisherUrl?, packageUrl?, licenseUrl?, releaseNotesUrl?,
 * moniker?, tags?, releaseDate?, installers: [{ arch, url, sha256 }] }.
 *
 * @param {WingetInput} pkg
 * @returns {Record<string, string>}
 */
export function wingetManifests(pkg) {
  checkPackage(pkg);
  const id = pkg.identifier;
  const version = pkg.version.replace(/^v/, "");
  const common = `PackageIdentifier: ${yamlString(id)}\nPackageVersion: ${yamlString(version)}\n`;
  /** @param {string} type */
  const footer = (type) => `ManifestType: ${type}\nManifestVersion: ${MANIFEST_VERSION}\n`;

  const versionYaml = header("version") + common + "DefaultLocale: en-US\n" + footer("version");

  const installers = [...pkg.installers]
    .sort((a, b) => a.arch.localeCompare(b.arch))
    .map(
      (i) =>
        `- Architecture: ${i.arch}\n` +
        `  InstallerUrl: ${yamlString(i.url)}\n` +
        `  InstallerSha256: ${i.sha256.toUpperCase()}\n`,
    )
    .join("");
  const installerYaml =
    header("installer") +
    common +
    "InstallerType: nullsoft\n" +
    "Scope: user\n" +
    "InstallModes:\n- interactive\n- silent\n- silentWithProgress\n" +
    "InstallerSwitches:\n  Silent: /S\n  SilentWithProgress: /P\n" +
    "UpgradeBehavior: install\n" +
    "ProductCode: 'Nail'\n" +
    list("Protocols", ["mailto", "webcal", "webcals"]) +
    list("FileExtensions", ["eml", "ics", "vcf"]) +
    (pkg.releaseDate ? `ReleaseDate: ${pkg.releaseDate}\n` : "") +
    "AppsAndFeaturesEntries:\n" +
    `- DisplayName: ${yamlString(pkg.name)}\n` +
    `  Publisher: ${yamlString(pkg.publisher)}\n` +
    "  ProductCode: 'Nail'\n" +
    "Installers:\n" +
    installers +
    footer("installer");

  /**
   * @param {string} key
   * @param {string | undefined} value
   */
  const optional = (key, value) => (value ? `${key}: ${yamlString(value)}\n` : "");
  const localeYaml =
    header("defaultLocale") +
    common +
    "PackageLocale: en-US\n" +
    `Publisher: ${yamlString(pkg.publisher)}\n` +
    optional("PublisherUrl", pkg.publisherUrl) +
    `PackageName: ${yamlString(pkg.name)}\n` +
    optional("PackageUrl", pkg.packageUrl) +
    `License: ${yamlString(pkg.license)}\n` +
    optional("LicenseUrl", pkg.licenseUrl) +
    `ShortDescription: ${yamlString(pkg.shortDescription)}\n` +
    optional("Description", pkg.description) +
    optional("Moniker", pkg.moniker) +
    list("Tags", pkg.tags) +
    optional("ReleaseNotesUrl", pkg.releaseNotesUrl) +
    footer("defaultLocale");

  const [publisherPart = "", ...rest] = id.split(".");
  const dir = path.posix.join(
    "manifests",
    publisherPart.charAt(0).toLowerCase(),
    publisherPart,
    rest.join("."),
    version,
  );
  return {
    [path.posix.join(dir, `${id}.yaml`)]: versionYaml,
    [path.posix.join(dir, `${id}.installer.yaml`)]: installerYaml,
    [path.posix.join(dir, `${id}.locale.en-US.yaml`)]: localeYaml,
  };
}

/**
 * `{{version}}` / `{{file}}` placeholders of a URL template.
 *
 * @param {string} template
 * @param {Partial<Record<string, string>>} values
 */
export function expandUrl(template, values) {
  return template.replace(
    /\{\{(\w+)\}\}/g,
    /**
     * @param {string} all
     * @param {string} key
     */
    (all, key) => {
      const value = values[key];
      if (value === undefined) throw new Error(`unknown placeholder ${all} in ${template}`);
      return encodeURIComponent(value);
    },
  );
}
