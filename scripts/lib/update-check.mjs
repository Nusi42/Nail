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

/** https, or http on loopback only (the app's rule with the test flag). */
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
  if (!res.ok) throw new Error(`manifest request failed: HTTP ${res.status}`);
  const manifest = await res.json();
  if (!manifest?.version || typeof manifest.platforms !== "object") {
    throw new Error("malformed manifest");
  }
  if (compareVersions(manifest.version, currentVersion) <= 0) return { available: false };
  const base = `windows-${updaterArch(arch)}`;
  for (const target of [`${base}-${installer}`, base]) {
    const entry = manifest.platforms[target];
    if (entry?.url && entry?.signature) {
      checkEndpoint(entry.url);
      return {
        available: true,
        version: manifest.version,
        notes: manifest.notes ?? "",
        url: entry.url,
        signature: entry.signature,
        target,
      };
    }
  }
  throw new Error(`the manifest has no entry for ${base}-${installer} or ${base}`);
}

/** Downloads the update and checks its signature; resolves the bytes. */
export async function downloadVerified({ update, publicKey, fetchImpl = fetch }) {
  const res = await fetchImpl(update.url);
  if (!res.ok) throw new Error(`download failed: HTTP ${res.status}`);
  const data = Buffer.from(await res.arrayBuffer());
  verifySignature({ data, signature: update.signature, publicKey, version: update.version });
  return data;
}
