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

/** Single-quoted YAML scalar (no escapes needed except doubled quotes). */
export function yamlString(value) {
  const text = String(value);
  if (/[\r\n]/.test(text)) throw new Error("multi-line values are not supported here");
  return `'${text.replaceAll("'", "''")}'`;
}

function header(kind) {
  return `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${kind}.${MANIFEST_VERSION}.schema.json\n`;
}

function list(key, values, indent = "") {
  if (!values?.length) return "";
  return `${indent}${key}:\n${values.map((v) => `${indent}- ${yamlString(v)}\n`).join("")}`;
}

/** Checks the package description (packaging/winget/package.json + release facts). */
export function checkPackage(pkg) {
  const id = pkg.identifier;
  if (!/^[A-Za-z0-9-]+\.[A-Za-z0-9.-]+$/.test(id ?? "")) {
    throw new Error(`invalid PackageIdentifier ${id}`);
  }
  parseVersion(pkg.version);
  for (const key of ["publisher", "name", "license", "shortDescription"]) {
    if (!pkg[key]) throw new Error(`winget: ${key} is required`);
  }
  if (!pkg.installers?.length) throw new Error("winget: no installers");
  const archs = new Set();
  for (const i of pkg.installers) {
    if (!["x64", "arm64"].includes(i.arch)) throw new Error(`winget: unsupported arch ${i.arch}`);
    if (archs.has(i.arch)) throw new Error(`winget: two installers for ${i.arch}`);
    archs.add(i.arch);
    if (!/^https:\/\//.test(i.url ?? ""))
      throw new Error(`winget: installer URL must be https: ${i.url}`);
    if (!/^[0-9A-Fa-f]{64}$/.test(i.sha256 ?? ""))
      throw new Error(`winget: bad SHA-256 for ${i.arch}`);
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
 */
export function wingetManifests(pkg) {
  checkPackage(pkg);
  const id = pkg.identifier;
  const version = String(pkg.version).replace(/^v/, "");
  const common = `PackageIdentifier: ${yamlString(id)}\nPackageVersion: ${yamlString(version)}\n`;
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

  const [publisherPart, ...rest] = id.split(".");
  const dir = path.posix.join(
    "manifests",
    publisherPart[0].toLowerCase(),
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

/** `{{version}}` / `{{file}}` placeholders of a URL template. */
export function expandUrl(template, values) {
  return template.replace(/\{\{(\w+)\}\}/g, (all, key) => {
    if (!(key in values)) throw new Error(`unknown placeholder ${all} in ${template}`);
    return encodeURIComponent(values[key]);
  });
}
