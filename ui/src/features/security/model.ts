/**
 * Pure logic of the security UI: badge verdicts, trust order, recipient
 * key summaries, the composer's send guard, formatting. No React here, so
 * every rule is unit-tested on its own (`model.test.ts`).
 */
import {
  ATTACHMENT_RISK_VALUES,
  AUTH_VERDICT_VALUES,
  CRYPTO_PREFERENCE_VALUES,
  CRYPTO_TECHNOLOGY_VALUES,
  MESSAGE_WARNING_KINDS,
  TRUST_DECISION_VALUES,
  TRUST_LEVEL_VALUES,
} from "../../api/generated";
import type {
  AttachmentRisk,
  AuthResults,
  AuthVerdict,
  CryptoKey,
  CryptoPreference,
  CryptoTechnology,
  IdentityCrypto,
  MessageWarning,
  RecipientKey,
  RecipientKeyStatus,
  RecipientKeysReport,
  SecurityStatus,
  SendCrypto,
  SignatureInfo,
  TrustDecision,
  TrustLevel,
} from "../../api/generated";
import { DATE_ONLY, DATE_TIME, formatDateTime as formatInLocale } from "../../i18n/format";

// ---------------------------------------------------------------------------
// Constants (also the source of runtime-built message ids)

/** Trust levels, worst → best (the order of `TrustLevel` in the core). */
export const TRUST_LEVELS: readonly TrustLevel[] = TRUST_LEVEL_VALUES;

export const TRUST_DECISIONS: readonly TrustDecision[] = TRUST_DECISION_VALUES;
export const TECHNOLOGIES: readonly CryptoTechnology[] = CRYPTO_TECHNOLOGY_VALUES;
export const PREFERENCES: readonly CryptoPreference[] = CRYPTO_PREFERENCE_VALUES;

/** The badge's signature verdicts, mildest first (the summary of several signatures is the most severe). */
export const SIGNATURE_VERDICTS = [
  "none",
  "verified",
  "valid",
  "unknown-key",
  "error",
  "key-problem",
  "mismatch",
  "invalid",
] as const;
export type SignatureVerdict = (typeof SIGNATURE_VERDICTS)[number];

export const ENCRYPTION_VERDICTS = ["none", "decrypted", "locked", "no-key", "failed"] as const;
export type EncryptionVerdict = (typeof ENCRYPTION_VERDICTS)[number];

/** How alarming a badge is (`data-tone`, styled by the theme). */
export type Tone = "good" | "neutral" | "warning" | "danger";

/** `IdentityCrypto::default()` of the core. */
export function defaultIdentityCrypto(): IdentityCrypto {
  return {
    technology: "auto",
    sign: false,
    encrypt: false,
    openpgp_key: null,
    smime_certificate: null,
    autocrypt: true,
    prefer_encrypt: "nopreference",
    protect_headers: true,
  };
}

/** Whether a crypto setting equals the defaults (the core then omits it). */
export function isDefaultCrypto(crypto: IdentityCrypto): boolean {
  const d = defaultIdentityCrypto();
  return (Object.keys(d) as (keyof IdentityCrypto)[]).every((k) => d[k] === crypto[k]);
}

// ---------------------------------------------------------------------------
// Trust

export function trustRank(level: TrustLevel): number {
  return TRUST_LEVELS.indexOf(level);
}

/** Whether a trust level means "this key belongs to the address". */
export function isTrusted(level: TrustLevel): boolean {
  return trustRank(level) >= trustRank("autocrypt");
}

/** The stored decision a key's effective trust reflects (for the editor). */
export function decisionOf(trust: TrustLevel): TrustDecision {
  if (trust === "verified") return "verified";
  if (trust === "rejected") return "rejected";
  return "unset";
}

// ---------------------------------------------------------------------------
// Message security badge

function signatureVerdict(sig: SignatureInfo): SignatureVerdict {
  switch (sig.state) {
    case "valid": {
      if (!sig.signer_matches_from) return "mismatch";
      if (sig.trust === "rejected") return "invalid";
      const chainOk = sig.chain === null || sig.chain.state === "valid";
      if (sig.chain !== null) return chainOk ? "verified" : "valid";
      return isTrusted(sig.trust) ? "verified" : "valid";
    }
    case "invalid":
      return "invalid";
    case "unknown-key":
      return "unknown-key";
    case "key-expired":
    case "key-revoked":
      return "key-problem";
    case "unsupported":
    case "error":
      return "error";
  }
}

const SIGNATURE_SEVERITY: readonly SignatureVerdict[] = SIGNATURE_VERDICTS;

/** What the badge shows for a message's security status. */
export interface SecurityVerdict {
  signature: SignatureVerdict;
  encryption: EncryptionVerdict;
  /** Only part of the message is protected. */
  partial: boolean;
  /** Protected headers differ from the outer ones. */
  headerMismatch: boolean;
  tone: Tone;
}

export function securityVerdict(status: SecurityStatus): SecurityVerdict {
  // The worst signature decides (several signatures are rare, but one bad
  // one must not hide behind a good one).
  let signature: SignatureVerdict = "none";
  for (const sig of status.signatures) {
    const v = signatureVerdict(sig);
    if (SIGNATURE_SEVERITY.indexOf(v) > SIGNATURE_SEVERITY.indexOf(signature)) signature = v;
  }
  let encryption: EncryptionVerdict = "none";
  if (status.encryption) {
    const s = status.encryption.state;
    encryption = s === "decrypted" ? "decrypted" : s === "unsupported" ? "failed" : s;
  }
  const headerMismatch = status.header_protection === "mismatch";
  let tone: Tone;
  if (signature === "invalid" || encryption === "failed") tone = "danger";
  else if (
    status.partial ||
    headerMismatch ||
    ["mismatch", "key-problem", "error"].includes(signature) ||
    encryption === "no-key" ||
    encryption === "locked"
  )
    tone = "warning";
  else if (signature === "verified" || (signature === "none" && encryption === "decrypted"))
    tone = "good";
  else tone = "neutral";
  return { signature, encryption, partial: status.partial, headerMismatch, tone };
}

// ---------------------------------------------------------------------------
// Sender authentication (DKIM / SPF / DMARC)

export const AUTH_VERDICTS: readonly AuthVerdict[] = AUTH_VERDICT_VALUES;

/** The badge verdict, `AuthResults::summary` (pinned by `model.vectors.test.ts`). */
export function authSummary(results: AuthResults): AuthVerdict {
  if (results.dmarc) return results.dmarc.verdict;
  if (results.dkim.some((d) => d.verdict === "pass")) return "pass";
  const first = results.dkim[0];
  if (first) return first.verdict;
  return results.spf?.verdict ?? "none";
}

export function authTone(verdict: AuthVerdict): Tone {
  switch (verdict) {
    case "pass":
      return "good";
    case "fail":
    case "permerror":
      return "danger";
    case "softfail":
    case "policy":
    case "temperror":
      return "warning";
    default:
      return "neutral";
  }
}

// ---------------------------------------------------------------------------
// Warnings

export const WARNING_KINDS: readonly MessageWarning["kind"][] = MESSAGE_WARNING_KINDS;

export const ATTACHMENT_RISKS: readonly AttachmentRisk[] = ATTACHMENT_RISK_VALUES;

type AttachmentWarning = Extract<MessageWarning, { kind: "dangerous-attachment" }>;
type LinkWarning = Exclude<MessageWarning, { kind: "dangerous-attachment" }>;

export function attachmentWarnings(warnings: readonly MessageWarning[]): AttachmentWarning[] {
  return warnings.filter((w): w is AttachmentWarning => w.kind === "dangerous-attachment");
}

export function linkWarnings(warnings: readonly MessageWarning[]): LinkWarning[] {
  return warnings.filter((w): w is LinkWarning => w.kind !== "dangerous-attachment");
}

/** The warning of one attachment, if any. */
export function attachmentWarning(
  warnings: readonly MessageWarning[] | undefined,
  index: number,
): AttachmentWarning | null {
  return attachmentWarnings(warnings ?? []).find((w) => w.index === index) ?? null;
}

/** All warnings of one link (by its number in document order). */
export function warningsForLink(
  warnings: readonly MessageWarning[] | undefined,
  link: number,
): LinkWarning[] {
  return linkWarnings(warnings ?? []).filter((w) => w.link === link);
}

// ---------------------------------------------------------------------------
// Keys

/** `ABCD EF01 …` — fingerprints in groups of four for reading aloud. */
export function formatFingerprint(fingerprint: string): string {
  const clean = fingerprint.replace(/[^0-9a-fA-F]/g, "").toUpperCase();
  return (clean.match(/.{1,4}/g) ?? []).join(" ");
}

/** The last 16 hex digits (the OpenPGP "long key id"). */
export function shortKeyId(fingerprint: string): string {
  const clean = fingerprint.replace(/[^0-9a-fA-F]/g, "").toUpperCase();
  return formatFingerprint(clean.slice(-16));
}

/** Keys state for `data-state` and the status text. */
export type KeyState = "revoked" | "expired" | "rejected" | "ok";

export function keyState(key: CryptoKey): KeyState {
  if (key.revoked) return "revoked";
  if (key.expired) return "expired";
  if (key.trust === "rejected") return "rejected";
  return "ok";
}

/** Own keys first, then by primary user id. */
export function sortKeys(keys: readonly CryptoKey[]): CryptoKey[] {
  return [...keys].sort(
    (a, b) =>
      Number(b.has_secret) - Number(a.has_secret) ||
      (a.user_ids[0] ?? "").localeCompare(b.user_ids[0] ?? "") ||
      a.fingerprint.localeCompare(b.fingerprint),
  );
}

/** Own keys usable for an identity address (for the per-identity key picker). */
export function ownKeysFor(
  keys: readonly CryptoKey[],
  technology: CryptoTechnology,
  email: string,
): CryptoKey[] {
  const address = email.toLowerCase();
  return keys.filter(
    (k) =>
      k.technology === technology &&
      k.has_secret &&
      !k.revoked &&
      !k.expired &&
      k.emails.includes(address),
  );
}

// ---------------------------------------------------------------------------
// Composer: recipients and the send guard

/** What the composer shows for one recipient in one technology. */
export type RecipientState = RecipientKey["state"] | "missing";

export function recipientKeyOf(
  status: RecipientKeyStatus,
  technology: CryptoTechnology,
): RecipientKey | null {
  return technology === "openpgp" ? status.openpgp : status.smime;
}

export function recipientState(
  status: RecipientKeyStatus,
  technology: CryptoTechnology,
): RecipientState {
  return recipientKeyOf(status, technology)?.state ?? "missing";
}

/**
 * The technology a send would use: the explicit choice, else the
 * identity's preference, else the first one every recipient supports
 * (OpenPGP before S/MIME), else OpenPGP.
 */
export function resolveTechnology(
  choice: SendCrypto,
  preference: CryptoPreference,
  report: RecipientKeysReport | null,
): CryptoTechnology {
  if (choice.technology) return choice.technology;
  if (preference !== "auto") return preference;
  if (report) {
    for (const tech of TECHNOLOGIES) {
      const a = report[tech];
      if (a.own_key && a.all_recipients) return tech;
    }
    for (const tech of TECHNOLOGIES) if (report[tech].own_key) return tech;
  }
  return "openpgp";
}

/** Why a send with this choice would be refused, or `null` when it can go. */
export interface SendBlock {
  /** Fluent id. */
  id: string;
  /** Recipients without a usable key (for `security-compose-block-missing`). */
  recipients: string[];
}

/**
 * The composer's guard: sending never falls back to plain text (ADR 0029),
 * so when a chosen protection cannot work the send button must say why
 * instead of sending. `report === null` means "not checked yet" → wait.
 */
export function sendBlock(
  choice: SendCrypto,
  preference: CryptoPreference,
  report: RecipientKeysReport | null,
): SendBlock | null {
  if (!choice.sign && !choice.encrypt) return null;
  if (report === null) return { id: "security-compose-block-checking", recipients: [] };
  const tech = resolveTechnology(choice, preference, report);
  if (!report[tech].own_key) {
    return { id: `security-compose-block-no-own-key-${tech}`, recipients: [] };
  }
  if (choice.encrypt) {
    const missing = report.recipients
      .filter((r) => recipientState(r, tech) !== "usable")
      .map((r) => r.email);
    if (report.recipients.length === 0) {
      return { id: "security-compose-block-no-recipients", recipients: [] };
    }
    if (missing.length > 0) return { id: "security-compose-block-missing", recipients: missing };
  }
  return null;
}

/** A plain choice (nothing on). */
export function plainChoice(): SendCrypto {
  return { sign: false, encrypt: false, technology: null, protect_headers: null };
}

/** Distinct, lower-case recipient addresses in order. */
export function distinctRecipients(emails: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of emails) {
    const email = raw.trim().toLowerCase();
    if (email !== "" && !seen.has(email)) {
      seen.add(email);
      out.push(email);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Autocrypt setup code

/** Digits of an Autocrypt setup code (Autocrypt Level 1: 36 digits). */
export const PASSCODE_DIGITS = 36;

/** `1234-5678-…`: nine blocks of four digits, as the setup message shows it. */
export function formatPasscode(input: string): string {
  const digits = input.replace(/\D/g, "").slice(0, PASSCODE_DIGITS);
  return (digits.match(/.{1,4}/g) ?? []).join("-");
}

export function isCompletePasscode(input: string): boolean {
  return input.replace(/\D/g, "").length === PASSCODE_DIGITS;
}

// ---------------------------------------------------------------------------
// Files

/** Base64 of bytes (for binary key files: `.p12`, `.pfx`, `.gpg`, `.der`). */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Whether a file is a PKCS#12 container (by name). */
export function isPkcs12Name(name: string): boolean {
  return /\.(p12|pfx)$/i.test(name);
}

/** Whether a key file is text (armored / PEM) rather than binary. */
export function looksArmored(text: string): boolean {
  return /-----BEGIN [A-Z0-9 ]+-----/.test(text);
}

/** `data:` URL of an exported key, for a download link. */
export function exportHref(mime: string, encoding: "text" | "base64", data: string): string {
  if (encoding === "base64") return `data:${mime};base64,${data}`;
  return `data:${mime};charset=utf-8,${encodeURIComponent(data)}`;
}

/** A date for display, or the raw value when it does not parse. */
export function formatDateTime(iso: string, locale: string, withTime = true): string {
  return formatInLocale(iso, locale, withTime ? DATE_TIME : DATE_ONLY);
}
