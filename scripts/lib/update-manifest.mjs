/**
 * Update manifests of the Tauri updater, one per channel (plan M4b WP112,
 * ADR 0087): `<base>/<channel>/latest.json` (`nail_api::desktop::
 * UpdateChannel::manifest_path`), Tauri's static JSON format:
 *
 *   { "version": "0.2.0", "notes": "<Markdown>", "pub_date": "<RFC 3339>",
 *     "platforms": { "windows-x86_64-nsis": { "url": "…", "signature": "…" },
 *                    "windows-x86_64-msi": …, "windows-x86_64": <nsis>, … } }
 *
 * The updater in Nail.exe looks up `windows-<arch>-<installer>` first (the
 * installer kind the bundler built into the exe), then `windows-<arch>`,
 * which therefore always names the NSIS installer — an MSI install never
 * gets an NSIS update. Every signature is checked against the update key
 * (and the version it was signed for) before a manifest is produced, so a
 * manifest never announces an installer the app would refuse.
 *
 * Pure helpers; the CLI is scripts/update-manifest.mjs, tests in
 * scripts/test/update-manifest.test.mjs.
 */
import { verifySignature } from "./minisign.mjs";

/**
 * One installer of a release in a manifest.
 *
 * @typedef {object} PlatformEntry
 * @property {string} arch
 * @property {string} installer
 * @property {string} name
 * @property {string} url
 * @property {string} signature
 */

/**
 * A channel manifest (Tauri static format).
 *
 * @typedef {object} Manifest
 * @property {string} version
 * @property {string} notes
 * @property {string} pub_date
 * @property {Record<string, { signature: string, url: string }>} platforms
 */

/** `nail_api::desktop::UpdateChannel` names. */
export const CHANNELS = ["stable", "beta"];

/** `nail_api::desktop::UPDATE_MANIFEST_FILE`. */
export const MANIFEST_FILE = "latest.json";

/** Release notes are Markdown shown as untrusted text; keep them small. */
export const MAX_NOTES_BYTES = 64 * 1024;

/**
 * `<channel>/latest.json` (`UpdateChannel::manifest_path`).
 *
 * @param {string} channel
 */
export function manifestPath(channel) {
  if (!CHANNELS.includes(channel)) throw new Error(`unknown channel ${channel}`);
  return `${channel}/${MANIFEST_FILE}`;
}

/**
 * Semantic version → parts; throws for anything else.
 *
 * @param {string} text
 * @returns {{ major: number, minor: number, patch: number, pre: string[] }}
 */
export function parseVersion(text) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text.trim());
  if (!m) throw new Error(`not a semantic version: ${text}`);
  const pre = m[4] ? m[4].split(".") : [];
  if (pre.some((p) => p === "" || (/^\d+$/.test(p) && p.length > 1 && p.startsWith("0")))) {
    throw new Error(`not a semantic version: ${text}`);
  }
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]), pre };
}

/**
 * Semver precedence: negative, 0 or positive.
 *
 * @param {string} a
 * @param {string} b
 */
export function compareVersions(a, b) {
  const x = parseVersion(a);
  const y = parseVersion(b);
  for (const k of /** @type {const} */ (["major", "minor", "patch"])) {
    if (x[k] !== y[k]) return x[k] - y[k];
  }
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const pNumeric = /^\d+$/.test(p);
    const qNumeric = /^\d+$/.test(q);
    if (pNumeric && qNumeric && Number(p) !== Number(q)) return Number(p) - Number(q);
    if (pNumeric !== qNumeric) return pNumeric ? -1 : 1;
    if (p !== q) return p < q ? -1 : 1;
  }
  return 0;
}

/** @type {Partial<Record<string, string>>} */
const UPDATER_ARCHS = { x64: "x86_64", arm64: "aarch64", x86: "i686" };

/**
 * The Tauri updater's arch name for an artifact arch (`x64`, `arm64`, `x86`).
 *
 * @param {string} arch
 * @returns {string}
 */
export function updaterArch(arch) {
  const name = UPDATER_ARCHS[arch];
  if (!name) throw new Error(`unsupported architecture ${arch}`);
  return name;
}

/**
 * Which installer an artifact file is, from the bundler's names
 * (scripts/lib/build.mjs `artifactNames`): `Nail_<v>_<arch>-setup.exe` →
 * nsis, `Nail_<v>_<arch>_<lang>.msi` → msi. The offline-WebView2 variant,
 * the portable zip and everything else is not an update (`null`).
 *
 * @param {string} name
 * @returns {{ version: string, arch: string, installer: "nsis" | "msi" } | null}
 */
export function classifyArtifact(name) {
  let m = /^Nail_(.+)_(x64|arm64|x86)-setup\.exe$/.exec(name);
  if (m?.[1] && m[2]) return { version: m[1], arch: m[2], installer: "nsis" };
  m = /^Nail_(.+)_(x64|arm64|x86)_([A-Za-z]{2}-[A-Za-z]{2})\.msi$/.exec(name);
  if (m?.[1] && m[2]) return { version: m[1], arch: m[2], installer: "msi" };
  return null;
}

/**
 * The update entries of a release: every installer with a `.sig` beside it.
 * `files` = [{ name, signature? }] (signature = the `.sig` text);
 * `downloadBase` = where the installers are published (no trailing slash
 * needed). Installers of another version or without a signature are
 * errors: a release is published whole or not at all.
 *
 * @param {{ files: { name: string, signature?: string | null }[], version: string,
 *   downloadBase: string }} options
 * @returns {PlatformEntry[]}
 */
export function collectPlatforms({ files, version, downloadBase }) {
  const base = downloadBase.replace(/\/+$/, "");
  if (!/^https?:\/\//.test(base)) throw new Error(`download base must be a URL: ${downloadBase}`);
  /** @type {PlatformEntry[]} */
  const entries = [];
  for (const file of files) {
    const kind = classifyArtifact(file.name);
    if (!kind) continue;
    if (kind.version !== version) {
      throw new Error(`${file.name} is not version ${version}`);
    }
    if (!file.signature) throw new Error(`${file.name} has no .sig (updater key not configured?)`);
    entries.push({
      arch: kind.arch,
      installer: kind.installer,
      name: file.name,
      url: `${base}/${encodeURIComponent(file.name)}`,
      signature: file.signature.trim(),
    });
  }
  if (!entries.some((e) => e.installer === "nsis")) {
    throw new Error("no NSIS installer with a signature among the artifacts");
  }
  return entries;
}

/**
 * Checks every entry's signature against the update key and the version
 * it was signed for. `read(name)` returns the installer's bytes.
 *
 * @param {{ entries: PlatformEntry[], publicKey: string, version: string,
 *   read: (name: string) => Uint8Array }} options
 */
export function verifyPlatforms({ entries, publicKey, version, read }) {
  for (const entry of entries) {
    try {
      verifySignature({ data: read(entry.name), signature: entry.signature, publicKey, version });
    } catch (e) {
      throw new Error(`${entry.name}: ${e instanceof Error ? e.message : String(e)}`, {
        cause: e,
      });
    }
  }
}

/**
 * One channel's manifest object (Tauri static format).
 *
 * @param {{ version: string, channel: string, notes?: string, pubDate: string,
 *   entries: PlatformEntry[] }} options
 * @returns {Manifest}
 */
export function buildManifest({ version, channel, notes = "", pubDate, entries }) {
  parseVersion(version);
  if (!CHANNELS.includes(channel)) throw new Error(`unknown channel ${channel}`);
  if (channel === "stable" && parseVersion(version).pre.length) {
    throw new Error(`a pre-release (${version}) cannot go to the stable channel`);
  }
  if (Buffer.byteLength(notes, "utf8") > MAX_NOTES_BYTES) {
    throw new Error(`release notes are larger than ${String(MAX_NOTES_BYTES)} bytes`);
  }
  const date = new Date(pubDate);
  if (Number.isNaN(date.getTime())) throw new Error(`invalid publication date ${pubDate}`);
  /** @type {Record<string, { signature: string, url: string }>} */
  const platforms = {};
  const seen = new Set();
  for (const e of [...entries].sort((a, b) => a.name.localeCompare(b.name))) {
    const target = `windows-${updaterArch(e.arch)}`;
    const key = `${target}-${e.installer}`;
    if (seen.has(key)) throw new Error(`two ${e.installer} installers for ${e.arch}`);
    seen.add(key);
    platforms[key] = { signature: e.signature, url: e.url };
    // Plain `windows-<arch>`: the NSIS installer (an MSI install always finds
    // its own `-msi` entry first).
    if (e.installer === "nsis") platforms[target] = { signature: e.signature, url: e.url };
  }
  return {
    version: version.replace(/^v/, ""),
    notes,
    pub_date: date.toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: Object.fromEntries(Object.entries(platforms).sort(([a], [b]) => a.localeCompare(b))),
  };
}

/**
 * Which channel manifests a release replaces: a beta release only
 * `beta/latest.json`; a stable release `stable/latest.json` and also
 * `beta/latest.json` unless the current beta (`existingBeta`, the parsed
 * manifest or null) is newer — beta users always get the newest build.
 *
 * @template {{ version: string }} M
 * @param {{ manifest: M, channel: string, existingBeta?: { version: string } | null }} options
 * @returns {{ path: string, manifest: M }[]}
 */
export function manifestsForRelease({ manifest, channel, existingBeta = null }) {
  const out = [{ path: manifestPath(channel), manifest }];
  if (channel === "stable") {
    const betaNewer = existingBeta && compareVersions(existingBeta.version, manifest.version) > 0;
    if (!betaNewer) out.push({ path: manifestPath("beta"), manifest });
  }
  return out;
}

/**
 * Stable JSON text of a manifest (2-space indent, final newline).
 *
 * @param {Manifest} manifest
 */
export function manifestJson(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}
