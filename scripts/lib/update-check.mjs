/**
 * What the app's updater does with a channel manifest, outside the app
 * (plan M4b WP112): the installer smoke test uses it to update an
 * installed build through the loopback update server exactly as
 * tauri-plugin-updater would — pick `windows-<arch>-<installer>`, else
 * `windows-<arch>`; only a newer version; the download must carry a valid
 * signature of the update key for the announced version
 * (`requireSignedVersion`). WP115's desktop scenario covers the app's own
 * client. Tests: scripts/test/update-check.test.mjs.
 */
import { verifySignature } from "./minisign.mjs";
import { compareVersions, updaterArch } from "./update-manifest.mjs";
import { isLoopbackUrl } from "./build.mjs";

/**
 * An update the manifest announces.
 *
 * @typedef {{ available: true, version: string, notes: string, url: string, signature: string,
 *   target: string }} Update
 */

/**
 * The `fetch` subset these helpers use.
 *
 * @typedef {(url: string, init?: { headers?: Record<string, string> }) => Promise<{
 *   ok: boolean, status: number, json(): Promise<unknown>,
 *   arrayBuffer(): Promise<ArrayBuffer> }>} Fetch
 */

/**
 * Whether `value` is a JSON object.
 *
 * @param {unknown} value
 * @returns {value is Record<string, unknown>}
 */
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * https, or http on loopback only (the app's rule with the test flag).
 *
 * @param {string} url
 */
export function checkEndpoint(url) {
  const parsed = new URL(url);
  if (parsed.protocol === "https:") return parsed;
  if (parsed.protocol === "http:" && isLoopbackUrl(url)) return parsed;
  throw new Error(`update endpoints must be https (or http on loopback): ${url}`);
}

/**
 * Reads the manifest and picks the update: `{ available: false }` or
 * `{ available: true, version, notes, url, signature, target }`.
 * `fetchImpl` = fetch.
 *
 * @param {{ endpoint: string, currentVersion: string, arch: string, installer: string,
 *   fetchImpl?: Fetch }} options
 * @returns {Promise<{ available: false, version?: undefined, target?: undefined } | Update>}
 */
export async function checkForUpdate({
  endpoint,
  currentVersion,
  arch,
  installer,
  fetchImpl = fetch,
}) {
  checkEndpoint(endpoint);
  const res = await fetchImpl(endpoint, { headers: { accept: "application/json" } });
  if (res.status === 204 || res.status === 404) return { available: false };
  if (!res.ok) throw new Error(`manifest request failed: HTTP ${String(res.status)}`);
  const manifest = await res.json();
  if (
    !isRecord(manifest) ||
    typeof manifest.version !== "string" ||
    !manifest.version ||
    !isRecord(manifest.platforms)
  ) {
    throw new Error("malformed manifest");
  }
  if (compareVersions(manifest.version, currentVersion) <= 0) return { available: false };
  const base = `windows-${updaterArch(arch)}`;
  for (const target of [`${base}-${installer}`, base]) {
    const entry = manifest.platforms[target];
    if (
      isRecord(entry) &&
      typeof entry.url === "string" &&
      entry.url &&
      typeof entry.signature === "string" &&
      entry.signature
    ) {
      checkEndpoint(entry.url);
      return {
        available: true,
        version: manifest.version,
        notes: typeof manifest.notes === "string" ? manifest.notes : "",
        url: entry.url,
        signature: entry.signature,
        target,
      };
    }
  }
  throw new Error(`the manifest has no entry for ${base}-${installer} or ${base}`);
}

/**
 * Downloads the update and checks its signature; resolves the bytes.
 *
 * @param {{ update: Pick<Update, "url" | "signature" | "version">, publicKey: string,
 *   fetchImpl?: Fetch }} options
 */
export async function downloadVerified({ update, publicKey, fetchImpl = fetch }) {
  const res = await fetchImpl(update.url);
  if (!res.ok) throw new Error(`download failed: HTTP ${String(res.status)}`);
  const data = Buffer.from(await res.arrayBuffer());
  verifySignature({ data, signature: update.signature, publicKey, version: update.version });
  return data;
}
