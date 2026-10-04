/**
 * Minisign keys and signatures as the Tauri updater uses them (plan M4b
 * WP112). The update manifests list one signature per installer; the app
 * (tauri-plugin-updater) checks it against the public key built into
 * Nail.exe. The release scripts check every signature before a manifest is
 * written, and the smoke test checks a downloaded update the way the app
 * does.
 *
 * Formats (https://jedisct1.github.io/minisign/):
 * - Tauri keeps keys and signatures as base64 of the whole minisign file
 *   text (`tauri signer generate` / `.sig` files of `tauri build`).
 * - Public key file: "untrusted comment: …\n" + base64("Ed" ‖ key id (8) ‖
 *   Ed25519 public key (32)).
 * - Signature file: "untrusted comment: …\n" + base64(algorithm ("ED" =
 *   BLAKE2b-512 prehashed, "Ed" = legacy) ‖ key id ‖ signature (64)) +
 *   "\ntrusted comment: …\n" + base64(global signature over signature ‖
 *   trusted comment).
 * - Tauri's trusted comment: `timestamp:<unix>\tfile:<name>\tversion:<v>`
 *   (the version since Tauri CLI 2.12; the plugin's `requireSignedVersion`
 *   rejects a manifest whose version differs from it).
 *
 * Pure Node (node:crypto Ed25519 + BLAKE2b-512); tests in
 * scripts/test/minisign.test.mjs, including a signature made by
 * `tauri signer sign`.
 */
import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";

const ALG_PREHASHED = "ED";
const ALG_LEGACY = "Ed";

/** @param {Uint8Array} bytes */
function b64url(bytes) {
  return Buffer.from(bytes).toString("base64url");
}

/**
 * Decodes Tauri's base64 wrapper; also accepts the plain minisign text.
 *
 * @param {string} text
 */
export function unwrapTauriText(text) {
  const trimmed = text.trim();
  if (trimmed.startsWith("untrusted comment:")) return trimmed;
  const decoded = Buffer.from(trimmed, "base64").toString("utf8");
  if (!decoded.startsWith("untrusted comment:")) {
    throw new Error("not a minisign key or signature (no untrusted comment)");
  }
  return decoded;
}

/**
 * Tauri's form of a minisign file: base64 of the text.
 *
 * @param {string} text
 */
export function wrapTauriText(text) {
  return Buffer.from(text, "utf8").toString("base64");
}

/** @param {string} text */
function lines(text) {
  return text
    .replace(/\r\n/g, "\n")
    .split("\n")
    .filter((l) => l.length > 0);
}

/**
 * Parses a public key (Tauri base64 or minisign text) → { keyId, publicKey }.
 *
 * @param {string} text
 */
export function parsePublicKey(text) {
  const [comment, data] = lines(unwrapTauriText(text));
  if (!comment?.startsWith("untrusted comment:") || !data) {
    throw new Error("malformed minisign public key");
  }
  const raw = Buffer.from(data, "base64");
  if (raw.length !== 42 || raw.subarray(0, 2).toString("latin1") !== ALG_LEGACY) {
    throw new Error("unsupported minisign public key");
  }
  return { keyId: raw.subarray(2, 10), publicKey: raw.subarray(10, 42) };
}

/**
 * Parses a signature (Tauri base64 or minisign text) →
 * { algorithm, keyId, signature, trustedComment, globalSignature }.
 *
 * @param {string} text
 */
export function parseSignature(text) {
  const [untrusted, data, trusted, global] = lines(unwrapTauriText(text));
  if (!untrusted?.startsWith("untrusted comment:") || !data || !trusted || !global) {
    throw new Error("malformed minisign signature");
  }
  const prefix = "trusted comment: ";
  if (!trusted.startsWith(prefix)) throw new Error("malformed minisign trusted comment");
  const raw = Buffer.from(data, "base64");
  const algorithm = raw.subarray(0, 2).toString("latin1");
  if (raw.length !== 74 || (algorithm !== ALG_PREHASHED && algorithm !== ALG_LEGACY)) {
    throw new Error("unsupported minisign signature");
  }
  const globalSignature = Buffer.from(global, "base64");
  if (globalSignature.length !== 64) throw new Error("malformed minisign global signature");
  return {
    algorithm,
    keyId: raw.subarray(2, 10),
    signature: raw.subarray(10, 74),
    trustedComment: trusted.slice(prefix.length),
    globalSignature,
  };
}

/**
 * The tab-separated `key:value` fields of Tauri's trusted comment.
 *
 * @param {string} trustedComment
 * @returns {Partial<Record<string, string>>}
 */
export function trustedFields(trustedComment) {
  /** @type {Partial<Record<string, string>>} */
  const fields = {};
  for (const part of trustedComment.split("\t")) {
    const colon = part.indexOf(":");
    if (colon > 0) fields[part.slice(0, colon)] = part.slice(colon + 1);
  }
  return fields;
}

/** @param {Uint8Array} raw */
function ed25519PublicKey(raw) {
  return createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: b64url(raw) }, format: "jwk" });
}

/** @param {Uint8Array} data */
function blake2b512(data) {
  return createHash("blake2b512").update(data).digest();
}

/**
 * Checks `data` against a signature and public key the way the Tauri
 * updater does: key ids match, the signature covers the data (BLAKE2b-512
 * prehashed or legacy), the global signature covers the trusted comment.
 * With `version`, the signed version (trusted comment) must be present and
 * equal (`requireSignedVersion`). Returns the trusted comment's fields;
 * throws with a reason otherwise.
 *
 * @param {{ data: Uint8Array, signature: string, publicKey: string, version?: string }} options
 */
export function verifySignature({ data, signature, publicKey, version }) {
  const key = parsePublicKey(publicKey);
  const sig = parseSignature(signature);
  if (!key.keyId.equals(sig.keyId)) {
    throw new Error(
      `signed with another key (key id ${hex(sig.keyId)}, expected ${hex(key.keyId)})`,
    );
  }
  const keyObject = ed25519PublicKey(key.publicKey);
  const message = sig.algorithm === ALG_PREHASHED ? blake2b512(data) : Buffer.from(data);
  if (!verify(null, message, keyObject, sig.signature)) {
    throw new Error("the signature does not match the file");
  }
  const globalMessage = Buffer.concat([sig.signature, Buffer.from(sig.trustedComment, "utf8")]);
  if (!verify(null, globalMessage, keyObject, sig.globalSignature)) {
    throw new Error("the trusted comment was changed");
  }
  const fields = trustedFields(sig.trustedComment);
  if (version !== undefined) {
    if (!fields.version) throw new Error("the signature carries no version");
    if (stripV(fields.version) !== stripV(version)) {
      throw new Error(`the signature is for version ${fields.version}, not ${version}`);
    }
  }
  return fields;
}

/** @param {string} version */
function stripV(version) {
  return version.trim().replace(/^v/, "");
}

/** @param {Uint8Array} bytes */
function hex(bytes) {
  return Buffer.from(bytes).toString("hex").toUpperCase();
}

/**
 * Makes a key pair in Tauri's public-key form plus a signer (tests and the
 * loopback smoke server only: real keys come from `tauri signer generate`,
 * whose secret key is password-protected). `keyPair` is an Ed25519 pair
 * from `crypto.generateKeyPairSync("ed25519")`.
 *
 * @param {{ privateKey: import("node:crypto").KeyObject }} keyPair
 * @param {Buffer} [keyId]
 */
export function testKey(keyPair, keyId = Buffer.from("0123456789abcdef", "hex")) {
  const jwk = keyPair.privateKey.export({ format: "jwk" });
  const raw = Buffer.from(jwk.x ?? "", "base64url");
  const pubText =
    `untrusted comment: minisign public key: ${hex(keyId)}\n` +
    `${Buffer.concat([Buffer.from(ALG_LEGACY, "latin1"), keyId, raw]).toString("base64")}\n`;
  const privateKey = createPrivateKey({ key: jwk, format: "jwk" });
  return {
    publicKey: wrapTauriText(pubText),
    /**
     * Signs like `tauri signer sign` (prehashed, Tauri's trusted comment).
     *
     * @param {Uint8Array} data
     * @param {{ file?: string, version?: string, timestamp?: number }} [options]
     */
    sign(data, { file = "file", version, timestamp = 0 } = {}) {
      const signature = sign(null, blake2b512(data), privateKey);
      let trusted = `timestamp:${String(timestamp)}\tfile:${file}`;
      if (version !== undefined) trusted += `\tversion:${version}`;
      const global = sign(
        null,
        Buffer.concat([signature, Buffer.from(trusted, "utf8")]),
        privateKey,
      );
      const text =
        "untrusted comment: signature from tauri secret key\n" +
        `${Buffer.concat([Buffer.from(ALG_PREHASHED, "latin1"), keyId, signature]).toString("base64")}\n` +
        `trusted comment: ${trusted}\n` +
        `${global.toString("base64")}\n`;
      return wrapTauriText(text);
    },
  };
}
