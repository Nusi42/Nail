/**
 * Where the development update key lives (plan M4b WP112; key handling in
 * packaging/README.md). The private key never enters the repository: it
 * is generated into `<build dir>/keys/` (default
 * `%LOCALAPPDATA%\nail-build\keys`), and only its public half is committed
 * as packaging/updater/dev.pub, so development builds and the loopback
 * smoke test can be checked by anyone. Release builds use the release key
 * from CI secrets instead (TAURI_SIGNING_PRIVATE_KEY, NAIL_UPDATER_PUBKEY).
 */
import path from "node:path";

/** Paths of the development key pair. */
export function devKeyPaths({ base, repoRoot }) {
  const privateKey = path.join(base, "keys", "nail-updater-dev.key");
  return {
    privateKey,
    /** What `tauri signer generate -w <privateKey>` writes next to it. */
    generatedPublicKey: `${privateKey}.pub`,
    /** The committed public half (embedded by --dev-updater-key builds). */
    publicKey: path.join(repoRoot, "packaging", "updater", "dev.pub"),
  };
}
