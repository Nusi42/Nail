/**
 * Pure helpers of the release scripts (plan M2b WP44): where build output
 * goes, the Tauri config overlay for the installer, artifact names.
 * Covered by scripts/test/build.test.mjs (`node --test scripts/test/`).
 */
import path from "node:path";

/**
 * Base directory for build output, never inside the repository (it lives in
 * OneDrive): `NAIL_BUILD_DIR`, else `%LOCALAPPDATA%\nail-build`, else
 * `<tmp>/nail-build`. Same rule as e2e/support/nail.ts.
 */
export function buildBase(env, tmp) {
  if (env.NAIL_BUILD_DIR) return env.NAIL_BUILD_DIR;
  if (env.LOCALAPPDATA) return path.join(env.LOCALAPPDATA, "nail-build");
  return path.join(tmp, "nail-build");
}

/** Refuses output locations inside the repository. */
export function assertOutsideRepo(dir, repoRoot) {
  const rel = path.relative(path.resolve(repoRoot), path.resolve(dir));
  const inside = rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
  // `.nail-build/` in the checkout is the one exception: CI runners have no
  // OneDrive, and the directory is gitignored.
  if (inside && !rel.split(/[\\/]/)[0].startsWith(".nail-build")) {
    throw new Error(`build output must stay outside the repository: ${dir}`);
  }
}

/**
 * Command-line options of build-installer.mjs.
 * `--skip-ui`: use the existing ui/dist. `--target-dir DIR`: cargo target
 * directory (default `<base>/installer-target`). `--out DIR`: where the
 * artifacts go (default `<base>/dist`). `--no-cli`: installer without the
 * `nail` CLI (faster local builds).
 */
export function parseArgs(argv) {
  const options = { skipUi: false, targetDir: null, outDir: null, cli: true };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined || next.startsWith("--")) {
        throw new Error(`${arg} needs a value`);
      }
      return next;
    };
    if (arg === "--skip-ui") options.skipUi = true;
    else if (arg === "--no-cli") options.cli = false;
    else if (arg === "--target-dir") options.targetDir = value();
    else if (arg === "--out") options.outDir = value();
    else throw new Error(`unknown option ${arg}`);
  }
  return options;
}

/**
 * The Tauri config merged over tauri.conf.json for a release build
 * (`tauri build --config <file>`): the `nail` CLI goes to `bin\nail.exe`
 * (only `bin\` is put on PATH), and the UI is built before, not by Tauri.
 */
export function installerConfigOverlay({ cliPath }) {
  // `null` removes the key (JSON merge patch): no before-build command.
  const overlay = { build: { beforeBuildCommand: null } };
  if (cliPath) {
    overlay.bundle = { resources: { [cliPath]: "bin/nail.exe" } };
  }
  return overlay;
}

/** Tauri's architecture name for a Node `process.arch`. */
export function tauriArch(arch) {
  const map = { x64: "x64", arm64: "arm64", ia32: "x86" };
  const tauri = map[arch];
  if (!tauri) throw new Error(`unsupported architecture ${arch}`);
  return tauri;
}

/** File names of the release artifacts. */
export function artifactNames(version, arch) {
  const a = tauriArch(arch);
  return {
    // What the Tauri NSIS bundler writes.
    installer: `Nail_${version}_${a}-setup.exe`,
    portableZip: `Nail_${version}_${a}-portable.zip`,
    sbomRust: `Nail_${version}_${a}-rust.cdx.json`,
    sbomUi: `Nail_${version}_${a}-ui.cdx.json`,
    checksums: `Nail_${version}_${a}-SHA256SUMS.txt`,
  };
}

/**
 * Files of the portable zip, relative to its `Nail/` folder: the app, the
 * CLI in `bin\` (as in the installer), the UI files in `ui\` (the remote
 * UI and `nail serve` serve them from disk; the installer puts them at the
 * same place through tauri.conf.json `bundle.resources`) and the
 * `portable` marker that keeps data beside the executable
 * (app/src-tauri/src/portable.rs). `uiFiles` are paths relative to `uiDir`
 * (see `listFiles`); they must include `index.html`.
 */
export function portableLayout({ appExe, cliExe, uiDir, uiFiles }) {
  if (!uiFiles?.some((f) => f.split(/[\\/]/).join("/") === "index.html")) {
    throw new Error(`the UI build is missing (no index.html in ${uiDir})`);
  }
  const files = [{ from: appExe, to: "Nail.exe" }];
  if (cliExe) files.push({ from: cliExe, to: "bin/nail.exe" });
  for (const file of [...uiFiles].sort()) {
    files.push({ from: path.join(uiDir, file), to: `ui/${file.split(/[\\/]/).join("/")}` });
  }
  files.push({ content: "", to: "portable" });
  return files;
}

/** Every file below `dir`, relative to it, sorted (`readdir` = fs.readdirSync). */
export function listFiles(dir, readdir) {
  const out = [];
  const walk = (rel) => {
    for (const entry of readdir(path.join(dir, rel), { withFileTypes: true })) {
      const child = rel ? path.join(rel, entry.name) : entry.name;
      if (entry.isDirectory()) walk(child);
      else if (entry.isFile()) out.push(child);
    }
  };
  walk("");
  return out.sort();
}

/** `sha256sum`-style lines ("<hex>  <name>"), sorted by name. */
export function checksumLines(entries) {
  return [...entries]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((e) => `${e.sha256}  ${e.name}`)
    .join("\n")
    .concat("\n");
}
