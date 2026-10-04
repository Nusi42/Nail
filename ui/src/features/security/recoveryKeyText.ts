/**
 * The vault recovery key's text form (mirrors `nail_api::recovery`,
 * ADR 0094): 32 bytes as Crockford base32 (52 characters) plus 4 check
 * characters (the top 20 bits of the CRC-32 of the bytes), in 14 groups of
 * 4 joined by `-`. Parsing forgives case, spaces and dashes and reads `O`
 * as `0`, `I` / `L` as `1`. The UI checks what is typed before asking the
 * core, so a typo is reported at once and never counts as a wrong key.
 */
import type { RecoveryKeyIssue } from "../../api/generated";

export const RECOVERY_KEY_BYTES = 32;
export const RECOVERY_KEY_CHARS = 56;
const DATA_CHARS = 52;
const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export const RECOVERY_KEY_ISSUES: readonly RecoveryKeyIssue[] = [
  "empty",
  "invalid-character",
  "wrong-length",
  "checksum-mismatch",
];

/** Fluent id of an issue (`recovery-key-issue-*`, `core-vault.ftl`). */
export const recoveryIssueId = (issue: RecoveryKeyIssue) => `recovery-key-issue-${issue}`;

/** CRC-32 (IEEE 802.3), as `nail_api::recovery`. */
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const b of bytes) {
    crc ^= b;
    for (let i = 0; i < 8; i++) {
      const mask = -(crc & 1);
      crc = (crc >>> 1) ^ (0xedb88320 & mask);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/** The text form of 32 bytes. */
export function formatRecoveryKey(key: Uint8Array): string {
  if (key.length !== RECOVERY_KEY_BYTES) throw new Error("a recovery key has 32 bytes");
  let plain = "";
  let acc = 0;
  let bits = 0;
  for (const b of key) {
    acc = ((acc << 8) | b) & 0xffff;
    bits += 8;
    while (bits >= 5 && plain.length < DATA_CHARS) {
      bits -= 5;
      plain += ALPHABET.charAt((acc >>> bits) & 31);
    }
  }
  if (bits > 0 && plain.length < DATA_CHARS) plain += ALPHABET.charAt((acc << (5 - bits)) & 31);
  const check = crc32(key) >>> 12;
  for (const shift of [15, 10, 5, 0]) plain += ALPHABET.charAt((check >>> shift) & 31);
  return (plain.match(/.{1,4}/g) ?? []).join("-");
}

function valueOf(ch: string): number {
  const upper = ch.toUpperCase();
  const c = upper === "O" ? "0" : upper === "I" || upper === "L" ? "1" : upper;
  return ALPHABET.indexOf(c);
}

export type ParsedRecoveryKey =
  { ok: true; bytes: Uint8Array } | { ok: false; issue: RecoveryKeyIssue };

/** Reads a typed key back into its bytes, or says what is wrong. */
export function parseRecoveryKey(text: string): ParsedRecoveryKey {
  const values: number[] = [];
  for (const ch of text) {
    if (/\s/.test(ch) || ch === "-") continue;
    const v = valueOf(ch);
    if (v < 0) return { ok: false, issue: "invalid-character" };
    values.push(v);
  }
  if (values.length === 0) return { ok: false, issue: "empty" };
  if (values.length !== RECOVERY_KEY_CHARS) return { ok: false, issue: "wrong-length" };
  const bytes = new Uint8Array(RECOVERY_KEY_BYTES);
  let acc = 0;
  let bits = 0;
  let index = 0;
  for (const v of values.slice(0, DATA_CHARS)) {
    acc = ((acc << 5) | v) & 0xffff;
    bits += 5;
    if (bits >= 8 && index < RECOVERY_KEY_BYTES) {
      bits -= 8;
      bytes[index++] = (acc >>> bits) & 0xff;
    }
  }
  // The 4 padding bits must be zero.
  if ((acc & ((1 << bits) - 1)) !== 0) return { ok: false, issue: "checksum-mismatch" };
  const check = values.slice(DATA_CHARS).reduce((a, v) => (a << 5) | v, 0);
  if (check !== crc32(bytes) >>> 12) return { ok: false, issue: "checksum-mismatch" };
  return { ok: true, bytes };
}

/** The typed key in its canonical form (for comparing), or `null`. */
export function normalizeRecoveryKey(text: string): string | null {
  const parsed = parseRecoveryKey(text);
  return parsed.ok ? formatRecoveryKey(parsed.bytes) : null;
}

/** The groups of a formatted key (for display in rows). */
export function recoveryKeyGroups(key: string): string[] {
  return key.split("-");
}
