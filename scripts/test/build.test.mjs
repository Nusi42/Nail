// Tests of scripts/lib/build.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";

import {
  artifactNames,
  assertOutsideRepo,
  buildBase,
  bumpPatch,
  checksumLines,
  combineOverlays,
  installerConfigOverlay,
  isLoopbackUrl,
  keysDir,
  listFiles,
  mapiArchs,
  mapiResourcePath,
  msiVersion,
  parseArgs,
  portableLayout,
  targetTriple,
  tauriArch,
  updaterKeyFromEnv,
  updaterPluginConfig,
} from "../lib/build.mjs";

test("build output goes to NAIL_BUILD_DIR, else the checkout's .nail-build", () => {
  assert.equal(buildBase({ NAIL_BUILD_DIR: "X", LOCALAPPDATA: "L" }, "R"), "X");
  assert.equal(buildBase({ LOCALAPPDATA: "L" }, "R"), path.join("R", ".nail-build"));
  assert.equal(buildBase({}, "R"), path.join("R", ".nail-build"));
});

test("signing keys stay outside every checkout", () => {
  assert.equal(
    keysDir({ NAIL_BUILD_DIR: "X", LOCALAPPDATA: "L" }, "T"),
    path.join("L", "nail-build", "keys"),
  );
  assert.equal(keysDir({}, "T"), path.join("T", "nail-build", "keys"));
});

test("output inside the repository is refused, except .nail-build", () => {
  const repo = path.resolve("repo");
  assert.throws(() => {
    assertOutsideRepo(repo, repo);
  }, /outside the repository/);
  assert.throws(() => {
    assertOutsideRepo(path.join(repo, "target"), repo);
  }, /outside/);
  assert.doesNotThrow(() => {
    assertOutsideRepo(path.join(repo, ".nail-build", "dist"), repo);
  });
  assert.doesNotThrow(() => {
    assertOutsideRepo(path.resolve("elsewhere"), repo);
  });
  assert.doesNotThrow(() => {
    assertOutsideRepo(path.resolve("repo-2"), repo);
  });
});

test("options", () => {
  const defaults = parseArgs([]);
  assert.equal(defaults.skipUi, false);
  assert.equal(defaults.cli, true);
  assert.equal(defaults.arch, "x64");
  assert.deepEqual(defaults.bundles, ["nsis", "msi"]);
  assert.equal(defaults.offlineWebview, false);
  assert.equal(defaults.mapi, true);
  assert.equal(defaults.portable, true);
  assert.equal(defaults.version, null);
  assert.equal(defaults.updateBase, null);
  assert.equal(defaults.devUpdaterKey, false);
  assert.equal(parseArgs([], { arch: "arm64" }).arch, "arm64");

  const all = parseArgs([
    "--skip-ui",
    "--no-cli",
    "--target-dir",
    "t",
    "--out",
    "o",
    "--arch",
    "arm64",
    "--bundles",
    "nsis",
    "--offline-webview",
    "--no-mapi",
    "--no-portable",
    "--version",
    "0.2.0",
    "--update-base",
    "http://127.0.0.1:1",
    "--insecure-loopback-updates",
    "--dev-updater-key",
  ]);
  assert.deepEqual(all, {
    skipUi: true,
    targetDir: "t",
    outDir: "o",
    cli: false,
    arch: "arm64",
    bundles: ["nsis"],
    offlineWebview: true,
    mapi: false,
    portable: false,
    version: "0.2.0",
    updateBase: "http://127.0.0.1:1",
    insecureLoopbackUpdates: true,
    devUpdaterKey: true,
  });
  assert.deepEqual(parseArgs(["--bundles", "msi, nsis"]).bundles, ["msi", "nsis"]);
  assert.throws(() => parseArgs(["--out"]), /needs a value/);
  assert.throws(() => parseArgs(["--out", "--skip-ui"]), /needs a value/);
  assert.throws(() => parseArgs(["--sign"]), /unknown option/);
  assert.throws(() => parseArgs(["--arch", "x86"]), /unsupported architecture/);
  assert.throws(() => parseArgs(["--bundles", "nsis,dmg"]), /--bundles/);
  assert.throws(() => parseArgs(["--bundles", "msi", "--offline-webview"]), /nsis/);
});

test("targets and MAPI DLLs per architecture", () => {
  assert.equal(targetTriple("x64"), "x86_64-pc-windows-msvc");
  assert.equal(targetTriple("arm64"), "aarch64-pc-windows-msvc");
  assert.equal(targetTriple("x86"), "i686-pc-windows-msvc");
  assert.throws(() => targetTriple("mips"), /unsupported/);
  // The package's own DLL plus x86 for 32-bit Office.
  assert.deepEqual(mapiArchs("x64"), ["x64", "x86"]);
  assert.deepEqual(mapiArchs("arm64"), ["arm64", "x86"]);
  assert.throws(() => mapiArchs("x86"), /unsupported/);
  assert.equal(mapiResourcePath("arm64"), "mapi/arm64/NailMapi.dll");
});

test("MSI versions and the smoke test's newer version", () => {
  assert.equal(msiVersion("0.1.0"), "0.1.0");
  assert.equal(msiVersion("v1.2.3"), "1.2.3");
  assert.equal(msiVersion("1.2.0-beta.3"), "1.2.0.3");
  assert.equal(msiVersion("1.2.0-rc"), "1.2.0");
  assert.equal(msiVersion("1.2.0+build.7"), "1.2.0");
  assert.throws(() => msiVersion("256.0.0"), /MSI ProductVersion/);
  assert.throws(() => msiVersion("1.2"), /semantic/);
  assert.equal(bumpPatch("0.1.0"), "0.1.1");
  assert.equal(bumpPatch("1.9.9-beta.1"), "1.9.10");
  assert.throws(() => bumpPatch("x"), /semantic/);
});

test("loopback URLs", () => {
  assert.ok(isLoopbackUrl("http://127.0.0.1:47811/"));
  assert.ok(isLoopbackUrl("http://localhost/x"));
  assert.ok(isLoopbackUrl("http://[::1]:5/"));
  assert.ok(!isLoopbackUrl("http://127.0.0.2/"));
  assert.ok(!isLoopbackUrl("http://example.org/"));
  assert.ok(!isLoopbackUrl("not a url"));
});

test("the updater config: https, loopback http only on request, signed versions", () => {
  assert.deepEqual(
    updaterPluginConfig({ pubkey: " KEY\n", updateBase: "https://u.example/nail/" }),
    {
      pubkey: "KEY",
      endpoints: ["https://u.example/nail/stable/latest.json"],
      requireSignedVersion: true,
      windows: { installMode: "passive" },
    },
  );
  // No base URL: no endpoint (the app reports "not configured").
  assert.deepEqual(updaterPluginConfig({ pubkey: "KEY" }).endpoints, []);
  assert.throws(() => updaterPluginConfig({ pubkey: "" }), /public key/);
  assert.throws(
    () => updaterPluginConfig({ pubkey: "K", updateBase: "http://u.example" }),
    /must be https/,
  );
  assert.throws(
    () => updaterPluginConfig({ pubkey: "K", updateBase: "http://127.0.0.1:47811" }),
    /must be https/,
  );
  assert.throws(
    () =>
      updaterPluginConfig({ pubkey: "K", updateBase: "http://u.example", insecureLoopback: true }),
    /must be https/,
  );
  assert.throws(
    () => updaterPluginConfig({ pubkey: "K", insecureLoopback: true }),
    /needs --update-base/,
  );
  const loopback = updaterPluginConfig({
    pubkey: "K",
    updateBase: "http://127.0.0.1:47811",
    insecureLoopback: true,
  });
  assert.equal(loopback.dangerousInsecureTransportProtocol, true);
  assert.deepEqual(loopback.endpoints, ["http://127.0.0.1:47811/stable/latest.json"]);
});

test("overlays combine and keep null (Tauri removes those keys)", () => {
  assert.deepEqual(combineOverlays({ a: { b: 1 } }, { a: { c: null }, d: [2] }), {
    a: { b: 1, c: null },
    d: [2],
  });
  assert.deepEqual(combineOverlays(undefined, { a: 1 }), { a: 1 });
  assert.deepEqual(combineOverlays({ a: [1] }, { a: [2] }), { a: [2] });
});

test("the overlay: no UI build, CLI in bin, MAPI DLLs in mapi", () => {
  assert.deepEqual(installerConfigOverlay({ cliPath: "C:\\t\\nail.exe" }), {
    build: { beforeBuildCommand: null },
    bundle: { resources: { "C:\\t\\nail.exe": "bin/nail.exe" } },
  });
  assert.deepEqual(installerConfigOverlay({ cliPath: null }), {
    build: { beforeBuildCommand: null },
  });
  const withDlls = installerConfigOverlay({
    cliPath: "n.exe",
    mapiDlls: { x86: "x86.dll", x64: "x64.dll" },
  });
  assert.deepEqual(/** @type {{ resources?: unknown }} */ (withDlls.bundle).resources, {
    "n.exe": "bin/nail.exe",
    "x64.dll": "mapi/x64/NailMapi.dll",
    "x86.dll": "mapi/x86/NailMapi.dll",
  });
});

test("the overlay: static settings, version, offline WebView2, signing, updater", () => {
  const staticOverlay = {
    bundle: { windows: { wix: { upgradeCode: "U", fragmentPaths: ["f.wxs"] } } },
  };
  const overlay = installerConfigOverlay({
    staticOverlay,
    version: "1.2.0-beta.3",
    webview: "offline",
    signing: { windows: { certificateThumbprint: "T" } },
    updater: { pubkey: "K", endpoints: [] },
  });
  assert.deepEqual(overlay, {
    bundle: {
      windows: {
        wix: { upgradeCode: "U", fragmentPaths: ["f.wxs"], version: "1.2.0.3" },
        webviewInstallMode: { type: "offlineInstaller", silent: true },
        certificateThumbprint: "T",
      },
      createUpdaterArtifacts: true,
    },
    build: { beforeBuildCommand: null },
    version: "1.2.0-beta.3",
    plugins: { updater: { pubkey: "K", endpoints: [] } },
  });
  // The static overlay itself is not changed.
  assert.deepEqual(staticOverlay.bundle.windows.wix, {
    upgradeCode: "U",
    fragmentPaths: ["f.wxs"],
  });
  assert.throws(() => installerConfigOverlay({ webview: "fixed" }), /WebView2 mode/);
});

test("the update key comes as a pair or not at all", () => {
  /** @param {string} f */
  const read = (f) => `pub from ${f}\n`;
  assert.equal(updaterKeyFromEnv({}, read), null);
  assert.deepEqual(
    updaterKeyFromEnv({ TAURI_SIGNING_PRIVATE_KEY: "k", NAIL_UPDATER_PUBKEY: " P " }, read),
    { pubkey: "P" },
  );
  assert.deepEqual(
    updaterKeyFromEnv({ TAURI_SIGNING_PRIVATE_KEY_PATH: "k", NAIL_UPDATER_PUBKEY_FILE: "f" }, read),
    { pubkey: "pub from f" },
  );
  assert.throws(
    () => updaterKeyFromEnv({ NAIL_UPDATER_PUBKEY: "P" }, read),
    /TAURI_SIGNING_PRIVATE_KEY is not/,
  );
  assert.throws(
    () => updaterKeyFromEnv({ TAURI_SIGNING_PRIVATE_KEY: "k" }, read),
    /public key is not/,
  );
});

test("artifact names follow the Tauri bundler", () => {
  assert.equal(tauriArch("x64"), "x64");
  assert.equal(tauriArch("arm64"), "arm64");
  assert.throws(() => tauriArch("mips"), /unsupported/);
  assert.deepEqual(artifactNames("0.1.0", "x64"), {
    installer: "Nail_0.1.0_x64-setup.exe",
    msi: "Nail_0.1.0_x64_en-US.msi",
    offlineInstaller: "Nail_0.1.0_x64-offline-setup.exe",
    portableZip: "Nail_0.1.0_x64-portable.zip",
    sbomRust: "Nail_0.1.0_x64-rust.cdx.json",
    sbomUi: "Nail_0.1.0_x64-ui.cdx.json",
    checksums: "Nail_0.1.0_x64-SHA256SUMS.txt",
  });
  assert.equal(artifactNames("0.2.0", "arm64").msi, "Nail_0.2.0_arm64_en-US.msi");
});

test("the portable zip has the marker, the CLI in bin and the UI in ui", () => {
  const ui = { uiDir: "dist", uiFiles: ["index.html", path.join("assets", "app.js")] };
  assert.deepEqual(portableLayout({ appExe: "a.exe", cliExe: "n.exe", ...ui }), [
    { from: "a.exe", to: "Nail.exe" },
    { from: "n.exe", to: "bin/nail.exe" },
    { from: path.join("dist", "assets", "app.js"), to: "ui/assets/app.js" },
    { from: path.join("dist", "index.html"), to: "ui/index.html" },
    { content: "", to: "portable" },
  ]);
  assert.equal(portableLayout({ appExe: "a.exe", cliExe: null, ...ui }).length, 4);
});

test("the portable zip refuses a missing UI build", () => {
  assert.throws(
    () => portableLayout({ appExe: "a.exe", cliExe: null, uiDir: "dist", uiFiles: [] }),
    /UI build is missing/,
  );
});

test("listFiles walks a directory tree", () => {
  const tree = {
    root: [
      { name: "index.html", isDirectory: () => false, isFile: () => true },
      { name: "assets", isDirectory: () => true, isFile: () => false },
    ],
    [path.join("root", "assets")]: [
      { name: "b.js", isDirectory: () => false, isFile: () => true },
      { name: "a.css", isDirectory: () => false, isFile: () => true },
    ],
  };
  /** @param {string} dir */
  const readdir = (dir) => tree[path.normalize(dir)] ?? [];
  assert.deepEqual(listFiles("root", readdir), [
    path.join("assets", "a.css"),
    path.join("assets", "b.js"),
    "index.html",
  ]);
});

test("checksum lines are sorted and sha256sum-compatible", () => {
  assert.equal(
    checksumLines([
      { name: "b", sha256: "22" },
      { name: "a", sha256: "11" },
    ]),
    "11  a\n22  b\n",
  );
});

test("the update key reaches only tauri bundle", async () => {
  const { withoutUpdaterSecrets, UPDATER_SECRET_VARS } = await import("../lib/build.mjs");
  const env = {
    PATH: "p",
    TAURI_SIGNING_PRIVATE_KEY: "secret",
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "pw",
    tauri_signing_private_key_path: "k",
    NAIL_UPDATER_PUBKEY: "pub",
  };
  const clean = withoutUpdaterSecrets(env);
  assert.deepEqual(clean, { PATH: "p", NAIL_UPDATER_PUBKEY: "pub" });
  assert.equal(env.TAURI_SIGNING_PRIVATE_KEY, "secret", "input untouched");
  assert.equal(UPDATER_SECRET_VARS.length, 3);

  // build-installer.mjs: every child process but `tauri bundle` gets the
  // clean environment.
  const { readFileSync } = await import("node:fs");
  const script = readFileSync(new URL("../build-installer.mjs", import.meta.url), "utf8");
  const runs = [...script.matchAll(/run\(\s*"(npx|npm|cargo)",\s*\[([^\]]*)\]/g)];
  assert.ok(runs.length >= 4, "found the child processes");
  for (const [call, command, args = ""] of runs) {
    const start = script.indexOf(call);
    const rest = script.slice(start, script.indexOf(");", start));
    if (command === "npx" && args.includes('"bundle"')) {
      assert.match(rest, /env: bundleEnv/, call);
    } else {
      assert.doesNotMatch(rest, /bundleEnv|process\.env/, call);
    }
  }
  assert.doesNotMatch(script, /test-overrides/, "no test-only feature in installer builds");
});

test("release.yml hands the update key to the build step only", async () => {
  const { readFileSync } = await import("node:fs");
  const workflow = readFileSync(
    new URL("../../.github/workflows/release.yml", import.meta.url),
    "utf8",
  );
  const lines = workflow.split(/\r?\n/);
  const uses = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line }) => /^\s*TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)?:/.test(line));
  assert.equal(uses.length, 2);
  for (const { line, index } of uses) {
    // Step-level env: indented below a step, and that step is the build.
    assert.match(line, /^ {10}\S/, line);
    const step = lines
      .slice(0, index)
      .reverse()
      .find((l) => /^ {6}- name:/.test(l));
    assert.equal(step?.trim(), "- name: Build installers");
  }
});
