// Tests of scripts/lib/winget.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  checkPackage,
  expandUrl,
  MANIFEST_VERSION,
  wingetManifests,
  yamlString,
} from "../lib/winget.mjs";
import { readJson } from "../lib/json.mjs";

/**
 * One generated manifest file by its path.
 *
 * @param {string} rel
 */
function manifestFile(rel) {
  const text = wingetManifests(pkg)[rel];
  assert.ok(text, rel);
  return text;
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const facts = /** @type {import("../lib/winget.mjs").WingetInput} */ (
  readJson(path.join(repoRoot, "packaging", "winget", "nail.winget.json"))
);
const sha = "ab".repeat(32);
// The committed facts are complete (checkPackage proves it in the tests).
const pkg = /** @type {import("../lib/winget.mjs").WingetPackage} */ ({
  ...facts,
  version: "0.2.0",
  releaseDate: "2026-10-02",
  releaseNotesUrl: "https://example.org/notes/0.2.0",
  installers: [
    { arch: "x64", url: "https://dl.example/v0.2.0/Nail_0.2.0_x64-setup.exe", sha256: sha },
    {
      arch: "arm64",
      url: "https://dl.example/v0.2.0/Nail_0.2.0_arm64-setup.exe",
      sha256: "CD".repeat(32),
    },
  ],
});

test("three files in the winget-pkgs layout", () => {
  const files = wingetManifests(pkg);
  assert.deepEqual(Object.keys(files), [
    "manifests/n/Nail/Nail/0.2.0/Nail.Nail.yaml",
    "manifests/n/Nail/Nail/0.2.0/Nail.Nail.installer.yaml",
    "manifests/n/Nail/Nail/0.2.0/Nail.Nail.locale.en-US.yaml",
  ]);
  for (const text of Object.values(files)) {
    assert.match(text, /^# yaml-language-server: \$schema=https:\/\/aka\.ms\/winget-manifest\./);
    assert.match(text, /PackageIdentifier: 'Nail\.Nail'\nPackageVersion: '0\.2\.0'\n/);
    assert.ok(text.endsWith(`ManifestVersion: ${MANIFEST_VERSION}\n`));
  }
});

test("the installer manifest wraps the per-user NSIS setup", () => {
  const text = manifestFile("manifests/n/Nail/Nail/0.2.0/Nail.Nail.installer.yaml");
  for (const line of [
    "InstallerType: nullsoft",
    "Scope: user",
    "  Silent: /S",
    "  SilentWithProgress: /P",
    "UpgradeBehavior: install",
    "ProductCode: 'Nail'",
    "ReleaseDate: 2026-10-02",
    "ManifestType: installer",
  ]) {
    assert.ok(text.includes(`${line}\n`), line);
  }
  // Same schemes and types as the installers register.
  assert.ok(text.includes("Protocols:\n- 'mailto'\n- 'webcal'\n- 'webcals'\n"));
  assert.ok(text.includes("FileExtensions:\n- 'eml'\n- 'ics'\n- 'vcf'\n"));
  // Installers sorted by architecture, hashes upper-case.
  const arm = text.indexOf("- Architecture: arm64");
  const x64 = text.indexOf("- Architecture: x64");
  assert.ok(arm > 0 && x64 > arm);
  assert.ok(text.includes(`InstallerSha256: ${sha.toUpperCase()}\n`));
  assert.ok(text.includes("InstallerUrl: 'https://dl.example/v0.2.0/Nail_0.2.0_x64-setup.exe'\n"));
});

test("the locale manifest carries the package facts", () => {
  const text = manifestFile("manifests/n/Nail/Nail/0.2.0/Nail.Nail.locale.en-US.yaml");
  assert.ok(text.includes("PackageLocale: en-US\n"));
  assert.ok(text.includes("Publisher: 'Nail'\n"));
  assert.ok(text.includes("PackageName: 'Nail'\n"));
  assert.ok(text.includes("License: 'Proprietary'\n"));
  assert.ok(text.includes("ShortDescription: 'Mail, calendar and contacts'\n"));
  assert.ok(text.includes("Moniker: 'nail'\n"));
  assert.ok(text.includes("ReleaseNotesUrl: 'https://example.org/notes/0.2.0'\n"));
  assert.ok(text.includes("Tags:\n- 'email'\n"));
  assert.ok(text.includes("ManifestType: defaultLocale\n"));
});

test("bad package descriptions are refused", () => {
  /** @param {import("../lib/winget.mjs").WingetInput} patch */
  const bad = (patch) => () => {
    checkPackage({ ...pkg, ...patch });
  };
  const [first] = pkg.installers;
  assert.ok(first);
  assert.throws(bad({ identifier: "Nail" }), /PackageIdentifier/);
  assert.throws(bad({ version: "1.0" }), /semantic/);
  assert.throws(bad({ license: "" }), /license is required/);
  assert.throws(bad({ installers: [] }), /no installers/);
  assert.throws(bad({ installers: [{ ...first, arch: "x86" }] }), /unsupported arch/);
  assert.throws(bad({ installers: [first, first] }), /two installers/);
  assert.throws(bad({ installers: [{ ...first, url: "http://x" }] }), /https/);
  assert.throws(bad({ installers: [{ ...first, sha256: "abc" }] }), /SHA-256/);
  assert.throws(bad({ releaseDate: "02.10.2026" }), /YYYY-MM-DD/);
});

test("YAML strings and URL templates", () => {
  assert.equal(yamlString("it's"), "'it''s'");
  assert.equal(yamlString("a: #b"), "'a: #b'");
  assert.throws(() => yamlString("a\nb"), /multi-line/);
  assert.equal(
    expandUrl("https://h/v{{version}}/{{file}}", { version: "1.0.0", file: "Nail 1.exe" }),
    "https://h/v1.0.0/Nail%201.exe",
  );
  assert.throws(() => expandUrl("https://h/{{owner}}", { version: "1" }), /unknown placeholder/);
});
