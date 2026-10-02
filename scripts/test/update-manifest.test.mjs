// Tests of scripts/lib/update-manifest.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { test } from "node:test";

import { testKey } from "../lib/minisign.mjs";
import {
  buildManifest,
  CHANNELS,
  classifyArtifact,
  collectPlatforms,
  compareVersions,
  manifestJson,
  manifestPath,
  manifestsForRelease,
  parseVersion,
  updaterArch,
  verifyPlatforms,
} from "../lib/update-manifest.mjs";

const key = testKey(generateKeyPairSync("ed25519"));
const bytes = {
  "Nail_0.2.0_x64-setup.exe": Buffer.from("nsis x64"),
  "Nail_0.2.0_x64_en-US.msi": Buffer.from("msi x64"),
  "Nail_0.2.0_arm64-setup.exe": Buffer.from("nsis arm64"),
};
const signed = (name, version = "0.2.0") => key.sign(bytes[name], { file: name, version });
const files = () => [
  ...Object.keys(bytes).map((name) => ({ name, signature: signed(name) })),
  { name: "Nail_0.2.0_x64-offline-setup.exe", signature: null },
  { name: "Nail_0.2.0_x64-portable.zip", signature: null },
  { name: "Nail_0.2.0_x64-SHA256SUMS.txt", signature: null },
];

test("channels and manifest paths match nail_api::desktop::UpdateChannel", () => {
  assert.deepEqual(CHANNELS, ["stable", "beta"]);
  assert.equal(manifestPath("stable"), "stable/latest.json");
  assert.equal(manifestPath("beta"), "beta/latest.json");
  assert.throws(() => manifestPath("nightly"), /unknown channel/);
});

test("semantic versions compare by precedence", () => {
  assert.deepEqual(parseVersion("v1.2.3-beta.1+b"), {
    major: 1,
    minor: 2,
    patch: 3,
    pre: ["beta", "1"],
  });
  assert.throws(() => parseVersion("1.2"), /semantic/);
  assert.throws(() => parseVersion("1.2.3-01"), /semantic/);
  const ordered = [
    "1.0.0-alpha",
    "1.0.0-alpha.1",
    "1.0.0-alpha.beta",
    "1.0.0-beta",
    "1.0.0-beta.2",
    "1.0.0-beta.11",
    "1.0.0-rc.1",
    "1.0.0",
    "1.0.1",
    "1.1.0",
    "2.0.0",
  ];
  for (let i = 0; i + 1 < ordered.length; i++) {
    assert.ok(compareVersions(ordered[i], ordered[i + 1]) < 0, `${ordered[i]} < ${ordered[i + 1]}`);
    assert.ok(compareVersions(ordered[i + 1], ordered[i]) > 0);
  }
  assert.equal(compareVersions("v1.0.0", "1.0.0+x"), 0);
});

test("artifacts are classified by the bundler's names", () => {
  assert.deepEqual(classifyArtifact("Nail_0.2.0_x64-setup.exe"), {
    version: "0.2.0",
    arch: "x64",
    installer: "nsis",
  });
  assert.deepEqual(classifyArtifact("Nail_1.0.0-beta.1_arm64_en-US.msi"), {
    version: "1.0.0-beta.1",
    arch: "arm64",
    installer: "msi",
  });
  for (const other of [
    "Nail_0.2.0_x64-offline-setup.exe",
    "Nail_0.2.0_x64-portable.zip",
    "Nail_0.2.0_x64-setup.exe.sig",
    "x.exe",
  ]) {
    assert.equal(classifyArtifact(other), null, other);
  }
  assert.equal(updaterArch("x64"), "x86_64");
  assert.equal(updaterArch("arm64"), "aarch64");
  assert.throws(() => updaterArch("mips"), /unsupported/);
});

test("a release's platforms: every installer with its signature, checked", () => {
  const entries = collectPlatforms({
    files: files(),
    version: "0.2.0",
    downloadBase: "https://dl.example/v0.2.0/",
  });
  assert.deepEqual(
    entries.map((e) => [e.arch, e.installer, e.url]),
    [
      ["x64", "nsis", "https://dl.example/v0.2.0/Nail_0.2.0_x64-setup.exe"],
      ["x64", "msi", "https://dl.example/v0.2.0/Nail_0.2.0_x64_en-US.msi"],
      ["arm64", "nsis", "https://dl.example/v0.2.0/Nail_0.2.0_arm64-setup.exe"],
    ],
  );
  verifyPlatforms({ entries, publicKey: key.publicKey, version: "0.2.0", read: (n) => bytes[n] });
  // A signature for another version, or of another file, stops the release.
  const stale = entries.map((e) =>
    e.installer === "msi" ? { ...e, signature: signed(e.name, "0.1.0") } : e,
  );
  assert.throws(
    () =>
      verifyPlatforms({
        entries: stale,
        publicKey: key.publicKey,
        version: "0.2.0",
        read: (n) => bytes[n],
      }),
    /Nail_0\.2\.0_x64_en-US\.msi: the signature is for version 0\.1\.0/,
  );
  assert.throws(
    () =>
      verifyPlatforms({
        entries,
        publicKey: key.publicKey,
        version: "0.2.0",
        read: () => Buffer.from("x"),
      }),
    /does not match the file/,
  );
});

test("a release is whole: unsigned or foreign installers are errors", () => {
  const unsigned = files().map((f) => (f.name.endsWith(".msi") ? { ...f, signature: null } : f));
  assert.throws(
    () => collectPlatforms({ files: unsigned, version: "0.2.0", downloadBase: "https://d" }),
    /no \.sig/,
  );
  assert.throws(
    () =>
      collectPlatforms({
        files: [...files(), { name: "Nail_0.1.0_x64-setup.exe", signature: "s" }],
        version: "0.2.0",
        downloadBase: "https://d",
      }),
    /not version 0\.2\.0/,
  );
  assert.throws(
    () => collectPlatforms({ files: [], version: "0.2.0", downloadBase: "https://d" }),
    /no NSIS installer/,
  );
  assert.throws(
    () => collectPlatforms({ files: files(), version: "0.2.0", downloadBase: "ftp://d" }),
    /must be a URL/,
  );
});

test("the manifest: Tauri's static format, plain targets name NSIS", () => {
  const entries = collectPlatforms({ files: files(), version: "0.2.0", downloadBase: "https://d" });
  const manifest = buildManifest({
    version: "0.2.0",
    channel: "stable",
    notes: "## Fixed\n- things",
    pubDate: "2026-10-02T08:00:00.123Z",
    entries,
  });
  assert.equal(manifest.version, "0.2.0");
  assert.equal(manifest.pub_date, "2026-10-02T08:00:00Z");
  assert.equal(manifest.notes, "## Fixed\n- things");
  assert.deepEqual(Object.keys(manifest.platforms), [
    "windows-aarch64",
    "windows-aarch64-nsis",
    "windows-x86_64",
    "windows-x86_64-msi",
    "windows-x86_64-nsis",
  ]);
  assert.equal(manifest.platforms["windows-x86_64"].url, "https://d/Nail_0.2.0_x64-setup.exe");
  assert.equal(manifest.platforms["windows-x86_64-msi"].url, "https://d/Nail_0.2.0_x64_en-US.msi");
  // Ed25519 is deterministic: the same file signs the same way.
  assert.equal(
    manifest.platforms["windows-x86_64-nsis"].signature,
    signed("Nail_0.2.0_x64-setup.exe").trim(),
  );
  assert.deepEqual(manifest.platforms["windows-x86_64"], manifest.platforms["windows-x86_64-nsis"]);
  assert.ok(manifestJson(manifest).endsWith("}\n"));
  assert.deepEqual(JSON.parse(manifestJson(manifest)), manifest);
});

test("manifest rules: pre-releases only on beta, notes limited, dates valid", () => {
  const entries = collectPlatforms({
    files: [{ name: "Nail_1.0.0-beta.1_x64-setup.exe", signature: "s" }],
    version: "1.0.0-beta.1",
    downloadBase: "https://d",
  });
  const base = { version: "1.0.0-beta.1", pubDate: "2026-10-02T00:00:00Z", entries };
  assert.throws(() => buildManifest({ ...base, channel: "stable" }), /pre-release/);
  assert.equal(buildManifest({ ...base, channel: "beta" }).version, "1.0.0-beta.1");
  assert.throws(
    () => buildManifest({ ...base, channel: "beta", notes: "x".repeat(70000) }),
    /larger than/,
  );
  assert.throws(() => buildManifest({ ...base, channel: "beta", pubDate: "soon" }), /date/);
  assert.throws(() => buildManifest({ ...base, channel: "nightly" }), /unknown channel/);
  const twice = [...entries, { ...entries[0], name: "b" }];
  assert.throws(
    () => buildManifest({ ...base, channel: "beta", entries: twice }),
    /two nsis installers/,
  );
});

test("a stable release refreshes beta unless the beta is newer", () => {
  const manifest = { version: "1.0.0" };
  assert.deepEqual(
    manifestsForRelease({ manifest, channel: "stable" }).map((m) => m.path),
    ["stable/latest.json", "beta/latest.json"],
  );
  assert.deepEqual(
    manifestsForRelease({ manifest, channel: "stable", existingBeta: { version: "0.9.0" } }).map(
      (m) => m.path,
    ),
    ["stable/latest.json", "beta/latest.json"],
  );
  assert.deepEqual(
    manifestsForRelease({
      manifest,
      channel: "stable",
      existingBeta: { version: "1.1.0-beta.1" },
    }).map((m) => m.path),
    ["stable/latest.json"],
  );
  assert.deepEqual(
    manifestsForRelease({ manifest: { version: "1.1.0-beta.1" }, channel: "beta" }).map(
      (m) => m.path,
    ),
    ["beta/latest.json"],
  );
});
