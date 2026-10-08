/**
 * The mock core's answers to the M2a security requests (ADR 0029), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.security`.
 *
 * The mock behaves like the core will after WP35, over fake key material:
 * a keyring of both technologies with trust decisions, imports of armored
 * / PEM text (key material is not real — fingerprints and user ids are
 * read from the text), PKCS#12 and the Windows store, generation, lookup
 * in a fixed key directory, recipient key status with Autocrypt, the
 * Autocrypt setup message, and an encryption-at-rest migration that
 * reports progress with `VaultProgress` events. Sending with sign / encrypt
 * stays refused (`api/mockOutbox.ts`) until WP35.
 */
import { KEY_LOOKUP_SOURCE_VALUES, VAULT_PHASE_VALUES } from "../../api/generated";
import type {
  AutocryptPeer,
  CryptoKey,
  CryptoTechnology,
  Identity,
  IdentityCrypto,
  KeyEncoding,
  KeyImportIssue,
  KeyLookupCandidate,
  KeyLookupFailure,
  MessageBody,
  RecipientKey,
  RecipientKeyState,
  RecipientKeyStatus,
  RecipientKeysReport,
  Request,
  Response,
  SecretProtection,
  SecuritySettings,
  SendCrypto,
  TrustDecision,
  TrustLevel,
  VaultPhase,
  VaultStatus,
  WindowsCertificate,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { defaultIdentityCrypto, sortKeys, trustRank } from "./model";
import {
  UNREACHABLE_DOMAINS,
  emailsOf,
  makeKey,
  mockFingerprint,
  mockHex,
  seedDirectory,
  seedKeys,
  seedPeers,
  seedWindowsStore,
  type DirectoryEntry,
  type MockKey,
  type SecurityOverlay,
} from "./mockSeed";
import { DAY_MS } from "../../state/clock";

export { securityDemoBySender } from "./mockSeed";
export type { SecurityOverlay } from "./mockSeed";

/** The requests this feature answers. */
export const SECURITY_REQUESTS = [
  "ListKeys",
  "GetKey",
  "ImportKeys",
  "ImportPkcs12",
  "ExportPublicKey",
  "GenerateKey",
  "DeleteKey",
  "SetKeyTrust",
  "UnlockKey",
  "LookupKeys",
  "ListWindowsCertificates",
  "ImportWindowsCertificate",
  "CheckRecipientKeys",
  "SendAutocryptSetup",
  "ImportAutocryptSetup",
  "GetSecuritySettings",
  "UpdateSecuritySettings",
  "SuggestAuthservId",
  "GetVaultStatus",
  "SetVaultEncryption",
] as const satisfies readonly Request["type"][];

export type SecurityRequest = Extract<Request, { type: (typeof SECURITY_REQUESTS)[number] }>;

export function isSecurityRequest(request: Request): request is SecurityRequest {
  return (SECURITY_REQUESTS as readonly string[]).includes(request.type);
}

/** Items per migration phase of the mock vault (small, so tests stay fast). */
export const VAULT_ITEMS: Readonly<Record<VaultPhase, number>> = {
  database: 2,
  blobs: 4,
  index: 2,
};

/** An Autocrypt setup message the mock "sent". */
export interface SetupMessage {
  identityId: string;
  outboxId: string;
  passcode: string;
}

/** Mock-core state of this feature. */
export interface SecurityMockState {
  settings: SecuritySettings;
  vault: VaultStatus;
  /** The keyring. */
  keys: MockKey[];
  /** User trust decisions by `technology:fingerprint` (kept across delete + re-import). */
  decisions: Record<string, TrustDecision>;
  /** Unlocked passphrase keys (key id → until, ms since epoch; `Infinity` = session). */
  unlocked: Record<string, number>;
  /** Keys found online, by address. */
  directory: Record<string, DirectoryEntry[]>;
  /** Windows certificate stores. */
  windowsStore: WindowsCertificate[];
  /** `false` = the platform is not Windows (store requests answer `unsupported`). */
  windows: boolean;
  /** Autocrypt peers by address. */
  peers: Record<string, AutocryptPeer>;
  /** Setup messages sent, newest last. */
  setupMessages: SetupMessage[];
  /**
   * Autocrypt setup messages in the mailbox that can be imported: message
   * id → passcode (a setup message sent from another device).
   */
  setupInbox: Record<string, string>;
  /** Findings per message id, added to `GetMessage` answers. */
  messages: Record<string, SecurityOverlay>;
  /** Findings per sender address (the dev demo), used when no id matches. */
  bySender: Record<string, SecurityOverlay>;
  /** Makes the next vault migration stop with this error. */
  vaultError: VaultStatus["error"];
  counter: number;
}

/** Defaults as the core has them (`SecuritySettings::default`, plain vault). */
export function createSecurityMockState(): SecurityMockState {
  const keys = seedKeys();
  return {
    settings: {
      index_decrypted: [],
      key_lookup: "on-demand",
      lookup_sources: [...KEY_LOOKUP_SOURCE_VALUES],
    },
    vault: {
      state: "plain",
      phase: null,
      done: 0,
      total: 0,
      key_in_keychain: false,
      needs_recovery: false,
      recovery: { configured: false, created_at: null, verified_at: null },
      error: null,
      message: null,
    },
    keys,
    // Seeded "verified" keys stand for a stored user decision.
    decisions: Object.fromEntries(
      keys.filter((k) => k.key.trust === "verified").map((k) => [decisionKey(k.key), "verified"]),
    ),
    unlocked: {},
    directory: seedDirectory(),
    windowsStore: seedWindowsStore(),
    windows: true,
    peers: seedPeers(),
    setupMessages: [],
    setupInbox: {},
    messages: {},
    bySender: {},
    vaultError: null,
    counter: 100,
  };
}

function state(core: MockCore): SecurityMockState {
  return core.data.features.security;
}

/**
 * Adds the mock's security findings to a message body (call from the
 * mock's `GetMessage`; `MessageBody.security` etc. are optional).
 */
export function decorateMessageBody(core: MockCore, body: MessageBody): MessageBody {
  const s = state(core);
  const from = body.headers.from[0]?.email.toLowerCase() ?? "";
  const overlay = s.messages[body.id] ?? s.bySender[from];
  return overlay ? { ...body, ...structuredClone(overlay) } : body;
}

// ---------------------------------------------------------------------------
// Keyring helpers

function identities(core: MockCore): Identity[] {
  return core.data.accountsConfig.accounts.flatMap((a) => a.identities);
}

function decisionKey(key: CryptoKey): string {
  return `${key.technology}:${key.fingerprint}`;
}

/** Effective trust: a decision wins, else own, else the source. */
function effectiveTrust(key: CryptoKey, decision: TrustDecision | undefined): TrustLevel {
  if (decision === "verified") return "verified";
  if (decision === "rejected") return "rejected";
  if (key.has_secret) return "own";
  if (key.source === "autocrypt") return "autocrypt";
  if (key.source === "autocrypt-gossip") return "autocrypt-gossip";
  return "unknown";
}

function refreshTrust(s: SecurityMockState, key: CryptoKey): CryptoKey {
  key.trust = effectiveTrust(key, s.decisions[decisionKey(key)]);
  return key;
}

function findKey(s: SecurityMockState, id: string): MockKey | undefined {
  return s.keys.find((k) => k.key.id === id);
}

function nextKeyId(s: SecurityMockState): string {
  s.counter++;
  return `key-${String(s.counter)}`;
}

function keysChanged(core: MockCore): void {
  core.emit({ type: "KeysChanged" });
}

/** Adds or merges a key; returns `created`. */
function upsert(s: SecurityMockState, incoming: CryptoKey, passphrase: string | null): boolean {
  const existing = s.keys.find(
    (k) => k.key.technology === incoming.technology && k.key.fingerprint === incoming.fingerprint,
  );
  if (existing) {
    const merged = existing.key;
    for (const uid of incoming.user_ids)
      if (!merged.user_ids.includes(uid)) merged.user_ids.push(uid);
    merged.emails = emailsOf(merged.user_ids);
    merged.revoked = merged.revoked || incoming.revoked;
    if (incoming.has_secret && !merged.has_secret) {
      merged.has_secret = true;
      merged.secret_storage = incoming.secret_storage;
      existing.passphrase = passphrase;
    }
    refreshTrust(s, merged);
    return false;
  }
  s.keys.push({ key: refreshTrust(s, incoming), passphrase });
  return true;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

// ---------------------------------------------------------------------------
// Fake key material: armor and PEM

const PGP_BLOCK =
  /-----BEGIN PGP (PUBLIC|PRIVATE) KEY BLOCK-----([\s\S]*?)-----END PGP \1 KEY BLOCK-----/g;
const PEM_BLOCK = /-----BEGIN CERTIFICATE-----([\s\S]*?)-----END CERTIFICATE-----/g;

/** The mock's armored export of a key: headers carry what an import reads back. */
export function armorKey(key: Pick<CryptoKey, "fingerprint" | "user_ids" | "algorithm">): string {
  const headers = [`Comment: ${key.fingerprint}`, ...key.user_ids.map((u) => `Comment: ${u}`)];
  return [
    "-----BEGIN PGP PUBLIC KEY BLOCK-----",
    ...headers,
    `Comment: ${key.algorithm}`,
    "",
    btoa(`mock key ${key.fingerprint}`),
    "-----END PGP PUBLIC KEY BLOCK-----",
    "",
  ].join("\n");
}

/** The mock's PEM export of a certificate. */
export function pemCertificate(key: Pick<CryptoKey, "fingerprint" | "user_ids">): string {
  return [
    "-----BEGIN CERTIFICATE-----",
    `Comment: ${key.fingerprint}`,
    ...key.user_ids.map((u) => `Comment: ${u}`),
    "",
    btoa(`mock certificate ${key.fingerprint}`),
    "-----END CERTIFICATE-----",
    "",
  ].join("\n");
}

function comments(block: string): string[] {
  return [...block.matchAll(/^Comment: (.+)$/gm)].map((m) => (m[1] ?? "").trim());
}

interface Parsed {
  technology: CryptoTechnology;
  fingerprint: string;
  userIds: string[];
  algorithm: string;
  secret: boolean;
}

function parseBlocks(text: string): { parsed: Parsed[]; issues: KeyImportIssue[] } {
  const parsed: Parsed[] = [];
  const issues: KeyImportIssue[] = [];
  let n = 0;
  const read = (technology: CryptoTechnology, block: string, secret: boolean) => {
    n++;
    const lines = comments(block);
    const fpLength = technology === "openpgp" ? 40 : 64;
    const fp = lines.find((l) => new RegExp(`^[0-9A-F]{${String(fpLength)}}$`).test(l));
    const userIds = lines.filter((l) => l.includes("@"));
    if (userIds.length === 0) {
      issues.push({
        item: `#${String(n)}`,
        code: "crypto",
        message: "the key has no user id with an e-mail address",
      });
      return;
    }
    parsed.push({
      technology,
      fingerprint: fp ?? mockFingerprint(technology, userIds.join("|")),
      userIds,
      algorithm:
        lines.find((l) => /^(Ed25519|RSA \d+|ECDSA P-\d+)$/.test(l)) ??
        (technology === "openpgp" ? "Ed25519" : "ECDSA P-256"),
      secret,
    });
  };
  for (const m of text.matchAll(PGP_BLOCK)) read("openpgp", m[2] ?? "", m[1] === "PRIVATE");
  for (const m of text.matchAll(PEM_BLOCK)) read("smime", m[1] ?? "", false);
  return { parsed, issues };
}

function decode(data: string, encoding: KeyEncoding): string | null {
  if (encoding === "text") return data;
  try {
    return atob(data.replace(/\s/g, ""));
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Recipient key status

function keyStateFor(key: CryptoKey): RecipientKeyState {
  if (key.trust === "rejected") return "rejected";
  if (key.revoked) return "revoked";
  if (key.expired) return "expired";
  if (!key.can_encrypt) return "cannot-encrypt";
  if (key.chain && key.chain.state !== "valid") return "invalid-chain";
  return "usable";
}

function bestKey(
  s: SecurityMockState,
  technology: CryptoTechnology,
  email: string,
): RecipientKey | null {
  const candidates = s.keys
    .map((k) => k.key)
    .filter((k) => k.technology === technology && k.emails.includes(email))
    .map((k) => ({ key: k, state: keyStateFor(k) }))
    .sort(
      (a, b) =>
        Number(b.state === "usable") - Number(a.state === "usable") ||
        trustRank(b.key.trust) - trustRank(a.key.trust) ||
        b.key.created_at.localeCompare(a.key.created_at),
    );
  const best = candidates[0];
  if (!best) return null;
  return {
    key_id: best.key.id,
    fingerprint: best.key.fingerprint,
    state: best.state,
    trust: best.key.trust,
    source: best.key.source,
  };
}

/** The own key an identity would sign with (setting, else newest usable). */
function ownKey(
  s: SecurityMockState,
  identity: Identity,
  technology: CryptoTechnology,
): CryptoKey | null {
  const crypto = identity.crypto ?? defaultIdentityCrypto();
  const chosen = technology === "openpgp" ? crypto.openpgp_key : crypto.smime_certificate;
  const own = s.keys
    .map((k) => k.key)
    .filter(
      (k) =>
        k.technology === technology &&
        k.has_secret &&
        !k.revoked &&
        !k.expired &&
        k.can_sign &&
        (chosen ? k.fingerprint === chosen : k.emails.includes(identity.email.toLowerCase())),
    )
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  return own[0] ?? null;
}

/**
 * `SendDraft` with signing / encryption, as the core checks it (ADR 0034):
 * `no_key` when the own key or a recipient's key is missing, `locked` when
 * the signing key needs its passphrase; `null` = it can be sent. Never
 * falls back to plain text.
 */
export function checkSendCrypto(
  core: MockCore,
  identity: Identity,
  recipients: readonly string[],
  crypto: SendCrypto,
): Response | null {
  const s = state(core);
  const emails = [...new Set(recipients.map((r) => r.trim().toLowerCase()).filter(Boolean))];
  const missing = (tech: CryptoTechnology) =>
    emails.filter((e) => bestKey(s, tech, e)?.state !== "usable");
  const preference = (identity.crypto ?? defaultIdentityCrypto()).technology;
  const works = (tech: CryptoTechnology) =>
    ownKey(s, identity, tech) !== null && (!crypto.encrypt || missing(tech).length === 0);
  const technology: CryptoTechnology =
    crypto.technology ??
    (preference !== "auto"
      ? preference
      : works("openpgp")
        ? "openpgp"
        : works("smime")
          ? "smime"
          : ownKey(s, identity, "openpgp") === null && ownKey(s, identity, "smime") !== null
            ? "smime"
            : "openpgp");
  const own = ownKey(s, identity, technology);
  if (!own) return errorResponse("no_key", `no own ${technology} key for ${identity.email}`);
  if (crypto.sign && own.secret_storage === "passphrase") {
    const until = s.unlocked[own.id];
    if (until === undefined || until < Date.now()) {
      return errorResponse("locked", `the key ${own.id} is locked`);
    }
  }
  const without = crypto.encrypt ? missing(technology) : [];
  if (without.length > 0) {
    return errorResponse("no_key", `no usable ${technology} key for ${without.join(", ")}`);
  }
  return null;
}

function lookupDomainFails(email: string): boolean {
  return UNREACHABLE_DOMAINS.includes(email.split("@")[1] ?? "");
}

function checkRecipients(
  s: SecurityMockState,
  identity: Identity,
  recipients: readonly string[],
  lookup: boolean,
): RecipientKeysReport {
  const crypto: IdentityCrypto = identity.crypto ?? defaultIdentityCrypto();
  const emails = [...new Set(recipients.map((r) => r.trim().toLowerCase()).filter(Boolean))];
  const statuses: RecipientKeyStatus[] = emails.map((email) => {
    const openpgp = bestKey(s, "openpgp", email);
    const smime = bestKey(s, "smime", email);
    let lookupState: RecipientKeyStatus["lookup"] = "not-attempted";
    if (lookup && s.settings.key_lookup !== "off" && openpgp?.state !== "usable") {
      if (lookupDomainFails(email)) lookupState = "failed";
      else lookupState = (s.directory[email]?.length ?? 0) > 0 ? "found" : "not-found";
    }
    return {
      email,
      openpgp,
      smime,
      autocrypt: crypto.autocrypt ? (clone(s.peers[email]) ?? null) : null,
      lookup: lookupState,
    };
  });
  const availability = (tech: CryptoTechnology) => ({
    own_key: ownKey(s, identity, tech) !== null,
    all_recipients:
      statuses.length > 0 &&
      statuses.every((r) => (tech === "openpgp" ? r.openpgp : r.smime)?.state === "usable"),
  });
  const openpgp = availability("openpgp");
  const smime = availability("smime");

  // Autocrypt Level 1 recommendation, simplified to the peer table.
  let autocrypt: RecipientKeysReport["autocrypt"] = "disable";
  if (crypto.autocrypt && statuses.length > 0 && openpgp.own_key && openpgp.all_recipients) {
    const peers = statuses.map((r) => r.autocrypt);
    if (peers.every((p) => p !== null && p.autocrypt_at !== null)) {
      autocrypt =
        crypto.prefer_encrypt === "mutual" && peers.every((p) => p?.prefer_encrypt === "mutual")
          ? "encrypt"
          : "available";
    } else if (peers.some((p) => p !== null && p.autocrypt_at === null)) {
      autocrypt = "discourage";
    } else {
      autocrypt = "available";
    }
  }
  const canEncrypt =
    (openpgp.own_key && openpgp.all_recipients) || (smime.own_key && smime.all_recipients);
  return {
    recipients: statuses,
    openpgp,
    smime,
    autocrypt,
    suggested: {
      sign: crypto.sign,
      encrypt: canEncrypt && (crypto.encrypt || autocrypt === "encrypt"),
      technology: null,
      protect_headers: null,
    },
  };
}

// ---------------------------------------------------------------------------
// Vault migration

function vaultStep(core: MockCore, target: "encrypted" | "plain"): void {
  const s = state(core);
  const v = s.vault;
  if (v.state !== "encrypting" && v.state !== "decrypting") return;
  const phase = v.phase ?? "database";
  if (s.vaultError && phase === "blobs" && v.done === 1) {
    v.error = s.vaultError;
    v.message = "mock migration stopped";
    s.vaultError = null;
    core.emit({ type: "VaultProgress", status: clone(v) });
    return;
  }
  if (v.done < v.total) {
    v.done++;
  } else {
    const next = VAULT_PHASE_VALUES[VAULT_PHASE_VALUES.indexOf(phase) + 1];
    if (next) {
      v.phase = next;
      v.done = 0;
      v.total = VAULT_ITEMS[next];
    } else {
      v.state = target;
      v.phase = null;
      v.done = 0;
      v.total = 0;
      v.key_in_keychain = target === "encrypted";
      core.emit({ type: "VaultProgress", status: clone(v) });
      return;
    }
  }
  core.emit({ type: "VaultProgress", status: clone(v) });
  core.later(() => {
    vaultStep(core, target);
  });
}

function setVault(core: MockCore, enabled: boolean): Response {
  const s = state(core);
  const v = s.vault;
  const migrating = v.state === "encrypting" || v.state === "decrypting";
  if (migrating && v.error === null) {
    return errorResponse("conflict", "a migration is already running");
  }
  const target = enabled ? "encrypted" : "plain";
  if (!migrating && v.state === target) return { type: "VaultStatus", status: clone(v) };
  s.vault = {
    state: enabled ? "encrypting" : "decrypting",
    phase: "database",
    done: 0,
    total: VAULT_ITEMS.database,
    key_in_keychain: v.key_in_keychain || enabled,
    needs_recovery: false,
    recovery: { configured: false, created_at: null, verified_at: null },
    error: null,
    message: null,
  };
  core.later(() => {
    vaultStep(core, target);
  });
  return { type: "VaultStatus", status: clone(s.vault) };
}

// ---------------------------------------------------------------------------
// Requests

function newOwnKey(
  s: SecurityMockState,
  technology: CryptoTechnology,
  userIds: string[],
  extra: Partial<CryptoKey>,
): CryptoKey {
  return makeKey(nextKeyId(s), technology, userIds, {
    has_secret: true,
    trust: "own",
    created_at: new Date().toISOString(),
    ...extra,
  });
}

function storageOf(protection: SecretProtection): {
  storage: CryptoKey["secret_storage"];
  passphrase: string | null;
} {
  return protection.kind === "passphrase"
    ? { storage: "passphrase", passphrase: protection.passphrase }
    : { storage: "keychain", passphrase: null };
}

/** Stores `key` (or refreshes the stored copy) and answers it as imported. */
function importedOne(core: MockCore, s: SecurityMockState, key: CryptoKey): Response {
  const created = upsert(s, key, null);
  const stored = s.keys.find((k) => k.key.fingerprint === key.fingerprint)?.key ?? key;
  return imported(core, [stored], created ? 1 : 0, []);
}

function imported(
  core: MockCore,
  keys: CryptoKey[],
  created: number,
  issues: KeyImportIssue[],
): Response {
  if (keys.length > 0) keysChanged(core);
  return {
    type: "KeysImported",
    result: { keys: clone(keys), created, updated: keys.length - created, issues },
  };
}

export function answerSecurity(core: MockCore, request: SecurityRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "GetSecuritySettings":
      return { type: "SecuritySettings", settings: clone(s.settings) };
    case "UpdateSecuritySettings": {
      const known = new Set(core.data.accounts.map((a) => a.id));
      const unknown = request.settings.index_decrypted.find((id) => !known.has(id));
      if (unknown !== undefined) {
        return errorResponse("invalid_request", `${unknown} is not a real account`);
      }
      s.settings = {
        index_decrypted: [...new Set(request.settings.index_decrypted)].sort(),
        key_lookup: request.settings.key_lookup,
        lookup_sources: [...new Set(request.settings.lookup_sources)],
      };
      core.emit({ type: "SecuritySettingsChanged" });
      return { type: "SecuritySettings", settings: clone(s.settings) };
    }
    case "SuggestAuthservId": {
      // The id of the seeded sender checks (the mock's "recent mail").
      if (!core.data.accountsConfig.accounts.some((a) => a.id === request.account_id)) {
        return errorResponse("not_found", `account ${request.account_id}`);
      }
      const overlays = [...Object.values(s.messages), ...Object.values(s.bySender)];
      const found = overlays.find((o) => o.auth_results)?.auth_results?.authserv_id ?? null;
      return { type: "AuthservIdSuggestion", account_id: request.account_id, authserv_id: found };
    }
    case "GetVaultStatus":
      return { type: "VaultStatus", status: clone(s.vault) };
    case "SetVaultEncryption":
      return setVault(core, request.enabled);

    case "ListKeys": {
      const f = request.filter;
      const text = f.text?.trim().toLowerCase() ?? "";
      const keys = s.keys
        .map((k) => k.key)
        .filter((k) => f.technology === null || k.technology === f.technology)
        .filter((k) => !f.own_only || k.has_secret)
        .filter(
          (k) =>
            text === "" ||
            k.user_ids.some((u) => u.toLowerCase().includes(text)) ||
            k.emails.some((e) => e.includes(text)) ||
            k.fingerprint.toLowerCase().includes(text.replace(/\s/g, "")),
        );
      return { type: "Keys", keys: clone(sortKeys(keys)) };
    }
    case "GetKey": {
      const k = findKey(s, request.key_id);
      return k ? { type: "Key", key: clone(k.key) } : errorResponse("not_found", request.key_id);
    }
    case "ImportKeys": {
      const text = decode(request.data, request.encoding);
      if (text === null) return errorResponse("invalid_request", "data is not valid base64");
      const { parsed, issues } = parseBlocks(text);
      if (parsed.length === 0 && issues.length === 0) {
        return errorResponse("crypto", "no OpenPGP key or certificate found");
      }
      const keys: CryptoKey[] = [];
      let created = 0;
      for (const p of parsed) {
        const key = makeKey(nextKeyId(s), p.technology, p.userIds, {
          fingerprint: p.fingerprint,
          algorithm: p.algorithm,
          has_secret: p.secret,
          secret_storage: p.secret ? "keychain" : null,
        });
        if (upsert(s, key, null)) created++;
        const stored = s.keys.find(
          (k) => k.key.technology === p.technology && k.key.fingerprint === p.fingerprint,
        );
        if (stored) keys.push(stored.key);
      }
      return imported(core, keys, created, issues);
    }
    case "ImportPkcs12": {
      const text = decode(request.data, "base64");
      if (text === null) return errorResponse("invalid_request", "data is not valid base64");
      if (request.passphrase !== "demo") {
        return errorResponse("auth_failed", "wrong PKCS#12 passphrase");
      }
      const userIds = [...new Set(emailsOf([text]))];
      const [firstId] = userIds;
      if (firstId === undefined) return errorResponse("crypto", "no certificate in the file");
      const { storage, passphrase } = storageOf(request.protection);
      const key = newOwnKey(s, "smime", [`CN=${firstId}`, ...userIds], {
        source: "pkcs12",
        secret_storage: storage,
      });
      const created = upsert(s, key, passphrase);
      const stored = s.keys.find((k) => k.key.fingerprint === key.fingerprint)?.key ?? key;
      return imported(core, [stored], created ? 1 : 0, []);
    }
    case "ExportPublicKey": {
      const k = findKey(s, request.key_id);
      if (!k) return errorResponse("not_found", request.key_id);
      const pgp = k.key.technology === "openpgp";
      const text = pgp ? armorKey(k.key) : pemCertificate(k.key);
      const base = (k.key.emails[0] ?? k.key.fingerprint.slice(-16)).replace(
        /[^a-z0-9.@_-]/gi,
        "_",
      );
      return {
        type: "KeyExported",
        filename: `${base}${pgp ? ".asc" : ".pem"}`,
        mime: pgp ? "application/pgp-keys" : "application/x-pem-file",
        encoding: request.encoding,
        data: request.encoding === "text" ? text : btoa(text),
      };
    }
    case "GenerateKey": {
      const identity = identities(core).find((i) => i.id === request.identity_id);
      if (!identity) return errorResponse("not_found", `identity ${request.identity_id}`);
      if (request.protection.kind === "passphrase" && request.protection.passphrase === "") {
        return errorResponse("invalid_request", "empty passphrase");
      }
      const uid = identity.name ? `${identity.name} <${identity.email}>` : identity.email;
      const { storage, passphrase } = storageOf(request.protection);
      const now = new Date();
      const expires =
        request.expires_days === null
          ? null
          : new Date(now.getTime() + request.expires_days * DAY_MS).toISOString();
      const key = newOwnKey(s, "openpgp", [uid], {
        fingerprint: mockFingerprint("openpgp", `${uid}|${now.toISOString()}|${String(s.counter)}`),
        algorithm: request.algorithm === "rsa4096" ? "RSA 4096" : "Ed25519",
        source: "generated",
        secret_storage: storage,
        expires_at: expires,
      });
      upsert(s, key, passphrase);
      keysChanged(core);
      return { type: "KeyGenerated", key: clone(key) };
    }
    case "DeleteKey": {
      const k = findKey(s, request.key_id);
      if (!k) return errorResponse("not_found", request.key_id);
      if (k.key.has_secret && !request.delete_secret) {
        return errorResponse("conflict", "an own key needs delete_secret");
      }
      s.keys = s.keys.filter((x) => x !== k);
      keysChanged(core);
      return { type: "Done" };
    }
    case "SetKeyTrust": {
      const k = findKey(s, request.key_id);
      if (!k) return errorResponse("not_found", request.key_id);
      const dk = decisionKey(k.key);
      if (request.decision === "unset") {
        s.decisions = Object.fromEntries(Object.entries(s.decisions).filter(([d]) => d !== dk));
      } else s.decisions[dk] = request.decision;
      k.key.trust = effectiveTrust(k.key, s.decisions[decisionKey(k.key)]);
      keysChanged(core);
      return { type: "KeySaved", key: clone(k.key) };
    }
    case "UnlockKey": {
      const k = findKey(s, request.key_id);
      if (!k) return errorResponse("not_found", request.key_id);
      if (!k.key.has_secret || k.key.secret_storage !== "passphrase") {
        return errorResponse("invalid_request", "the key has no passphrase");
      }
      if (request.passphrase !== k.passphrase) {
        return errorResponse("auth_failed", "wrong passphrase");
      }
      s.unlocked[k.key.id] =
        request.remember_minutes === null
          ? Number.POSITIVE_INFINITY
          : Date.now() + request.remember_minutes * 60_000;
      // Messages that could not be decrypted while locked now can.
      for (const overlay of [...Object.values(s.messages), ...Object.values(s.bySender)]) {
        const enc = overlay.security?.encryption;
        if (enc?.state === "locked" && enc.key_id === k.key.id) enc.state = "decrypted";
      }
      keysChanged(core);
      return { type: "Done" };
    }
    case "LookupKeys": {
      const email = request.email.trim().toLowerCase();
      if (!email.includes("@")) return errorResponse("invalid_request", "not an address");
      if (s.settings.key_lookup === "off") {
        return errorResponse("unsupported", "online key lookup is turned off");
      }
      const candidates: KeyLookupCandidate[] = [];
      const failures: KeyLookupFailure[] = [];
      for (const source of request.sources) {
        if (lookupDomainFails(email)) {
          failures.push({ source, code: "network", message: "connection refused" });
          continue;
        }
        for (const entry of s.directory[email] ?? []) {
          if (entry.source !== source) continue;
          const fingerprint = mockFingerprint("openpgp", `${entry.user_ids.join("|")}|${source}`);
          const known =
            s.keys.find((k) => k.key.technology === "openpgp" && k.key.fingerprint === fingerprint)
              ?.key.id ?? null;
          candidates.push({
            source,
            fingerprint,
            user_ids: [...entry.user_ids],
            algorithm: entry.algorithm,
            created_at: entry.created_at,
            expires_at: null,
            known,
            armored: armorKey({
              fingerprint,
              user_ids: entry.user_ids,
              algorithm: entry.algorithm,
            }),
          });
        }
      }
      return { type: "KeyLookup", email, candidates, failures };
    }
    case "ListWindowsCertificates": {
      if (!s.windows) return errorResponse("unsupported", "not on Windows");
      const certificates = s.windowsStore
        .filter((c) => c.store === request.store)
        .map((c) => ({
          ...clone(c),
          imported:
            s.keys.find((k) => k.key.fingerprint === c.certificate.fingerprint)?.key.id ?? null,
        }));
      return { type: "WindowsCertificates", certificates };
    }
    case "ImportWindowsCertificate": {
      if (!s.windows) return errorResponse("unsupported", "not on Windows");
      const cert = s.windowsStore.find(
        (c) => c.store === request.store && c.thumbprint === request.thumbprint,
      );
      if (!cert) return errorResponse("not_found", request.thumbprint);
      if (!cert.usable_for_email) {
        return errorResponse("invalid_request", "the certificate cannot be used for e-mail");
      }
      const own = cert.has_private_key;
      const key = makeKey(nextKeyId(s), "smime", [cert.certificate.subject, ...cert.emails], {
        fingerprint: cert.certificate.fingerprint,
        has_secret: own,
        secret_storage: own ? "windows-store" : null,
        source: "windows-store",
        issuer: cert.certificate.issuer,
        serial: cert.certificate.serial,
      });
      return importedOne(core, s, key);
    }
    case "CheckRecipientKeys": {
      const identity = identities(core).find((i) => i.id === request.identity_id);
      if (!identity) return errorResponse("not_found", `identity ${request.identity_id}`);
      return {
        type: "RecipientKeys",
        report: checkRecipients(s, identity, request.recipients, request.lookup),
      };
    }
    case "SendAutocryptSetup": {
      const identity = identities(core).find((i) => i.id === request.identity_id);
      if (!identity) return errorResponse("not_found", `identity ${request.identity_id}`);
      if (!ownKey(s, identity, "openpgp")) {
        return errorResponse("no_key", "the identity has no own OpenPGP key");
      }
      const digits = mockHex(`setup:${identity.id}:${String(s.setupMessages.length)}`, 48)
        .split("")
        .map((c) => (parseInt(c, 16) % 10).toString())
        .join("")
        .slice(0, 36);
      const passcode = (digits.match(/.{4}/g) ?? []).join("-");
      s.counter++;
      const outboxId = `out-autocrypt-${String(s.counter)}`;
      s.setupMessages.push({ identityId: identity.id, outboxId, passcode });
      return { type: "AutocryptSetupSent", outbox_id: outboxId, passcode };
    }
    case "ImportAutocryptSetup": {
      const expected = s.setupInbox[request.message_id];
      if (expected === undefined) {
        return errorResponse("invalid_request", "the message is not an Autocrypt setup message");
      }
      const digits = (v: string) => v.replace(/\D/g, "");
      if (digits(request.passcode) !== digits(expected)) {
        return errorResponse("auth_failed", "wrong setup code");
      }
      const body = core.data.bodies.get(request.message_id);
      const email = body?.headers.to[0]?.email ?? identities(core)[0]?.email ?? "me@example.org";
      const key = newOwnKey(s, "openpgp", [email], {
        fingerprint: mockFingerprint("openpgp", `setup:${request.message_id}`),
        source: "setup-message",
        secret_storage: "keychain",
      });
      return importedOne(core, s, key);
    }
  }
}
