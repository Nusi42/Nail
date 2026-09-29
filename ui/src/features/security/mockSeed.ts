/**
 * Demo data of the security mock: a keyring with own and peer keys of
 * both technologies, a key directory for online lookups, Windows store
 * certificates, Autocrypt peers and message-security overlays for the
 * reader. Addresses match the mock mailbox (`src/api/mockData.ts`).
 */
import type {
  AutocryptPeer,
  CertificateChain,
  CryptoKey,
  CryptoTechnology,
  KeyLookupSource,
  MessageBody,
  WindowsCertificate,
} from "../../api/generated";

/** The mock's view of one keyring entry. */
export interface MockKey {
  key: CryptoKey;
  /** Passphrase of a passphrase-protected own key (the mock checks it). */
  passphrase: string | null;
}

/** A key the mock "finds" online. */
export interface DirectoryEntry {
  source: KeyLookupSource;
  user_ids: string[];
  algorithm: string;
  created_at: string;
}

/** Security findings added to a message body (`MessageBody` M2a fields). */
export type SecurityOverlay = Pick<MessageBody, "security" | "auth_results" | "warnings">;

/** Deterministic hex of a given length from a seed (FNV-1a rounds). */
export function mockHex(seed: string, length: number): string {
  let out = "";
  let round = 0;
  while (out.length < length) {
    let h = 0x811c9dc5 ^ round;
    for (let i = 0; i < seed.length; i++) {
      h ^= seed.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    out += h.toString(16).padStart(8, "0");
    round++;
  }
  return out.slice(0, length).toUpperCase();
}

/** A fingerprint of the technology's length (OpenPGP v4: 40, S/MIME SHA-256: 64). */
export function mockFingerprint(technology: CryptoTechnology, seed: string): string {
  return mockHex(`${technology}:${seed}`, technology === "openpgp" ? 40 : 64);
}

export function emailsOf(userIds: readonly string[]): string[] {
  const out: string[] = [];
  for (const uid of userIds) {
    for (const m of uid.matchAll(/([A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)/g)) {
      const email = (m[1] ?? "").toLowerCase();
      if (!out.includes(email)) out.push(email);
    }
  }
  return out;
}

function chain(subject: string, fingerprint: string, state: CertificateChain["state"]) {
  const c: CertificateChain = {
    state,
    revocation: "not-checked",
    certificates: [
      {
        subject,
        issuer: "CN=Demo Mail CA,O=Demo Trust",
        serial: mockHex(`serial:${subject}`, 16),
        not_before: "2026-01-01T00:00:00Z",
        not_after: "2027-12-31T23:59:59Z",
        fingerprint,
      },
      {
        subject: "CN=Demo Mail CA,O=Demo Trust",
        issuer: "CN=Demo Root CA,O=Demo Trust",
        serial: "02",
        not_before: "2025-01-01T00:00:00Z",
        not_after: "2030-12-31T23:59:59Z",
        fingerprint: mockHex("demo-mail-ca", 64),
      },
      {
        subject: "CN=Demo Root CA,O=Demo Trust",
        issuer: "CN=Demo Root CA,O=Demo Trust",
        serial: "01",
        not_before: "2025-01-01T00:00:00Z",
        not_after: "2035-12-31T23:59:59Z",
        fingerprint: mockHex("demo-root-ca", 64),
      },
    ],
  };
  return c;
}

/** Builds a keyring entry with sensible defaults. */
export function makeKey(
  id: string,
  technology: CryptoTechnology,
  userIds: string[],
  extra: Partial<CryptoKey> = {},
): CryptoKey {
  const fingerprint = extra.fingerprint ?? mockFingerprint(technology, userIds.join("|"));
  const smime = technology === "smime";
  const subject = userIds[0] ?? "";
  return {
    id,
    technology,
    fingerprint,
    user_ids: userIds,
    emails: emailsOf(userIds),
    algorithm: smime ? "ECDSA P-256" : "Ed25519",
    created_at: "2026-02-01T09:00:00Z",
    expires_at: smime ? "2027-12-31T23:59:59Z" : null,
    expired: false,
    revoked: false,
    can_sign: true,
    can_encrypt: true,
    has_secret: false,
    secret_storage: null,
    source: "imported",
    trust: "unknown",
    issuer: smime ? "CN=Demo Mail CA,O=Demo Trust" : null,
    serial: smime ? mockHex(`serial:${subject}`, 16) : null,
    chain: smime ? chain(subject, fingerprint, "valid") : null,
    ...extra,
  };
}

/** The keyring the mock starts with. */
export function seedKeys(): MockKey[] {
  const ada = makeKey("key-1", "openpgp", ["Ada Lovelace <ada@example.org>"], {
    has_secret: true,
    secret_storage: "keychain",
    source: "generated",
    trust: "own",
  });
  const adaWork = makeKey(
    "key-2",
    "smime",
    ["CN=Ada Lovelace,E=ada@work.example", "ada@work.example"],
    {
      has_secret: true,
      secret_storage: "passphrase",
      source: "pkcs12",
      trust: "own",
    },
  );
  const grace = makeKey("key-3", "openpgp", ["Grace Hopper <grace@navy.example>"], {
    trust: "verified",
  });
  const alan = makeKey("key-4", "openpgp", ["Alan Turing <alan@bletchley.example>"], {
    source: "autocrypt",
    trust: "autocrypt",
  });
  const katherine = makeKey(
    "key-5",
    "smime",
    ["CN=Katherine Johnson,E=katherine@nasa.example", "katherine@nasa.example"],
    { source: "message" },
  );
  const edsger = makeKey("key-6", "openpgp", ["Edsger Dijkstra <ewd@tue.example>"], {
    algorithm: "RSA 4096",
    created_at: "2021-03-01T09:00:00Z",
    expires_at: "2025-03-01T09:00:00Z",
    expired: true,
  });
  return [
    { key: ada, passphrase: null },
    { key: adaWork, passphrase: "demo" },
    { key: grace, passphrase: null },
    { key: alan, passphrase: null },
    { key: katherine, passphrase: null },
    { key: edsger, passphrase: null },
  ];
}

/** Keys the mock finds online (`LookupKeys`), by address. */
export function seedDirectory(): Record<string, DirectoryEntry[]> {
  return {
    "liskov@mit.example": [
      {
        source: "wkd",
        user_ids: ["Barbara Liskov <liskov@mit.example>"],
        algorithm: "Ed25519",
        created_at: "2026-05-04T10:00:00Z",
      },
    ],
    "ewd@tue.example": [
      {
        source: "keys-openpgp-org",
        user_ids: ["Edsger Dijkstra <ewd@tue.example>"],
        algorithm: "Ed25519",
        created_at: "2026-04-01T08:00:00Z",
      },
    ],
  };
}

/** Domains whose lookups fail with a network error (to show failures). */
export const UNREACHABLE_DOMAINS: readonly string[] = ["unreachable.example"];

/** Certificates in the Windows stores. */
export function seedWindowsStore(): WindowsCertificate[] {
  const summary = (subject: string, seed: string) => ({
    subject,
    issuer: "CN=Contoso Issuing CA,O=Contoso",
    serial: mockHex(`win:${seed}`, 16),
    not_before: "2026-01-15T00:00:00Z",
    not_after: "2028-01-15T00:00:00Z",
    fingerprint: mockHex(`win-fp:${seed}`, 64),
  });
  return [
    {
      store: "personal",
      thumbprint: mockHex("win:ada-work", 40),
      certificate: summary("CN=Ada Lovelace,E=ada@work.example", "ada-work"),
      emails: ["ada@work.example"],
      has_private_key: true,
      usable_for_email: true,
      imported: null,
    },
    {
      store: "personal",
      thumbprint: mockHex("win:tls-client", 40),
      certificate: summary("CN=ADA-LAPTOP", "tls-client"),
      emails: [],
      has_private_key: true,
      usable_for_email: false,
      imported: null,
    },
    {
      store: "other-people",
      thumbprint: mockHex("win:liskov", 40),
      certificate: summary("CN=Barbara Liskov,E=liskov@mit.example", "liskov"),
      emails: ["liskov@mit.example"],
      has_private_key: false,
      usable_for_email: true,
      imported: null,
    },
  ];
}

/** Autocrypt peer state per peer address. */
export function seedPeers(): Record<string, AutocryptPeer> {
  const alanKey = mockFingerprint("openpgp", "Alan Turing <alan@bletchley.example>");
  return {
    "alan@bletchley.example": {
      email: "alan@bletchley.example",
      last_seen: "2026-09-20T12:00:00Z",
      autocrypt_at: "2026-09-20T12:00:00Z",
      prefer_encrypt: "mutual",
      key_fingerprint: alanKey,
      gossip_fingerprint: null,
    },
  };
}

/**
 * Demo findings per sender address, for the dev mock (`?mock=1`): the
 * reader shows every badge and warning kind on the mock mailbox.
 */
export function securityDemoBySender(): Record<string, SecurityOverlay> {
  const grace = mockFingerprint("openpgp", "Grace Hopper <grace@navy.example>");
  const ada = mockFingerprint("openpgp", "Ada Lovelace <ada@example.org>");
  const katherineSubject = "CN=Katherine Johnson,E=katherine@nasa.example";
  const katherine = mockFingerprint("smime", `${katherineSubject}|katherine@nasa.example`);
  return {
    "grace@navy.example": {
      security: {
        technology: "openpgp",
        signatures: [
          {
            state: "valid",
            signer: "grace@navy.example",
            signer_matches_from: true,
            key_id: "key-3",
            fingerprint: grace,
            trust: "verified",
            signed_at: "2026-09-28T08:15:00Z",
            chain: null,
          },
        ],
        encryption: {
          state: "decrypted",
          key_id: "key-1",
          recipients: [shortId(ada), shortId(grace)],
          algorithm: "AES-256",
        },
        header_protection: "protected",
        partial: false,
        inline_pgp: false,
      },
      auth_results: {
        authserv_id: "imap.example.org",
        dkim: [
          { verdict: "pass", domain: "navy.example", selector: "s1", aligned: true, reason: null },
        ],
        spf: {
          verdict: "pass",
          domain: "navy.example",
          selector: null,
          aligned: true,
          reason: null,
        },
        dmarc: {
          verdict: "pass",
          domain: "navy.example",
          selector: null,
          aligned: true,
          reason: null,
        },
        arc: null,
      },
    },
    "alan@bletchley.example": {
      security: {
        technology: "openpgp",
        signatures: [
          {
            state: "valid",
            signer: "alan@bletchley.example",
            signer_matches_from: true,
            key_id: "key-4",
            fingerprint: mockFingerprint("openpgp", "Alan Turing <alan@bletchley.example>"),
            trust: "autocrypt",
            signed_at: "2026-09-27T17:40:00Z",
            chain: null,
          },
        ],
        encryption: null,
        header_protection: "none",
        partial: false,
        inline_pgp: false,
      },
    },
    "katherine@nasa.example": {
      security: {
        technology: "smime",
        signatures: [
          {
            state: "valid",
            signer: "katherine@nasa.example",
            signer_matches_from: true,
            key_id: "key-5",
            fingerprint: katherine,
            trust: "unknown",
            signed_at: "2026-09-26T11:05:00Z",
            chain: chain(katherineSubject, katherine, "valid"),
          },
        ],
        encryption: null,
        header_protection: "none",
        partial: false,
        inline_pgp: false,
      },
    },
    "ewd@tue.example": {
      security: {
        technology: "openpgp",
        signatures: [
          {
            state: "invalid",
            signer: "ewd@tue.example",
            signer_matches_from: true,
            key_id: "key-6",
            fingerprint: mockFingerprint("openpgp", "Edsger Dijkstra <ewd@tue.example>"),
            trust: "unknown",
            signed_at: "2026-09-25T07:30:00Z",
            chain: null,
          },
        ],
        encryption: null,
        header_protection: "none",
        partial: false,
        inline_pgp: false,
      },
    },
    "noreply@shop.example": {
      auth_results: {
        authserv_id: "imap.example.org",
        dkim: [
          {
            verdict: "fail",
            domain: "shop.example",
            selector: "mail",
            aligned: true,
            reason: "signature did not verify",
          },
        ],
        spf: {
          verdict: "softfail",
          domain: "shop.example",
          selector: null,
          aligned: true,
          reason: null,
        },
        dmarc: {
          verdict: "fail",
          domain: "shop.example",
          selector: null,
          aligned: null,
          reason: null,
        },
        arc: null,
      },
      warnings: [
        {
          kind: "link-mismatch",
          link: 0,
          href: "https://login.evil.example/track",
          shown_host: "shop.example",
          target_host: "login.evil.example",
        },
        {
          kind: "lookalike-domain",
          link: 1,
          href: "https://xn--shp-5cd.example/account",
          ascii_host: "xn--shp-5cd.example",
          unicode_host: "shоp.example",
          resembles: "shop.example",
        },
        {
          kind: "dangerous-attachment",
          index: 0,
          filename: "invoice.pdf.exe",
          risk: "disguised-name",
        },
      ],
    },
  };
}

function shortId(fingerprint: string): string {
  return fingerprint.slice(-16);
}
