/**
 * Pure helpers of the release scripts (plan M2b WP44, M4b WP112): where
 * build output goes, the options of build-installer.mjs, the Tauri config
 * overlay for the installers (MSI, offline WebView2, MAPI DLLs, signing,
 * updater), artifact names. Covered by scripts/test/build.test.mjs
 * (`node --test scripts/test/`).
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

/** Bundles build-installer.mjs can make (Tauri bundler names). */
export const BUNDLES = ["nsis", "msi"];

/** Architectures Nail ships for Windows (design: x64 and ARM64). */
export const ARCHS = ["x64", "arm64"];

/**
 * Command-line options of build-installer.mjs:
 * - `--skip-ui`: use the existing ui/dist.
 * - `--target-dir DIR`: cargo target directory (default `<base>/installer-target`).
 * - `--out DIR`: where the artifacts go (default `<base>/dist`).
 * - `--no-cli`: installers without the `nail` CLI (faster local builds).
 * - `--arch x64|arm64`: the architecture (default: this machine's).
 * - `--bundles nsis,msi`: which installers (default both).
 * - `--offline-webview`: also the NSIS variant with the full WebView2 runtime.
 * - `--no-mapi`: without the Simple MAPI DLLs.
 * - `--no-portable`: without the portable zip.
 * - `--version V`: build as version V (smoke tests build a newer version).
 * - `--update-base URL`: where the update manifests live
 *   (`<URL>/<channel>/latest.json`; or `NAIL_UPDATE_BASE_URL`).
 * - `--insecure-loopback-updates`: allow `http://127.0.0.1` update URLs
 *   (the loopback smoke test only; refused for any other host).
 * - `--dev-updater-key`: sign updates with the local development key
 *   (scripts/updater-key.mjs; never for releases).
 */
export function parseArgs(argv, defaults = {}) {
  const options = {
    skipUi: false,
    targetDir: null,
    outDir: null,
    cli: true,
    arch: defaults.arch ?? "x64",
    bundles: [...BUNDLES],
    offlineWebview: false,
    mapi: true,
    portable: true,
    version: null,
    updateBase: null,
    insecureLoopbackUpdates: false,
    devUpdaterKey: false,
  };
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
    else if (arg === "--arch") options.arch = value();
    else if (arg === "--bundles")
      options.bundles = value()
        .split(",")
        .map((b) => b.trim());
    else if (arg === "--offline-webview") options.offlineWebview = true;
    else if (arg === "--no-mapi") options.mapi = false;
    else if (arg === "--no-portable") options.portable = false;
    else if (arg === "--version") options.version = value();
    else if (arg === "--update-base") options.updateBase = value();
    else if (arg === "--insecure-loopback-updates") options.insecureLoopbackUpdates = true;
    else if (arg === "--dev-updater-key") options.devUpdaterKey = true;
    else throw new Error(`unknown option ${arg}`);
  }
  if (!ARCHS.includes(options.arch)) throw new Error(`unsupported architecture ${options.arch}`);
  if (!options.bundles.length || options.bundles.some((b) => !BUNDLES.includes(b))) {
    throw new Error(`--bundles takes ${BUNDLES.join(", ")}`);
  }
  if (options.offlineWebview && !options.bundles.includes("nsis")) {
    throw new Error("--offline-webview needs the nsis bundle");
  }
  return options;
}

/** Rust target triple of an architecture. */
export function targetTriple(arch) {
  const map = {
    x64: "x86_64-pc-windows-msvc",
    arm64: "aarch64-pc-windows-msvc",
    x86: "i686-pc-windows-msvc",
  };
  if (!map[arch]) throw new Error(`unsupported architecture ${arch}`);
  return map[arch];
}

/**
 * Which NailMapi.dll builds an installer carries (design: x64, x86 for
 * 32-bit Office, ARM64): its own architecture plus x86 for the 32-bit
 * registry view.
 */
export function mapiArchs(arch) {
  if (!ARCHS.includes(arch)) throw new Error(`unsupported architecture ${arch}`);
  return [arch, "x86"];
}

/** `mapi/<arch>/NailMapi.dll` (nail_api::mapi::mapi_dll_relative_path). */
export function mapiResourcePath(arch) {
  return `mapi/${arch}/NailMapi.dll`;
}

/**
 * MSI ProductVersion for a semantic version: `major.minor.patch`, plus a
 * fourth field from a numeric last pre-release part (`1.2.0-beta.3` →
 * `1.2.0.3`). MSI fields are limited to 255.255.65535.65535.
 */
export function msiVersion(version) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/.exec(String(version));
  if (!m) throw new Error(`not a semantic version: ${version}`);
  const [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (major > 255 || minor > 255 || patch > 65535) {
    throw new Error(`version ${version} does not fit an MSI ProductVersion`);
  }
  let out = `${major}.${minor}.${patch}`;
  if (m[4]) {
    const last = m[4].split(".").pop();
    if (/^\d+$/.test(last) && Number(last) <= 65535) out += `.${Number(last)}`;
  }
  return out;
}

/** The next patch version (`0.1.0` → `0.1.1`; smoke tests' "newer build"). */
export function bumpPatch(version) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)/.exec(String(version));
  if (!m) throw new Error(`not a semantic version: ${version}`);
  return `${m[1]}.${m[2]}.${Number(m[3]) + 1}`;
}

/** `http(s)://127.0.0.1`, `[::1]` or `localhost`. */
export function isLoopbackUrl(text) {
  let url;
  try {
    url = new URL(text);
  } catch {
    return false;
  }
  return ["127.0.0.1", "[::1]", "localhost"].includes(url.hostname);
}

/**
 * `plugins.updater` of the app's config (tauri-plugin-updater):
 * the update key's public half, the stable manifest as the one endpoint
 * (`<base>/stable/latest.json`; the shell swaps the channel part for
 * `UpdateChannel::manifest_path`), passive install, and
 * `requireSignedVersion` (a manifest cannot pair a version with another
 * release's installer). Without a base URL there is no endpoint: the app
 * reports updates as not configured. Plain http is allowed only for a
 * loopback base with `insecureLoopback` (the smoke test's server).
 */
export function updaterPluginConfig({ pubkey, updateBase = null, insecureLoopback = false }) {
  if (!pubkey) throw new Error("the updater needs a public key");
  const config = {
    pubkey: pubkey.trim(),
    endpoints: [],
    requireSignedVersion: true,
    windows: { installMode: "passive" },
  };
  if (updateBase) {
    const base = updateBase.replace(/\/+$/, "");
    const url = new URL(`${base}/stable/latest.json`);
    if (url.protocol !== "https:") {
      if (!(insecureLoopback && url.protocol === "http:" && isLoopbackUrl(base))) {
        throw new Error(
          `update URLs must be https (http only for a loopback test server with --insecure-loopback-updates): ${base}`,
        );
      }
      config.dangerousInsecureTransportProtocol = true;
    }
    config.endpoints.push(url.toString());
  } else if (insecureLoopback) {
    throw new Error("--insecure-loopback-updates needs --update-base");
  }
  return config;
}

/**
 * Combines two config overlays into one: objects merge, everything else
 * replaces — including `null`, which must reach Tauri: it merges `--config`
 * files over tauri.conf.json as a JSON merge patch (RFC 7386), where `null`
 * removes the key.
 */
export function combineOverlays(a, b) {
  const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
  if (!isObject(a) || !isObject(b)) return b;
  const out = { ...a };
  for (const [key, value] of Object.entries(b)) {
    out[key] = key in out ? combineOverlays(out[key], value) : value;
  }
  return out;
}

/**
 * The Tauri config merged over tauri.conf.json for a release build
 * (`tauri build|bundle --config <file>`):
 * - `staticOverlay`: app/src-tauri/windows/bundle.json (the WiX fragment,
 *   upgrade code — settings every build shares);
 * - no before-build command (the UI is built first, by the script);
 * - the `nail` CLI → `bin\nail.exe` (only `bin\` goes on PATH), the MAPI
 *   DLLs → `mapi\<arch>\NailMapi.dll`;
 * - `version` (+ the MSI ProductVersion) when building another version;
 * - `webview: "offline"`: the full WebView2 runtime in the installer;
 * - `signing` (scripts/lib/sign.mjs `tauriSigning`): Authenticode through
 *   the bundler (app, installers, uninstaller);
 * - `updater` (`updaterPluginConfig`): the plug-in config and signed update
 *   artifacts (`.sig` next to each installer).
 */
export function installerConfigOverlay({
  cliPath = null,
  mapiDlls = {},
  version = null,
  webview = "bootstrapper",
  signing = null,
  updater = null,
  staticOverlay = {},
} = {}) {
  // `null` removes the key (JSON merge patch): no before-build command.
  let overlay = combineOverlays(staticOverlay, { build: { beforeBuildCommand: null } });
  const resources = {};
  if (cliPath) resources[cliPath] = "bin/nail.exe";
  for (const [arch, dll] of Object.entries(mapiDlls).sort(([a], [b]) => a.localeCompare(b))) {
    resources[dll] = mapiResourcePath(arch);
  }
  const patch = {};
  if (Object.keys(resources).length) patch.bundle = { resources };
  if (version) {
    patch.version = version;
    patch.bundle = combineOverlays(patch.bundle, {
      windows: { wix: { version: msiVersion(version) } },
    });
  }
  if (webview === "offline") {
    patch.bundle = combineOverlays(patch.bundle, {
      windows: { webviewInstallMode: { type: "offlineInstaller", silent: true } },
    });
  } else if (webview !== "bootstrapper") {
    throw new Error(`unknown WebView2 mode ${webview}`);
  }
  if (signing) patch.bundle = combineOverlays(patch.bundle, signing);
  if (updater) {
    patch.bundle = combineOverlays(patch.bundle, { createUpdaterArtifacts: true });
    patch.plugins = { updater };
  }
  overlay = combineOverlays(overlay, patch);
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
    // What the Tauri bundler writes.
    installer: `Nail_${version}_${a}-setup.exe`,
    msi: `Nail_${version}_${a}_en-US.msi`,
    // Renamed by build-installer.mjs (the bundler writes the same name).
    offlineInstaller: `Nail_${version}_${a}-offline-setup.exe`,
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
 * (see `listFiles`); they must include `index.html`. No MAPI DLLs: a
 * portable copy registers nothing.
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

/**
 * The update key for a build, from the environment:
 * `TAURI_SIGNING_PRIVATE_KEY` (key text or path; read by the Tauri CLI) and
 * the public half in `NAIL_UPDATER_PUBKEY` (text) or
 * `NAIL_UPDATER_PUBKEY_FILE` (path, read with `readFile`). Neither → `null`
 * (no update artifacts; the app reports updates as not configured). Only
 * one half → an error: a build must never embed a key it cannot sign for.
 */
export function updaterKeyFromEnv(env, readFile) {
  const hasPrivate = Boolean(env.TAURI_SIGNING_PRIVATE_KEY || env.TAURI_SIGNING_PRIVATE_KEY_PATH);
  let pubkey = env.NAIL_UPDATER_PUBKEY || null;
  if (!pubkey && env.NAIL_UPDATER_PUBKEY_FILE) pubkey = readFile(env.NAIL_UPDATER_PUBKEY_FILE);
  pubkey = pubkey?.trim() || null;
  if (!hasPrivate && !pubkey) return null;
  if (!hasPrivate) {
    throw new Error("NAIL_UPDATER_PUBKEY is set but TAURI_SIGNING_PRIVATE_KEY is not");
  }
  if (!pubkey) {
    throw new Error(
      "TAURI_SIGNING_PRIVATE_KEY is set but the public key is not (NAIL_UPDATER_PUBKEY or NAIL_UPDATER_PUBKEY_FILE)",
    );
  }
  return { pubkey };
}
