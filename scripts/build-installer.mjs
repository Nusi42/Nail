#!/usr/bin/env node
/**
 * Builds Nail's Windows release artifacts (plan M2b WP44, M4b WP112):
 *
 *   Nail_<version>_<arch>-setup.exe           per-user NSIS installer (default)
 *   Nail_<version>_<arch>_en-US.msi           per-machine MSI (WiX) for IT
 *   Nail_<version>_<arch>-offline-setup.exe   NSIS with the full WebView2 runtime
 *                                             (--offline-webview)
 *   *.sig                                     update signatures (update key set)
 *   Nail_<version>_<arch>-portable.zip        Nail.exe + bin\nail.exe + ui\ + `portable`
 *   Nail_<version>_<arch>-{rust,ui}.cdx.json  CycloneDX SBOMs
 *   Nail_<version>_<arch>-SHA256SUMS.txt
 *
 * Usage: node scripts/build-installer.mjs [options] (see scripts/lib/build.mjs
 * `parseArgs`, or scripts/README.md).
 *
 * Output goes to <build dir>/dist (the gitignored .nail-build), never into tracked files;
 * the cargo target directory defaults to <build dir>/installer-target.
 * The Tauri bundler downloads NSIS, WiX 3.14 and the WebView2 installers
 * into %LOCALAPPDATA%\tauri on first use.
 *
 * Steps: UI (`npm run build -w ui`, unless --skip-ui) → `nail` CLI and the
 * MAPI DLLs (release, static C runtime, so neither needs the VC++
 * redistributable; the DLL is loaded into other programs) → `tauri build
 * --no-bundle` (Nail.exe) → portable copy of Nail.exe (before the bundler
 * marks the exe with its installer kind) → Authenticode for the files the
 * bundler does not sign (when configured) → `tauri bundle` (NSIS + MSI,
 * update signatures) → offline NSIS variant → portable zip → SBOMs →
 * checksums.
 *
 * Signing (scripts/lib/sign.mjs): only with NAIL_SIGN_THUMBPRINT; skipped
 * otherwise. Update signatures (scripts/lib/build.mjs updaterKeyFromEnv):
 * only with TAURI_SIGNING_PRIVATE_KEY (+ _PASSWORD) and the public key in
 * NAIL_UPDATER_PUBKEY / NAIL_UPDATER_PUBKEY_FILE, or --dev-updater-key.
 */
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  artifactNames,
  assertOutsideRepo,
  buildBase,
  checksumLines,
  keysDir,
  installerConfigOverlay,
  isOverlay,
  listFiles,
  mapiArchs,
  parseArgs,
  portableLayout,
  targetTriple,
  tauriArch,
  updaterKeyFromEnv,
  updaterPluginConfig,
  withoutUpdaterSecrets,
} from "./lib/build.mjs";
import { readJson } from "./lib/json.mjs";
import { findSigntool, signingFromEnv, signtoolArgs, tauriSigning } from "./lib/sign.mjs";
import { devKeyPaths } from "./lib/updater-key.mjs";
import { writeSboms } from "./sbom.mjs";
import { assertWorkFolder } from "./lib/work-folder.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const shell = process.platform === "win32";

/**
 * Quotes an argument for cmd.exe when needed.
 *
 * @param {string} arg
 */
function quote(arg) {
  return /[\s"&|<>^]/.test(arg) ? `"${arg.replaceAll('"', '""')}"` : arg;
}

/**
 * Runs a command with inherited output; throws when it fails.
 *
 * @param {string} command
 * @param {string[]} args
 * @param {{ env?: NodeJS.ProcessEnv, shell?: boolean }} [options]
 */
function run(command, args, options = {}) {
  const useShell = options.shell ?? shell;
  console.log(`> ${command} ${args.join(" ")}`);
  // Without an explicit env a child never sees the update key.
  /** @type {import("node:child_process").ExecFileSyncOptions} */
  const spawnOptions = {
    cwd: repoRoot,
    stdio: "inherit",
    ...options,
    env: options.env ?? withoutUpdaterSecrets(process.env),
    shell: useShell,
  };
  if (useShell) {
    // npm / npx are .cmd files: they need a shell, which takes one string.
    execFileSync([command, ...args].map(quote).join(" "), spawnOptions);
  } else {
    execFileSync(command, args, spawnOptions);
  }
}

/** @param {string} file */
function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/**
 * First existing path, or an error naming all candidates.
 *
 * @param {string[]} candidates
 * @param {string} what
 */
function firstExisting(candidates, what) {
  const found = candidates.find((c) => existsSync(c));
  if (!found) throw new Error(`${what} not found; looked for:\n  ${candidates.join("\n  ")}`);
  return found;
}

/** Rust targets installed through rustup (empty when rustup is missing). */
function installedTargets() {
  try {
    return execFileSync("rustup", ["target", "list", "--installed"], { encoding: "utf8" })
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean);
  } catch {
    return [];
  }
}

function main() {
  // Releases are built in a work folder on a `main` commit, never in the master.
  assertWorkFolder("node scripts/build-installer.mjs");
  if (process.platform !== "win32") {
    throw new Error("the Windows installers are built on Windows");
  }
  const options = parseArgs(process.argv.slice(2), { arch: tauriArch(process.arch) });
  const base = buildBase(process.env, repoRoot);
  const targetDir = path.resolve(options.targetDir ?? path.join(base, "installer-target"));
  const outDir = path.resolve(options.outDir ?? path.join(base, "dist"));
  assertOutsideRepo(targetDir, repoRoot);
  assertOutsideRepo(outDir, repoRoot);
  mkdirSync(outDir, { recursive: true });
  const ci = Boolean(process.env.CI);
  const locked = ci ? ["--locked"] : [];
  const triple = targetTriple(options.arch);
  const targets = installedTargets();
  if (targets.length && !targets.includes(triple)) {
    throw new Error(`Rust target ${triple} is not installed (rustup target add ${triple})`);
  }

  const appDir = path.join(repoRoot, "app", "src-tauri");
  const conf = /** @type {{ version: string }} */ (readJson(path.join(appDir, "tauri.conf.json")));
  const version = options.version ?? conf.version;
  const names = artifactNames(version, options.arch);
  const staticOverlay = /** @type {import("./lib/build.mjs").Overlay} */ (
    readJson(path.join(appDir, "windows", "bundle.json"))
  );

  // Authenticode: only with a configured certificate.
  const signing = signingFromEnv(process.env);
  const signtool = signing
    ? findSigntool({
        signing,
        programFilesX86: process.env["ProgramFiles(x86)"],
        readdir: (dir) => readdirSync(dir),
        exists: existsSync,
      })
    : null;
  console.log(
    signing
      ? `Authenticode: signing with certificate ${signing.thumbprint} (${signtool ?? ""})`
      : "Authenticode: no certificate configured (NAIL_SIGN_THUMBPRINT) - building unsigned",
  );
  /** @param {string} file */
  const sign = (file) => {
    if (signing && signtool) run(signtool, signtoolArgs(signing, file), { shell: false });
  };

  // Update key: the release key from the environment, or the local dev key.
  // Only `tauri bundle` gets it (bundleEnv); everything else runs with
  // buildEnv, which never holds it.
  const buildEnv = withoutUpdaterSecrets({ ...process.env, CARGO_TARGET_DIR: targetDir });
  /** @type {NodeJS.ProcessEnv} */
  const bundleEnv = { ...process.env, CARGO_TARGET_DIR: targetDir };
  /** @type {{ pubkey: string } | null} */
  let updaterKey;
  if (options.devUpdaterKey) {
    const dev = devKeyPaths({ keysDir: keysDir(process.env, tmpdir()), repoRoot });
    if (!existsSync(dev.privateKey)) {
      throw new Error(
        `no development update key at ${dev.privateKey}; run node scripts/updater-key.mjs`,
      );
    }
    bundleEnv.TAURI_SIGNING_PRIVATE_KEY = dev.privateKey;
    bundleEnv.TAURI_SIGNING_PRIVATE_KEY_PASSWORD = process.env.NAIL_DEV_UPDATER_KEY_PASSWORD ?? "";
    updaterKey = { pubkey: readFileSync(dev.publicKey, "utf8").trim() };
  } else {
    updaterKey = updaterKeyFromEnv(process.env, (file) => readFileSync(file, "utf8"));
  }
  const updateBase = options.updateBase ?? process.env.NAIL_UPDATE_BASE_URL ?? null;
  const updater = updaterKey
    ? updaterPluginConfig({
        pubkey: updaterKey.pubkey,
        updateBase,
        insecureLoopback: options.insecureLoopbackUpdates,
      })
    : null;
  if (!updater && (updateBase || options.insecureLoopbackUpdates)) {
    throw new Error(
      "--update-base needs an update key (TAURI_SIGNING_PRIVATE_KEY or --dev-updater-key)",
    );
  }
  console.log(
    updater
      ? `Updater: signed update artifacts; endpoint ${updater.endpoints[0] ?? "(none: not configured)"}`
      : "Updater: no update key configured - no update artifacts, updates not configured in the app",
  );

  if (!options.skipUi) run("npm", ["run", "build", "-w", "ui"], { env: buildEnv });
  if (!existsSync(path.join(repoRoot, "ui", "dist", "index.html"))) {
    throw new Error("ui/dist is missing; run without --skip-ui");
  }

  // The CLI and the MAPI DLLs, with the C runtime linked statically (their
  // own target directory: the flag changes every crate's fingerprint).
  const staticTarget = path.join(targetDir, "static-crt");
  const rustflags = [process.env.RUSTFLAGS, "-C target-feature=+crt-static"]
    .filter(Boolean)
    .join(" ");
  /** @param {string[]} args */
  const cargoStatic = (args) => {
    run("cargo", ["build", "--release", ...locked, ...args, "--target-dir", staticTarget], {
      env: { ...buildEnv, CARGO_TARGET_DIR: staticTarget, RUSTFLAGS: rustflags },
      shell: false,
    });
  };
  /** @type {string | null} */
  let cliExe = null;
  if (options.cli) {
    cargoStatic(["-p", "nail-cli", "--target", triple]);
    cliExe = path.join(staticTarget, triple, "release", "nail.exe");
    sign(cliExe);
  }
  /** @type {Record<string, string>} */
  const mapiDlls = {};
  if (options.mapi) {
    for (const arch of mapiArchs(options.arch)) {
      const dllTriple = targetTriple(arch);
      if (targets.length && !targets.includes(dllTriple)) {
        const message = `Rust target ${dllTriple} is not installed: no ${arch} NailMapi.dll`;
        if (ci) throw new Error(message);
        console.warn(`warning: ${message} (rustup target add ${dllTriple})`);
        continue;
      }
      cargoStatic(["-p", "nail-mapi", "--lib", "--target", dllTriple]);
      const built = path.join(staticTarget, dllTriple, "release", "nail_mapi.dll");
      // Shipped as NailMapi.dll (nail_api::mapi::MAPI_DLL_FILE).
      const staged = path.join(targetDir, "mapi-staging", arch, "NailMapi.dll");
      mkdirSync(path.dirname(staged), { recursive: true });
      copyFileSync(built, staged);
      sign(staged);
      mapiDlls[arch] = staged;
    }
  }

  // Tauri merges this over tauri.conf.json.
  mkdirSync(targetDir, { recursive: true });
  const overlay = installerConfigOverlay({
    cliPath: cliExe,
    mapiDlls,
    version: options.version,
    signing: tauriSigning(signing),
    updater,
    staticOverlay,
  });
  const overlayFile = path.join(targetDir, "nail-installer.conf.json");
  writeFileSync(overlayFile, JSON.stringify(overlay, null, 2));

  // 1. Nail.exe only.
  run(
    "npx",
    ["tauri", "build", "--no-bundle", "--ci", "--target", triple, "--config", overlayFile],
    {
      env: buildEnv,
    },
  );
  const release = path.join(targetDir, triple, "release");
  const appExe = firstExisting(
    [path.join(release, "Nail.exe"), path.join(release, "nail-app.exe")],
    "the app executable",
  );
  // The bundler marks the exe with its installer kind (the updater picks
  // its update by it); the portable copy is taken before that.
  const portableExe = path.join(targetDir, "portable-exe", "Nail.exe");
  mkdirSync(path.dirname(portableExe), { recursive: true });
  copyFileSync(appExe, portableExe);
  sign(portableExe);

  // 2. The installers (+ .sig with an update key).
  run(
    "npx",
    [
      "tauri",
      "bundle",
      "--ci",
      "--target",
      triple,
      "--bundles",
      options.bundles.join(","),
      "--config",
      overlayFile,
    ],
    { env: bundleEnv },
  );
  /** @type {string[]} */
  const artifacts = [];
  /**
   * @param {string} from
   * @param {string} name
   */
  const copyArtifact = (from, name) => {
    copyFileSync(from, path.join(outDir, name));
    artifacts.push(name);
    if (updater) {
      const sig = `${from}.sig`;
      if (!existsSync(sig)) throw new Error(`the bundler wrote no update signature for ${from}`);
      copyFileSync(sig, path.join(outDir, `${name}.sig`));
      artifacts.push(`${name}.sig`);
    }
  };
  if (options.bundles.includes("nsis")) {
    copyArtifact(
      firstExisting([path.join(release, "bundle", "nsis", names.installer)], "the NSIS installer"),
      names.installer,
    );
  }
  if (options.bundles.includes("msi")) {
    copyArtifact(
      firstExisting([path.join(release, "bundle", "msi", names.msi)], "the MSI"),
      names.msi,
    );
  }

  // 3. Offline WebView2 variant: same app, the runtime inside. Not an update
  // target (the updater keeps installs on the normal installer).
  if (options.offlineWebview) {
    const offlineOverlay = installerConfigOverlay({
      cliPath: cliExe,
      mapiDlls,
      version: options.version,
      signing: tauriSigning(signing),
      updater: updater && { ...updater },
      staticOverlay,
      webview: "offline",
    });
    // No second set of update signatures for the variant.
    if (isOverlay(offlineOverlay.bundle)) offlineOverlay.bundle.createUpdaterArtifacts = false;
    const offlineFile = path.join(targetDir, "nail-installer-offline.conf.json");
    writeFileSync(offlineFile, JSON.stringify(offlineOverlay, null, 2));
    run(
      "npx",
      ["tauri", "bundle", "--ci", "--target", triple, "--bundles", "nsis", "--config", offlineFile],
      {
        env: bundleEnv,
      },
    );
    copyFileSync(
      path.join(release, "bundle", "nsis", names.installer),
      path.join(outDir, names.offlineInstaller),
    );
    artifacts.push(names.offlineInstaller);
  }

  // 4. Portable zip: Nail/ with the app, bin\nail.exe, the UI files (ui\, as
  // the installer lays them out) and the marker.
  if (options.portable) {
    const staging = path.join(targetDir, "portable-staging");
    rmSync(staging, { recursive: true, force: true });
    const root = path.join(staging, "Nail");
    const uiDir = path.join(repoRoot, "ui", "dist");
    const uiFiles = existsSync(uiDir) ? listFiles(uiDir, readdirSync) : [];
    for (const file of portableLayout({ appExe: portableExe, cliExe, uiDir, uiFiles })) {
      const to = path.join(root, file.to);
      mkdirSync(path.dirname(to), { recursive: true });
      if ("from" in file) copyFileSync(file.from, to);
      else writeFileSync(to, file.content);
    }
    const zip = path.join(outDir, names.portableZip);
    rmSync(zip, { force: true });
    // Windows' bsdtar writes a standard zip ("/" separators); PowerShell 5's
    // Compress-Archive writes "\" and breaks other unzip tools.
    const tar = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
    run(tar, ["-a", "-c", "-f", zip, "-C", staging, "Nail"], { shell: false });
    artifacts.push(names.portableZip);
  }

  writeSboms({ outDir, target: triple, arch: options.arch, version });
  artifacts.push(names.sbomRust, names.sbomUi);

  writeFileSync(
    path.join(outDir, names.checksums),
    checksumLines(artifacts.map((name) => ({ name, sha256: sha256(path.join(outDir, name)) }))),
  );
  console.log(`\nArtifacts in ${outDir}:`);
  for (const name of [...artifacts, names.checksums]) console.log(`  ${name}`);
}

main();
