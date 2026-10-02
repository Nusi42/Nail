/**
 * Authenticode signing of the Windows artifacts (plan M4b WP112, scope
 * decision "signing steps that run only when a certificate is configured";
 * the EV certificate comes later). Signing is configured by environment
 * variables only — nothing secret is ever in the repository:
 *
 *   NAIL_SIGN_THUMBPRINT     SHA-1 thumbprint of a code-signing certificate
 *                            in the current user's or machine's store (a
 *                            PFX imported by CI, or an EV token's certificate)
 *   NAIL_SIGN_TIMESTAMP_URL  RFC 3161 timestamp server (default below)
 *   NAIL_SIGN_DESCRIPTION    the signature's description (default "Nail")
 *   NAIL_SIGNTOOL            signtool.exe to use (default: newest Windows SDK)
 *
 * Without NAIL_SIGN_THUMBPRINT nothing is signed and the build says so; an
 * unsigned build is a normal development build. The Tauri bundler signs
 * Nail.exe, the installers and the uninstaller (`tauriSigning`);
 * build-installer.mjs signs the rest (bin\nail.exe, the MAPI DLLs, the
 * portable copy of Nail.exe) with `signtoolArgs` before they are bundled.
 * Tests: scripts/test/sign.test.mjs.
 */
import path from "node:path";

export const DEFAULT_TIMESTAMP_URL = "http://timestamp.digicert.com";

/** The signing settings, or `null` when no certificate is configured. */
export function signingFromEnv(env) {
  const thumbprint = (env.NAIL_SIGN_THUMBPRINT ?? "").replace(/[\s:]/g, "").toUpperCase();
  if (!thumbprint) return null;
  if (!/^[0-9A-F]{40}$/.test(thumbprint)) {
    throw new Error("NAIL_SIGN_THUMBPRINT must be a 40-digit SHA-1 thumbprint");
  }
  const timestampUrl = env.NAIL_SIGN_TIMESTAMP_URL || DEFAULT_TIMESTAMP_URL;
  if (!/^https?:\/\//.test(timestampUrl)) {
    throw new Error(`NAIL_SIGN_TIMESTAMP_URL must be a URL: ${timestampUrl}`);
  }
  return {
    thumbprint,
    timestampUrl,
    description: env.NAIL_SIGN_DESCRIPTION || "Nail",
    signtool: env.NAIL_SIGNTOOL || null,
  };
}

/** The bundler's part (`bundle.windows` of the config overlay). */
export function tauriSigning(signing) {
  if (!signing) return null;
  return {
    windows: {
      certificateThumbprint: signing.thumbprint,
      digestAlgorithm: "sha256",
      timestampUrl: signing.timestampUrl,
      tsp: true,
    },
  };
}

/** `signtool sign` arguments for one file (SHA-256, RFC 3161 timestamp). */
export function signtoolArgs(signing, file) {
  return [
    "sign",
    "/sha1",
    signing.thumbprint,
    "/fd",
    "sha256",
    "/tr",
    signing.timestampUrl,
    "/td",
    "sha256",
    "/d",
    signing.description,
    file,
  ];
}

/** `signtool verify` arguments (Authenticode policy, all signatures). */
export function verifyArgs(file) {
  return ["verify", "/pa", "/all", file];
}

/**
 * signtool.exe: `NAIL_SIGNTOOL`, else the newest Windows 10/11 SDK's x64
 * build. `readdir(dir)` lists a directory (names), `exists(file)` tests a
 * file; `programFilesX86` is `%ProgramFiles(x86)%`.
 */
export function findSigntool({ signing, programFilesX86, readdir, exists }) {
  if (signing?.signtool) return signing.signtool;
  const bin = path.join(programFilesX86 ?? "C:\\Program Files (x86)", "Windows Kits", "10", "bin");
  let versions = [];
  try {
    versions = readdir(bin).filter((v) => /^10\.\d+\.\d+\.\d+$/.test(v));
  } catch {
    versions = [];
  }
  versions.sort((a, b) => {
    const x = a.split(".").map(Number);
    const y = b.split(".").map(Number);
    for (let i = 0; i < 4; i++) if (x[i] !== y[i]) return y[i] - x[i];
    return 0;
  });
  for (const version of versions) {
    const candidate = path.join(bin, version, "x64", "signtool.exe");
    if (exists(candidate)) return candidate;
  }
  throw new Error(`signtool.exe not found under ${bin}; set NAIL_SIGNTOOL`);
}
