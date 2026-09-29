#!/usr/bin/env node
/**
 * Builds Nail's Windows release artifacts (plan M2b WP44):
 *
 *   Nail_<version>_<arch>-setup.exe      per-user NSIS installer (Tauri bundler)
 *   Nail_<version>_<arch>-portable.zip   Nail.exe + bin\nail.exe + `portable` marker
 *   Nail_<version>_<arch>-{rust,ui}.cdx.json   CycloneDX SBOMs
 *   Nail_<version>_<arch>-SHA256SUMS.txt
 *
 * Usage: node scripts/build-installer.mjs [--skip-ui] [--no-cli]
 *                                         [--target-dir DIR] [--out DIR]
 *
 * Output goes to <build dir>/dist, never into the repository (OneDrive);
 * the cargo target directory defaults to <build dir>/installer-target.
 * The Tauri bundler downloads NSIS and the WebView2 bootstrapper into
 * %LOCALAPPDATA%\tauri on its first run.
 *
 * Steps: UI (`npm run build -w ui`, unless --skip-ui) → `nail` CLI
 * (release, static C runtime, so bin\nail.exe needs no VC++ redistributable;
 * Nail.exe gets the same from tauri-build) → `tauri build --bundles nsis`
 * with an overlay config that adds the CLI → portable zip → SBOMs →
 * checksums.
 */
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  artifactNames,
  assertOutsideRepo,
  buildBase,
  checksumLines,
  installerConfigOverlay,
  parseArgs,
  portableLayout,
} from "./lib/build.mjs";
import { writeSboms } from "./sbom.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const shell = process.platform === "win32";

/** Quotes an argument for cmd.exe when needed. */
function quote(arg) {
  return /[\s"&|<>^]/.test(arg) ? `"${arg.replaceAll('"', '""')}"` : arg;
}

function run(command, args, options = {}) {
  const useShell = options.shell ?? shell;
  console.log(`> ${command} ${args.join(" ")}`);
  const spawnOptions = { cwd: repoRoot, stdio: "inherit", ...options, shell: useShell };
  if (useShell) {
    // npm / npx are .cmd files: they need a shell, which takes one string.
    execFileSync([command, ...args].map(quote).join(" "), spawnOptions);
  } else {
    execFileSync(command, args, spawnOptions);
  }
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/** First existing path, or an error naming all candidates. */
function firstExisting(candidates, what) {
  const found = candidates.find((c) => existsSync(c));
  if (!found) throw new Error(`${what} not found; looked for:\n  ${candidates.join("\n  ")}`);
  return found;
}

function main() {
  if (process.platform !== "win32") {
    throw new Error("the NSIS installer is built on Windows");
  }
  const options = parseArgs(process.argv.slice(2));
  const base = buildBase(process.env, tmpdir());
  const targetDir = path.resolve(options.targetDir ?? path.join(base, "installer-target"));
  const outDir = path.resolve(options.outDir ?? path.join(base, "dist"));
  assertOutsideRepo(targetDir, repoRoot);
  assertOutsideRepo(outDir, repoRoot);
  mkdirSync(outDir, { recursive: true });

  const conf = JSON.parse(
    readFileSync(path.join(repoRoot, "app", "src-tauri", "tauri.conf.json"), "utf8"),
  );
  const version = conf.version;
  const names = artifactNames(version, process.arch);

  if (!options.skipUi) run("npm", ["run", "build", "-w", "ui"]);
  if (!existsSync(path.join(repoRoot, "ui", "dist", "index.html"))) {
    throw new Error("ui/dist is missing; run without --skip-ui");
  }

  // The CLI, with the C runtime linked statically (its own target
  // directory: the flag changes every crate's fingerprint).
  let cliExe = null;
  if (options.cli) {
    const cliTarget = path.join(targetDir, "cli-static");
    const rustflags = [process.env.RUSTFLAGS, "-C target-feature=+crt-static"]
      .filter(Boolean)
      .join(" ");
    const locked = process.env.CI ? ["--locked"] : [];
    run("cargo", ["build", "--release", ...locked, "-p", "nail-cli", "--target-dir", cliTarget], {
      env: { ...process.env, RUSTFLAGS: rustflags },
      shell: false,
    });
    cliExe = path.join(cliTarget, "release", "nail.exe");
  }

  // Tauri merges this over tauri.conf.json.
  const overlayFile = path.join(targetDir, "nail-installer.conf.json");
  mkdirSync(targetDir, { recursive: true });
  writeFileSync(overlayFile, JSON.stringify(installerConfigOverlay({ cliPath: cliExe }), null, 2));
  run("npx", ["tauri", "build", "--bundles", "nsis", "--ci", "--config", overlayFile], {
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
  });

  const release = path.join(targetDir, "release");
  const installer = firstExisting(
    [path.join(release, "bundle", "nsis", names.installer)],
    "the NSIS installer",
  );
  copyFileSync(installer, path.join(outDir, names.installer));

  // Portable zip: Nail/ with the app, bin\nail.exe and the marker.
  const appExe = firstExisting(
    [path.join(release, "Nail.exe"), path.join(release, "nail-app.exe")],
    "the app executable",
  );
  const staging = path.join(targetDir, "portable-staging");
  rmSync(staging, { recursive: true, force: true });
  const root = path.join(staging, "Nail");
  for (const file of portableLayout({ appExe, cliExe })) {
    const to = path.join(root, file.to);
    mkdirSync(path.dirname(to), { recursive: true });
    if (file.from) copyFileSync(file.from, to);
    else writeFileSync(to, file.content);
  }
  const zip = path.join(outDir, names.portableZip);
  rmSync(zip, { force: true });
  // Windows' bsdtar writes a standard zip ("/" separators); PowerShell 5's
  // Compress-Archive writes "\" and breaks other unzip tools.
  const tar = path.join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
  run(tar, ["-a", "-c", "-f", zip, "-C", staging, "Nail"], { shell: false });

  writeSboms({ outDir, arch: process.arch });

  const artifacts = [names.installer, names.portableZip, names.sbomRust, names.sbomUi];
  writeFileSync(
    path.join(outDir, names.checksums),
    checksumLines(artifacts.map((name) => ({ name, sha256: sha256(path.join(outDir, name)) }))),
  );
  console.log(`\nArtifacts in ${outDir}:`);
  for (const name of [...artifacts, names.checksums]) console.log(`  ${name}`);
}

main();
